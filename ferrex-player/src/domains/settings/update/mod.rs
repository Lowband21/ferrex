pub mod device_management;
pub mod navigation;
pub mod preferences;
pub mod profile;
pub mod security;

use super::messages::SettingsMessage;
use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Main settings update handler
/// Returns a DomainUpdateResult containing both the task and any events to emit
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update_settings(
    state: &mut State,
    message: SettingsMessage,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infra::profiling_scopes::scopes::SETTINGS_UPDATE);

    match message {
        // Navigation
        SettingsMessage::ShowProfile => navigation::handle_show_profile(state),
        SettingsMessage::ShowPreferences => {
            navigation::handle_show_preferences(state)
        }
        SettingsMessage::ShowSecurity => {
            navigation::handle_show_security(state)
        }
        SettingsMessage::BackToMain => navigation::handle_back_to_main(state),
        SettingsMessage::BackToHome => navigation::handle_back_to_home(state),

        // Security - Password
        SettingsMessage::ShowChangePassword => {
            security::handle_show_change_password(state)
        }
        SettingsMessage::UpdatePasswordCurrent(value) => {
            security::handle_update_password_current(state, value)
        }
        SettingsMessage::UpdatePasswordNew(value) => {
            security::handle_update_password_new(state, value)
        }
        SettingsMessage::UpdatePasswordConfirm(value) => {
            security::handle_update_password_confirm(state, value)
        }
        SettingsMessage::TogglePasswordVisibility => {
            security::handle_toggle_password_visibility(state)
        }
        SettingsMessage::SubmitPasswordChange => {
            security::handle_submit_password_change(state)
        }
        SettingsMessage::PasswordChangeResult(result) => {
            security::handle_password_change_result(state, result)
        }
        SettingsMessage::CancelPasswordChange => {
            security::handle_cancel_password_change(state)
        }

        // Security - PIN
        SettingsMessage::CheckUserHasPin => {
            security::handle_check_user_has_pin(state)
        }
        SettingsMessage::UserHasPinResult(has_pin) => {
            security::handle_user_has_pin_result(state, has_pin)
        }
        SettingsMessage::ShowSetPin => security::handle_show_set_pin(state),
        SettingsMessage::ShowChangePin => {
            security::handle_show_change_pin(state)
        }
        SettingsMessage::UpdatePinCurrent(value) => {
            security::handle_update_pin_current(state, value)
        }
        SettingsMessage::UpdatePinNew(value) => {
            security::handle_update_pin_new(state, value)
        }
        SettingsMessage::UpdatePinConfirm(value) => {
            security::handle_update_pin_confirm(state, value)
        }
        SettingsMessage::SubmitPinChange => {
            security::handle_submit_pin_change(state)
        }
        SettingsMessage::PinChangeResult(result) => {
            security::handle_pin_change_result(state, result)
        }
        SettingsMessage::CancelPinChange => {
            security::handle_cancel_pin_change(state)
        }

        // Preferences
        SettingsMessage::ToggleAutoLogin(enabled) => {
            preferences::handle_toggle_auto_login(state, enabled)
        }
        SettingsMessage::AutoLoginToggled(result) => {
            preferences::handle_auto_login_toggled(state, result)
        }
        SettingsMessage::SetUserScale(user_scale) => {
            preferences::handle_set_user_scale(state, user_scale)
        }

        // Profile
        SettingsMessage::UpdateDisplayName(name) => {
            profile::handle_update_display_name(state, name)
        }
        SettingsMessage::UpdateEmail(email) => {
            profile::handle_update_email(state, email)
        }
        SettingsMessage::SubmitProfileChanges => {
            profile::handle_submit_profile_changes(state)
        }
        SettingsMessage::ProfileChangeResult(result) => {
            profile::handle_profile_change_result(state, result)
        }

        // Device Management
        SettingsMessage::LoadDevices => {
            device_management::handle_load_devices(state)
        }
        SettingsMessage::DevicesLoaded(result) => {
            device_management::handle_devices_loaded(state, result)
        }
        SettingsMessage::RefreshDevices => {
            device_management::handle_refresh_devices(state)
        }
        SettingsMessage::RevokeDevice(device_id) => {
            device_management::handle_revoke_device(state, device_id)
        }
        SettingsMessage::DeviceRevoked(result) => {
            device_management::handle_device_revoked(state, result)
        }
    }
}
