//! Authentication module for device-based auth and PIN support
//!
//! This module provides a secure device-based authentication system that allows
//! users to authenticate with passwords initially, then use PINs for convenience
//! on trusted devices.

pub mod crypto;
pub mod device;
pub mod domain;
#[cfg(feature = "database")]
pub mod infrastructure;
pub mod pin;
pub mod policy;
pub mod rate_limit;
pub mod session;
pub mod state;
pub mod state_machine;

pub use crypto::{AuthCrypto, AuthCryptoError};
pub use device::*;
#[cfg(feature = "database")]
pub use pin::*;
pub use policy::{AuthSecuritySettings, PasswordPolicy, PasswordPolicyCheck, PasswordPolicyRule};
// Re-export session types with explicit naming to avoid conflicts
pub use session::{
    CreateSessionRequest, CreateSessionResponse, ListSessionsRequest, RevokeSessionRequest,
    SessionActivity, SessionConfig, SessionSummary, SessionValidationResult,
    generate_session_token,
};
// Export session DeviceSession with alias to avoid conflict with domain DeviceSession
pub use session::DeviceSession as SessionDeviceSession;
pub use state::*;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Authentication context for various auth methods
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub device_info: Option<DeviceInfo>,
}

/// Result of authentication attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResult {
    pub user_id: Uuid,
    pub session_token: String,
    pub device_registration: Option<DeviceRegistration>,
    pub requires_pin_setup: bool,
}

/// Authentication method trait
#[async_trait::async_trait]
pub trait AuthenticationMethod {
    async fn authenticate(&self, ctx: &AuthContext) -> Result<AuthResult, AuthError>;
}

/// Errors that can occur during authentication
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum AuthError {
    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Device not trusted")]
    DeviceNotTrusted,

    #[error("Too many failed attempts")]
    TooManyAttempts { locked_until: i64 },

    #[error("PIN required but not set")]
    PinNotSet,

    #[error("Invalid PIN")]
    InvalidPin { attempts_remaining: u8 },

    #[error("Device revoked")]
    DeviceRevoked,

    #[error("Session expired")]
    SessionExpired,

    #[error("Internal error")]
    InternalError,
}

/// Authentication event types for audit logging
#[cfg_attr(feature = "database", derive(sqlx::Type))]
#[cfg_attr(
    feature = "database",
    sqlx(type_name = "auth_event_type", rename_all = "snake_case")
)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthEventType {
    #[serde(rename = "password_login_success")]
    PasswordLoginSuccess,
    #[serde(rename = "password_login_failure")]
    PasswordLoginFailure,
    #[serde(rename = "pin_login_success")]
    PinLoginSuccess,
    #[serde(rename = "pin_login_failure")]
    PinLoginFailure,
    #[serde(rename = "device_registered")]
    DeviceRegistered,
    #[serde(rename = "device_revoked")]
    DeviceRevoked,
    #[serde(rename = "pin_set")]
    PinSet,
    #[serde(rename = "pin_removed")]
    PinRemoved,
    #[serde(rename = "session_created")]
    SessionCreated,
    #[serde(rename = "session_revoked")]
    SessionRevoked,
    #[serde(rename = "auto_login")]
    AutoLogin,
}

impl AuthEventType {
    /// Convert to database string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PasswordLoginSuccess => "password_login_success",
            Self::PasswordLoginFailure => "password_login_failure",
            Self::PinLoginSuccess => "pin_login_success",
            Self::PinLoginFailure => "pin_login_failure",
            Self::DeviceRegistered => "device_registered",
            Self::DeviceRevoked => "device_revoked",
            Self::PinSet => "pin_set",
            Self::PinRemoved => "pin_removed",
            Self::SessionCreated => "session_created",
            Self::SessionRevoked => "session_revoked",
            Self::AutoLogin => "auto_login",
        }
    }

    /// Parse from database string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "password_login_success" => Some(Self::PasswordLoginSuccess),
            "password_login_failure" => Some(Self::PasswordLoginFailure),
            "pin_login_success" => Some(Self::PinLoginSuccess),
            "pin_login_failure" => Some(Self::PinLoginFailure),
            "device_registered" => Some(Self::DeviceRegistered),
            "device_revoked" => Some(Self::DeviceRevoked),
            "pin_set" => Some(Self::PinSet),
            "pin_removed" => Some(Self::PinRemoved),
            "session_created" => Some(Self::SessionCreated),
            "session_revoked" => Some(Self::SessionRevoked),
            "auto_login" => Some(Self::AutoLogin),
            _ => None,
        }
    }
}

/// Authentication event for audit logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthEvent {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub device_session_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub event_type: AuthEventType,
    pub success: bool,
    pub failure_reason: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
