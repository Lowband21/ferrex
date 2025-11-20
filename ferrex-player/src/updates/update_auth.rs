use crate::{messages::auth, state::State};
use iced::Task;
use log::{error, info};

/// Handle auth domain messages
pub fn update_auth(state: &mut State, message: auth::Message) -> Task<auth::Message> {
    match message {
        // Core auth flow
        auth::Message::CheckAuthStatus => super::auth_updates::handle_check_auth_status(state),

        auth::Message::AuthStatusConfirmedWithPin => {
            super::auth_updates::handle_auth_status_confirmed_with_pin(state)
        }

        auth::Message::CheckSetupStatus => super::auth_updates::handle_check_setup_status(state),

        auth::Message::SetupStatusChecked(needs_setup) => {
            super::auth_updates::handle_setup_status_checked(state, needs_setup)
        }

        // User management
        auth::Message::LoadUsers => super::auth_updates::handle_load_users(state),

        auth::Message::UsersLoaded(result) => {
            super::auth_updates::handle_users_loaded(state, result)
        }

        auth::Message::SelectUser(user_id) => {
            super::auth_updates::handle_select_user(state, user_id)
        }

        auth::Message::ShowCreateUser => super::auth_updates::handle_show_create_user(state),

        auth::Message::BackToUserSelection => {
            super::auth_updates::handle_back_to_user_selection(state)
        }

        // PIN authentication
        auth::Message::ShowPinEntry(user) => {
            super::auth_updates::handle_show_pin_entry(state, user)
        }

        auth::Message::PinDigitPressed(digit) => {
            super::auth_updates::handle_pin_digit_pressed(state, digit)
        }

        auth::Message::PinBackspace => super::auth_updates::handle_pin_backspace(state),

        auth::Message::PinClear => super::auth_updates::handle_pin_clear(state),

        auth::Message::PinSubmit => super::auth_updates::handle_pin_submit(state),

        // Login results
        auth::Message::LoginSuccess(user, permissions) => {
            super::auth_updates::handle_login_success(state, user, permissions)
        }

        auth::Message::LoginError(error) => super::auth_updates::handle_login_error(state, error),

        auth::Message::WatchStatusLoaded(result) => {
            super::auth_updates::handle_watch_status_loaded(state, result)
        }

        auth::Message::Logout => super::auth_updates::handle_logout(state),

        auth::Message::LogoutComplete => super::auth_updates::handle_logout_complete(state),

        // Password login
        auth::Message::ShowPasswordLogin(username) => {
            super::auth_updates::handle_show_password_login(state, username)
        }

        auth::Message::PasswordLoginUpdateUsername(username) => {
            super::auth_updates::handle_password_login_update_username(state, username)
        }

        auth::Message::PasswordLoginUpdatePassword(password) => {
            super::auth_updates::handle_password_login_update_password(state, password)
        }

        auth::Message::PasswordLoginToggleVisibility => {
            super::auth_updates::handle_password_login_toggle_visibility(state)
        }

        auth::Message::PasswordLoginToggleRemember => {
            super::auth_updates::handle_password_login_toggle_remember(state)
        }

        auth::Message::PasswordLoginSubmit => {
            super::auth_updates::handle_password_login_submit(state)
        }

        // Device auth flow
        auth::Message::DeviceStatusChecked(user, result) => {
            super::auth_updates::handle_device_status_checked(state, user, result)
        }

        auth::Message::UpdateCredential(input) => {
            super::auth_updates::handle_auth_flow_update_credential(state, input)
        }

        auth::Message::SubmitCredential => {
            super::auth_updates::handle_auth_flow_submit_credential(state)
        }

        auth::Message::TogglePasswordVisibility => {
            super::auth_updates::handle_auth_flow_toggle_password_visibility(state)
        }

        auth::Message::ToggleRememberDevice => {
            super::auth_updates::handle_auth_flow_toggle_remember_device(state)
        }

        auth::Message::AuthResult(result) => {
            super::auth_updates::handle_auth_flow_auth_result(state, result)
        }

        auth::Message::SetupPin => {
            Task::none() // TODO: Implement
        }

        auth::Message::UpdatePin(pin) => {
            Task::none() // TODO: Implement
        }

        auth::Message::UpdateConfirmPin(pin) => {
            Task::none() // TODO: Implement
        }

        auth::Message::SubmitPin => super::auth_updates::handle_auth_flow_submit_pin(state),

        auth::Message::PinSet(result) => {
            super::auth_updates::handle_auth_flow_pin_set(state, result)
        }

        auth::Message::Retry => {
            Task::none() // TODO: Implement
        }

        auth::Message::Back => {
            Task::none() // TODO: Implement
        }

        // First-run setup
        auth::Message::FirstRunUpdateUsername(username) => {
            super::first_run_updates::handle_update_username(state, username)
        }

        auth::Message::FirstRunUpdateDisplayName(display_name) => {
            super::first_run_updates::handle_update_display_name(state, display_name)
        }

        auth::Message::FirstRunUpdatePassword(password) => {
            super::first_run_updates::handle_update_password(state, password)
        }

        auth::Message::FirstRunUpdateConfirmPassword(confirm_password) => {
            super::first_run_updates::handle_update_confirm_password(state, confirm_password)
        }

        auth::Message::FirstRunTogglePasswordVisibility => {
            super::first_run_updates::handle_toggle_password_visibility(state)
        }

        auth::Message::FirstRunSubmit => super::first_run_updates::handle_submit(state),

        auth::Message::FirstRunSuccess => super::first_run_updates::handle_success(state),

        auth::Message::FirstRunError(error) => super::first_run_updates::handle_error(state, error),

        // Admin PIN unlock management
        auth::Message::EnableAdminPinUnlock => {
            super::auth_updates::handle_enable_admin_pin_unlock(state)
        }
        
        auth::Message::DisableAdminPinUnlock => {
            super::auth_updates::handle_disable_admin_pin_unlock(state)
        }
        
        auth::Message::AdminPinUnlockToggled(result) => {
            super::auth_updates::handle_admin_pin_unlock_toggled(state, result)
        }
        
        // Admin setup flow
        auth::Message::UpdateSetupField(field) => {
            super::auth_updates::handle_update_setup_field(state, field)
        }
        
        auth::Message::ToggleSetupPasswordVisibility => {
            super::auth_updates::handle_toggle_setup_password_visibility(state)
        }
        
        auth::Message::SubmitSetup => {
            super::auth_updates::handle_submit_setup(state)
        }
        
        auth::Message::SetupComplete(access_token, refresh_token) => {
            super::auth_updates::handle_setup_complete(state, access_token, refresh_token)
        }
        
        auth::Message::SetupError(error) => {
            super::auth_updates::handle_setup_error(state, error)
        }
        
        // Command execution
        auth::Message::ExecuteCommand(command) => {
            handle_auth_command(state, command)
        }
        
        auth::Message::CommandResult(command, result) => {
            handle_auth_command_result(state, command, result)
        }

        // Internal cross-domain coordination
        auth::Message::_EmitCrossDomainEvent(_) => {
            // This should be handled by the main update loop, not here
            log::warn!("_EmitCrossDomainEvent should be handled by main update loop");
            Task::none()
        }
    }
}

