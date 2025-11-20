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
    AuthEventRepository, AuthSessionRecord, AuthSessionRepository, DeviceSessionRepository,
    RefreshTokenRepository, UserAuthenticationRepository,
};
use crate::auth::domain::value_objects::{
    DeviceFingerprint, RefreshToken, RevocationReason, SessionScope, SessionToken,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

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
    challenge_repo: Option<Arc<dyn crate::auth::domain::repositories::DeviceChallengeRepository>>,
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
            challenge_repo: None,
        }
    }

    pub fn with_event_repository(mut self, event_repo: Arc<dyn AuthEventRepository>) -> Self {
        self.event_repo = Some(event_repo);
        self
    }

    pub fn with_challenge_repository(
        mut self,
        repo: Arc<dyn crate::auth::domain::repositories::DeviceChallengeRepository>,
    ) -> Self {
        self.challenge_repo = Some(repo);
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

    /// Authenticate using a client-derived PIN proof on a registered device
    pub async fn authenticate_with_pin(
        &self,
        user_id: Uuid,
        device_fingerprint: &DeviceFingerprint,
        pin_proof: &str,
    ) -> Result<SessionToken, AuthenticationError> {
        let bundle = self
            .authenticate_device_with_pin(user_id, device_fingerprint, pin_proof)
            .await?;

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
                session_scope,
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

    /// Authenticate using a client-derived PIN proof with a device fingerprint
    pub async fn authenticate_device_with_pin(
        &self,
        user_id: Uuid,
        device_fingerprint: &DeviceFingerprint,
        pin_proof: &str,
    ) -> Result<TokenBundle, AuthenticationError> {
        // Load user and device session separately
        let mut user = self
            .user_repo
            .find_by_id(user_id)
            .await
            .map_err(AuthenticationError::from)?
            .ok_or(AuthenticationError::UserNotFound)?;

        let mut session = self
            .session_repo
            .find_by_user_and_fingerprint(user_id, device_fingerprint)
            .await?
            .ok_or(AuthenticationError::DeviceNotFound)?;

        // Enforce trust window (remember-me): require password after 30 days of inactivity
        if Utc::now() - session.last_activity() > Duration::days(30) {
            // Persist any state changes (none here) then reject
            return Err(AuthenticationError::DeviceNotTrusted);
        }

        // Enforce lockout gate then verify the user-level PIN proof
        if let Err(err) = session.ensure_pin_available(3) {
            let events = session.take_events();
            self.session_repo.save(&session).await?;
            self.publish_events(events, AuthEventContext::default()).await?;
            return Err(map_device_pin_error(err));
        }

        let verified = user
            .verify_user_pin(pin_proof, &self.crypto)
            .map_err(|_| AuthenticationError::InvalidPin)?;

        let session_token = if verified {
            session.record_pin_success();
            session
                .issue_pin_session(Duration::hours(24))
                .map_err(|_| AuthenticationError::InvalidCredentials)?
        } else {
            let err = session.register_pin_failure(3);
            let events = session.take_events();
            // persist device changes (failed attempt)
            self.session_repo.save(&session).await?;
            self.publish_events(events, AuthEventContext::default()).await?;
            return Err(map_device_pin_error(err));
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
                SessionScope::Playback,
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

        if record.origin_scope == SessionScope::Playback && record.device_session_id.is_none() {
            // Sticky scope enforcement: playback-origin refresh must remain tied to a device
            return Err(AuthenticationError::InvalidCredentials);
        }

        if let Some(device_session_id) = record.device_session_id {
            let mut session = self
                .session_repo
                .find_by_id(device_session_id)
                .await?
                .ok_or(AuthenticationError::DeviceNotFound)?;

            // Enforce trust window for playback-origin refreshes
            if record.origin_scope == SessionScope::Playback
                && Utc::now() - session.last_activity() > Duration::days(30)
            {
                return Err(AuthenticationError::DeviceNotTrusted);
            }

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
                    SessionScope::Playback,
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
                    SessionScope::Full,
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

    /// Authenticate using a client-derived PIN proof against a device session id
    pub async fn authenticate_with_pin_session(
        &self,
        device_session_id: Uuid,
        pin_proof: &str,
        challenge_id: Uuid,
        device_signature: &[u8],
    ) -> Result<TokenBundle, AuthenticationError> {
        let mut session = self
            .session_repo
            .find_by_id(device_session_id)
            .await?
            .ok_or(AuthenticationError::DeviceNotFound)?;

        let user_id = session.user_id();

        // Load user for verification
        let mut user = self
            .user_repo
            .find_by_id(user_id)
            .await
            .map_err(AuthenticationError::from)?
            .ok_or(AuthenticationError::UserNotFound)?;

        // Verify possession first if repo configured
        if let Some(challenges) = self.challenge_repo.as_ref() {
            use crate::auth::domain::repositories::DeviceChallengeRepository as _;
            // Atomically consume challenge if fresh
            let consumed = challenges
                .consume_if_fresh(challenge_id)
                .await
                .map_err(AuthenticationError::from)?
                .ok_or(AuthenticationError::InvalidCredentials)?;

            let (challenged_session, nonce) = consumed;
            if challenged_session != device_session_id {
                return Err(AuthenticationError::InvalidCredentials);
            }

            // Verify signature using device public key
            let (alg_opt, pk_opt) = (session.device_key_alg(), session.device_public_key());
            let (alg, pk) = match (alg_opt, pk_opt) {
                (Some(a), Some(k)) => (a, k),
                _ => return Err(AuthenticationError::DeviceNotTrusted),
            };

            let verified = verify_device_signature(
                alg,
                pk,
                challenge_id,
                &nonce,
                user_id,
                device_signature,
            )
            .map_err(|e| AuthenticationError::DatabaseError(anyhow::anyhow!(e)))?;
            if !verified {
                // Do not increment device failed_attempts for possession failures
                return Err(AuthenticationError::InvalidCredentials);
            }
        }

        // Enforce trust window (remember-me): require password after 30 days of inactivity
        if Utc::now() - session.last_activity() > Duration::days(30) {
            return Err(AuthenticationError::DeviceNotTrusted);
        }

        if let Err(err) = session.ensure_pin_available(3) {
            let events = session.take_events();
            self.session_repo.save(&session).await?;
            self.publish_events(events, AuthEventContext::default()).await?;
            return Err(map_device_pin_error(err));
        }

        let verified = user
            .verify_user_pin(pin_proof, &self.crypto)
            .map_err(|_| AuthenticationError::InvalidPin)?;

        let session_token = if verified {
            session.record_pin_success();
            session
                .issue_pin_session(Duration::hours(24))
                .map_err(|_| AuthenticationError::InvalidCredentials)?
        } else {
            let err = session.register_pin_failure(3);
            let events = session.take_events();
            self.session_repo.save(&session).await?;
            self.publish_events(events, AuthEventContext::default()).await?;
            return Err(map_device_pin_error(err));
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
                SessionScope::Playback,
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

    pub async fn find_session_by_id(
        &self,
        session_id: Uuid,
    ) -> Result<Option<AuthSessionRecord>, AuthenticationError> {
        self.session_store
            .find_by_id(session_id)
            .await
            .map_err(AuthenticationError::from)
    }

    pub async fn list_sessions_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<AuthSessionRecord>, AuthenticationError> {
        self.session_store
            .list_by_user(user_id)
            .await
            .map_err(AuthenticationError::from)
    }

    pub async fn revoke_session_by_id(
        &self,
        session_id: Uuid,
        reason: RevocationReason,
    ) -> Result<(), AuthenticationError> {
        self.session_store
            .revoke_by_id(session_id, reason)
            .await
            .map_err(AuthenticationError::from)?;
        self.refresh_repo
            .revoke_for_session(session_id, reason)
            .await
            .map_err(AuthenticationError::from)?;
        Ok(())
    }

    pub async fn revoke_all_sessions_for_user(
        &self,
        user_id: Uuid,
        reason: RevocationReason,
    ) -> Result<(), AuthenticationError> {
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

    /// Register a new device for a user
    // Note: device registration is handled by DeviceTrustService.

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

    /// Create a short-lived device possession challenge nonce
    pub async fn create_device_challenge(
        &self,
        device_session_id: Uuid,
        ttl_seconds: i64,
    ) -> Result<(Uuid, Vec<u8>), AuthenticationError> {
        use rand::RngCore;
        let repo = self
            .challenge_repo
            .as_ref()
            .ok_or_else(|| AuthenticationError::DatabaseError(anyhow::anyhow!(
                "device challenge repository not configured"
            )))?;

        // Ensure the device exists
        let _ = self
            .session_repo
            .find_by_id(device_session_id)
            .await?
            .ok_or(AuthenticationError::DeviceNotFound)?;

        let mut nonce = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut nonce);
        let expires_at = Utc::now() + Duration::seconds(ttl_seconds.max(30));

        let id = repo
            .insert_challenge(device_session_id, &nonce, expires_at)
            .await
            .map_err(AuthenticationError::from)?;
        Ok((id, nonce))
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

fn verify_device_signature(
    alg: &str,
    pk_b64: &str,
    challenge_id: Uuid,
    nonce: &[u8],
    user_id: Uuid,
    signature: &[u8],
) -> Result<bool, String> {
    match alg {
        "ed25519" => {
            use ed25519_dalek::{Signature, VerifyingKey};

            let pk_bytes = BASE64
                .decode(pk_b64)
                .map_err(|e| format!("invalid base64 public key: {e}"))?;
            if pk_bytes.len() != 32 {
                return Err(format!(
                    "unexpected ed25519 public key length: {}",
                    pk_bytes.len()
                ));
            }
            let vk = VerifyingKey::from_bytes(
                pk_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| "failed to parse verifying key")?,
            )
            .map_err(|e| format!("invalid verifying key: {e}"))?;

            // Construct message (v1): "Ferrex-PIN-v1" || challenge_id || nonce || user_id
            const CTX: &[u8] = b"Ferrex-PIN-v1";
            let mut msg = Vec::with_capacity(CTX.len() + 16 + nonce.len() + 16);
            msg.extend_from_slice(CTX);
            msg.extend_from_slice(challenge_id.as_bytes());
            msg.extend_from_slice(nonce);
            msg.extend_from_slice(user_id.as_bytes());

            let sig = Signature::from_slice(signature)
                .map_err(|e| format!("invalid signature bytes: {e}"))?;
            Ok(vk.verify_strict(&msg, &sig).is_ok())
        }
        other => Err(format!("unsupported device key algorithm: {other}")),
    }
}

impl AuthenticationService {
    /// Verify device possession by atomically consuming the given challenge and
    /// checking the provided signature against the stored device public key.
    pub async fn verify_device_possession(
        &self,
        device_session_id: Uuid,
        challenge_id: Uuid,
        device_signature: &[u8],
    ) -> Result<(), AuthenticationError> {
        let session = self
            .session_repo
            .find_by_id(device_session_id)
            .await?
            .ok_or(AuthenticationError::DeviceNotFound)?;

        let user_id = session.user_id();
        let repo = self
            .challenge_repo
            .as_ref()
            .ok_or_else(|| AuthenticationError::DatabaseError(anyhow::anyhow!(
                "device challenge repository not configured"
            )))?;
        let consumed = repo
            .consume_if_fresh(challenge_id)
            .await
            .map_err(AuthenticationError::from)?
            .ok_or(AuthenticationError::InvalidCredentials)?;
        let (challenged_session, nonce) = consumed;
        if challenged_session != device_session_id {
            return Err(AuthenticationError::InvalidCredentials);
        }

        let (alg_opt, pk_opt) = (session.device_key_alg(), session.device_public_key());
        let (alg, pk) = match (alg_opt, pk_opt) {
            (Some(a), Some(k)) => (a, k),
            _ => return Err(AuthenticationError::DeviceNotTrusted),
        };
        let verified = verify_device_signature(alg, pk, challenge_id, &nonce, user_id, device_signature)
        .map_err(|e| AuthenticationError::DatabaseError(anyhow::anyhow!(e)))?;
        if !verified {
            return Err(AuthenticationError::InvalidCredentials);
        }
        Ok(())
    }
}
