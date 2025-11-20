use crate::{
    messages::{settings, CrossDomainEvent},
    security::SecureCredential,
    state::{State, SettingsSubview},
};
use iced::Task;

/// Handle showing profile view
pub fn handle_show_profile(state: &mut State) -> Task<settings::Message> {
    state.settings_subview = SettingsSubview::Profile;
    
    // Load current user profile data
    if let Some(auth_manager) = &state.auth_manager {
        let auth_manager = auth_manager.clone();
        
        return Task::perform(
            async move {
                auth_manager.get_current_user().await
                    .ok_or_else(|| "No current user".to_string())
            },
            |result| match result {
                Ok(user) => settings::Message::UpdateDisplayName(user.display_name),
                Err(e) => {
                    log::error!("Failed to load user profile: {}", e);
                    settings::Message::ProfileChangeResult(Err(e))
                }
            },
        );
    }
    
    Task::none()
}

/// Handle showing preferences view
pub fn handle_show_preferences(state: &mut State) -> Task<settings::Message> {
    state.settings_subview = SettingsSubview::Preferences;
    
    // Load current preferences
    if let Some(auth_manager) = &state.auth_manager {
        let auth_manager = auth_manager.clone();
        
        return Task::perform(
            async move {
                // Get auto-login preference from auth storage
                auth_manager.is_auto_login_enabled().await
            },
            |enabled| {
                // Set the initial state then return message
                settings::Message::AutoLoginToggled(Ok(enabled))
            },
        );
    }
    
    Task::none()
}

/// Handle showing security view
pub fn handle_show_security(state: &mut State) -> Task<settings::Message> {
    state.settings_subview = SettingsSubview::Security;
    
    // Check if user has PIN when entering security view
    Task::done(settings::Message::CheckUserHasPin)
}

/// Handle back to main settings view
pub fn handle_back_to_main(state: &mut State) -> Task<settings::Message> {
    state.settings_subview = SettingsSubview::Main;
    
    // Clear any sensitive data from security state
    state.security_settings_state.password_current = SecureCredential::new(String::new());
    state.security_settings_state.password_new = SecureCredential::new(String::new());
    state.security_settings_state.password_confirm = SecureCredential::new(String::new());
    state.security_settings_state.pin_current = SecureCredential::new(String::new());
    state.security_settings_state.pin_new = SecureCredential::new(String::new());
    state.security_settings_state.pin_confirm = SecureCredential::new(String::new());
    state.security_settings_state.password_error = None;
    state.security_settings_state.pin_error = None;
    
    Task::none()
}

/// Handle back to home (exit settings)
pub fn handle_back_to_home(state: &mut State) -> Task<settings::Message> {
    // Clear sensitive data from security state
    state.security_settings_state.password_current = SecureCredential::new(String::new());
    state.security_settings_state.password_new = SecureCredential::new(String::new());
    state.security_settings_state.password_confirm = SecureCredential::new(String::new());
    state.security_settings_state.pin_current = SecureCredential::new(String::new());
    state.security_settings_state.pin_new = SecureCredential::new(String::new());
    state.security_settings_state.pin_confirm = SecureCredential::new(String::new());
    state.security_settings_state.password_error = None;
    state.security_settings_state.pin_error = None;
    
    // Emit cross-domain event to navigate home
    Task::done(settings::Message::_EmitCrossDomainEvent(
        CrossDomainEvent::NavigateHome,
    ))
}