/// Handle auth command execution
fn handle_auth_command(state: &mut State, command: auth::AuthCommand) -> Task<auth::Message> {
    info!("Executing auth command: {}", command.sanitized_display());
    
    let auth_manager = state.auth_manager.clone();
    
    Task::perform(
        async move {
            let result = execute_auth_command(&auth_manager, &command).await;
            (command, result)
        },
        |(cmd, result)| auth::Message::CommandResult(cmd, result),
    )
}

/// Execute an auth command using the auth manager
async fn execute_auth_command(
    auth_manager: &Option<crate::auth_manager::AuthManager>,
    command: &auth::AuthCommand,
) -> auth::AuthCommandResult {
    match command {
        auth::AuthCommand::ChangePassword { old_password, new_password } => {
            // Note: This would require a change_password method on AuthManager
            // For now, return not implemented
            auth::AuthCommandResult::Error("Password change not yet implemented".to_string())
        }
        
        auth::AuthCommand::SetDevicePin { pin } => {
            match auth_manager {
                Some(manager) => {
                    match manager.set_device_pin(pin.expose_secret().to_string()).await {
                        Ok(()) => auth::AuthCommandResult::Success,
                        Err(e) => auth::AuthCommandResult::Error(e.to_string()),
                    }
                }
                None => auth::AuthCommandResult::Error("Auth manager not available".to_string()),
            }
        }
        
        auth::AuthCommand::RemoveDevicePin => {
            // Note: This would require a remove_device_pin method on AuthManager
            // For now, return not implemented
            auth::AuthCommandResult::Error("PIN removal not yet implemented".to_string())
        }
        
        auth::AuthCommand::EnableAdminPinUnlock => {
            match auth_manager {
                Some(manager) => {
                    match manager.enable_admin_pin_unlock().await {
                        Ok(()) => auth::AuthCommandResult::Success,
                        Err(e) => auth::AuthCommandResult::Error(e.to_string()),
                    }
                }
                None => auth::AuthCommandResult::Error("Auth manager not available".to_string()),
            }
        }
        
        auth::AuthCommand::ChangeDevicePin { current_pin, new_pin } => {
            match auth_manager {
                Some(manager) => {
                    match manager.change_device_pin(
                        current_pin.expose_secret().to_string(),
                        new_pin.expose_secret().to_string(),
                    ).await {
                        Ok(()) => auth::AuthCommandResult::Success,
                        Err(e) => auth::AuthCommandResult::Error(e.to_string()),
                    }
                }
                None => auth::AuthCommandResult::Error("Auth manager not available".to_string()),
            }
        }
    }
}

/// Handle auth command result
fn handle_auth_command_result(
    state: &mut State,
    command: auth::AuthCommand,
    result: auth::AuthCommandResult,
) -> Task<auth::Message> {
    match &result {
        auth::AuthCommandResult::Success => {
            info!("Auth command executed successfully: {}", command.name());
            
            // Emit completion event for other domains to handle
            let completion_task = Task::done(auth::Message::_EmitCrossDomainEvent(
                crate::messages::CrossDomainEvent::AuthCommandCompleted(command.clone(), result.clone())
            ));
            
            // Handle specific command side effects
            match command {
                auth::AuthCommand::EnableAdminPinUnlock => {
                    // Also emit configuration change event
                    Task::batch([
                        completion_task,
                        Task::done(auth::Message::_EmitCrossDomainEvent(
                            crate::messages::CrossDomainEvent::AuthConfigurationChanged
                        ))
                    ])
                }
                _ => completion_task,
            }
        }
        auth::AuthCommandResult::Error(error) => {
            error!("Auth command failed: {} - {}", command.name(), error);
            
            // Emit completion event even for failures so settings can handle the error
            Task::done(auth::Message::_EmitCrossDomainEvent(
                crate::messages::CrossDomainEvent::AuthCommandCompleted(command, result)
            ))
        }
    }
}
