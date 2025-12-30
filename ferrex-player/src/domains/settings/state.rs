use ferrex_core::player_prelude::UserScale;

use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::ui::views::settings::device_management::DeviceManagementState;

use serde::{Deserialize, Serialize};

/// Settings domain state
#[derive(Debug, Clone, Default)]
pub struct SettingsState {
    /// Current section in the unified settings sidebar (new)
    pub current_section: SettingsSection,

    // /// Current settings view (legacy - being replaced by SettingsSection)
    // pub current_view: SettingsView,
    pub security: SecurityState,
    pub profile: ProfileState,
    pub preferences: PreferencesState,
    pub device_management_state: DeviceManagementState,
}

// =============================================================================
// New Settings Section Architecture
// =============================================================================

/// Settings section for the unified settings sidebar
///
/// This enum represents all available settings sections in the new unified
/// settings view. Each variant corresponds to a sub-domain in sections/.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SettingsSection {
    // User sections (always visible)
    #[default]
    Profile,
    Playback,
    Display,
    Theme,
    Performance,
    Security,
    Devices,

    // Admin sections (permission-gated)
    Libraries,
    Users,
    Server,
}

impl SettingsSection {
    /// Check if this is an admin-only section
    pub fn is_admin(&self) -> bool {
        matches!(self, Self::Libraries | Self::Users | Self::Server)
    }

    /// Get the display label for this section
    pub fn label(&self) -> &'static str {
        match self {
            Self::Profile => "Profile",
            Self::Playback => "Playback",
            Self::Display => "Display",
            Self::Theme => "Theme",
            Self::Performance => "Performance",
            Self::Security => "Security",
            Self::Devices => "Devices",
            Self::Libraries => "Libraries",
            Self::Users => "Users",
            Self::Server => "Server",
        }
    }

    /// Get all user sections (always visible)
    pub const fn user_sections() -> &'static [SettingsSection] {
        &[
            Self::Profile,
            Self::Playback,
            Self::Display,
            Self::Theme,
            Self::Performance,
            Self::Security,
            Self::Devices,
        ]
    }

    /// Get all admin sections (permission-gated)
    pub const fn admin_sections() -> &'static [SettingsSection] {
        &[Self::Libraries, Self::Users, Self::Server]
    }

    /// Get all sections
    pub const fn all() -> &'static [SettingsSection] {
        &[
            Self::Profile,
            Self::Playback,
            Self::Display,
            Self::Theme,
            Self::Performance,
            Self::Security,
            Self::Devices,
            Self::Libraries,
            Self::Users,
            Self::Server,
        ]
    }
}

impl std::fmt::Display for SettingsSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Security settings state
#[derive(Debug, Clone)]
pub struct SecurityState {
    // Password change fields
    pub password_current: SecureCredential,
    pub password_new: SecureCredential,
    pub password_confirm: SecureCredential,
    pub password_error: Option<String>,
    pub password_loading: bool,
    pub password_show: bool,
    pub showing_password_change: bool,

    // PIN change fields
    pub pin_current: SecureCredential,
    pub pin_new: SecureCredential,
    pub pin_confirm: SecureCredential,
    pub pin_error: Option<String>,
    pub pin_loading: bool,
    pub showing_pin_change: bool,

    // Device has PIN?
    pub has_pin: bool,
    pub checking_pin_status: bool,
}

/// Password change state
#[derive(Debug, Clone)]
pub struct PasswordChangeState {
    pub current: String,
    pub new: String,
    pub confirm: String,
    pub show_password: bool,
    pub loading: bool,
    pub error: Option<String>,
}

/// PIN change state
#[derive(Debug, Clone)]
pub struct PinChangeState {
    pub current: String, // Only needed when changing existing PIN
    pub new: String,
    pub confirm: String,
    pub loading: bool,
    pub error: Option<String>,
    pub is_new_pin: bool, // true = setting new PIN, false = changing existing
}

