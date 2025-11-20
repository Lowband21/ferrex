use crate::common::messages::{
    CrossDomainEvent, DomainMessage, DomainUpdateResult,
};
use crate::domains::auth::messages as auth;
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::settings::messages as settings;
use crate::state_refactored::State;
use iced::Task;

/// Handle show change password modal
pub fn handle_show_change_password(state: &mut State) -> DomainUpdateResult {
    state.domains.settings.security.showing_password_change = true;
    state.domains.settings.security.password_current =
        SecureCredential::from("");
    state.domains.settings.security.password_new = SecureCredential::from("");
    state.domains.settings.security.password_confirm =
        SecureCredential::from("");
    state.domains.settings.security.password_error = None;
    state.domains.settings.security.password_loading = false;
    DomainUpdateResult::task(Task::none())
}

/// Handle update current password
pub fn handle_update_password_current(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    state.domains.settings.security.password_current =
        SecureCredential::from(value);
    state.domains.settings.security.password_error = None;
    DomainUpdateResult::task(Task::none())
}

/// Handle update new password
pub fn handle_update_password_new(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    state.domains.settings.security.password_new =
        SecureCredential::from(value);
    state.domains.settings.security.password_error = None;
    DomainUpdateResult::task(Task::none())
}

/// Handle update confirm password
pub fn handle_update_password_confirm(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    state.domains.settings.security.password_confirm =
        SecureCredential::from(value);
    state.domains.settings.security.password_error = None;
    DomainUpdateResult::task(Task::none())
}

/// Handle toggle password visibility
pub fn handle_toggle_password_visibility(
    state: &mut State,
) -> DomainUpdateResult {
    state.domains.settings.security.password_show =
        !state.domains.settings.security.password_show;
    DomainUpdateResult::task(Task::none())
}

/// Handle submit password change
pub fn handle_submit_password_change(state: &mut State) -> DomainUpdateResult {
    // Clone the values we need before validation
    let password_current =
        state.domains.settings.security.password_current.clone();
    let password_new = state.domains.settings.security.password_new.clone();
    let password_confirm =
        state.domains.settings.security.password_confirm.clone();

    // Validate inputs
    if password_current.is_empty() {
        state.domains.settings.security.password_error =
            Some("Current password is required".to_string());
        return DomainUpdateResult::task(Task::none());
    }
    if password_new.is_empty() {
        state.domains.settings.security.password_error =
            Some("New password is required".to_string());
        return DomainUpdateResult::task(Task::none());
    }
    if password_new.len() < 8 {
        state.domains.settings.security.password_error =
            Some("Password must be at least 8 characters".to_string());
        return DomainUpdateResult::task(Task::none());
    }
    if password_new != password_confirm {
        state.domains.settings.security.password_error =
            Some("Passwords do not match".to_string());
        return DomainUpdateResult::task(Task::none());
    }
    if password_current.as_str() == password_new.as_str() {
        state.domains.settings.security.password_error = Some(
            "New password must be different from current password".to_string(),
        );
        return DomainUpdateResult::task(Task::none());
    }

    // Check password complexity
    let has_upper = password_new.as_str().chars().any(|c| c.is_uppercase());
    let has_lower = password_new.as_str().chars().any(|c| c.is_lowercase());
    let has_digit = password_new.as_str().chars().any(|c| c.is_ascii_digit());

    if !has_upper || !has_lower || !has_digit {
        state.domains.settings.security.password_error = Some(
            "Password must contain uppercase, lowercase, and numbers"
                .to_string(),
        );
        return DomainUpdateResult::task(Task::none());
    }

    // Set loading state
    state.domains.settings.security.password_loading = true;

    // Create auth command to change password
    let command = auth::AuthCommand::ChangePassword {
        old_password: password_current,
        new_password: password_new,
    };

    // Emit cross-domain event to execute auth command
    DomainUpdateResult::with_events(
        Task::none(),
        vec![CrossDomainEvent::AuthCommandRequested(command)],
    )
}

