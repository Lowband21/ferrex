use serde::{Deserialize, Serialize};

/// Settings domain state
#[derive(Debug, Clone, Default)]
pub struct SettingsState {
    pub current_view: SettingsView,
    pub security: SecurityState,
    pub profile: ProfileState,
    pub preferences: PreferencesState,
}

/// Current settings view
#[derive(Debug, Clone, Default, PartialEq)]
pub enum SettingsView {
    #[default]
    Main,
    Profile,
    Preferences,
    Security,
}

/// Security settings state
#[derive(Debug, Clone, Default)]
pub struct SecurityState {
    // Password change modal
    pub password_modal: Option<PasswordChangeState>,
    
    // PIN management modal
    pub pin_modal: Option<PinChangeState>,
    
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
    pub current: String,  // Only needed when changing existing PIN
    pub new: String,
    pub confirm: String,
    pub loading: bool,
    pub error: Option<String>,
    pub is_new_pin: bool,  // true = setting new PIN, false = changing existing
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
#[derive(Debug, Clone, Default)]
pub struct PreferencesState {
    pub auto_login_enabled: bool,
    pub theme: ThemePreference,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl SecurityState {
    /// Clear all sensitive data
    pub fn clear_sensitive_data(&mut self) {
        if let Some(modal) = &mut self.password_modal {
            modal.current.clear();
            modal.new.clear();
            modal.confirm.clear();
        }
        if let Some(modal) = &mut self.pin_modal {
            modal.current.clear();
            modal.new.clear();
            modal.confirm.clear();
        }
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
            return Err("New password must be different from current password".to_string());
        }
        
        // Check password complexity
        let has_upper = self.new.chars().any(|c| c.is_uppercase());
        let has_lower = self.new.chars().any(|c| c.is_lowercase());
        let has_digit = self.new.chars().any(|c| c.is_digit(10));
        
        if !has_upper || !has_lower || !has_digit {
            return Err("Password must contain uppercase, lowercase, and numbers".to_string());
        }
        
        Ok(())
    }
}

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
        if !self.new.chars().all(|c| c.is_digit(10)) {
            return Err("PIN must contain only digits".to_string());
        }
        if self.new != self.confirm {
            return Err("PINs do not match".to_string());
        }
        if !self.is_new_pin && self.current == self.new {
            return Err("New PIN must be different from current PIN".to_string());
        }
        
        Ok(())
    }
}