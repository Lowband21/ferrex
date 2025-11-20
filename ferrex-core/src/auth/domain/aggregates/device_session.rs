use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::auth::domain::events::AuthEvent;
use crate::auth::domain::value_objects::{DeviceFingerprint, SessionToken};

/// Errors that can occur with device sessions
#[derive(Debug, Error)]
pub enum DeviceSessionError {
    #[error("Device has been revoked")]
    DeviceRevoked,

    #[error("Session has expired")]
    SessionExpired,

    #[error("Invalid state transition")]
    InvalidStateTransition,

    #[error("Too many failed attempts")]
    TooManyFailedAttempts,

    #[error("PIN required but not set")]
    PinNotSet,

    #[error("Invalid PIN")]
    InvalidPin,

    #[error("Device not trusted")]
    DeviceNotTrusted,
}

/// Device trust status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStatus {
    /// Device is pending trust (needs PIN verification)
    Pending,

    /// Device is trusted and can authenticate
    Trusted,

    /// Device has been revoked and cannot authenticate
    Revoked,
}

/// Device session aggregate root
///
/// This aggregate manages the lifecycle of a device's authentication session,
/// including trust relationships, PIN management, and session tokens.
#[derive(Debug, Clone)]
pub struct DeviceSession {
    /// Unique session ID
    id: Uuid,

    /// User this session belongs to
    user_id: Uuid,

    /// Device fingerprint
    device_fingerprint: DeviceFingerprint,

    /// Human-readable device name
    device_name: String,

    /// Device-bound public key for possession validation (PEM/base64 format)
    device_public_key: Option<String>,

    /// Public key algorithm identifier (e.g., 'ed25519')
    device_key_alg: Option<String>,

    /// Current device status
    status: DeviceStatus,

    /// Whether the user currently has a PIN configured
    pin_configured: bool,

    /// Current session token
    session_token: Option<SessionToken>,

    /// Failed PIN attempts
    failed_attempts: u8,

    /// When the device was first registered
    created_at: DateTime<Utc>,

    /// Last activity timestamp
    last_activity: DateTime<Utc>,

    /// Domain events to be published
    events: Vec<AuthEvent>,
}

