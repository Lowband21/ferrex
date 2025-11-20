use anyhow::Result;
use chrono::Duration;
use std::fmt;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::AuthCrypto;
use crate::auth::domain::aggregates::{
    DeviceSession, DeviceSessionError, UserAuthentication, UserAuthenticationError,
};
use crate::auth::domain::repositories::{
    AuthSessionRepository, DeviceSessionRepository, RefreshTokenRecord, RefreshTokenRepository,
    UserAuthenticationRepository,
};
use crate::auth::domain::value_objects::{
    DeviceFingerprint, PinPolicy, RefreshToken, SessionToken,
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
}

#[derive(Debug)]
pub struct TokenBundle {
    pub session_token: SessionToken,
    pub refresh_token: RefreshToken,
    pub session_record_id: Uuid,
    pub refresh_record_id: Uuid,
    pub device_session_id: Option<Uuid>,
    pub user_id: Uuid,
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
        }
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

        match user.authenticate_password(password, &self.crypto) {
            Ok(()) => {
                self.user_repo.save(&user).await?;
                Ok(user)
            }
            Err(err) => {
                self.user_repo.save(&user).await?;
                Err(map_user_auth_error(err))
            }
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
        let session_record_id = self
            .session_store
            .insert_session(
                user.user_id(),
                None,
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
        })
    }

    pub async fn authenticate_device_with_pin(
        &self,
        user_id: Uuid,
        device_fingerprint: &DeviceFingerprint,
        pin: &str,
    ) -> Result<TokenBundle, AuthenticationError> {
        // Find the device session
        let mut session = self
            .session_repo
            .find_by_user_and_fingerprint(user_id, device_fingerprint)
            .await?
            .ok_or(AuthenticationError::DeviceNotFound)?;

        // Authenticate with PIN using domain logic
        let session_token = session
            .authenticate_with_pin(pin, 3, Duration::hours(24))
            .map_err(|e| match e {
                DeviceSessionError::DeviceNotTrusted => AuthenticationError::DeviceNotTrusted,
                DeviceSessionError::InvalidPin => AuthenticationError::InvalidPin,
                DeviceSessionError::TooManyFailedAttempts => {
                    AuthenticationError::TooManyFailedAttempts
                }
                _ => AuthenticationError::InvalidCredentials,
            })?;

        // Hash the token for storage and replace the in-memory representation so
        // the repository can persist the correct digest.
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

        // Save the updated session and capture the auth_sessions row id
        let session_record_id = self.session_repo.save(&session).await?.ok_or_else(|| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("failed to persist session token"))
        })?;

        // Issue a refresh token bound to the same device session
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

        if record.revoked || record.token.is_expired() {
            return Err(AuthenticationError::SessionExpired);
        }

        let device_session_id = record
            .device_session_id
            .ok_or(AuthenticationError::DeviceNotFound)?;

        self.refresh_repo.mark_used(record.id, "rotation").await?;

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

            let session_record_id = self.session_repo.save(&session).await?.ok_or_else(|| {
                AuthenticationError::DatabaseError(anyhow::anyhow!(
                    "failed to persist refreshed session token"
                ))
            })?;

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

        let session_token = session
            .authenticate_with_pin(pin, 3, Duration::hours(24))
            .map_err(|e| match e {
                DeviceSessionError::DeviceNotTrusted => AuthenticationError::DeviceNotTrusted,
                DeviceSessionError::InvalidPin => AuthenticationError::InvalidPin,
                DeviceSessionError::TooManyFailedAttempts => {
                    AuthenticationError::TooManyFailedAttempts
                }
                _ => AuthenticationError::InvalidCredentials,
            })?;

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

        let session_record_id = self.session_repo.save(&session).await?.ok_or_else(|| {
            AuthenticationError::DatabaseError(anyhow::anyhow!("failed to persist session token"))
        })?;

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

        // Save the session
        let _ = self.session_repo.save(&session).await?;

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

        // Revoke the device using domain logic
        session
            .revoke()
            .map_err(|_| AuthenticationError::InvalidCredentials)?;

        // Save the updated session
        let _ = self.session_repo.save(&session).await?;

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
