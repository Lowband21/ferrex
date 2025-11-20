use crate::{
    messages::{settings, auth},
    security::SecureCredential,
    state::State,
};
use iced::Task;

/// Handle show change password modal
pub fn handle_show_change_password(state: &mut State) -> Task<settings::Message> {
    state.security_settings_state.showing_password_change = true;
    state.security_settings_state.password_current = SecureCredential::from("");
    state.security_settings_state.password_new = SecureCredential::from("");
    state.security_settings_state.password_confirm = SecureCredential::from("");
    state.security_settings_state.password_error = None;
    state.security_settings_state.password_loading = false;
    Task::none()
}

/// Handle update current password
pub fn handle_update_password_current(state: &mut State, value: String) -> Task<settings::Message> {
    state.security_settings_state.password_current = SecureCredential::from(value);
    state.security_settings_state.password_error = None;
    Task::none()
}

/// Handle update new password
pub fn handle_update_password_new(state: &mut State, value: String) -> Task<settings::Message> {
    state.security_settings_state.password_new = SecureCredential::from(value);
    state.security_settings_state.password_error = None;
    Task::none()
}

/// Handle update confirm password
pub fn handle_update_password_confirm(state: &mut State, value: String) -> Task<settings::Message> {
    state.security_settings_state.password_confirm = SecureCredential::from(value);
    state.security_settings_state.password_error = None;
    Task::none()
}

/// Handle toggle password visibility
pub fn handle_toggle_password_visibility(state: &mut State) -> Task<settings::Message> {
    state.security_settings_state.password_show = !state.security_settings_state.password_show;
    Task::none()
}

/// Handle submit password change
pub fn handle_submit_password_change(state: &mut State) -> Task<settings::Message> {
    // Clone the values we need before validation
    let password_current = state.security_settings_state.password_current.clone();
    let password_new = state.security_settings_state.password_new.clone();
    let password_confirm = state.security_settings_state.password_confirm.clone();
    
    // Validate inputs
    if password_current.is_empty() {
        state.security_settings_state.password_error = Some("Current password is required".to_string());
        return Task::none();
    }
    if password_new.is_empty() {
        state.security_settings_state.password_error = Some("New password is required".to_string());
        return Task::none();
    }
    if password_new.len() < 8 {
        state.security_settings_state.password_error = Some("Password must be at least 8 characters".to_string());
        return Task::none();
    }
    if password_new != password_confirm {
        state.security_settings_state.password_error = Some("Passwords do not match".to_string());
        return Task::none();
    }
    if password_current.as_str() == password_new.as_str() {
        state.security_settings_state.password_error = Some("New password must be different from current password".to_string());
        return Task::none();
    }
    
    // Check password complexity
    let has_upper = password_new.as_str().chars().any(|c| c.is_uppercase());
    let has_lower = password_new.as_str().chars().any(|c| c.is_lowercase());
    let has_digit = password_new.as_str().chars().any(|c| c.is_digit(10));
    
    if !has_upper || !has_lower || !has_digit {
        state.security_settings_state.password_error = Some("Password must contain uppercase, lowercase, and numbers".to_string());
        return Task::none();
    }
    
    // Set loading state
    state.security_settings_state.password_loading = true;
    
    // Create auth command to change password
    let command = auth::AuthCommand::ChangePassword {
        old_password: password_current,
        new_password: password_new,
    };
    
    // Emit cross-domain event to execute auth command
    Task::done(settings::Message::_EmitCrossDomainEvent(
        crate::messages::CrossDomainEvent::AuthCommandRequested(command)
    ))
}

/// Handle password change result
pub fn handle_password_change_result(state: &mut State, result: Result<(), String>) -> Task<settings::Message> {
    match result {
        Ok(()) => {
            // Clear password fields on success
            state.security_settings_state.password_current = SecureCredential::from("");
            state.security_settings_state.password_new = SecureCredential::from("");
            state.security_settings_state.password_confirm = SecureCredential::from("");
            state.security_settings_state.password_error = None;
            state.security_settings_state.password_loading = false;
            state.security_settings_state.showing_password_change = false;
            
            // TODO: Show success notification
            log::info!("Password changed successfully");
        }
        Err(error) => {
            state.security_settings_state.password_error = Some(error);
            state.security_settings_state.password_loading = false;
        }
    }
    Task::none()
}

/// Handle cancel password change
pub fn handle_cancel_password_change(state: &mut State) -> Task<settings::Message> {
    // Clear password fields
    state.security_settings_state.password_current = SecureCredential::from("");
    state.security_settings_state.password_new = SecureCredential::from("");
    state.security_settings_state.password_confirm = SecureCredential::from("");
    state.security_settings_state.password_error = None;
    state.security_settings_state.password_loading = false;
    state.security_settings_state.showing_password_change = false;
    Task::none()
}

/// Handle check if user has PIN
pub fn handle_check_user_has_pin(state: &mut State) -> Task<settings::Message> {
    
    if let Some(auth_manager) = &state.auth_manager {
        let auth_manager = auth_manager.clone();
        
        return Task::perform(
            async move {
                if let Some(user) = auth_manager.get_current_user().await {
                    auth_manager.check_device_auth(user.id).await
                        .map(|status| status.has_pin)
                        .unwrap_or(false)
                } else {
                    false
                }
            },
            settings::Message::UserHasPinResult,
        );
    }
    
    Task::done(settings::Message::UserHasPinResult(false))
}

/// Handle user has PIN result
pub fn handle_user_has_pin_result(state: &mut State, has_pin: bool) -> Task<settings::Message> {
    state.security_settings_state.has_pin = has_pin;
    Task::none()
}

