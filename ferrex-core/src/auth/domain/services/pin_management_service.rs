use std::fmt;
use std::sync::Arc;

use anyhow::anyhow;
use serde_json::json;
use uuid::Uuid;

use super::{AuthEventContext, map_domain_events};
use crate::auth::domain::aggregates::DeviceSession;
use crate::auth::domain::aggregates::DeviceSessionError;
use crate::auth::domain::repositories::{
    AuthEventRepository, DeviceSessionRepository, UserAuthenticationRepository,
};
use crate::auth::domain::value_objects::{DeviceFingerprint, PinPolicy};

#[derive(Debug, thiserror::Error)]
pub enum PinManagementError {
    #[error("User not found")]
    UserNotFound,
    #[error("User is inactive")]
    UserInactive,
    #[error("User account is locked")]
    UserLocked,
    #[error("Device not found")]
    DeviceNotFound,
    #[error("Device is revoked")]
    DeviceRevoked,
    #[error("PIN is not configured")]
    PinNotSet,
    #[error("PIN format is invalid")]
    InvalidPinFormat,
    #[error("PIN verification failed")]
    PinVerificationFailed,
    #[error("Too many failed attempts")]
    TooManyFailedAttempts,
    #[error("Database error: {0}")]
    DatabaseError(#[from] anyhow::Error),
}

pub struct PinManagementService {
    user_repo: Arc<dyn UserAuthenticationRepository>,
    session_repo: Arc<dyn DeviceSessionRepository>,
    event_repo: Arc<dyn AuthEventRepository>,
}

impl fmt::Debug for PinManagementService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PinManagementService")
            .field("user_repo_refs", &Arc::strong_count(&self.user_repo))
            .field("session_repo_refs", &Arc::strong_count(&self.session_repo))
            .field("event_repo_refs", &Arc::strong_count(&self.event_repo))
            .finish()
    }
}

impl PinManagementService {
    pub fn new(
        user_repo: Arc<dyn UserAuthenticationRepository>,
        session_repo: Arc<dyn DeviceSessionRepository>,
        event_repo: Arc<dyn AuthEventRepository>,
    ) -> Self {
        Self {
            user_repo,
            session_repo,
            event_repo,
        }
    }

    /// Configure or replace a device PIN, transitioning the device to a trusted state.
    pub async fn set_pin(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
        new_pin: String,
        policy: &PinPolicy,
        context: Option<AuthEventContext>,
    ) -> Result<(), PinManagementError> {
        let mut ctx = context.unwrap_or_default();
        ctx.insert_metadata("operation", json!("set"));
        let mut session = self.load_session(user_id, fingerprint).await?;

        session
            .set_pin(new_pin, policy)
            .map_err(|err| map_pin_error(err, false))?;

        self.persist_session(&mut session, ctx).await
    }

    /// Rotate an existing PIN after verifying the current value.
    pub async fn rotate_pin(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
        current_pin: &str,
        new_pin: String,
        policy: &PinPolicy,
        max_attempts: u8,
        context: Option<AuthEventContext>,
    ) -> Result<(), PinManagementError> {
        let mut ctx = context.unwrap_or_default();
        ctx.insert_metadata("operation", json!("rotate"));
        let mut session = self.load_session(user_id, fingerprint).await?;

        session
            .verify_pin(current_pin, max_attempts)
            .map_err(|err| map_pin_error(err, true))?;

        session
            .set_pin(new_pin, policy)
            .map_err(|err| map_pin_error(err, false))?;

        self.persist_session(&mut session, ctx).await
    }

    /// Remove the configured PIN, returning the device to a pending trust state.
    pub async fn clear_pin(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
        current_pin: &str,
        max_attempts: u8,
        context: Option<AuthEventContext>,
    ) -> Result<(), PinManagementError> {
        let mut ctx = context.unwrap_or_default();
        ctx.insert_metadata("operation", json!("clear"));
        let mut session = self.load_session(user_id, fingerprint).await?;

        session
            .verify_pin(current_pin, max_attempts)
            .map_err(|err| map_pin_error(err, true))?;
        session
            .clear_pin()
            .map_err(|err| map_pin_error(err, true))?;

        self.persist_session(&mut session, ctx).await
    }

