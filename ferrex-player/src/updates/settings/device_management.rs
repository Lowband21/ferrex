//! Device management update handlers

use crate::{
    messages::{settings, ui},
    state::State,
    views::settings::device_management::{UserDevice, DeviceManagementState},
};
use iced::Task;
use log::{debug, error, info};
use uuid::Uuid;
use std::str::FromStr;

/// Handle loading devices when the view is shown or refreshed
pub fn handle_load_devices(state: &mut State) -> Task<ui::Message> {
    info!("Loading user devices");
    state.device_management_state.loading = true;
    state.device_management_state.error_message = None;
    
    // Check if we have an API client
    let Some(api_client) = state.api_client.clone() else {
        error!("No API client available");
        return Task::done(ui::Message::DevicesLoaded(Err("Not authenticated".to_string())));
    };
    
    // TODO: Get current device ID from auth manager or stored session
    // For now, we'll mark devices as current based on backend data
    let current_device_id: Option<uuid::Uuid> = None;
    
    Task::perform(
        async move {
            match api_client.list_user_devices().await {
                Ok(devices) => {
                    // Convert AuthenticatedDevice to UserDevice
                    let user_devices: Vec<UserDevice> = devices.into_iter()
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
                            }.to_string();
                            
                            // Extract location from metadata if available
                            let location = device.metadata.get("location")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            
                            UserDevice {
                                device_id: device.id.to_string(),
                                device_name: device.name.clone(),
                                device_type,
                                last_active: device.last_seen_at,
                                is_current_device: false, // TODO: Implement proper current device detection
                                location,
                            }
                        })
                        .collect();
                    
                    Ok(user_devices)
                }
                Err(e) => Err(format!("Failed to load devices: {}", e))
            }
        },
        ui::Message::DevicesLoaded
    )
}

/// Handle devices loaded result
pub fn handle_devices_loaded(state: &mut State, result: Result<Vec<UserDevice>, String>) -> Task<ui::Message> {
    state.device_management_state.loading = false;
    
    match result {
        Ok(devices) => {
            info!("Successfully loaded {} devices", devices.len());
            state.device_management_state.devices = devices;
            state.device_management_state.error_message = None;
        }
        Err(error) => {
            error!("Failed to load devices: {}", error);
            state.device_management_state.error_message = Some(error);
        }
    }
    
    Task::none()
}

/// Handle refresh devices
pub fn handle_refresh_devices(state: &mut State) -> Task<ui::Message> {
    info!("Refreshing device list");
    handle_load_devices(state)
}

/// Handle revoke device
pub fn handle_revoke_device(state: &mut State, device_id: String) -> Task<ui::Message> {
    info!("Revoking device: {}", device_id);
    
    // Check if we have an API client
    let Some(api_client) = state.api_client.clone() else {
        error!("No API client available");
        return Task::done(ui::Message::DeviceRevoked(Err("Not authenticated".to_string())));
    };
    
    // Parse device ID
    let device_uuid = match Uuid::from_str(&device_id) {
        Ok(id) => id,
        Err(e) => {
            error!("Invalid device ID: {}", e);
            return Task::done(ui::Message::DeviceRevoked(Err(format!("Invalid device ID: {}", e))));
        }
    };
    
    Task::perform(
        async move {
            match api_client.revoke_device(device_uuid).await {
                Ok(_) => Ok(device_id),
                Err(e) => Err(format!("Failed to revoke device: {}", e))
            }
        },
        ui::Message::DeviceRevoked
    )
}

/// Handle device revoked result
pub fn handle_device_revoked(state: &mut State, result: Result<String, String>) -> Task<ui::Message> {
    match result {
        Ok(device_id) => {
            info!("Successfully revoked device: {}", device_id);
            // Remove the device from the list
            state.device_management_state.devices.retain(|d| d.device_id != device_id);
        }
        Err(error) => {
            error!("Failed to revoke device: {}", error);
            state.device_management_state.error_message = Some(error);
        }
    }
    
    Task::none()
}