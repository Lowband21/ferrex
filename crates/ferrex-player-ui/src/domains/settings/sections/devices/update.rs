//! Devices section update adapter.

use super::messages::DevicesMessage;
use crate::{
    common::messages::DomainUpdateResult,
    domains::settings::{messages::SettingsMessage, update::update_settings},
    state::State,
};

/// Route devices section messages through the settings reducer.
pub fn handle_message(
    state: &mut State,
    message: DevicesMessage,
) -> DomainUpdateResult {
    let message = match message {
        DevicesMessage::LoadDevices => SettingsMessage::LoadDevices,
        DevicesMessage::DevicesLoaded(result) => {
            SettingsMessage::DevicesLoaded(result)
        }
        DevicesMessage::RefreshDevices => SettingsMessage::RefreshDevices,
        DevicesMessage::RevokeDevice(device_id) => {
            SettingsMessage::RevokeDevice(device_id)
        }
        DevicesMessage::DeviceRevoked(result) => {
            SettingsMessage::DeviceRevoked(result)
        }
    };
    update_settings(state, message)
}
