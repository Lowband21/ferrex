use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Domain events for the authentication bounded context
///
/// These events represent important state changes in the authentication
/// domain and can be used for:
/// - Event sourcing
/// - Integration with other bounded contexts
/// - Audit logging
/// - Real-time notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthEvent {
    /// A new device was registered
    DeviceRegistered {
        session_id: Uuid,
        user_id: Uuid,
        device_name: String,
        timestamp: DateTime<Utc>,
    },

    /// A device was trusted (PIN set)
    DeviceTrusted {
        session_id: Uuid,
        user_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// A device was revoked
    DeviceRevoked {
        session_id: Uuid,
        user_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// All devices for a user were revoked
    AllDevicesRevoked {
        user_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// PIN was set or updated
    PinSet {
        session_id: Uuid,
        user_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// PIN was removed from the device, returning it to pending status
    PinRemoved {
        session_id: Uuid,
        user_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// A new session was created
    SessionCreated {
        session_id: Uuid,
        user_id: Uuid,
        expires_at: DateTime<Utc>,
        timestamp: DateTime<Utc>,
    },

    /// A session was refreshed
    SessionRefreshed {
        session_id: Uuid,
        user_id: Uuid,
        expires_at: DateTime<Utc>,
        timestamp: DateTime<Utc>,
    },

    /// Authentication failed
    AuthenticationFailed {
        session_id: Uuid,
        user_id: Uuid,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    /// User authenticated with password
    PasswordAuthenticated {
        user_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// User's password was changed
    PasswordChanged {
        user_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// Account was locked
    AccountLocked {
        user_id: Uuid,
        locked_until: DateTime<Utc>,
        timestamp: DateTime<Utc>,
    },

    /// Account was unlocked
    AccountUnlocked {
        user_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// Account was deactivated
    AccountDeactivated {
        user_id: Uuid,
        timestamp: DateTime<Utc>,
    },
}

impl AuthEvent {
    /// Get the timestamp of the event
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::DeviceRegistered { timestamp, .. } => *timestamp,
            Self::DeviceTrusted { timestamp, .. } => *timestamp,
            Self::DeviceRevoked { timestamp, .. } => *timestamp,
            Self::AllDevicesRevoked { timestamp, .. } => *timestamp,
            Self::PinSet { timestamp, .. } => *timestamp,
            Self::PinRemoved { timestamp, .. } => *timestamp,
            Self::SessionCreated { timestamp, .. } => *timestamp,
            Self::SessionRefreshed { timestamp, .. } => *timestamp,
            Self::AuthenticationFailed { timestamp, .. } => *timestamp,
            Self::PasswordAuthenticated { timestamp, .. } => *timestamp,
            Self::PasswordChanged { timestamp, .. } => *timestamp,
            Self::AccountLocked { timestamp, .. } => *timestamp,
            Self::AccountUnlocked { timestamp, .. } => *timestamp,
            Self::AccountDeactivated { timestamp, .. } => *timestamp,
        }
    }

    /// Get the user ID associated with the event
    pub fn user_id(&self) -> Uuid {
        match self {
            Self::DeviceRegistered { user_id, .. } => *user_id,
            Self::DeviceTrusted { user_id, .. } => *user_id,
            Self::DeviceRevoked { user_id, .. } => *user_id,
            Self::AllDevicesRevoked { user_id, .. } => *user_id,
            Self::PinSet { user_id, .. } => *user_id,
            Self::PinRemoved { user_id, .. } => *user_id,
            Self::SessionCreated { user_id, .. } => *user_id,
            Self::SessionRefreshed { user_id, .. } => *user_id,
            Self::AuthenticationFailed { user_id, .. } => *user_id,
            Self::PasswordAuthenticated { user_id, .. } => *user_id,
            Self::PasswordChanged { user_id, .. } => *user_id,
            Self::AccountLocked { user_id, .. } => *user_id,
            Self::AccountUnlocked { user_id, .. } => *user_id,
            Self::AccountDeactivated { user_id, .. } => *user_id,
        }
    }

    /// Get the event type as a string
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::DeviceRegistered { .. } => "device_registered",
            Self::DeviceTrusted { .. } => "device_trusted",
            Self::DeviceRevoked { .. } => "device_revoked",
            Self::AllDevicesRevoked { .. } => "all_devices_revoked",
            Self::PinSet { .. } => "pin_set",
            Self::PinRemoved { .. } => "pin_removed",
            Self::SessionCreated { .. } => "session_created",
            Self::SessionRefreshed { .. } => "session_refreshed",
            Self::AuthenticationFailed { .. } => "authentication_failed",
            Self::PasswordAuthenticated { .. } => "password_authenticated",
            Self::PasswordChanged { .. } => "password_changed",
            Self::AccountLocked { .. } => "account_locked",
            Self::AccountUnlocked { .. } => "account_unlocked",
            Self::AccountDeactivated { .. } => "account_deactivated",
        }
    }
}

#[cfg(feature = "compat")]
pub type DomainEvent = AuthEvent;
