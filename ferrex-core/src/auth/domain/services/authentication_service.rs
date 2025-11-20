use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use std::fmt;
use std::sync::Arc;
use uuid::Uuid;

use super::{AuthEventContext, map_domain_events};
use crate::auth::AuthCrypto;
use crate::auth::domain::aggregates::{
    DeviceSession, DeviceSessionError, UserAuthentication, UserAuthenticationError,
};
use crate::auth::domain::events::AuthEvent;
use crate::auth::domain::repositories::{
    AuthEventRepository, AuthSessionRepository, DeviceSessionRepository, RefreshTokenRepository,
    UserAuthenticationRepository,
};
use crate::auth::domain::value_objects::{
    DeviceFingerprint, PinPolicy, RefreshToken, RevocationReason, SessionScope, SessionToken,
};

#[derive(Debug, thiserror::Error)]
pub enum AuthenticationError {
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("User not found")]
    UserNotFound,
    #[error("Device not found")]
    DeviceNotFound,
    #[error("Device not trusted")]
    DeviceNotTrusted,
    #[error("Invalid PIN")]
    InvalidPin,
    #[error("Too many failed attempts")]
    TooManyFailedAttempts,
    #[error("Session expired")]
    SessionExpired,
    #[error("Database error: {0}")]
    DatabaseError(#[from] anyhow::Error),
}

pub struct AuthenticationService {
    user_repo: Arc<dyn UserAuthenticationRepository>,
    session_repo: Arc<dyn DeviceSessionRepository>,
    refresh_repo: Arc<dyn RefreshTokenRepository>,
    session_store: Arc<dyn AuthSessionRepository>,
    crypto: Arc<AuthCrypto>,
    event_repo: Option<Arc<dyn AuthEventRepository>>,
}

#[derive(Debug, Clone)]
pub struct ValidatedSession {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub device_session_id: Option<Uuid>,
    pub expires_at: DateTime<Utc>,
    pub scope: SessionScope,
}

#[derive(Debug)]
pub struct TokenBundle {
    pub session_token: SessionToken,
    pub refresh_token: RefreshToken,
    pub session_record_id: Uuid,
    pub refresh_record_id: Uuid,
    pub device_session_id: Option<Uuid>,
    pub user_id: Uuid,
    pub scope: SessionScope,
}

#[derive(Debug, Clone)]
pub enum PasswordChangeActor {
    UserInitiated,
    AdminInitiated { admin_user_id: Uuid },
}

impl PasswordChangeActor {
    fn revocation_reason(&self) -> RevocationReason {
        match self {
            Self::UserInitiated => RevocationReason::PasswordChange,
            Self::AdminInitiated { .. } => RevocationReason::AdminPasswordReset,
        }
    }

