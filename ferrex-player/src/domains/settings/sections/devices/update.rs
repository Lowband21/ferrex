//! Devices section update handlers

use super::messages::DevicesMessage;
use super::state::UserDevice;
use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Main message handler for devices section
pub fn handle_message(
    state: &mut State,
    message: DevicesMessage,
) -> DomainUpdateResult {
    match message {
        DevicesMessage::LoadDevices => handle_load_devices(state),
        DevicesMessage::DevicesLoaded(result) => {
            handle_devices_loaded(state, result)
        }
        DevicesMessage::RefreshDevices => handle_refresh_devices(state),
        DevicesMessage::RevokeDevice(device_id) => {
            handle_revoke_device(state, device_id)
        }
        DevicesMessage::DeviceRevoked(result) => {
            handle_device_revoked(state, result)
        }
    }
}

fn handle_load_devices(state: &mut State) -> DomainUpdateResult {
    // TODO: Delegate to existing device management handler
    let _ = state;
    DomainUpdateResult::none()
}

fn handle_devices_loaded(
    state: &mut State,
    result: Result<Vec<UserDevice>, String>,
) -> DomainUpdateResult {
    let _ = (state, result);
    DomainUpdateResult::none()
}

fn handle_refresh_devices(state: &mut State) -> DomainUpdateResult {
    let _ = state;
    DomainUpdateResult::none()
}

fn handle_revoke_device(
    state: &mut State,
    device_id: String,
) -> DomainUpdateResult {
    let _ = (state, device_id);
    DomainUpdateResult::none()
}

fn handle_device_revoked(
    state: &mut State,
    result: Result<String, String>,
) -> DomainUpdateResult {
    let _ = (state, result);
    DomainUpdateResult::none()
}
