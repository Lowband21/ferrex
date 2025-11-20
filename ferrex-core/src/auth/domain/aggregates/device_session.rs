use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::auth::domain::events::DomainEvent;
use crate::auth::domain::value_objects::{DeviceFingerprint, PinCode, PinPolicy, SessionToken};

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

    /// Current device status
    status: DeviceStatus,

    /// Optional PIN for this device
    pin: Option<PinCode>,

    /// Current session token
    session_token: Option<SessionToken>,

    /// Failed PIN attempts
    failed_attempts: u8,

    /// When the device was first registered
    created_at: DateTime<Utc>,

    /// Last activity timestamp
    last_activity: DateTime<Utc>,

    /// Domain events to be published
    events: Vec<DomainEvent>,
}

impl DeviceSession {
    /// Create a new device session
    pub fn new(user_id: Uuid, device_fingerprint: DeviceFingerprint, device_name: String) -> Self {
        let now = Utc::now();
        let id = Uuid::now_v7();

        let mut session = Self {
            id,
            user_id,
            device_fingerprint,
            device_name: device_name.clone(),
            status: DeviceStatus::Pending,
            pin: None,
            session_token: None,
            failed_attempts: 0,
            created_at: now,
            last_activity: now,
            events: Vec::new(),
        };

        session.add_event(DomainEvent::DeviceRegistered {
            session_id: id,
            user_id,
            device_name,
            timestamp: now,
        });

        session
    }

    /// Set or update the PIN for this device
    pub fn set_pin(
        &mut self,
        pin_value: String,
        policy: &PinPolicy,
    ) -> Result<(), DeviceSessionError> {
        // Create the PIN with validation
        let pin = PinCode::new(pin_value, policy).map_err(|_| DeviceSessionError::InvalidPin)?;

        self.pin = Some(pin);
        self.failed_attempts = 0;

        // If device was pending, it's now trusted
        if self.status == DeviceStatus::Pending {
            self.status = DeviceStatus::Trusted;
            self.add_event(DomainEvent::DeviceTrusted {
                session_id: self.id,
                user_id: self.user_id,
                timestamp: Utc::now(),
            });
        }

        self.add_event(DomainEvent::PinSet {
            session_id: self.id,
            user_id: self.user_id,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Verify PIN and create a new session token
    pub fn authenticate_with_pin(
        &mut self,
        pin_value: &str,
        max_attempts: u8,
        session_lifetime: Duration,
    ) -> Result<SessionToken, DeviceSessionError> {
        // Check device status
        match self.status {
            DeviceStatus::Revoked => return Err(DeviceSessionError::DeviceRevoked),
            DeviceStatus::Pending => return Err(DeviceSessionError::DeviceNotTrusted),
            DeviceStatus::Trusted => {}
        }

        // Check if PIN is set
        let pin = self.pin.as_ref().ok_or(DeviceSessionError::PinNotSet)?;

        // Check failed attempts
        if self.failed_attempts >= max_attempts {
            return Err(DeviceSessionError::TooManyFailedAttempts);
        }

        // Verify PIN
        let valid = pin
            .verify(pin_value)
            .map_err(|_| DeviceSessionError::InvalidPin)?;

        if !valid {
            self.failed_attempts += 1;
            self.add_event(DomainEvent::AuthenticationFailed {
                session_id: self.id,
                user_id: self.user_id,
                reason: "Invalid PIN".to_string(),
                timestamp: Utc::now(),
            });
            return Err(DeviceSessionError::InvalidPin);
        }

        // Reset failed attempts
        self.failed_attempts = 0;

        // Generate new session token
        let token = SessionToken::generate(session_lifetime)
            .map_err(|_| DeviceSessionError::InvalidStateTransition)?;

        self.session_token = Some(token.clone());
        self.last_activity = Utc::now();

        self.add_event(DomainEvent::SessionCreated {
            session_id: self.id,
            user_id: self.user_id,
            expires_at: token.expires_at(),
            timestamp: Utc::now(),
        });

        Ok(token)
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
        self.last_activity = Utc::now();

        self.add_event(DomainEvent::SessionRefreshed {
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

        self.add_event(DomainEvent::DeviceRevoked {
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
    fn add_event(&mut self, event: DomainEvent) {
        self.events.push(event);
    }

    /// Take all pending events (for publishing)
    pub fn take_events(&mut self) -> Vec<DomainEvent> {
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
    pub fn status(&self) -> DeviceStatus {
        self.status
    }
    pub fn has_pin(&self) -> bool {
        self.pin.is_some()
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

        // Set PIN
        let policy = PinPolicy::default();
        session.set_pin("5823".to_string(), &policy).unwrap();

        // Now trusted
        assert_eq!(session.status(), DeviceStatus::Trusted);
        assert!(session.has_pin());

        // Authenticate
        let token = session
            .authenticate_with_pin("5823", 3, Duration::hours(1))
            .unwrap();
        assert!(token.is_valid());

        // Revoke
        session.revoke().unwrap();
        assert_eq!(session.status(), DeviceStatus::Revoked);
    }
}
