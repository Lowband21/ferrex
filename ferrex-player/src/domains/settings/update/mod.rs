pub mod device_management;
pub mod navigation;
pub mod preferences;
pub mod profile;
pub mod security;

use super::messages::Message;
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
    message: Message,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(
        crate::infrastructure::profiling_scopes::scopes::SETTINGS_UPDATE
    );

    match message {
        // Navigation
        Message::ShowProfile => navigation::handle_show_profile(state),
        Message::ShowPreferences => navigation::handle_show_preferences(state),
        Message::ShowSecurity => navigation::handle_show_security(state),
        Message::BackToMain => navigation::handle_back_to_main(state),
        Message::BackToHome => navigation::handle_back_to_home(state),

        // Security - Password
        Message::ShowChangePassword => {
            security::handle_show_change_password(state)
        }
        Message::UpdatePasswordCurrent(value) => {
            security::handle_update_password_current(state, value)
        }
        Message::UpdatePasswordNew(value) => {
            security::handle_update_password_new(state, value)
        }
        Message::UpdatePasswordConfirm(value) => {
            security::handle_update_password_confirm(state, value)
        }
        Message::TogglePasswordVisibility => {
            security::handle_toggle_password_visibility(state)
        }
        Message::SubmitPasswordChange => {
            security::handle_submit_password_change(state)
        }
        Message::PasswordChangeResult(result) => {
            security::handle_password_change_result(state, result)
        }
        Message::CancelPasswordChange => {
            security::handle_cancel_password_change(state)
        }

        // Security - PIN
        Message::CheckUserHasPin => security::handle_check_user_has_pin(state),
        Message::UserHasPinResult(has_pin) => {
            security::handle_user_has_pin_result(state, has_pin)
        }
        Message::ShowSetPin => security::handle_show_set_pin(state),
        Message::ShowChangePin => security::handle_show_change_pin(state),
        Message::UpdatePinCurrent(value) => {
            security::handle_update_pin_current(state, value)
        }
        Message::UpdatePinNew(value) => {
            security::handle_update_pin_new(state, value)
        }
        Message::UpdatePinConfirm(value) => {
            security::handle_update_pin_confirm(state, value)
        }
        Message::SubmitPinChange => security::handle_submit_pin_change(state),
        Message::PinChangeResult(result) => {
            security::handle_pin_change_result(state, result)
        }
        Message::CancelPinChange => security::handle_cancel_pin_change(state),

        // Preferences
        Message::ToggleAutoLogin(enabled) => {
            preferences::handle_toggle_auto_login(state, enabled)
        }
        Message::AutoLoginToggled(result) => {
            preferences::handle_auto_login_toggled(state, result)
        }

        // Profile
        Message::UpdateDisplayName(name) => {
            profile::handle_update_display_name(state, name)
        }
        Message::UpdateEmail(email) => {
            profile::handle_update_email(state, email)
        }
        Message::SubmitProfileChanges => {
            profile::handle_submit_profile_changes(state)
        }
        Message::ProfileChangeResult(result) => {
            profile::handle_profile_change_result(state, result)
        }

        // Device Management
        Message::LoadDevices => device_management::handle_load_devices(state),
        Message::DevicesLoaded(result) => {
            device_management::handle_devices_loaded(state, result)
        }
        Message::RefreshDevices => {
            device_management::handle_refresh_devices(state)
        }
        Message::RevokeDevice(device_id) => {
            device_management::handle_revoke_device(state, device_id)
        }
        Message::DeviceRevoked(result) => {
            device_management::handle_device_revoked(state, result)
        }
    }
}