/// Handle password change result
pub fn handle_password_change_result(
    state: &mut State,
    result: Result<(), String>,
) -> DomainUpdateResult {
    match result {
        Ok(()) => {
            // Clear password fields on success
            state.domains.settings.security.password_current =
                SecureCredential::from("");
            state.domains.settings.security.password_new =
                SecureCredential::from("");
            state.domains.settings.security.password_confirm =
                SecureCredential::from("");
            state.domains.settings.security.password_error = None;
            state.domains.settings.security.password_loading = false;
            state.domains.settings.security.showing_password_change = false;

            // TODO: Show success notification
            log::info!("Password changed successfully");
        }
        Err(error) => {
            state.domains.settings.security.password_error = Some(error);
            state.domains.settings.security.password_loading = false;
        }
    }
    DomainUpdateResult::task(Task::none())
}

/// Handle cancel password change
pub fn handle_cancel_password_change(state: &mut State) -> DomainUpdateResult {
    // Clear password fields
    state.domains.settings.security.password_current =
        SecureCredential::from("");
    state.domains.settings.security.password_new = SecureCredential::from("");
    state.domains.settings.security.password_confirm =
        SecureCredential::from("");
    state.domains.settings.security.password_error = None;
    state.domains.settings.security.password_loading = false;
    state.domains.settings.security.showing_password_change = false;
    DomainUpdateResult::task(Task::none())
}

/// Handle check if user has PIN
pub fn handle_check_user_has_pin(state: &mut State) -> DomainUpdateResult {
    let svc = state.domains.auth.state.auth_service.clone();
    let task = Task::perform(
        async move {
            let maybe_user = svc.get_current_user().await.ok().flatten();
            if let Some(user) = maybe_user {
                svc.check_device_auth(user.id)
                    .await
                    .map(|status| status.has_pin)
                    .unwrap_or(false)
            } else {
                false
            }
        },
        settings::Message::UserHasPinResult,
    );
    DomainUpdateResult::task(task.map(DomainMessage::Settings))
}

/// Handle user has PIN result
pub fn handle_user_has_pin_result(
    state: &mut State,
    has_pin: bool,
) -> DomainUpdateResult {
    state.domains.settings.security.has_pin = has_pin;
    DomainUpdateResult::task(Task::none())
}

/// Handle show set PIN modal
pub fn handle_show_set_pin(state: &mut State) -> DomainUpdateResult {
    state.domains.settings.security.showing_pin_change = true;
    state.domains.settings.security.pin_current = SecureCredential::from("");
    state.domains.settings.security.pin_new = SecureCredential::from("");
    state.domains.settings.security.pin_confirm = SecureCredential::from("");
    state.domains.settings.security.pin_error = None;
    state.domains.settings.security.pin_loading = false;
    DomainUpdateResult::task(Task::none())
}

/// Handle show change PIN modal
pub fn handle_show_change_pin(state: &mut State) -> DomainUpdateResult {
    state.domains.settings.security.showing_pin_change = true;
    state.domains.settings.security.pin_current = SecureCredential::from("");
    state.domains.settings.security.pin_new = SecureCredential::from("");
    state.domains.settings.security.pin_confirm = SecureCredential::from("");
    state.domains.settings.security.pin_error = None;
    state.domains.settings.security.pin_loading = false;
    DomainUpdateResult::task(Task::none())
}

/// Handle update current PIN
pub fn handle_update_pin_current(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    // Only allow digits and limit to 4 characters
    let filtered: String = value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(4)
        .collect();
    state.domains.settings.security.pin_current =
        SecureCredential::from(filtered);
    state.domains.settings.security.pin_error = None;
    DomainUpdateResult::task(Task::none())
}

/// Handle update new PIN
pub fn handle_update_pin_new(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    // Only allow digits and limit to 4 characters
    let filtered: String = value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(4)
        .collect();
    state.domains.settings.security.pin_new = SecureCredential::from(filtered);
    state.domains.settings.security.pin_error = None;
    DomainUpdateResult::task(Task::none())
}

