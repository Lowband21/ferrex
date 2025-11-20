use std::fmt;
use std::sync::Arc;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use super::{AuthEventContext, map_domain_events};
use crate::auth::domain::aggregates::{DeviceSession, DeviceStatus};
use crate::auth::domain::events::AuthEvent;
use crate::auth::domain::repositories::{
    AuthEventRepository, AuthSessionRepository, DevicePinStatus, DeviceSessionRepository,
    RefreshTokenRepository, UserAuthenticationRepository,
};
use crate::auth::domain::value_objects::{DeviceFingerprint, RevocationReason};

#[derive(Debug, thiserror::Error)]
pub enum DeviceTrustError {
    #[error("User not found")]
    UserNotFound,
    #[error("User is inactive")]
    UserInactive,
    #[error("User account is locked")]
    UserLocked,
    #[error("Device not found")]
    DeviceNotFound,
    #[error("Device already trusted")]
    DeviceAlreadyTrusted,
    #[error("Device has been revoked")]
    DeviceRevoked,
    #[error("Too many devices registered (limit {limit})")]
    TooManyDevices { limit: usize },
    #[error("Device not trusted")]
    DeviceNotTrusted,
    #[error("Database error: {0}")]
    DatabaseError(#[from] anyhow::Error),
}

pub struct DeviceTrustService {
    user_repo: Arc<dyn UserAuthenticationRepository>,
    session_repo: Arc<dyn DeviceSessionRepository>,
    event_repo: Arc<dyn AuthEventRepository>,
    session_store: Arc<dyn AuthSessionRepository>,
    refresh_repo: Arc<dyn RefreshTokenRepository>,
}

impl fmt::Debug for DeviceTrustService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceTrustService")
            .field("user_repo_refs", &Arc::strong_count(&self.user_repo))
            .field("session_repo_refs", &Arc::strong_count(&self.session_repo))
            .field("event_repo_refs", &Arc::strong_count(&self.event_repo))
            .finish()
    }
}

impl DeviceTrustService {
    pub fn new(
        user_repo: Arc<dyn UserAuthenticationRepository>,
        session_repo: Arc<dyn DeviceSessionRepository>,
        event_repo: Arc<dyn AuthEventRepository>,
        session_store: Arc<dyn AuthSessionRepository>,
        refresh_repo: Arc<dyn RefreshTokenRepository>,
    ) -> Self {
        Self {
            user_repo,
            session_repo,
            event_repo,
            session_store,
            refresh_repo,
        }
    }

    /// Register a device for the specified user, creating a pending trust session when necessary.
    pub async fn register_device(
        &self,
        user_id: Uuid,
        fingerprint: DeviceFingerprint,
        device_name: String,
        context: Option<AuthEventContext>,
    ) -> Result<DeviceSession, DeviceTrustError> {
        let ctx = context.unwrap_or_default();
        let user = self
            .user_repo
            .find_by_id(user_id)
            .await?
            .ok_or(DeviceTrustError::UserNotFound)?;

        if !user.is_active() {
            return Err(DeviceTrustError::UserInactive);
        }

        if user.is_locked() {
            return Err(DeviceTrustError::UserLocked);
        }

        let existing = self
            .session_repo
            .find_by_user_and_fingerprint(user_id, &fingerprint)
            .await?;

        if let Some(mut session) = existing {
            if session.is_revoked() {
                // Treat revoked sessions as fresh registrations.
                return self
                    .create_new_session(user_id, fingerprint, device_name, ctx)
                    .await;
            }

            // Update last activity to reflect the device check-in.
            session.update_activity();
            self.session_repo.save(&session).await?;
            Ok(session)
        } else {
            // Enforce per-user device limit before creating a new session.
            let sessions = self.session_repo.find_by_user_id(user_id).await?;
            let limit = user.max_devices();
            let active_count = sessions
                .iter()
                .filter(|session| !session.is_revoked())
                .count();

            if active_count >= limit {
                return Err(DeviceTrustError::TooManyDevices { limit });
            }

            self.create_new_session(user_id, fingerprint, device_name, ctx)
                .await
        }
    }