    /// Force clear a PIN without verifying the current value. Intended for
    /// administrative overrides when the overseeing user has already been
    /// authenticated out-of-band.
    pub async fn force_clear_pin(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
        context: Option<AuthEventContext>,
    ) -> Result<(), PinManagementError> {
        let mut ctx = context.unwrap_or_default();
        ctx.insert_metadata("operation", json!("force_clear"));

        let mut session = self.load_session(user_id, fingerprint).await?;
        session
            .clear_pin()
            .map_err(|err| map_pin_error(err, false))?;

        self.persist_session(&mut session, ctx).await
    }

    /// Verify a PIN without issuing a session token.
    pub async fn verify_pin(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
        pin: &str,
        max_attempts: u8,
    ) -> Result<(), PinManagementError> {
        let mut session = self.load_session(user_id, fingerprint).await?;
        session
            .verify_pin(pin, max_attempts)
            .map_err(|err| map_pin_error(err, true))?;
        Ok(())
    }

    async fn load_session(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
    ) -> Result<DeviceSession, PinManagementError> {
        let user = self
            .user_repo
            .find_by_id(user_id)
            .await?
            .ok_or(PinManagementError::UserNotFound)?;

        if !user.is_active() {
            return Err(PinManagementError::UserInactive);
        }

        if user.is_locked() {
            return Err(PinManagementError::UserLocked);
        }

        let session = self
            .session_repo
            .find_by_user_and_fingerprint(user_id, fingerprint)
            .await?
            .ok_or(PinManagementError::DeviceNotFound)?;

        if session.is_revoked() {
            return Err(PinManagementError::DeviceRevoked);
        }

        Ok(session)
    }

    async fn persist_session(
        &self,
        session: &mut DeviceSession,
        context: AuthEventContext,
    ) -> Result<(), PinManagementError> {
        let events = session.take_events();
        self.session_repo.save(session).await?;

        if !events.is_empty() {
            let audit_events = map_domain_events(events, &context);
            if !audit_events.is_empty() {
                self.event_repo.record(audit_events).await?;
            }
        }

        Ok(())
    }
}

