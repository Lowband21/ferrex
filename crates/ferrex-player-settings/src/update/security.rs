//! Security settings reducers.

use ferrex_player_auth::{
    messages::AuthCommand,
    pin_policy::{PinPolicyRules, validate_pin_with_policy},
    security::secure_credential::SecureCredential,
};

use crate::state::SecurityState;

use super::{SettingsEffect, SettingsUpdate};

pub fn show_change_password(security: &mut SecurityState) -> SettingsUpdate {
    security.showing_password_change = true;
    clear_password_fields(security);
    security.password_error = None;
    security.password_loading = false;
    SettingsUpdate::none()
}

pub fn update_password_current(
    security: &mut SecurityState,
    value: String,
) -> SettingsUpdate {
    security.password_current = SecureCredential::from(value);
    security.password_error = None;
    SettingsUpdate::none()
}

pub fn update_password_new(
    security: &mut SecurityState,
    value: String,
) -> SettingsUpdate {
    security.password_new = SecureCredential::from(value);
    security.password_error = None;
    SettingsUpdate::none()
}

pub fn update_password_confirm(
    security: &mut SecurityState,
    value: String,
) -> SettingsUpdate {
    security.password_confirm = SecureCredential::from(value);
    security.password_error = None;
    SettingsUpdate::none()
}

pub fn toggle_password_visibility(
    security: &mut SecurityState,
) -> SettingsUpdate {
    security.password_show = !security.password_show;
    SettingsUpdate::none()
}

pub fn submit_password_change(security: &mut SecurityState) -> SettingsUpdate {
    let password_current = security.password_current.clone();
    let password_new = security.password_new.clone();
    let password_confirm = security.password_confirm.clone();

    if let Err(message) = validate_password_change(
        &password_current,
        &password_new,
        &password_confirm,
    ) {
        security.password_error = Some(message);
        security.password_loading = false;
        return SettingsUpdate::none();
    }

    security.password_loading = true;
    security.password_error = None;
    SettingsUpdate::effect(SettingsEffect::AuthCommandRequested(
        AuthCommand::ChangePassword {
            old_password: password_current,
            new_password: password_new,
        },
    ))
}

pub fn password_change_result(
    security: &mut SecurityState,
    result: Result<(), String>,
) -> SettingsUpdate {
    match result {
        Ok(()) => {
            clear_password_fields(security);
            security.password_error = None;
            security.password_loading = false;
            security.showing_password_change = false;
        }
        Err(error) => {
            security.password_error = Some(error);
            security.password_loading = false;
        }
    }
    SettingsUpdate::none()
}

pub fn cancel_password_change(security: &mut SecurityState) -> SettingsUpdate {
    clear_password_fields(security);
    security.password_error = None;
    security.password_loading = false;
    security.showing_password_change = false;
    SettingsUpdate::none()
}

pub fn check_user_has_pin() -> SettingsUpdate {
    SettingsUpdate::effect(SettingsEffect::CheckUserHasPin)
}

pub fn user_has_pin_result(
    security: &mut SecurityState,
    has_pin: bool,
) -> SettingsUpdate {
    security.has_pin = has_pin;
    security.checking_pin_status = false;
    SettingsUpdate::none()
}

pub fn show_set_pin(security: &mut SecurityState) -> SettingsUpdate {
    security.showing_pin_change = true;
    clear_pin_fields(security);
    security.pin_error = None;
    security.pin_loading = false;
    SettingsUpdate::none()
}

pub fn show_change_pin(security: &mut SecurityState) -> SettingsUpdate {
    security.showing_pin_change = true;
    clear_pin_fields(security);
    security.pin_error = None;
    security.pin_loading = false;
    SettingsUpdate::none()
}

pub fn update_pin_current(
    security: &mut SecurityState,
    value: String,
    policy: PinPolicyRules,
) -> SettingsUpdate {
    security.pin_current = SecureCredential::from(filter_pin(value, policy));
    security.pin_error = None;
    SettingsUpdate::none()
}

pub fn update_pin_new(
    security: &mut SecurityState,
    value: String,
    policy: PinPolicyRules,
) -> SettingsUpdate {
    security.pin_new = SecureCredential::from(filter_pin(value, policy));
    security.pin_error = None;
    SettingsUpdate::none()
}

pub fn update_pin_confirm(
    security: &mut SecurityState,
    value: String,
    policy: PinPolicyRules,
) -> SettingsUpdate {
    security.pin_confirm = SecureCredential::from(filter_pin(value, policy));
    security.pin_error = None;
    SettingsUpdate::none()
}

pub fn submit_pin_change(
    security: &mut SecurityState,
    policy: PinPolicyRules,
) -> SettingsUpdate {
    let is_new_pin = !security.has_pin;
    let pin_current = security.pin_current.clone();
    let pin_new = security.pin_new.clone();
    let pin_confirm = security.pin_confirm.clone();

    if let Err(message) = validate_pin_change(
        is_new_pin,
        &pin_current,
        &pin_new,
        &pin_confirm,
        policy,
    ) {
        security.pin_error = Some(message);
        security.pin_loading = false;
        return SettingsUpdate::none();
    }

    security.pin_loading = true;
    security.pin_error = None;

    let command = if is_new_pin {
        AuthCommand::SetUserPin { pin: pin_new }
    } else {
        AuthCommand::ChangeUserPin {
            current_pin: pin_current,
            new_pin: pin_new,
        }
    };

    SettingsUpdate::effect(SettingsEffect::AuthCommandRequested(command))
}