impl DeviceSession {
    /// Rehydrate a device session from persisted storage.
    pub(crate) fn hydrate(
        id: Uuid,
        user_id: Uuid,
        device_fingerprint: DeviceFingerprint,
        device_name: String,
        status: DeviceStatus,
        pin_configured: bool,
        session_token: Option<SessionToken>,
        failed_attempts: u8,
        created_at: DateTime<Utc>,
        last_activity: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            user_id,
            device_fingerprint,
            device_name,
            device_public_key: None,
            device_key_alg: None,
            status,
            pin_configured,
            session_token,
            failed_attempts,
            created_at,
            last_activity,
            events: Vec::new(),
        }
    }

    /// Create a new device session
    pub fn new(user_id: Uuid, device_fingerprint: DeviceFingerprint, device_name: String) -> Self {
        let now = Utc::now();
        let id = Uuid::now_v7();

        let mut session = Self {
            id,
            user_id,
            device_fingerprint,
            device_name: device_name.clone(),
            device_public_key: None,
            device_key_alg: None,
            status: DeviceStatus::Pending,
            pin_configured: false,
            session_token: None,
            failed_attempts: 0,
            created_at: now,
            last_activity: now,
            events: Vec::new(),
        };

        session.add_event(AuthEvent::DeviceRegistered {
            session_id: id,
            user_id,
            device_name,
            timestamp: now,
        });

        session
    }

    /// Attach a device public key and algorithm once registered
    pub fn set_device_public_key(&mut self, alg: impl Into<String>, key: impl Into<String>) {
        self.device_key_alg = Some(alg.into());
        self.device_public_key = Some(key.into());
        self.last_activity = Utc::now();
    }

    /// Mark the device as trusted after a PIN has been configured for the user.
    pub fn mark_trusted_after_pin_setup(&mut self) {
        if self.status == DeviceStatus::Pending {
            self.status = DeviceStatus::Trusted;
            self.add_event(AuthEvent::DeviceTrusted {
                session_id: self.id,
                user_id: self.user_id,
                timestamp: Utc::now(),
            });
        }

        self.pin_configured = true;
        self.failed_attempts = 0;
        self.last_activity = Utc::now();

        self.add_event(AuthEvent::PinSet {
            session_id: self.id,
            user_id: self.user_id,
            timestamp: Utc::now(),
        });
    }

    /// Remove the configured PIN association and return the device to a pending state.
    pub fn clear_pin_association(&mut self) -> Result<(), DeviceSessionError> {
        if self.status == DeviceStatus::Revoked {
            return Err(DeviceSessionError::DeviceRevoked);
        }

        if self.status != DeviceStatus::Pending {
            self.status = DeviceStatus::Pending;
        }

        self.session_token = None;
        self.failed_attempts = 0;
        self.last_activity = Utc::now();

        self.add_event(AuthEvent::PinRemoved {
            session_id: self.id,
            user_id: self.user_id,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Ensure the device can attempt PIN authentication.
    pub fn ensure_pin_available(&self, max_attempts: u8) -> Result<(), DeviceSessionError> {
        match self.status {
            DeviceStatus::Revoked => return Err(DeviceSessionError::DeviceRevoked),
            DeviceStatus::Pending => return Err(DeviceSessionError::DeviceNotTrusted),
            DeviceStatus::Trusted => {}
        }

        if !self.pin_configured {
            return Err(DeviceSessionError::PinNotSet);
        }

        if self.failed_attempts >= max_attempts {
            return Err(DeviceSessionError::TooManyFailedAttempts);
        }

        Ok(())
    }

    /// Record a failed PIN authentication attempt for this device.
    pub fn register_pin_failure(&mut self, max_attempts: u8) -> DeviceSessionError {
        self.failed_attempts = self.failed_attempts.saturating_add(1);
        self.last_activity = Utc::now();

        self.add_event(AuthEvent::AuthenticationFailed {
            session_id: self.id,
            user_id: self.user_id,
            reason: "Invalid PIN".to_string(),
            timestamp: Utc::now(),
        });

        if self.failed_attempts >= max_attempts {
            DeviceSessionError::TooManyFailedAttempts
        } else {
            DeviceSessionError::InvalidPin
        }
    }

    /// Issue a session token after a successful PIN verification.
    pub fn issue_pin_session(
        &mut self,
        session_lifetime: Duration,
    ) -> Result<SessionToken, DeviceSessionError> {
        if self.status != DeviceStatus::Trusted {
            return Err(DeviceSessionError::DeviceNotTrusted);
        }

        let token = SessionToken::generate(session_lifetime)
            .map_err(|_| DeviceSessionError::InvalidStateTransition)?;

        self.session_token = Some(token.clone());
        self.failed_attempts = 0;
        self.last_activity = Utc::now();

        self.add_event(AuthEvent::SessionCreated {
            session_id: self.id,
            user_id: self.user_id,
            expires_at: token.expires_at(),
            timestamp: Utc::now(),
        });

        Ok(token)
    }

    /// Reset failure counters after a successful PIN verification.
    pub fn record_pin_success(&mut self) {
        self.failed_attempts = 0;
        self.last_activity = Utc::now();
    }

    /// Refresh the session token if valid
    pub fn refresh_token(
        &mut self,
        session_lifetime: Duration,
    ) -> Result<SessionToken, DeviceSessionError> {
        // Check device status
        if self.status != DeviceStatus::Trusted {
            return Err(DeviceSessionError::DeviceNotTrusted);
        }

        // Check if current token is still valid
        let current_token = self
            .session_token
            .as_ref()
            .ok_or(DeviceSessionError::SessionExpired)?;

        if current_token.is_expired() {
            return Err(DeviceSessionError::SessionExpired);
        }

        // Generate new token
        let token = SessionToken::generate(session_lifetime)
            .map_err(|_| DeviceSessionError::InvalidStateTransition)?;

        self.session_token = Some(token.clone());

        self.add_event(AuthEvent::SessionRefreshed {
            session_id: self.id,
            user_id: self.user_id,
            expires_at: token.expires_at(),
            timestamp: Utc::now(),
        });

        Ok(token)
    }

    /// Revoke this device session
    pub fn revoke(&mut self) -> Result<(), DeviceSessionError> {
        if self.status == DeviceStatus::Revoked {
            return Ok(()); // Already revoked
        }

        self.status = DeviceStatus::Revoked;
        self.session_token = None;

        self.add_event(AuthEvent::DeviceRevoked {
            session_id: self.id,
            user_id: self.user_id,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Update last activity timestamp
    pub fn update_activity(&mut self) {
        self.last_activity = Utc::now();
    }

    /// Check if the session token is valid
    pub fn is_token_valid(&self) -> bool {
        self.session_token
            .as_ref()
            .map(|t| t.is_valid())
            .unwrap_or(false)
    }

    /// Add a domain event
    fn add_event(&mut self, event: AuthEvent) {
        self.events.push(event);
    }

    /// Take all pending events (for publishing)
    pub fn take_events(&mut self) -> Vec<AuthEvent> {
        std::mem::take(&mut self.events)
    }

    // Getters for read-only access
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn user_id(&self) -> Uuid {
        self.user_id
    }
    pub fn device_fingerprint(&self) -> &DeviceFingerprint {
        &self.device_fingerprint
    }
    pub fn device_name(&self) -> &str {
        &self.device_name
    }
    pub fn device_public_key(&self) -> Option<&str> { self.device_public_key.as_deref() }
    pub fn device_key_alg(&self) -> Option<&str> { self.device_key_alg.as_deref() }
    pub fn status(&self) -> DeviceStatus {
        self.status
    }
    pub fn has_pin(&self) -> bool {
        self.pin_configured
    }
    pub fn failed_attempts(&self) -> u8 {
        self.failed_attempts
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn last_activity(&self) -> DateTime<Utc> {
        self.last_activity
    }
    pub fn session_token(&self) -> Option<&SessionToken> {
        self.session_token.as_ref()
    }

    /// Whether the device session is currently trusted.
    pub fn is_trusted(&self) -> bool {
        self.status == DeviceStatus::Trusted
    }

    /// Whether the device session has been revoked.
    pub fn is_revoked(&self) -> bool {
        self.status == DeviceStatus::Revoked
    }

    /// Replace the in-memory session token (typically with the persisted hash)
    pub fn set_persisted_token(&mut self, token: Option<SessionToken>) {
        self.session_token = token;
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_lifecycle() {
        let fingerprint = DeviceFingerprint::new(
            "Linux".to_string(),
            Some("Intel".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let mut session =
            DeviceSession::new(Uuid::now_v7(), fingerprint, "Test Device".to_string());

        // Initially pending
        assert_eq!(session.status(), DeviceStatus::Pending);
        assert!(!session.has_pin());

        // Mark trusted after PIN setup
        session.mark_trusted_after_pin_setup();

        // Now trusted
        assert_eq!(session.status(), DeviceStatus::Trusted);
        assert!(session.has_pin());

        // Authenticate (server verification happens upstream)
        session.ensure_pin_available(3).unwrap();
        let token = session
            .issue_pin_session(Duration::hours(1))
            .unwrap();
        assert!(token.is_valid());

        // Simulate a failure and ensure lockout rules apply
        session.ensure_pin_available(3).unwrap();
        assert!(matches!(
            session.register_pin_failure(3),
            DeviceSessionError::InvalidPin
        ));

        session.clear_pin_association().unwrap();
        assert_eq!(session.status(), DeviceStatus::Pending);
        assert!(!session.has_pin());

        // Revoke
        session.revoke().unwrap();
        assert_eq!(session.status(), DeviceStatus::Revoked);
    }
}