    fn describe(&self) -> (&'static str, Option<Uuid>) {
        match self {
            Self::UserInitiated => ("user", None),
            Self::AdminInitiated { admin_user_id } => ("admin", Some(*admin_user_id)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PasswordChangeRequest {
    pub user_id: Uuid,
    pub new_password: String,
    pub current_password: Option<String>,
    pub actor: PasswordChangeActor,
    pub context: Option<AuthEventContext>,
}

impl fmt::Debug for AuthenticationService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthenticationService")
            .field("user_repo_refs", &Arc::strong_count(&self.user_repo))
            .field("session_repo_refs", &Arc::strong_count(&self.session_repo))
            .field("refresh_repo_refs", &Arc::strong_count(&self.refresh_repo))
            .field(
                "session_store_refs",
                &Arc::strong_count(&self.session_store),
            )
            .field(
                "event_repo_refs",
                &self.event_repo.as_ref().map(Arc::strong_count),
            )
            .finish()
    }
}

impl AuthenticationService {
    pub fn new(
        user_repo: Arc<dyn UserAuthenticationRepository>,
        session_repo: Arc<dyn DeviceSessionRepository>,
        refresh_repo: Arc<dyn RefreshTokenRepository>,
        session_store: Arc<dyn AuthSessionRepository>,
        crypto: Arc<AuthCrypto>,
    ) -> Self {
        Self {
            user_repo,
            session_repo,
            refresh_repo,
            session_store,
            crypto,
            event_repo: None,
        }
    }

    pub fn with_event_repository(mut self, event_repo: Arc<dyn AuthEventRepository>) -> Self {
        self.event_repo = Some(event_repo);
        self
    }

    pub async fn change_password(
        &self,
        request: PasswordChangeRequest,
    ) -> Result<(), AuthenticationError> {
        let PasswordChangeRequest {
            user_id,
            new_password,
            current_password,
            actor,
            context,
        } = request;

        let mut user = self
            .user_repo
            .find_by_id(user_id)
            .await
            .map_err(AuthenticationError::from)?
            .ok_or(AuthenticationError::UserNotFound)?;

        let requires_current = matches!(actor, PasswordChangeActor::UserInitiated);
        if requires_current && current_password.is_none() {
            return Err(AuthenticationError::InvalidCredentials);
        }

        if let Some(current) = current_password.as_ref() {
            let verified = self
                .crypto
                .verify_password(current, user.password_hash())
                .map_err(|err| {
                    AuthenticationError::DatabaseError(anyhow::anyhow!(err.to_string()))
                })?;
            if !verified {
                return Err(AuthenticationError::InvalidCredentials);
            }
        }

        let password_hash = self
            .crypto
            .hash_password(&new_password)
            .map_err(|err| AuthenticationError::DatabaseError(anyhow::anyhow!(err.to_string())))?;

        user.update_password(password_hash);

        let mut base_context = context.unwrap_or_default();
        let (label, admin_id) = actor.describe();
        base_context.insert_metadata("initiator", json!(label));
        if let Some(admin) = admin_id {
            base_context.insert_metadata("admin_user_id", json!(admin));
        }

        let user_events = user.take_events();
        self.user_repo
            .save(&user)
            .await
            .map_err(AuthenticationError::from)?;
        self.publish_events(user_events, base_context.clone())
            .await?;

        let mut sessions = self
            .session_repo
            .find_by_user_id(user_id)
            .await
            .map_err(AuthenticationError::from)?;

        for session in sessions.iter_mut() {
            if session.is_revoked() {
                continue;
            }

            if let Err(err) = session.revoke() {
                if !matches!(err, DeviceSessionError::DeviceRevoked) {
                    return Err(AuthenticationError::DatabaseError(anyhow::anyhow!(
                        err.to_string()
                    )));
                }
            }

            let events = session.take_events();
            self.session_repo
                .save(session)
                .await
                .map_err(AuthenticationError::from)?;

            if !events.is_empty() {
                self.publish_events(events, base_context.clone()).await?;
            }
        }

        let reason = actor.revocation_reason();
        self.session_store
            .revoke_by_user(user_id, reason)
            .await
            .map_err(AuthenticationError::from)?;
        self.refresh_repo
            .revoke_for_user(user_id, reason)
            .await
            .map_err(AuthenticationError::from)?;

        Ok(())
    }

    pub async fn validate_session_token(
        &self,
        token: &str,
    ) -> Result<ValidatedSession, AuthenticationError> {
        let token_hash = self.crypto.hash_token(token);

        let record = self
            .session_store
            .find_by_hash(&token_hash)
            .await
            .map_err(AuthenticationError::from)?
            .ok_or(AuthenticationError::SessionExpired)?;

        if record.revoked {
            return Err(AuthenticationError::SessionExpired);
        }

        if record.expires_at < Utc::now() {
            return Err(AuthenticationError::SessionExpired);
        }

        self.session_store
            .touch(record.id)
            .await
            .map_err(AuthenticationError::from)?;

        Ok(ValidatedSession {
            session_id: record.id,
            user_id: record.user_id,
            device_session_id: record.device_session_id,
            expires_at: record.expires_at,
            scope: record.scope,
        })
    }

    pub async fn authenticate_user(
        &self,
        username: &str,
        password: &str,
    ) -> Result<UserAuthentication, AuthenticationError> {
        let mut user = self
            .user_repo
            .find_by_username(username)
            .await?
            .ok_or(AuthenticationError::UserNotFound)?;

        let result = user.authenticate_password(password, &self.crypto);
        let events = user.take_events();
        self.user_repo.save(&user).await?;
        self.publish_events(events, AuthEventContext::default())
            .await?;

        match result {
            Ok(()) => Ok(user),
            Err(err) => Err(map_user_auth_error(err)),
        }
    }

    /// Authenticate using PIN on a registered device
    pub async fn authenticate_with_pin(
        &self,
        user_id: Uuid,
        device_fingerprint: &DeviceFingerprint,
        pin: &str,
    ) -> Result<SessionToken, AuthenticationError> {
        let bundle = self
            .authenticate_device_with_pin(user_id, device_fingerprint, pin)
            .await
            .map_err(AuthenticationError::from)?;

        Ok(bundle.session_token)
    }

    pub async fn authenticate_with_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<TokenBundle, AuthenticationError> {
        let user = self.authenticate_user(username, password).await?;

        let session_token = SessionToken::generate(Duration::hours(24)).map_err(|_| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("failed to generate session token"))
        })?;

        let session_hash = self.crypto.hash_token(session_token.as_str());
        let session_scope = SessionScope::Full;
        let session_record_id = self
            .session_store
            .insert_session(
                user.user_id(),
                None,
                session_scope,
                &session_hash,
                session_token.created_at(),
                session_token.expires_at(),
            )
            .await?;

        let refresh_token = RefreshToken::generate(Duration::days(30)).map_err(|_| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("failed to generate refresh token"))
        })?;

        let refresh_hash = self.crypto.hash_token(refresh_token.as_str());
        let refresh_generation = i32::try_from(refresh_token.generation()).map_err(|_| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("refresh token generation overflow"))
        })?;

        let refresh_record_id = self
            .refresh_repo
            .insert_refresh_token(
                &refresh_hash,
                user.user_id(),
                None,
                Some(session_record_id),
                refresh_token.issued_at(),
                refresh_token.expires_at(),
                refresh_token.family_id(),
                refresh_generation,
            )
            .await?;

        Ok(TokenBundle {
            session_token,
            refresh_token,
            session_record_id,
            refresh_record_id,
            device_session_id: None,
            user_id: user.user_id(),
            scope: session_scope,
        })
    }

    pub async fn authenticate_device_with_pin(
        &self,
        user_id: Uuid,
        device_fingerprint: &DeviceFingerprint,
        pin: &str,
    ) -> Result<TokenBundle, AuthenticationError> {
        let mut session = self
            .session_repo
            .find_by_user_and_fingerprint(user_id, device_fingerprint)
            .await?
            .ok_or(AuthenticationError::DeviceNotFound)?;

        let auth_result = session.authenticate_with_pin(pin, 3, Duration::hours(24));

        let session_token = match auth_result {
            Ok(token) => token,
            Err(err) => {
                let events = session.take_events();
                self.session_repo.save(&session).await?;
                self.publish_events(events, AuthEventContext::default())
                    .await?;
                return Err(map_device_pin_error(err));
            }
        };

        let persisted_hash = self.crypto.hash_token(session_token.as_str());
        let persisted_token = SessionToken::from_value(
            persisted_hash,
            session_token.created_at(),
            session_token.expires_at(),
        )
        .map_err(|_| {
            AuthenticationError::DatabaseError(anyhow::anyhow!(
                "failed to construct persisted session token"
            ))
        })?;
        session.set_persisted_token(Some(persisted_token));

        let events = session.take_events();

        let session_record_id = self.session_repo.save(&session).await?.ok_or_else(|| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("failed to persist session token"))
        })?;

        let mut context = AuthEventContext::default();
        context.auth_session_id = Some(session_record_id);
        self.publish_events(events, context).await?;

        let refresh_token = RefreshToken::generate(Duration::days(30)).map_err(|_| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("failed to generate refresh token"))
        })?;

        let refresh_hash = self.crypto.hash_token(refresh_token.as_str());
        let refresh_generation = i32::try_from(refresh_token.generation()).map_err(|_| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("refresh token generation overflow"))
        })?;

        let refresh_record_id = self
            .refresh_repo
            .insert_refresh_token(
                &refresh_hash,
                user_id,
                Some(session.id()),
                Some(session_record_id),
                refresh_token.issued_at(),
                refresh_token.expires_at(),
                refresh_token.family_id(),
                refresh_generation,
            )
            .await?;

        Ok(TokenBundle {
            session_token,
            refresh_token,
            session_record_id,
            refresh_record_id,
            device_session_id: Some(session.id()),
            user_id,
            scope: SessionScope::Playback,
        })
    }

    pub async fn refresh_session(
        &self,
        refresh_token: &str,
    ) -> Result<TokenBundle, AuthenticationError> {
        let token_hash = self.crypto.hash_token(refresh_token);

        let record = self
            .refresh_repo
            .get_active_refresh_token(&token_hash)
            .await?
            .ok_or(AuthenticationError::SessionExpired)?;

        if record.revoked {
            let reused_after_rotation = record.used_count > 0
                && matches!(
                    record.revoked_reason.as_deref(),
                    Some(reason) if reason == RevocationReason::Rotation.as_str()
                );

            if reused_after_rotation {
                self.refresh_repo
                    .revoke_family(record.token.family_id(), RevocationReason::ReuseDetected)
                    .await?;
            }

            return Err(AuthenticationError::SessionExpired);
        }

        if record.token.is_expired() {
            return Err(AuthenticationError::SessionExpired);
        }

        self.refresh_repo
            .mark_used(record.id, RevocationReason::Rotation)
            .await?;

        if let Some(device_session_id) = record.device_session_id {
            let mut session = self
                .session_repo
                .find_by_id(device_session_id)
                .await?
                .ok_or(AuthenticationError::DeviceNotFound)?;

            let session_token = session
                .refresh_token(Duration::hours(24))
                .map_err(|_| AuthenticationError::SessionExpired)?;

            let persisted_hash = self.crypto.hash_token(session_token.as_str());
            let persisted_token = SessionToken::from_value(
                persisted_hash,
                session_token.created_at(),
                session_token.expires_at(),
            )
            .map_err(|_| {
                AuthenticationError::DatabaseError(anyhow::anyhow!(
                    "failed to construct persisted session token"
                ))
            })?;
            session.set_persisted_token(Some(persisted_token));

            let events = session.take_events();

            let session_record_id = self.session_repo.save(&session).await?.ok_or_else(|| {
                AuthenticationError::DatabaseError(anyhow::anyhow!(
                    "failed to persist refreshed session token"
                ))
            })?;

            let mut context = AuthEventContext::default();
            context.auth_session_id = Some(session_record_id);
            self.publish_events(events, context).await?;

            let refresh_token = record.token.rotate(Duration::days(30)).map_err(|_| {
                AuthenticationError::DatabaseError(anyhow::anyhow!(
                    "failed to rotate refresh token"
                ))
            })?;

            let refresh_hash = self.crypto.hash_token(refresh_token.as_str());
            let refresh_generation = i32::try_from(refresh_token.generation()).map_err(|_| {
                AuthenticationError::DatabaseError(anyhow::anyhow!(
                    "refresh token generation overflow"
                ))
            })?;

            let refresh_record_id = self
                .refresh_repo
                .insert_refresh_token(
                    &refresh_hash,
                    record.user_id,
                    Some(device_session_id),
                    Some(session_record_id),
                    refresh_token.issued_at(),
                    refresh_token.expires_at(),
                    refresh_token.family_id(),
                    refresh_generation,
                )
                .await?;

            Ok(TokenBundle {
                session_token,
                refresh_token,
                session_record_id,
                refresh_record_id,
                device_session_id: Some(device_session_id),
                user_id: record.user_id,
                scope: SessionScope::Playback,
            })
        } else {
            let session_token = SessionToken::generate(Duration::hours(24)).map_err(|_| {
                AuthenticationError::DatabaseError(anyhow::anyhow!(
                    "failed to generate session token"
                ))
            })?;

            let session_hash = self.crypto.hash_token(session_token.as_str());
            let session_record_id = self
                .session_store
                .insert_session(
                    record.user_id,
                    None,
                    SessionScope::Full,
                    &session_hash,
                    session_token.created_at(),
                    session_token.expires_at(),
                )
                .await?;

            let refresh_token = record.token.rotate(Duration::days(30)).map_err(|_| {
                AuthenticationError::DatabaseError(anyhow::anyhow!(
                    "failed to rotate refresh token"
                ))
            })?;

            let refresh_hash = self.crypto.hash_token(refresh_token.as_str());
            let refresh_generation = i32::try_from(refresh_token.generation()).map_err(|_| {
                AuthenticationError::DatabaseError(anyhow::anyhow!(
                    "refresh token generation overflow"
                ))
            })?;

            let refresh_record_id = self
                .refresh_repo
                .insert_refresh_token(
                    &refresh_hash,
                    record.user_id,
                    None,
                    Some(session_record_id),
                    refresh_token.issued_at(),
                    refresh_token.expires_at(),
                    refresh_token.family_id(),
                    refresh_generation,
                )
                .await?;

            Ok(TokenBundle {
                session_token,
                refresh_token,
                session_record_id,
                refresh_record_id,
                device_session_id: None,
                user_id: record.user_id,
                scope: SessionScope::Full,
            })
        }
    }

    pub async fn authenticate_with_pin_session(
        &self,
        device_session_id: Uuid,
        pin: &str,
    ) -> Result<TokenBundle, AuthenticationError> {
        let mut session = self
            .session_repo
            .find_by_id(device_session_id)
            .await?
            .ok_or(AuthenticationError::DeviceNotFound)?;

        let user_id = session.user_id();

        let auth_result = session.authenticate_with_pin(pin, 3, Duration::hours(24));

        let session_token = match auth_result {
            Ok(token) => token,
            Err(err) => {
                let events = session.take_events();
                self.session_repo.save(&session).await?;
                self.publish_events(events, AuthEventContext::default())
                    .await?;
                return Err(map_device_pin_error(err));
            }
        };

        let persisted_hash = self.crypto.hash_token(session_token.as_str());
        let persisted_token = SessionToken::from_value(
            persisted_hash,
            session_token.created_at(),
            session_token.expires_at(),
        )
        .map_err(|_| {
            AuthenticationError::DatabaseError(anyhow::anyhow!(
                "failed to construct persisted session token"
            ))
        })?;
        session.set_persisted_token(Some(persisted_token));

        let events = session.take_events();

        let session_record_id = self.session_repo.save(&session).await?.ok_or_else(|| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("failed to persist session token"))
        })?;

        let mut context = AuthEventContext::default();
        context.auth_session_id = Some(session_record_id);
        self.publish_events(events, context).await?;

        let refresh_token = RefreshToken::generate(Duration::days(30)).map_err(|_| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("failed to generate refresh token"))
        })?;

        let refresh_hash = self.crypto.hash_token(refresh_token.as_str());
        let refresh_generation = i32::try_from(refresh_token.generation()).map_err(|_| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("refresh token generation overflow"))
        })?;

        let refresh_record_id = self
            .refresh_repo
            .insert_refresh_token(
                &refresh_hash,
                user_id,
                Some(device_session_id),
                Some(session_record_id),
                refresh_token.issued_at(),
                refresh_token.expires_at(),
                refresh_token.family_id(),
                refresh_generation,
            )
            .await?;

        Ok(TokenBundle {
            session_token,
            refresh_token,
            session_record_id,
            refresh_record_id,
            device_session_id: Some(device_session_id),
            user_id,
            scope: SessionScope::Playback,
        })
    }

    /// Register a new device for a user
    pub async fn register_device(
        &self,
        user_id: Uuid,
        device_fingerprint: DeviceFingerprint,
        device_name: String,
        pin: String,
    ) -> Result<DeviceSession, AuthenticationError> {
        // Check if device is already registered
        if let Some(_existing) = self
            .session_repo
            .find_by_user_and_fingerprint(user_id, &device_fingerprint)
            .await?
        {
            return Err(AuthenticationError::InvalidCredentials); // Device already exists
        }

        // Create new device session
        let mut session = DeviceSession::new(user_id, device_fingerprint, device_name);

        // Set the PIN to make device trusted
        let policy = PinPolicy::default();
        session
            .set_pin(pin, &policy)
            .map_err(|_| AuthenticationError::InvalidPin)?;

        let events = session.take_events();
        self.session_repo.save(&session).await?;
        self.publish_events(events, AuthEventContext::default())
            .await?;

        Ok(session)
    }

    /// Revoke a specific device
    pub async fn revoke_device(
        &self,
        user_id: Uuid,
        device_fingerprint: &DeviceFingerprint,
    ) -> Result<(), AuthenticationError> {
        // Find the device session
        let mut session = self
            .session_repo
            .find_by_user_and_fingerprint(user_id, device_fingerprint)
            .await?
            .ok_or(AuthenticationError::DeviceNotFound)?;

        session
            .revoke()
            .map_err(|_| AuthenticationError::InvalidCredentials)?;

        let events = session.take_events();
        self.session_repo.save(&session).await?;
        self.publish_events(events, AuthEventContext::default())
            .await?;

        self.session_store
            .revoke_by_device(session.id(), RevocationReason::DeviceRevoked)
            .await?;
        self.refresh_repo
            .revoke_for_device(session.id(), RevocationReason::DeviceRevoked)
            .await?;

        Ok(())
    }

    /// Verify a session token is valid
    pub async fn verify_session(
        &self,
        session_id: Uuid,
        token: &SessionToken,
    ) -> Result<DeviceSession, AuthenticationError> {
        // Find the session
        let session = self
            .session_repo
            .find_by_id(session_id)
            .await?
            .ok_or(AuthenticationError::DeviceNotFound)?;

        // Check if the token matches and is valid
        match session.session_token() {
            Some(stored_token)
                if stored_token.secure_compare(token.as_str()) && stored_token.is_valid() =>
            {
                Ok(session)
            }
            Some(_) => Err(AuthenticationError::SessionExpired),
            None => Err(AuthenticationError::SessionExpired),
        }
    }

    async fn publish_events(
        &self,
        events: Vec<AuthEvent>,
        context: AuthEventContext,
    ) -> Result<(), AuthenticationError> {
        if let Some(repo) = self.event_repo.as_ref() {
            let logs = map_domain_events(events, &context);
            if !logs.is_empty() {
                repo.record(logs).await.map_err(AuthenticationError::from)?;
            }
        }
        Ok(())
    }
}

fn map_device_pin_error(err: DeviceSessionError) -> AuthenticationError {
    match err {
        DeviceSessionError::DeviceNotTrusted | DeviceSessionError::DeviceRevoked => {
            AuthenticationError::DeviceNotTrusted
        }
        DeviceSessionError::InvalidPin => AuthenticationError::InvalidPin,
        DeviceSessionError::TooManyFailedAttempts => AuthenticationError::TooManyFailedAttempts,
        _ => AuthenticationError::InvalidCredentials,
    }
}

fn map_user_auth_error(err: UserAuthenticationError) -> AuthenticationError {
    use AuthenticationError as AE;
    use UserAuthenticationError as U;

    match err {
        U::InvalidCredentials => AE::InvalidCredentials,
        U::AccountInactive => AE::InvalidCredentials,
        U::AccountLocked => AE::TooManyFailedAttempts,
        U::UserNotFound => AE::UserNotFound,
        other => AE::DatabaseError(anyhow::anyhow!("unexpected auth error: {other}")),
    }
}
