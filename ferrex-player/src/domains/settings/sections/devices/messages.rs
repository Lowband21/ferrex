//! Devices section messages

use super::state::UserDevice;

/// Messages for the devices settings section
#[derive(Debug, Clone)]
pub enum DevicesMessage {
    /// Load list of devices
    LoadDevices,
    /// Devices loaded result
    DevicesLoaded(Result<Vec<UserDevice>, String>),
    /// Refresh device list
    RefreshDevices,
    /// Revoke device access
    RevokeDevice(String),
    /// Device revocation result
    DeviceRevoked(Result<String, String>),
}

impl DevicesMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::LoadDevices => "Devices::LoadDevices",
            Self::DevicesLoaded(_) => "Devices::DevicesLoaded",
            Self::RefreshDevices => "Devices::RefreshDevices",
            Self::RevokeDevice(_) => "Devices::RevokeDevice",
            Self::DeviceRevoked(_) => "Devices::DeviceRevoked",
        }
    }
}