pub fn submit_pin_removal(security: &mut SecurityState) -> SettingsUpdate {
    let pin_current = security.pin_current.clone();

    if !security.has_pin {
        security.pin_error = Some("No PIN is currently set".to_string());
        security.pin_loading = false;
        return SettingsUpdate::none();
    }

    if pin_current.is_empty() {
        security.pin_error =
            Some("Current PIN is required to remove PIN login".to_string());
        security.pin_loading = false;
        return SettingsUpdate::none();
    }

    security.pin_loading = true;
    security.pin_error = None;
    SettingsUpdate::effect(SettingsEffect::AuthCommandRequested(
        AuthCommand::RemoveUserPin {
            current_pin: pin_current,
        },
    ))
}

pub fn pin_change_result(
    security: &mut SecurityState,
    result: Result<(), String>,
) -> SettingsUpdate {
    match result {
        Ok(()) => {
            clear_pin_fields(security);
            security.pin_error = None;
            security.pin_loading = false;
            security.showing_pin_change = false;
            security.has_pin = true;
        }
        Err(error) => {
            security.pin_error = Some(error);
            security.pin_loading = false;
        }
    }
    SettingsUpdate::none()
}

pub fn pin_removal_result(
    security: &mut SecurityState,
    result: Result<(), String>,
) -> SettingsUpdate {
    match result {
        Ok(()) => {
            clear_pin_fields(security);
            security.pin_error = None;
            security.pin_loading = false;
            security.showing_pin_change = false;
            security.has_pin = false;
        }
        Err(error) => {
            security.pin_error = Some(error);
            security.pin_loading = false;
        }
    }
    SettingsUpdate::none()
}

pub fn cancel_pin_change(security: &mut SecurityState) -> SettingsUpdate {
    clear_pin_fields(security);
    security.pin_error = None;
    security.pin_loading = false;
    security.showing_pin_change = false;
    SettingsUpdate::none()
}

fn validate_password_change(
    current: &SecureCredential,
    new: &SecureCredential,
    confirm: &SecureCredential,
) -> Result<(), String> {
    if current.is_empty() {
        return Err("Current password is required".to_string());
    }
    if new.is_empty() {
        return Err("New password is required".to_string());
    }
    if new.len() < 8 {
        return Err("Password must be at least 8 characters".to_string());
    }
    if new != confirm {
        return Err("Passwords do not match".to_string());
    }
    if current.as_str() == new.as_str() {
        return Err(
            "New password must be different from current password".to_string()
        );
    }

    let has_upper = new.as_str().chars().any(char::is_uppercase);
    let has_lower = new.as_str().chars().any(char::is_lowercase);
    let has_digit = new.as_str().chars().any(|c| c.is_ascii_digit());

    if !has_upper || !has_lower || !has_digit {
        return Err("Password must contain uppercase, lowercase, and numbers"
            .to_string());
    }

    Ok(())
}

fn validate_pin_change(
    is_new_pin: bool,
    current: &SecureCredential,
    new: &SecureCredential,
    confirm: &SecureCredential,
    policy: PinPolicyRules,
) -> Result<(), String> {
    if !is_new_pin && current.is_empty() {
        return Err("Current PIN is required".to_string());
    }
    if new.is_empty() {
        return Err("New PIN is required".to_string());
    }
    validate_pin_with_policy(new.as_str(), policy)?;
    if new.as_str() != confirm.as_str() {
        return Err("PINs do not match".to_string());
    }
    if !is_new_pin && current.as_str() == new.as_str() {
        return Err("New PIN must be different from current PIN".to_string());
    }

    Ok(())
}

fn filter_pin(value: String, policy: PinPolicyRules) -> String {
    value
        .chars()
        .filter(|c| !policy.require_numeric || c.is_ascii_digit())
        .take(policy.max_length)
        .collect()
}

fn clear_password_fields(security: &mut SecurityState) {
    security.password_current = SecureCredential::from("");
    security.password_new = SecureCredential::from("");
    security.password_confirm = SecureCredential::from("");
}

fn clear_pin_fields(security: &mut SecurityState) {
    security.pin_current = SecureCredential::from("");
    security.pin_new = SecureCredential::from("");
    security.pin_confirm = SecureCredential::from("");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_change_requests_auth_command_for_valid_inputs() {
        let mut security = SecurityState::default();
        security.password_current = SecureCredential::from("OldPass123");
        security.password_new = SecureCredential::from("NewPass123");
        security.password_confirm = SecureCredential::from("NewPass123");

        let update = submit_password_change(&mut security);

        assert!(security.password_loading);
        assert!(matches!(
            update.effects.as_slice(),
            [SettingsEffect::AuthCommandRequested(
                AuthCommand::ChangePassword { .. }
            )]
        ));
    }

    #[test]
    fn pin_removal_result_clears_pin_state() {
        let mut security = SecurityState {
            has_pin: true,
            ..Default::default()
        };
        security.pin_current = SecureCredential::from("2580");
        let update = submit_pin_removal(&mut security);
        assert!(security.pin_loading);
        assert!(matches!(
            update.effects.as_slice(),
            [SettingsEffect::AuthCommandRequested(
                AuthCommand::RemoveUserPin { .. }
            )]
        ));

        pin_removal_result(&mut security, Ok(()));
        assert!(!security.has_pin);
        assert!(!security.pin_loading);
        assert!(security.pin_current.is_empty());
    }
}