/// Profile settings state
#[derive(Debug, Clone, Default)]
pub struct ProfileState {
    pub display_name: String,
    pub email: String,
    pub loading: bool,
    pub error: Option<String>,
    pub success_message: Option<String>,
}

/// Preferences state
#[derive(Debug, Clone)]
pub struct PreferencesState {
    pub auto_login_enabled: bool,
    pub theme: ThemePreference,
    /// UI grid size / scale preference
    pub user_scale: UserScale,
    pub loading: bool,
    pub error: Option<String>,
}

impl Default for PreferencesState {
    fn default() -> Self {
        Self {
            auto_login_enabled: false,
            theme: ThemePreference::default(),
            user_scale: UserScale::Medium, // Default to medium scale
            loading: false,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl Default for SecurityState {
    fn default() -> Self {
        Self {
            password_current: SecureCredential::from(""),
            password_new: SecureCredential::from(""),
            password_confirm: SecureCredential::from(""),
            password_error: None,
            password_loading: false,
            password_show: false,
            showing_password_change: false,
            pin_current: SecureCredential::from(""),
            pin_new: SecureCredential::from(""),
            pin_confirm: SecureCredential::from(""),
            pin_error: None,
            pin_loading: false,
            showing_pin_change: false,
            has_pin: false,
            checking_pin_status: false,
        }
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl SecurityState {
    /// Clear all sensitive data
    pub fn clear_sensitive_data(&mut self) {
        self.password_current = SecureCredential::from("");
        self.password_new = SecureCredential::from("");
        self.password_confirm = SecureCredential::from("");
        self.pin_current = SecureCredential::from("");
        self.pin_new = SecureCredential::from("");
        self.pin_confirm = SecureCredential::from("");
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl Default for PasswordChangeState {
    fn default() -> Self {
        Self::new()
    }
}

impl PasswordChangeState {
    pub fn new() -> Self {
        Self {
            current: String::new(),
            new: String::new(),
            confirm: String::new(),
            show_password: false,
            loading: false,
            error: None,
        }
    }

    /// Validate password change inputs
    pub fn validate(&self) -> Result<(), String> {
        if self.current.is_empty() {
            return Err("Current password is required".to_string());
        }
        if self.new.is_empty() {
            return Err("New password is required".to_string());
        }
        if self.new.len() < 8 {
            return Err("Password must be at least 8 characters".to_string());
        }
        if self.new != self.confirm {
            return Err("Passwords do not match".to_string());
        }
        if self.current == self.new {
            return Err("New password must be different from current password"
                .to_string());
        }

        // Check password complexity
        let has_upper = self.new.chars().any(|c| c.is_uppercase());
        let has_lower = self.new.chars().any(|c| c.is_lowercase());
        let has_digit = self.new.chars().any(|c| c.is_ascii_digit());

        if !has_upper || !has_lower || !has_digit {
            return Err(
                "Password must contain uppercase, lowercase, and numbers"
                    .to_string(),
            );
        }

        Ok(())
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl PinChangeState {
    pub fn new(is_new_pin: bool) -> Self {
        Self {
            current: String::new(),
            new: String::new(),
            confirm: String::new(),
            loading: false,
            error: None,
            is_new_pin,
        }
    }

    /// Validate PIN inputs
    pub fn validate(&self) -> Result<(), String> {
        if !self.is_new_pin && self.current.is_empty() {
            return Err("Current PIN is required".to_string());
        }
        if self.new.is_empty() {
            return Err("New PIN is required".to_string());
        }
        if self.new.len() != 4 {
            return Err("PIN must be exactly 4 digits".to_string());
        }
        if !self.new.chars().all(|c| c.is_ascii_digit()) {
            return Err("PIN must contain only digits".to_string());
        }
        if self.new != self.confirm {
            return Err("PINs do not match".to_string());
        }
        if !self.is_new_pin && self.current == self.new {
            return Err(
                "New PIN must be different from current PIN".to_string()
            );
        }

        Ok(())
    }
}
