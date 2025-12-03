//! Server section state (Admin)

use serde::{Deserialize, Serialize};

/// Server settings state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerState {
    // Session Policies subsection
    /// Access token lifetime in seconds (default: 900 = 15 min)
    pub session_access_token_lifetime_secs: u32,
    /// Refresh token lifetime in days (default: 30)
    pub session_refresh_token_lifetime_days: u32,
    /// Maximum concurrent sessions per user (None = unlimited)
    pub session_max_concurrent: Option<u32>,

    // Device Policies subsection
    /// Device trust duration in days (default: 30)
    pub device_trust_duration_days: u32,
    /// Maximum trusted devices per user (None = unlimited)
    pub device_max_trusted_per_user: Option<u32>,
    /// Require PIN for new device trust
    pub device_require_pin_for_new: bool,

    // Password Policies subsection
    /// Admin password policy
    pub password_admin_policy: PasswordPolicy,
    /// User password policy
    pub password_user_policy: PasswordPolicy,

    // Curated Content subsection (from constants::curated)
    /// Maximum items in curated carousels (default: 50)
    pub curated_max_carousel_items: usize,
    /// Head window for preview (default: 200)
    pub curated_head_window: usize,

    // UI state
    /// Whether settings are loading
    pub loading: bool,
    /// Error message from last operation
    pub error: Option<String>,
    /// Success message from last save
    pub success_message: Option<String>,
}

/// Password policy configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PasswordPolicy {
    /// Whether policy is enforced (vs advisory)
    pub enforce: bool,
    /// Minimum password length
    pub min_length: u16,
    /// Require uppercase letter
    pub require_uppercase: bool,
    /// Require lowercase letter
    pub require_lowercase: bool,
    /// Require number
    pub require_number: bool,
    /// Require special character
    pub require_special: bool,
}

impl ServerState {
    /// Create with sensible defaults
    pub fn with_defaults() -> Self {
        Self {
            // Session Policies
            session_access_token_lifetime_secs: 900,
            session_refresh_token_lifetime_days: 30,
            session_max_concurrent: None,

            // Device Policies
            device_trust_duration_days: 30,
            device_max_trusted_per_user: None,
            device_require_pin_for_new: false,

            // Password Policies
            password_admin_policy: PasswordPolicy {
                enforce: false,
                min_length: 8,
                require_uppercase: true,
                require_lowercase: true,
                require_number: true,
                require_special: false,
            },
            password_user_policy: PasswordPolicy {
                enforce: false,
                min_length: 8,
                require_uppercase: false,
                require_lowercase: false,
                require_number: false,
                require_special: false,
            },

            // Curated Content (matches constants::curated)
            curated_max_carousel_items: 50,
            curated_head_window: 200,

            // UI state
            loading: false,
            error: None,
            success_message: None,
        }
    }
}
