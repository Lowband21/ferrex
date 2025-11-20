use anyhow::Result;
use chrono::Duration;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::domain::aggregates::{DeviceSession, DeviceSessionError, UserAuthentication};
use crate::auth::domain::repositories::{DeviceSessionRepository, UserAuthenticationRepository};
use crate::auth::domain::value_objects::{DeviceFingerprint, PinPolicy, SessionToken};

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
}

impl AuthenticationService {
    pub fn new(
        user_repo: Arc<dyn UserAuthenticationRepository>,
        session_repo: Arc<dyn DeviceSessionRepository>,
    ) -> Self {
        Self {
            user_repo,
            session_repo,
        }
    }

    pub async fn authenticate_user(
        &self,
        username: &str,
        password: &str,
    ) -> Result<UserAuthentication, AuthenticationError> {
        let user = self
            .user_repo
            .find_by_username(username)
            .await?
            .ok_or(AuthenticationError::UserNotFound)?;

        // For now, just return the user if found
        // TODO: Add password verification
        Ok(user)
    }

    /// Authenticate using PIN on a registered device
    pub async fn authenticate_with_pin(
        &self,
        user_id: Uuid,
        device_fingerprint: &DeviceFingerprint,
        pin: &str,
    ) -> Result<SessionToken, AuthenticationError> {
        // Find the device session
        let mut session = self
            .session_repo
            .find_by_user_and_fingerprint(user_id, device_fingerprint)
            .await?
            .ok_or(AuthenticationError::DeviceNotFound)?;

        // Authenticate with PIN using domain logic
        let token = session
            .authenticate_with_pin(pin, 3, Duration::hours(24))
            .map_err(|e| match e {
                DeviceSessionError::DeviceNotTrusted => AuthenticationError::DeviceNotTrusted,
                DeviceSessionError::InvalidPin => AuthenticationError::InvalidPin,
                DeviceSessionError::TooManyFailedAttempts => {
                    AuthenticationError::TooManyFailedAttempts
                }
                _ => AuthenticationError::InvalidCredentials,
            })?;

        // Save the updated session
        self.session_repo.save(&session).await?;

        Ok(token)
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
        self.session_repo.save(&session).await?;

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
        self.session_repo.save(&session).await?;

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
