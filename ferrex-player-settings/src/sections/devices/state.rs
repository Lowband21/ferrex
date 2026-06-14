//! Devices section state and DTOs.
//!
//! These types are UI-agnostic view models consumed by settings update logic
//! and rendered by the desktop player UI.

/// Device management state.
#[derive(Debug, Clone, Default)]
pub struct DeviceManagementState {
    pub devices: Vec<UserDevice>,
    pub loading: bool,
    pub error_message: Option<String>,
}

impl DeviceManagementState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn has_error(&self) -> bool {
        self.error_message.is_some()
    }

    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

/// User device information shown in settings.
#[derive(Debug, Clone)]
pub struct UserDevice {
    pub device_id: String,
    pub device_name: String,
    pub device_type: String,
    pub last_active: chrono::DateTime<chrono::Utc>,
    pub is_current_device: bool,
    pub location: Option<String>,
}