/// Handle show set PIN modal
pub fn handle_show_set_pin(state: &mut State) -> Task<settings::Message> {
    state.security_settings_state.showing_pin_change = true;
    state.security_settings_state.pin_current = SecureCredential::from("");
    state.security_settings_state.pin_new = SecureCredential::from("");
    state.security_settings_state.pin_confirm = SecureCredential::from("");
    state.security_settings_state.pin_error = None;
    state.security_settings_state.pin_loading = false;
    Task::none()
}

/// Handle show change PIN modal
pub fn handle_show_change_pin(state: &mut State) -> Task<settings::Message> {
    state.security_settings_state.showing_pin_change = true;
    state.security_settings_state.pin_current = SecureCredential::from("");
    state.security_settings_state.pin_new = SecureCredential::from("");
    state.security_settings_state.pin_confirm = SecureCredential::from("");
    state.security_settings_state.pin_error = None;
    state.security_settings_state.pin_loading = false;
    Task::none()
}

/// Handle update current PIN
pub fn handle_update_pin_current(state: &mut State, value: String) -> Task<settings::Message> {
    // Only allow digits and limit to 4 characters
    let filtered: String = value.chars()
        .filter(|c| c.is_digit(10))
        .take(4)
        .collect();
    state.security_settings_state.pin_current = SecureCredential::from(filtered);
    state.security_settings_state.pin_error = None;
    Task::none()
}

/// Handle update new PIN
pub fn handle_update_pin_new(state: &mut State, value: String) -> Task<settings::Message> {
    // Only allow digits and limit to 4 characters
    let filtered: String = value.chars()
        .filter(|c| c.is_digit(10))
        .take(4)
        .collect();
    state.security_settings_state.pin_new = SecureCredential::from(filtered);
    state.security_settings_state.pin_error = None;
    Task::none()
}

/// Handle update confirm PIN
pub fn handle_update_pin_confirm(state: &mut State, value: String) -> Task<settings::Message> {
    // Only allow digits and limit to 4 characters
    let filtered: String = value.chars()
        .filter(|c| c.is_digit(10))
        .take(4)
        .collect();
    state.security_settings_state.pin_confirm = SecureCredential::from(filtered);
    state.security_settings_state.pin_error = None;
    Task::none()
}

/// Handle submit PIN change
pub fn handle_submit_pin_change(state: &mut State) -> Task<settings::Message> {
    // Clone the values we need before validation
    let is_new_pin = !state.security_settings_state.has_pin;
    let pin_current = state.security_settings_state.pin_current.clone();
    let pin_new = state.security_settings_state.pin_new.clone();
    let pin_confirm = state.security_settings_state.pin_confirm.clone();
    
    // Validate inputs
    if !is_new_pin && pin_current.is_empty() {
        state.security_settings_state.pin_error = Some("Current PIN is required".to_string());
        return Task::none();
    }
    if pin_new.is_empty() {
        state.security_settings_state.pin_error = Some("New PIN is required".to_string());
        return Task::none();
    }
    if pin_new.len() != 4 {
        state.security_settings_state.pin_error = Some("PIN must be exactly 4 digits".to_string());
        return Task::none();
    }
    if !pin_new.as_str().chars().all(|c| c.is_digit(10)) {
        state.security_settings_state.pin_error = Some("PIN must contain only digits".to_string());
        return Task::none();
    }
    if pin_new.as_str() != pin_confirm.as_str() {
        state.security_settings_state.pin_error = Some("PINs do not match".to_string());
        return Task::none();
    }
    if !is_new_pin && pin_current.as_str() == pin_new.as_str() {
        state.security_settings_state.pin_error = Some("New PIN must be different from current PIN".to_string());
        return Task::none();
    }
    
    // Set loading state
    state.security_settings_state.pin_loading = true;
    
    // Create appropriate auth command
    let command = if is_new_pin {
        auth::AuthCommand::SetDevicePin {
            pin: pin_new,
        }
    } else {
        auth::AuthCommand::ChangeDevicePin {
            current_pin: pin_current,
            new_pin: pin_new,
        }
    };
    
    // Emit cross-domain event to execute auth command
    Task::done(settings::Message::_EmitCrossDomainEvent(
        crate::messages::CrossDomainEvent::AuthCommandRequested(command)
    ))
}

/// Handle PIN change result
pub fn handle_pin_change_result(state: &mut State, result: Result<(), String>) -> Task<settings::Message> {
    match result {
        Ok(()) => {
            // Update has_pin flag
            state.security_settings_state.has_pin = true;
            
            // Clear PIN fields on success
            state.security_settings_state.pin_current = SecureCredential::from("");
            state.security_settings_state.pin_new = SecureCredential::from("");
            state.security_settings_state.pin_confirm = SecureCredential::from("");
            state.security_settings_state.pin_error = None;
            state.security_settings_state.pin_loading = false;
            state.security_settings_state.showing_pin_change = false;
            
            // TODO: Show success notification
            log::info!("PIN changed successfully");
        }
        Err(error) => {
            state.security_settings_state.pin_error = Some(error);
            state.security_settings_state.pin_loading = false;
        }
    }
    Task::none()
}

/// Handle cancel PIN change
pub fn handle_cancel_pin_change(state: &mut State) -> Task<settings::Message> {
    // Clear PIN fields
    state.security_settings_state.pin_current = SecureCredential::from("");
    state.security_settings_state.pin_new = SecureCredential::from("");
    state.security_settings_state.pin_confirm = SecureCredential::from("");
    state.security_settings_state.pin_error = None;
    state.security_settings_state.pin_loading = false;
    state.security_settings_state.showing_pin_change = false;
    Task::none()
}