fn map_pin_error(err: DeviceSessionError, verification: bool) -> PinManagementError {
    use DeviceSessionError as D;

    match err {
        D::DeviceRevoked => PinManagementError::DeviceRevoked,
        D::DeviceNotTrusted | D::PinNotSet => PinManagementError::PinNotSet,
        D::InvalidPin => {
            if verification {
                PinManagementError::PinVerificationFailed
            } else {
                PinManagementError::InvalidPinFormat
            }
        }
        D::TooManyFailedAttempts => PinManagementError::TooManyFailedAttempts,
        D::SessionExpired => PinManagementError::PinVerificationFailed,
        _ => PinManagementError::DatabaseError(anyhow!("unexpected device session error: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::domain::aggregates::{DeviceSession, DeviceStatus, UserAuthentication};
    use crate::auth::domain::repositories::{AuthAuditEventKind, AuthEventLog, DevicePinStatus};
    use crate::auth::domain::value_objects::{DeviceFingerprint, PinPolicy};
    use async_trait::async_trait;
    use futures::FutureExt;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

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

    fn sample_user() -> UserAuthentication {
        let user_id = Uuid::now_v7();
        UserAuthentication::new(user_id, "user".to_string(), "hash".to_string(), 5)
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
        session: DeviceSession,
    ) -> (
        PinManagementService,
        Arc<InMemoryDeviceRepo>,
        Arc<InMemoryEventRepo>,
        Uuid,
        DeviceFingerprint,
    ) {
        let user_repo = Arc::new(InMemoryUserRepo::default());
        let device_repo = Arc::new(InMemoryDeviceRepo::default());
        let event_repo = Arc::new(InMemoryEventRepo::default());

        let user_id = user.user_id();
        let fingerprint = session.device_fingerprint().clone();

        user_repo.save(&user).now_or_never().unwrap().unwrap();

        device_repo.save(&session).now_or_never().unwrap().unwrap();

        let service = PinManagementService::new(user_repo, device_repo.clone(), event_repo.clone());
        (service, device_repo, event_repo, user_id, fingerprint)
    }

    #[tokio::test]
    async fn set_pin_trusts_device_and_logs_events() {
        let user = sample_user();
        let mut session =
            DeviceSession::new(user.user_id(), sample_fingerprint(), "Test".to_string());
        let _ = session.take_events();
        let (service, device_repo, event_repo, user_id, fingerprint) = build_service(user, session);

        service
            .set_pin(
                user_id,
                &fingerprint,
                "4821".to_string(),
                &PinPolicy::default(),
                None,
            )
            .await
            .unwrap();

        let stored = device_repo
            .find_by_user_and_fingerprint(user_id, &fingerprint)
            .await
            .unwrap()
            .expect("session stored");
        assert!(stored.is_trusted());
        assert!(stored.has_pin());

        let events = event_repo.events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event.event_type, AuthAuditEventKind::PinSet))
        );
    }

    #[tokio::test]
    async fn force_clear_pin_removes_pin_without_current_value() {
        let user = sample_user();
        let mut session =
            DeviceSession::new(user.user_id(), sample_fingerprint(), "Trusted".to_string());
        session
            .set_pin("1357".to_string(), &PinPolicy::default())
            .unwrap();
        let (service, device_repo, event_repo, user_id, fingerprint) = build_service(user, session);

        service
            .force_clear_pin(user_id, &fingerprint, None)
            .await
            .unwrap();

        let stored = device_repo
            .find_by_user_and_fingerprint(user_id, &fingerprint)
            .await
            .unwrap()
            .expect("session stored");
        assert!(!stored.has_pin());
        assert_eq!(stored.status(), DeviceStatus::Pending);

        let events = event_repo.events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event.event_type, AuthAuditEventKind::PinRemoved))
        );
    }

    #[tokio::test]
    async fn rotate_pin_requires_current_value() {
        let user = sample_user();
        let mut session =
            DeviceSession::new(user.user_id(), sample_fingerprint(), "Test".to_string());
        let _ = session.take_events();
        session
            .set_pin("4861".to_string(), &PinPolicy::default())
            .unwrap();
        let _ = session.take_events();

        let (service, _device_repo, _event_repo, user_id, fingerprint) =
            build_service(user, session);

        let result = service
            .rotate_pin(
                user_id,
                &fingerprint,
                "9999",
                "7359".to_string(),
                &PinPolicy::default(),
                3,
                None,
            )
            .await;

        assert!(matches!(
            result,
            Err(PinManagementError::PinVerificationFailed)
        ));
    }

    #[tokio::test]
    async fn clear_pin_returns_device_to_pending_state() {
        let user = sample_user();
        let mut session =
            DeviceSession::new(user.user_id(), sample_fingerprint(), "Test".to_string());
        let _ = session.take_events();
        session
            .set_pin("1357".to_string(), &PinPolicy::default())
            .unwrap();
        let _ = session.take_events();

        let (service, device_repo, event_repo, user_id, fingerprint) = build_service(user, session);

        service
            .clear_pin(user_id, &fingerprint, "1357", 3, None)
            .await
            .unwrap();

        let stored = device_repo
            .find_by_user_and_fingerprint(user_id, &fingerprint)
            .await
            .unwrap()
            .expect("session stored");
        assert!(!stored.has_pin());
        assert_eq!(stored.status(), DeviceStatus::Pending);

        let events = event_repo.events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event.event_type, AuthAuditEventKind::PinRemoved))
        );
    }
}
