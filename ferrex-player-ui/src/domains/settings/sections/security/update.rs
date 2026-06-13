//! Security section update adapter.

use super::messages::SecurityMessage;
use crate::{
    common::messages::DomainUpdateResult,
    domains::settings::{messages::SettingsMessage, update::update_settings},
    state::State,
};

/// Route security section messages through the settings reducer.
pub fn handle_message(
    state: &mut State,
    message: SecurityMessage,
) -> DomainUpdateResult {
    let message = match message {
        SecurityMessage::ShowChangePassword => {
            SettingsMessage::ShowChangePassword
        }
        SecurityMessage::UpdatePasswordCurrent(value) => {
            SettingsMessage::UpdatePasswordCurrent(value)
        }
        SecurityMessage::UpdatePasswordNew(value) => {
            SettingsMessage::UpdatePasswordNew(value)
        }
        SecurityMessage::UpdatePasswordConfirm(value) => {
            SettingsMessage::UpdatePasswordConfirm(value)
        }
        SecurityMessage::TogglePasswordVisibility => {
            SettingsMessage::TogglePasswordVisibility
        }
        SecurityMessage::SubmitPasswordChange => {
            SettingsMessage::SubmitPasswordChange
        }
        SecurityMessage::PasswordChangeResult(result) => {
            SettingsMessage::PasswordChangeResult(result)
        }
        SecurityMessage::CancelPasswordChange => {
            SettingsMessage::CancelPasswordChange
        }
        SecurityMessage::CheckUserHasPin => SettingsMessage::CheckUserHasPin,
        SecurityMessage::UserHasPinResult(has_pin) => {
            SettingsMessage::UserHasPinResult(has_pin)
        }
        SecurityMessage::ShowSetPin => SettingsMessage::ShowSetPin,
        SecurityMessage::ShowChangePin => SettingsMessage::ShowChangePin,
        SecurityMessage::UpdatePinCurrent(value) => {
            SettingsMessage::UpdatePinCurrent(value)
        }
        SecurityMessage::UpdatePinNew(value) => {
            SettingsMessage::UpdatePinNew(value)
        }
        SecurityMessage::UpdatePinConfirm(value) => {
            SettingsMessage::UpdatePinConfirm(value)
        }
        SecurityMessage::SubmitPinChange => SettingsMessage::SubmitPinChange,
        SecurityMessage::PinChangeResult(result) => {
            SettingsMessage::PinChangeResult(result)
        }
        SecurityMessage::CancelPinChange => SettingsMessage::CancelPinChange,
    };
    update_settings(state, message)
}