/// Handle update confirm PIN
pub fn handle_update_pin_confirm(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    // Only allow digits and limit to 4 characters
    let filtered: String = value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(4)
        .collect();
    state.domains.settings.security.pin_confirm =
        SecureCredential::from(filtered);
    state.domains.settings.security.pin_error = None;
    DomainUpdateResult::task(Task::none())
}

/// Handle submit PIN change
pub fn handle_submit_pin_change(state: &mut State) -> DomainUpdateResult {
    // Clone the values we need before validation
    let is_new_pin = !state.domains.settings.security.has_pin;
    let pin_current = state.domains.settings.security.pin_current.clone();
    let pin_new = state.domains.settings.security.pin_new.clone();
    let pin_confirm = state.domains.settings.security.pin_confirm.clone();

    // Validate inputs
    if !is_new_pin && pin_current.is_empty() {
        state.domains.settings.security.pin_error =
            Some("Current PIN is required".to_string());
        return DomainUpdateResult::task(Task::none());
    }
    if pin_new.is_empty() {
        state.domains.settings.security.pin_error =
            Some("New PIN is required".to_string());
        return DomainUpdateResult::task(Task::none());
    }
    if pin_new.len() != 4 {
        state.domains.settings.security.pin_error =
            Some("PIN must be exactly 4 digits".to_string());
        return DomainUpdateResult::task(Task::none());
    }
    if !pin_new.as_str().chars().all(|c| c.is_ascii_digit()) {
        state.domains.settings.security.pin_error =
            Some("PIN must contain only digits".to_string());
        return DomainUpdateResult::task(Task::none());
    }
    if pin_new.as_str() != pin_confirm.as_str() {
        state.domains.settings.security.pin_error =
            Some("PINs do not match".to_string());
        return DomainUpdateResult::task(Task::none());
    }
    if !is_new_pin && pin_current.as_str() == pin_new.as_str() {
        state.domains.settings.security.pin_error =
            Some("New PIN must be different from current PIN".to_string());
        return DomainUpdateResult::task(Task::none());
    }

    // Set loading state
    state.domains.settings.security.pin_loading = true;

    // Create auth command based on whether this is a new PIN or a change
    let command = if is_new_pin {
        auth::AuthCommand::SetUserPin { pin: pin_new }
    } else {
        auth::AuthCommand::ChangeUserPin {
            current_pin: pin_current,
            new_pin: pin_new,
        }
    };

    // Emit cross-domain event to execute auth command
    DomainUpdateResult::with_events(
        Task::none(),
        vec![CrossDomainEvent::AuthCommandRequested(command)],
    )
}

/// Handle PIN change result
pub fn handle_pin_change_result(
    state: &mut State,
    result: Result<(), String>,
) -> DomainUpdateResult {
    match result {
        Ok(()) => {
            // Clear PIN fields on success
            state.domains.settings.security.pin_current =
                SecureCredential::from("");
            state.domains.settings.security.pin_new =
                SecureCredential::from("");
            state.domains.settings.security.pin_confirm =
                SecureCredential::from("");
            state.domains.settings.security.pin_error = None;
            state.domains.settings.security.pin_loading = false;
            state.domains.settings.security.showing_pin_change = false;
            state.domains.settings.security.has_pin = true;

            // TODO: Show success notification
            log::info!("PIN changed successfully");
        }
        Err(error) => {
            state.domains.settings.security.pin_error = Some(error);
            state.domains.settings.security.pin_loading = false;
        }
    }
    DomainUpdateResult::task(Task::none())
}

/// Handle cancel PIN change
pub fn handle_cancel_pin_change(state: &mut State) -> DomainUpdateResult {
    // Clear PIN fields
    state.domains.settings.security.pin_current = SecureCredential::from("");
    state.domains.settings.security.pin_new = SecureCredential::from("");
    state.domains.settings.security.pin_confirm = SecureCredential::from("");
    state.domains.settings.security.pin_error = None;
    state.domains.settings.security.pin_loading = false;
    state.domains.settings.security.showing_pin_change = false;
    DomainUpdateResult::task(Task::none())
}