    /// Retrieve all device sessions for a user.
    pub async fn list_devices(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<DeviceSession>, DeviceTrustError> {
        self.ensure_user_exists(user_id).await?;
        self.session_repo
            .find_by_user_id(user_id)
            .await
            .map_err(DeviceTrustError::from)
    }

    /// Fetch a specific device session by fingerprint.
    pub async fn get_device(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
    ) -> Result<DeviceSession, DeviceTrustError> {
        self.session_repo
            .find_by_user_and_fingerprint(user_id, fingerprint)
            .await?
            .ok_or(DeviceTrustError::DeviceNotFound)
    }

    pub async fn get_device_by_session_id(
        &self,
        session_id: Uuid,
    ) -> Result<DeviceSession, DeviceTrustError> {
        self.session_repo
            .find_by_id(session_id)
            .await?
            .ok_or(DeviceTrustError::DeviceNotFound)
    }

    /// Return the current trust status for a device.
    pub async fn device_status(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
    ) -> Result<DeviceStatus, DeviceTrustError> {
        Ok(self.get_device(user_id, fingerprint).await?.status())
    }

    /// Return whether any session exists for the given fingerprint.
    pub async fn is_known_device(
        &self,
        fingerprint: &DeviceFingerprint,
    ) -> Result<bool, DeviceTrustError> {
        self.session_repo
            .exists_by_fingerprint(fingerprint)
            .await
            .map_err(DeviceTrustError::from)
    }

    /// Retrieve PIN status per user for the provided fingerprint.
    pub async fn pin_status_by_device(
        &self,
        fingerprint: &DeviceFingerprint,
    ) -> Result<Vec<DevicePinStatus>, DeviceTrustError> {
        self.session_repo
            .pin_status_by_fingerprint(fingerprint)
            .await
            .map_err(DeviceTrustError::from)
    }

    /// Revoke a specific device, optionally recording a reason in the audit log.
    pub async fn revoke_device(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
        reason: Option<String>,
        context: Option<AuthEventContext>,
    ) -> Result<(), DeviceTrustError> {
        let mut session = self.get_device(user_id, fingerprint).await?;

        session
            .revoke()
            .map_err(|_| DeviceTrustError::DeviceRevoked)?;

        let mut ctx = context.unwrap_or_default();
        if let Some(reason) = reason {
            ctx.insert_metadata("reason", json!(reason));
        }

        self.persist_session(&mut session, Some(ctx)).await?;

        self.session_store
            .revoke_by_device(session.id(), RevocationReason::DeviceRevoked)
            .await?;
        self.refresh_repo
            .revoke_for_device(session.id(), RevocationReason::DeviceRevoked)
            .await?;

        Ok(())
    }

    /// Revoke all devices associated with the user.
    pub async fn revoke_all_devices(
        &self,
        user_id: Uuid,
        reason: Option<String>,
        context: Option<AuthEventContext>,
    ) -> Result<(), DeviceTrustError> {
        let mut sessions = self.session_repo.find_by_user_id(user_id).await?;

        if sessions.is_empty() {
            return Ok(());
        }

        let mut ctx = context.unwrap_or_default();
        if let Some(reason) = reason {
            ctx.insert_metadata("reason", json!(reason));
        }

        for session in sessions.iter_mut() {
            if session.is_revoked() {
                continue;
            }
            session
                .revoke()
                .map_err(|_| DeviceTrustError::DeviceRevoked)?;
            self.persist_session(session, Some(ctx.clone())).await?;
            self.session_store
                .revoke_by_device(session.id(), RevocationReason::DeviceRevoked)
                .await?;
            self.refresh_repo
                .revoke_for_device(session.id(), RevocationReason::DeviceRevoked)
                .await?;
        }

        let mut events = vec![AuthEvent::AllDevicesRevoked {
            user_id,
            timestamp: Utc::now(),
        }];
        let audit_events = map_domain_events(std::mem::take(&mut events), &ctx);
        if !audit_events.is_empty() {
            self.event_repo.record(audit_events).await?;
        }

        Ok(())
    }

    async fn create_new_session(
        &self,
        user_id: Uuid,
        fingerprint: DeviceFingerprint,
        device_name: String,
        context: AuthEventContext,
    ) -> Result<DeviceSession, DeviceTrustError> {
        let mut session = DeviceSession::new(user_id, fingerprint, device_name);
        let events = session.take_events();
        self.session_repo.save(&session).await?;
        self.publish_events(events, &context).await?;
        Ok(session)
    }

    async fn persist_session(
        &self,
        session: &mut DeviceSession,
        context: Option<AuthEventContext>,
    ) -> Result<(), DeviceTrustError> {
        let events = session.take_events();
        self.session_repo.save(session).await?;
        if !events.is_empty() {
            let ctx = context.unwrap_or_default();
            self.publish_events(events, &ctx).await?;
        }
        Ok(())
    }

    /// Attach and persist a device public key for possession validation
    pub async fn set_device_public_key(
        &self,
        device_session_id: Uuid,
        alg: String,
        public_key: String,
    ) -> Result<(), DeviceTrustError> {
        let mut session = self
            .session_repo
            .find_by_id(device_session_id)
            .await?
            .ok_or(DeviceTrustError::DeviceNotFound)?;

        session.set_device_public_key(alg, public_key);
        self.persist_session(&mut session, None).await
    }

    async fn publish_events(
        &self,
        events: Vec<AuthEvent>,
        context: &AuthEventContext,
    ) -> Result<(), DeviceTrustError> {
        let audit_events = map_domain_events(events, context);
        if !audit_events.is_empty() {
            self.event_repo.record(audit_events).await?;
        }
        Ok(())
    }

    async fn ensure_user_exists(&self, user_id: Uuid) -> Result<(), DeviceTrustError> {
        if self.user_repo.find_by_id(user_id).await?.is_none() {
            return Err(DeviceTrustError::UserNotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthCrypto;
    use crate::auth::domain::aggregates::{DeviceSession, DeviceStatus, UserAuthentication};
    use crate::auth::domain::repositories::{
        AuthAuditEventKind, AuthEventLog, AuthSessionRecord, AuthSessionRepository,
        DevicePinStatus, RefreshTokenRecord, RefreshTokenRepository,
    };
    use crate::auth::domain::value_objects::RevocationReason;
    use crate::auth::domain::value_objects::{DeviceFingerprint, SessionScope};
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use futures::FutureExt;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct InMemoryUserRepo {
        users: Mutex<HashMap<Uuid, UserAuthentication>>,
    }

    #[async_trait]
    impl UserAuthenticationRepository for InMemoryUserRepo {
        async fn find_by_id(&self, user_id: Uuid) -> anyhow::Result<Option<UserAuthentication>> {
            let users = self.users.lock().unwrap();
            Ok(users.get(&user_id).cloned())
        }

        async fn find_by_username(
            &self,
            username: &str,
        ) -> anyhow::Result<Option<UserAuthentication>> {
            let users = self.users.lock().unwrap();
            Ok(users
                .values()
                .find(|user| user.username() == username)
                .cloned())
        }

        async fn save(&self, user_auth: &UserAuthentication) -> anyhow::Result<()> {
            let mut users = self.users.lock().unwrap();
            users.insert(user_auth.user_id(), user_auth.clone());
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct InMemoryDeviceRepo {
        sessions: Mutex<HashMap<Uuid, DeviceSession>>,
    }

    #[async_trait]
    impl DeviceSessionRepository for InMemoryDeviceRepo {
        async fn find_by_id(&self, session_id: Uuid) -> anyhow::Result<Option<DeviceSession>> {
            let sessions = self.sessions.lock().unwrap();
            Ok(sessions.get(&session_id).cloned())
        }

        async fn find_by_user_and_fingerprint(
            &self,
            user_id: Uuid,
            fingerprint: &DeviceFingerprint,
        ) -> anyhow::Result<Option<DeviceSession>> {
            let sessions = self.sessions.lock().unwrap();
            Ok(sessions
                .values()
                .find(|session| {
                    session.user_id() == user_id
                        && session.device_fingerprint().as_str() == fingerprint.as_str()
                })
                .cloned())
        }

        async fn find_by_user_id(&self, user_id: Uuid) -> anyhow::Result<Vec<DeviceSession>> {
            let sessions = self.sessions.lock().unwrap();
            Ok(sessions
                .values()
                .filter(|session| session.user_id() == user_id)
                .cloned()
                .collect())
        }

        async fn save(&self, session: &DeviceSession) -> anyhow::Result<Option<Uuid>> {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.insert(session.id(), session.clone());
            Ok(None)
        }

        async fn exists_by_fingerprint(
            &self,
            fingerprint: &DeviceFingerprint,
        ) -> anyhow::Result<bool> {
            let sessions = self.sessions.lock().unwrap();
            Ok(sessions
                .values()
                .any(|session| session.device_fingerprint().as_str() == fingerprint.as_str()))
        }

        async fn pin_status_by_fingerprint(
            &self,
            fingerprint: &DeviceFingerprint,
        ) -> anyhow::Result<Vec<DevicePinStatus>> {
            let sessions = self.sessions.lock().unwrap();
            Ok(sessions
                .values()
                .filter(|session| session.device_fingerprint().as_str() == fingerprint.as_str())
                .map(|session| DevicePinStatus {
                    user_id: session.user_id(),
                    has_pin: session.has_pin(),
                })
                .collect())
        }
    }

    #[derive(Debug, Default)]
    struct InMemoryEventRepo {
        events: Mutex<Vec<AuthEventLog>>,
    }

    #[async_trait]
    impl AuthEventRepository for InMemoryEventRepo {
        async fn record(&self, events: Vec<AuthEventLog>) -> anyhow::Result<()> {
            let mut storage = self.events.lock().unwrap();
            storage.extend(events);
            Ok(())
        }
    }

    impl InMemoryEventRepo {
        fn events(&self) -> Vec<AuthEventLog> {
            self.events.lock().unwrap().clone()
        }
    }

    #[derive(Debug, Default)]
    struct RecordingSessionRepo {
        revoked_devices: Mutex<Vec<Uuid>>,
    }

    #[async_trait]
    impl AuthSessionRepository for RecordingSessionRepo {
        async fn find_by_id(&self, _session_id: Uuid) -> anyhow::Result<Option<AuthSessionRecord>> {
            Ok(None)
        }

        async fn insert_session(
            &self,
            _user_id: Uuid,
            _device_session_id: Option<Uuid>,
            _scope: SessionScope,
            _token_hash: &str,
            _created_at: DateTime<Utc>,
            _expires_at: DateTime<Utc>,
        ) -> anyhow::Result<Uuid> {
            Ok(Uuid::now_v7())
        }

        async fn revoke_by_hash(
            &self,
            _token_hash: &str,
            _reason: RevocationReason,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn find_by_hash(
            &self,
            _token_hash: &str,
        ) -> anyhow::Result<Option<AuthSessionRecord>> {
            Ok(None)
        }

        async fn touch(&self, _session_id: Uuid) -> anyhow::Result<()> {
            Ok(())
        }

        async fn list_by_user(&self, _user_id: Uuid) -> anyhow::Result<Vec<AuthSessionRecord>> {
            Ok(Vec::new())
        }

        async fn revoke_by_user(
            &self,
            _user_id: Uuid,
            _reason: RevocationReason,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn revoke_by_device(
            &self,
            _device_session_id: Uuid,
            _reason: RevocationReason,
        ) -> anyhow::Result<()> {
            self.revoked_devices
                .lock()
                .unwrap()
                .push(_device_session_id);
            Ok(())
        }

        async fn revoke_by_id(
            &self,
            session_id: Uuid,
            _reason: RevocationReason,
        ) -> anyhow::Result<()> {
            self.revoked_devices.lock().unwrap().push(session_id);
            Ok(())
        }
    }

    impl RecordingSessionRepo {
        fn revoked_devices(&self) -> Vec<Uuid> {
            self.revoked_devices.lock().unwrap().clone()
        }
    }

    #[derive(Debug, Default)]
    struct RecordingRefreshRepo {
        revoked_devices: Mutex<Vec<Uuid>>,
    }

    #[async_trait]
    impl RefreshTokenRepository for RecordingRefreshRepo {
        async fn insert_refresh_token(
            &self,
            _token_hash: &str,
            _user_id: Uuid,
            _device_session_id: Option<Uuid>,
            _session_id: Option<Uuid>,
            _issued_at: DateTime<Utc>,
            _expires_at: DateTime<Utc>,
            _family_id: Uuid,
            _generation: i32,
            _origin_scope: crate::auth::domain::value_objects::SessionScope,
        ) -> anyhow::Result<Uuid> {
            Ok(Uuid::now_v7())
        }

        async fn get_active_refresh_token(
            &self,
            _token_hash: &str,
        ) -> anyhow::Result<Option<RefreshTokenRecord>> {
            Ok(None)
        }

        async fn mark_used(
            &self,
            _token_id: Uuid,
            _reason: RevocationReason,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn revoke_family(
            &self,
            _family_id: Uuid,
            _reason: RevocationReason,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn revoke_for_user(
            &self,
            _user_id: Uuid,
            _reason: RevocationReason,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn revoke_for_device(
            &self,
            _device_session_id: Uuid,
            _reason: RevocationReason,
        ) -> anyhow::Result<()> {
            self.revoked_devices
                .lock()
                .unwrap()
                .push(_device_session_id);
            Ok(())
        }

        async fn revoke_for_session(
            &self,
            _session_id: Uuid,
            _reason: RevocationReason,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl RecordingRefreshRepo {
        fn revoked_devices(&self) -> Vec<Uuid> {
            self.revoked_devices.lock().unwrap().clone()
        }
    }

    fn sample_user(max_devices: usize) -> UserAuthentication {
        let user_id = Uuid::now_v7();
        UserAuthentication::new(user_id, "user".to_string(), "hash".to_string(), max_devices)
    }

    fn sample_fingerprint() -> DeviceFingerprint {
        DeviceFingerprint::new(
            "Linux".to_string(),
            Some("CPU".to_string()),
            None,
            None,
            None,
        )
        .unwrap()
    }

    fn build_service(
        user: UserAuthentication,
    ) -> (
        DeviceTrustService,
        Arc<InMemoryDeviceRepo>,
        Arc<InMemoryEventRepo>,
        Arc<RecordingSessionRepo>,
        Arc<RecordingRefreshRepo>,
        Uuid,
    ) {
        let user_repo = Arc::new(InMemoryUserRepo::default());
        let device_repo = Arc::new(InMemoryDeviceRepo::default());
        let event_repo = Arc::new(InMemoryEventRepo::default());
        let session_repo = Arc::new(RecordingSessionRepo::default());
        let refresh_repo = Arc::new(RecordingRefreshRepo::default());

        user_repo
            .save(&user)
            .now_or_never()
            .expect("save user")
            .unwrap();

        let service = DeviceTrustService::new(
            user_repo,
            device_repo.clone(),
            event_repo.clone(),
            session_repo.clone(),
            refresh_repo.clone(),
        );
        (
            service,
            device_repo,
            event_repo,
            session_repo,
            refresh_repo,
            user.user_id(),
        )
    }

    #[tokio::test]
    async fn register_device_creates_pending_session_and_logs_event() {
        let user = sample_user(3);
        let (service, device_repo, event_repo, _session_repo, _refresh_repo, user_id) =
            build_service(user);
        let fingerprint = sample_fingerprint();

        let session = service
            .register_device(
                user_id,
                fingerprint.clone(),
                "Test Device".to_string(),
                None,
            )
            .await
            .expect("register device");

        assert_eq!(session.status(), DeviceStatus::Pending);

        let stored = device_repo
            .find_by_user_and_fingerprint(user_id, &fingerprint)
            .await
            .unwrap()
            .expect("stored session");
        assert_eq!(stored.id(), session.id());

        let events = event_repo.events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event.event_type, AuthAuditEventKind::DeviceRegistered))
        );
    }

    #[tokio::test]
    async fn register_device_enforces_device_limit() {
        let user = sample_user(1);
        let (service, device_repo, _event_repo, _session_repo, _refresh_repo, user_id) =
            build_service(user);

        let fingerprint = sample_fingerprint();
        let mut session = DeviceSession::new(user_id, fingerprint.clone(), "Existing".to_string());
        let _ = session.take_events();
        // No user-level PIN needed for device limit tests; inserting a device session is sufficient
        let _ = session.take_events();
        device_repo.save(&session).await.unwrap();

        let new_fingerprint = DeviceFingerprint::new(
            "Linux".to_string(),
            Some("CPU2".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let result = service
            .register_device(user_id, new_fingerprint, "Overflow".to_string(), None)
            .await;

        match result {
            Err(DeviceTrustError::TooManyDevices { limit }) => assert_eq!(limit, 1),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn revoke_device_marks_session_and_logs_reason() {
        let user = sample_user(3);
        let (service, device_repo, event_repo, _session_repo, _refresh_repo, user_id) =
            build_service(user);
        let fingerprint = sample_fingerprint();

        service
            .register_device(user_id, fingerprint.clone(), "Test".to_string(), None)
            .await
            .unwrap();

        service
            .revoke_device(
                user_id,
                &fingerprint,
                Some("user_request".to_string()),
                None,
            )
            .await
            .unwrap();

        let session = device_repo
            .find_by_user_and_fingerprint(user_id, &fingerprint)
            .await
            .unwrap()
            .expect("session present");
        assert_eq!(session.status(), DeviceStatus::Revoked);

        let events = event_repo.events();
        let revoke_event = events
            .iter()
            .find(|event| matches!(event.event_type, AuthAuditEventKind::DeviceRevoked))
            .expect("revocation event");
        assert_eq!(
            revoke_event
                .metadata
                .as_object()
                .and_then(|obj| obj.get("reason"))
                .and_then(|value| value.as_str()),
            Some("user_request")
        );
    }

    #[tokio::test]
    async fn revoke_device_cascades_to_session_and_refresh_records() {
        let user = sample_user(3);
        let (service, device_repo, _event_repo, session_repo, refresh_repo, user_id) =
            build_service(user);
        let fingerprint = sample_fingerprint();

        let session = service
            .register_device(
                user_id,
                fingerprint.clone(),
                "Cascade Device".to_string(),
                None,
            )
            .await
            .expect("register device");

        // Ensure device persisted before revocation
        assert!(
            device_repo
                .find_by_user_and_fingerprint(user_id, &fingerprint)
                .await
                .unwrap()
                .is_some()
        );

        service
            .revoke_device(user_id, &fingerprint, Some("security".to_string()), None)
            .await
            .expect("revoke device");

        assert_eq!(session_repo.revoked_devices(), vec![session.id()]);
        assert_eq!(refresh_repo.revoked_devices(), vec![session.id()]);
    }

    #[tokio::test]
    async fn revoke_all_devices_revokes_sessions_and_refresh_records() {
        let user = sample_user(3);
        let (service, _device_repo, _event_repo, session_repo, refresh_repo, user_id) =
            build_service(user);

        let first = sample_fingerprint();
        let second = DeviceFingerprint::new(
            "Linux".to_string(),
            Some("CPU2".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let session_one = service
            .register_device(user_id, first.clone(), "One".to_string(), None)
            .await
            .expect("register first");
        let session_two = service
            .register_device(user_id, second.clone(), "Two".to_string(), None)
            .await
            .expect("register second");

        service
            .revoke_all_devices(user_id, Some("operator".to_string()), None)
            .await
            .expect("revoke all");

        let mut revoked_sessions = session_repo.revoked_devices();
        revoked_sessions.sort();
        let mut revoked_refresh = refresh_repo.revoked_devices();
        revoked_refresh.sort();

        let mut expected = vec![session_one.id(), session_two.id()];
        expected.sort();

        assert_eq!(revoked_sessions, expected);
        assert_eq!(revoked_refresh, expected);
    }
}
