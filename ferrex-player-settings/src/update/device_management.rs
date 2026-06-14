//! Device-management settings reducers and DTO mapping helpers.

use std::str::FromStr;

use ferrex_core::player_prelude::{AuthenticatedDevice, Platform};
use uuid::Uuid;

use crate::sections::devices::state::{DeviceManagementState, UserDevice};

use super::{SettingsEffect, SettingsUpdate};

/// Mark the device list as loading and request an app-shell fetch.
pub fn load_devices(state: &mut DeviceManagementState) -> SettingsUpdate {
    state.loading = true;
    state.error_message = None;
    SettingsUpdate::effect(SettingsEffect::LoadDevices)
}

/// Apply a device list result.
pub fn devices_loaded(
    state: &mut DeviceManagementState,
    result: Result<Vec<UserDevice>, String>,
) -> SettingsUpdate {
    state.loading = false;
    match result {
        Ok(devices) => {
            state.devices = devices;
            state.error_message = None;
        }
        Err(error) => {
            state.error_message = Some(error);
        }
    }
    SettingsUpdate::none()
}

/// Refresh the device list.
pub fn refresh_devices(state: &mut DeviceManagementState) -> SettingsUpdate {
    load_devices(state)
}

/// Validate a device id and request revocation.
pub fn revoke_device(
    state: &mut DeviceManagementState,
    device_id: String,
) -> SettingsUpdate {
    match Uuid::from_str(&device_id) {
        Ok(uuid) => SettingsUpdate::effect(SettingsEffect::RevokeDevice {
            device_id: uuid,
            original: device_id,
        }),
        Err(error) => {
            state.error_message = Some(format!("Invalid device id: {error}"));
            SettingsUpdate::none()
        }
    }
}

/// Apply a revoke-device result.
pub fn device_revoked(
    state: &mut DeviceManagementState,
    result: Result<String, String>,
) -> SettingsUpdate {
    match result {
        Ok(device_id) => {
            state.devices.retain(|device| device.device_id != device_id);
            state.error_message = None;
        }
        Err(error) => {
            state.error_message = Some(error);
        }
    }
    SettingsUpdate::none()
}

/// Convert authenticated-device API records into settings view models.
pub fn user_devices_from_authenticated(
    devices: Vec<AuthenticatedDevice>,
    current_device_session_id: Option<Uuid>,
) -> Vec<UserDevice> {
    devices
        .into_iter()
        .filter(|device| !device.is_revoked())
        .map(|device| {
            let device_type = match device.platform {
                Platform::Android | Platform::IOS => "mobile",
                Platform::TvOS => "tv",
                Platform::Windows | Platform::MacOS | Platform::Linux => {
                    "desktop"
                }
                Platform::Web => "web",
                Platform::Unknown => "unknown",
            }
            .to_string();

            let location = device
                .metadata
                .get("location")
                .and_then(|value| value.as_str())
                .map(ToString::to_string);

            UserDevice {
                device_id: device.id.to_string(),
                device_name: device.name,
                device_type,
                last_active: device.last_activity,
                is_current_device: current_device_session_id
                    .is_some_and(|current_id| device.id == current_id),
                location,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_loaded_result_replaces_devices() {
        let mut state = DeviceManagementState {
            loading: true,
            ..Default::default()
        };
        let device = UserDevice {
            device_id: "device-1".into(),
            device_name: "Desktop".into(),
            device_type: "desktop".into(),
            last_active: chrono::Utc::now(),
            is_current_device: false,
            location: None,
        };

        devices_loaded(&mut state, Ok(vec![device]));

        assert!(!state.loading);
        assert_eq!(state.devices.len(), 1);
        assert!(state.error_message.is_none());
    }
}
