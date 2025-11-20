pub mod device_management;
pub mod navigation;
pub mod preferences;
pub mod profile;
pub mod security;

use crate::{
    messages::settings,
    state::State,
};
use iced::Task;

/// Main settings update handler
pub fn update_settings(state: &mut State, message: settings::Message) -> Task<settings::Message> {
    
    match message {
        // Navigation
        settings::Message::ShowProfile => navigation::handle_show_profile(state),
        settings::Message::ShowPreferences => navigation::handle_show_preferences(state),
        settings::Message::ShowSecurity => navigation::handle_show_security(state),
        settings::Message::BackToMain => navigation::handle_back_to_main(state),
        settings::Message::BackToHome => navigation::handle_back_to_home(state),
        
        // Security - Password
        settings::Message::ShowChangePassword => security::handle_show_change_password(state),
        settings::Message::UpdatePasswordCurrent(value) => security::handle_update_password_current(state, value),
        settings::Message::UpdatePasswordNew(value) => security::handle_update_password_new(state, value),
        settings::Message::UpdatePasswordConfirm(value) => security::handle_update_password_confirm(state, value),
        settings::Message::TogglePasswordVisibility => security::handle_toggle_password_visibility(state),
        settings::Message::SubmitPasswordChange => security::handle_submit_password_change(state),
        settings::Message::PasswordChangeResult(result) => security::handle_password_change_result(state, result),
        settings::Message::CancelPasswordChange => security::handle_cancel_password_change(state),
        
        // Security - PIN
        settings::Message::CheckUserHasPin => security::handle_check_user_has_pin(state),
        settings::Message::UserHasPinResult(has_pin) => security::handle_user_has_pin_result(state, has_pin),
        settings::Message::ShowSetPin => security::handle_show_set_pin(state),
        settings::Message::ShowChangePin => security::handle_show_change_pin(state),
        settings::Message::UpdatePinCurrent(value) => security::handle_update_pin_current(state, value),
        settings::Message::UpdatePinNew(value) => security::handle_update_pin_new(state, value),
        settings::Message::UpdatePinConfirm(value) => security::handle_update_pin_confirm(state, value),
        settings::Message::SubmitPinChange => security::handle_submit_pin_change(state),
        settings::Message::PinChangeResult(result) => security::handle_pin_change_result(state, result),
        settings::Message::CancelPinChange => security::handle_cancel_pin_change(state),
        
        // Preferences
        settings::Message::ToggleAutoLogin(enabled) => preferences::handle_toggle_auto_login(state, enabled),
        settings::Message::AutoLoginToggled(result) => preferences::handle_auto_login_toggled(state, result),
        
        // Profile
        settings::Message::UpdateDisplayName(name) => profile::handle_update_display_name(state, name),
        settings::Message::UpdateEmail(email) => profile::handle_update_email(state, email),
        settings::Message::SubmitProfileChanges => profile::handle_submit_profile_changes(state),
        settings::Message::ProfileChangeResult(result) => profile::handle_profile_change_result(state, result),
        
        // Internal cross-domain coordination
        settings::Message::_EmitCrossDomainEvent(_) => {
            log::warn!("_EmitCrossDomainEvent should be handled by main update loop");
            Task::none()
        }
    }
}