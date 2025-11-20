//! Device management update handlers

use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::domains::settings::messages as settings;
use crate::domains::ui::views::settings::device_management::UserDevice;
use crate::infrastructure::services::api::ApiService;
use crate::state_refactored::State;
use iced::Task;
use log::{error, info};
use std::str::FromStr;
use uuid::Uuid;

/// Handle loading devices when the view is shown or refreshed
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_load_devices(state: &mut State) -> DomainUpdateResult {
    info!("Loading user devices");
    state.domains.settings.device_management_state.loading = true;
    state.domains.settings.device_management_state.error_message = None;

    // Use trait-based SettingsService for device operations
    let settings_service = state.domains.settings.settings_service.clone();
    let auth_service = state.domains.settings.auth_service.clone();

    let task = Task::perform(
        async move {
            let current_device_id = auth_service
                .current_device_id()
                .await
                .map_err(|e| e.to_string())?;

            match settings_service.list_user_devices().await {
                Ok(devices) => {
                    // Convert AuthenticatedDevice to UserDevice
                    let user_devices: Vec<UserDevice> = devices
                        .into_iter()
                        .filter(|d| !d.revoked) // Don't show revoked devices
                        .map(|device| {
                            // Determine device type from platform
                            let device_type = match device.platform {
                                ferrex_core::auth::Platform::Android => "mobile",
                                ferrex_core::auth::Platform::IOS => "mobile",
                                ferrex_core::auth::Platform::TvOS => "tv",
                                ferrex_core::auth::Platform::Windows => "desktop",
                                ferrex_core::auth::Platform::MacOS => "desktop",
                                ferrex_core::auth::Platform::Linux => "desktop",
                                ferrex_core::auth::Platform::Web => "web",
                                ferrex_core::auth::Platform::Unknown => "unknown",
                            }
                            .to_string();

                            // Extract location from metadata if available
                            let location = device
                                .metadata
                                .get("location")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            UserDevice {
                                device_id: device.id.to_string(),
                                device_name: device.name.clone(),
                                device_type,
                                last_active: device.last_seen_at,
                                is_current_device: device.id == current_device_id,
                                location,
                            }
                        })
                        .collect();

                    Ok(user_devices)
                }
                Err(e) => Err(format!("Failed to load devices: {}", e)),
            }
        },
        |result| settings::Message::DevicesLoaded(result),
    );
    DomainUpdateResult::task(task.map(DomainMessage::Settings))
}

/// Handle devices loaded result
pub fn handle_devices_loaded(
    state: &mut State,
    result: Result<Vec<UserDevice>, String>,
) -> DomainUpdateResult {
    state.domains.settings.device_management_state.loading = false;

    match result {
        Ok(devices) => {
            info!("Successfully loaded {} devices", devices.len());
            state.domains.settings.device_management_state.devices = devices;
            state.domains.settings.device_management_state.error_message = None;
        }
        Err(error) => {
            error!("Failed to load devices: {}", error);
            state.domains.settings.device_management_state.error_message = Some(error);
        }
    }

    DomainUpdateResult::task(Task::none())
}

/// Handle refresh devices
pub fn handle_refresh_devices(state: &mut State) -> DomainUpdateResult {
    info!("Refreshing device list");
    handle_load_devices(state)
}

/// Handle revoke device
pub fn handle_revoke_device(state: &mut State, device_id: String) -> DomainUpdateResult {
    info!("Revoking device: {}", device_id);

    // Use trait-based SettingsService for device operations
    let settings_service = state.domains.settings.settings_service.clone();

    // Parse device ID
    let device_uuid = match Uuid::from_str(&device_id) {
        Ok(id) => id,
        Err(e) => {
            error!("Invalid device ID: {}", e);
            return DomainUpdateResult::task(Task::none());
        }
    };

    let task = Task::perform(
        async move {
            match settings_service.revoke_device(device_uuid).await {
                Ok(_) => Ok(device_id),
                Err(e) => Err(format!("Failed to revoke device: {}", e)),
            }
        },
        |result| settings::Message::DeviceRevoked(result),
    );
    DomainUpdateResult::task(task.map(DomainMessage::Settings))
}

/// Handle device revoked result
pub fn handle_device_revoked(
    state: &mut State,
    result: Result<String, String>,
) -> DomainUpdateResult {
    match result {
        Ok(device_id) => {
            info!("Successfully revoked device: {}", device_id);
            // Remove the device from the list
            state
                .domains
                .settings
                .device_management_state
                .devices
                .retain(|d| d.device_id != device_id);
        }
        Err(error) => {
            error!("Failed to revoke device: {}", error);
            state.domains.settings.device_management_state.error_message = Some(error);
        }
    }

    DomainUpdateResult::task(Task::none())
}
