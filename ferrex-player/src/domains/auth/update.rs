use crate::common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult};
use crate::domains::auth::messages as auth;
use crate::state_refactored::State;
use iced::Task;
use log::{error, info};

// Helper macro to wrap task results
macro_rules! wrap_task {
    ($task:expr_2021) => {
        DomainUpdateResult::task($task.map(DomainMessage::Auth))
    };
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update_auth(state: &mut State, message: auth::Message) -> DomainUpdateResult {
    match message {
        // Core auth flow
        auth::Message::CheckAuthStatus => {
            wrap_task!(super::update_handlers::auth_updates::handle_check_auth_status(state))
        }

        auth::Message::AuthStatusConfirmedWithPin => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_auth_status_confirmed_with_pin(state)
            )
        }

        auth::Message::CheckSetupStatus => {
            wrap_task!(super::update_handlers::auth_updates::handle_check_setup_status(state))
        }

        auth::Message::SetupStatusChecked(needs_setup) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_setup_status_checked(
                    state,
                    needs_setup
                )
            )
        }

        auth::Message::AutoLoginCheckComplete => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_auto_login_check_complete(state)
            )
        }

        auth::Message::AutoLoginSuccessful(user) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_auto_login_successful(state, user)
            )
        }

        // User management
        auth::Message::LoadUsers => wrap_task!(
            super::update_handlers::auth_updates::handle_load_users(state)
        ),

        auth::Message::UsersLoaded(result) => {
            wrap_task!(super::update_handlers::auth_updates::handle_users_loaded(
                state, result
            ))
        }

        auth::Message::SelectUser(user_id) => {
            wrap_task!(super::update_handlers::auth_updates::handle_select_user(
                state, user_id
            ))
        }

        auth::Message::ShowCreateUser => {
            wrap_task!(super::update_handlers::auth_updates::handle_show_create_user(state))
        }

        auth::Message::BackToUserSelection => {
            wrap_task!(super::update_handlers::auth_updates::handle_back_to_user_selection(state))
        }

        // PIN authentication
        auth::Message::ShowPinEntry(user) => {
            wrap_task!(super::update_handlers::auth_updates::handle_show_pin_entry(
                state, user
            ))
        }

        auth::Message::PinDigitPressed(digit) => {
            wrap_task!(super::update_handlers::auth_updates::handle_pin_digit_pressed(state, digit))
        }

        auth::Message::PinBackspace => {
            wrap_task!(super::update_handlers::auth_updates::handle_pin_backspace(
                state
            ))
        }

        auth::Message::PinClear => wrap_task!(
            super::update_handlers::auth_updates::handle_pin_clear(state)
        ),

        auth::Message::PinSubmit => wrap_task!(
            super::update_handlers::auth_updates::handle_pin_submit(state)
        ),

        // Login results
        auth::Message::LoginSuccess(user, permissions) => {
            // Handle login success with cross-domain events
            let task = super::update_handlers::auth_updates::handle_login_success(
                state,
                user.clone(),
                permissions.clone(),
            );
            let events = vec![
                CrossDomainEvent::UserAuthenticated(user, permissions),
                CrossDomainEvent::AuthenticationComplete,
            ];
            DomainUpdateResult::with_events(task.map(DomainMessage::Auth), events)
        }

        auth::Message::WatchStatusLoaded(result) => {
            let task = super::update_handlers::auth_updates::handle_watch_status_loaded(
                state,
                result.clone(),
            );

            // For successful manual login, emit AuthenticationComplete to trigger library loading
            // This only happens after manual auth (not auto-login which goes through LoginSuccess)
            let events = if result.is_ok() && state.is_authenticated {
                log::info!(
                    "[Auth] WatchStatusLoaded after manual auth - emitting AuthenticationComplete"
                );
                vec![CrossDomainEvent::AuthenticationComplete]
            } else {
                vec![]
            };

            DomainUpdateResult::with_events(task.map(DomainMessage::Auth), events)
        }

        auth::Message::Logout => {
            wrap_task!(super::update_handlers::auth_updates::handle_logout(state))
        }

        auth::Message::LogoutComplete => {
            // Handle logout complete with cross-domain events
            let task = super::update_handlers::auth_updates::handle_logout_complete(state);
            let events = vec![
                CrossDomainEvent::ClearMediaStore,
                CrossDomainEvent::ClearLibraries,
                CrossDomainEvent::ClearCurrentShowData,
                CrossDomainEvent::UserLoggedOut,
            ];
            DomainUpdateResult::with_events(task.map(DomainMessage::Auth), events)
        }

        // Password login
        auth::Message::ShowPasswordLogin(username) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_show_password_login(state, username)
            )
        }

        auth::Message::PasswordLoginUpdateUsername(username) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_password_login_update_username(
                    state, username,
                )
            )
        }

        auth::Message::PasswordLoginUpdatePassword(password) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_password_login_update_password(
                    state, password,
                )
            )
        }

        auth::Message::PasswordLoginToggleVisibility => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_password_login_toggle_visibility(
                    state
                )
            )
        }

        auth::Message::PasswordLoginToggleRemember => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_password_login_toggle_remember(state)
            )
        }

        auth::Message::PasswordLoginSubmit => {
            wrap_task!(super::update_handlers::auth_updates::handle_password_login_submit(state))
        }

        // Device auth flow
        auth::Message::DeviceStatusChecked(user, result) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_device_status_checked(
                    state, user, result
                )
            )
        }

        auth::Message::UpdateCredential(input) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_auth_flow_update_credential(
                    state, input
                )
            )
        }

        auth::Message::SubmitCredential => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_auth_flow_submit_credential(state)
            )
        }

        auth::Message::TogglePasswordVisibility => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_auth_flow_toggle_password_visibility(
                    state
                )
            )
        }

        auth::Message::ToggleRememberDevice => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_auth_flow_toggle_remember_device(
                    state
                )
            )
        }

        auth::Message::AuthResult(result) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_auth_flow_auth_result(state, result)
            )
        }

        auth::Message::SetupPin => {
            wrap_task!(Task::none()) // TODO: Implement
        }

        auth::Message::UpdatePin(_pin) => {
            wrap_task!(Task::none()) // TODO: Implement
        }

        auth::Message::UpdateConfirmPin(_pin) => {
            wrap_task!(Task::none()) // TODO: Implement
        }

        auth::Message::SubmitPin => {
            wrap_task!(super::update_handlers::auth_updates::handle_auth_flow_submit_pin(state))
        }

        auth::Message::PinSet(result) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_auth_flow_pin_set(state, result)
            )
        }

        auth::Message::Retry => {
            wrap_task!(Task::none()) // TODO: Implement
        }

        auth::Message::Back => {
            wrap_task!(Task::none()) // TODO: Implement
        }

        // First-run setup
        auth::Message::FirstRunUpdateUsername(username) => {
            wrap_task!(
                super::update_handlers::first_run_updates::handle_update_username(state, username)
            )
        }

        auth::Message::FirstRunUpdateDisplayName(display_name) => {
            wrap_task!(
                super::update_handlers::first_run_updates::handle_update_display_name(
                    state,
                    display_name,
                )
            )
        }

        auth::Message::FirstRunUpdatePassword(password) => {
            wrap_task!(
                super::update_handlers::first_run_updates::handle_update_password(state, password)
            )
        }

        auth::Message::FirstRunUpdateConfirmPassword(confirm_password) => {
            wrap_task!(
                super::update_handlers::first_run_updates::handle_update_confirm_password(
                    state,
                    confirm_password,
                )
            )
        }

        auth::Message::FirstRunTogglePasswordVisibility => {
            wrap_task!(
                super::update_handlers::first_run_updates::handle_toggle_password_visibility(state)
            )
        }

        auth::Message::FirstRunSubmit => {
            wrap_task!(super::update_handlers::first_run_updates::handle_submit(
                state
            ))
        }

        auth::Message::FirstRunSuccess => {
            wrap_task!(super::update_handlers::first_run_updates::handle_success(
                state
            ))
        }

        auth::Message::FirstRunError(error) => {
            wrap_task!(super::update_handlers::first_run_updates::handle_error(
                state, error
            ))
        }

        // Admin PIN unlock management
        auth::Message::EnableAdminPinUnlock => {
            wrap_task!(super::update_handlers::auth_updates::handle_enable_admin_pin_unlock(state))
        }

        auth::Message::DisableAdminPinUnlock => {
            wrap_task!(super::update_handlers::auth_updates::handle_disable_admin_pin_unlock(state))
        }

        auth::Message::AdminPinUnlockToggled(result) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_admin_pin_unlock_toggled(
                    state, result
                )
            )
        }

        // Admin setup flow
        auth::Message::UpdateSetupField(field) => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_update_setup_field(state, field)
            )
        }

        auth::Message::ToggleSetupPasswordVisibility => {
            wrap_task!(
                super::update_handlers::auth_updates::handle_toggle_setup_password_visibility(
                    state
                )
            )
        }

        auth::Message::SubmitSetup => {
            wrap_task!(super::update_handlers::auth_updates::handle_submit_setup(
                state
            ))
        }

        auth::Message::SetupComplete(access_token, refresh_token) => {
            wrap_task!(super::update_handlers::auth_updates::handle_setup_complete(
                state,
                access_token,
                refresh_token,
            ))
        }

        auth::Message::SetupError(error) => {
            wrap_task!(super::update_handlers::auth_updates::handle_setup_error(
                state, error
            ))
        }

        // Command execution
        auth::Message::ExecuteCommand(command) => handle_auth_command(state, command),

        auth::Message::CommandResult(command, result) => {
            handle_auth_command_result(state, command, result)
        }
    }
}

/// Handle auth command execution
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn handle_auth_command(state: &mut State, command: auth::AuthCommand) -> DomainUpdateResult {
    info!("Executing auth command: {}", command.sanitized_display());

    // RUS-136: Use trait-based auth_service instead of auth_manager
    let auth_service = state.domains.auth.state.auth_service.clone();

    let task = Task::perform(
        async move {
            let result = execute_auth_command(&auth_service, &command).await;
            (command, result)
        },
        |(cmd, result)| auth::Message::CommandResult(cmd, result),
    );
    wrap_task!(task)
}

/// Execute an auth command using the auth service
async fn execute_auth_command(
    auth_service: &std::sync::Arc<dyn crate::infrastructure::services::auth::AuthService>,
    command: &auth::AuthCommand,
) -> auth::AuthCommandResult {
    match command {
        auth::AuthCommand::ChangePassword {
            old_password,
            new_password,
        } => {
            // Note: This would require a change_password method on AuthManager
            // For now, return not implemented
            auth::AuthCommandResult::Error("Password change not yet implemented".to_string())
        }

        auth::AuthCommand::SetUserPin { pin } => {
            match auth_service
                .set_device_pin(pin.expose_secret().to_string())
                .await
            {
                Ok(()) => auth::AuthCommandResult::Success,
                Err(e) => auth::AuthCommandResult::Error(e.to_string()),
            }
        }

        auth::AuthCommand::RemoveUserPin => {
            // Note: This would require a remove_device_pin method on AuthManager
            // For now, return not implemented
            auth::AuthCommandResult::Error("PIN removal not yet implemented".to_string())
        }

        auth::AuthCommand::EnableAdminPinUnlock => {
            match auth_service.enable_admin_pin_unlock().await {
                Ok(()) => auth::AuthCommandResult::Success,
                Err(e) => auth::AuthCommandResult::Error(e.to_string()),
            }
        }

        auth::AuthCommand::ChangeUserPin {
            current_pin,
            new_pin,
        } => {
            // TODO: Add change_device_pin method to AuthService trait
            auth::AuthCommandResult::Error(
                "PIN change not yet implemented in AuthService".to_string(),
            )
        }
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn handle_auth_command_result(
    _state: &mut State,
    command: auth::AuthCommand,
    result: auth::AuthCommandResult,
) -> DomainUpdateResult {
    match &result {
        auth::AuthCommandResult::Success => {
            info!("Auth command executed successfully: {}", command.name());

            // Emit completion event for other domains to handle
            let events = match command {
                auth::AuthCommand::EnableAdminPinUnlock => {
                    // Also emit configuration change event
                    vec![
                        crate::common::messages::CrossDomainEvent::AuthCommandCompleted(
                            command.clone(),
                            result.clone(),
                        ),
                        crate::common::messages::CrossDomainEvent::AuthConfigurationChanged,
                    ]
                }
                _ => vec![
                    crate::common::messages::CrossDomainEvent::AuthCommandCompleted(
                        command.clone(),
                        result.clone(),
                    ),
                ],
            };

            DomainUpdateResult::with_events(Task::none(), events)
        }
        auth::AuthCommandResult::Error(error) => {
            error!("Auth command failed: {} - {}", command.name(), error);

            // Emit completion event even for failures so settings can handle the error
            DomainUpdateResult::with_events(
                Task::none(),
                vec![
                    crate::common::messages::CrossDomainEvent::AuthCommandCompleted(
                        command, result,
                    ),
                ],
            )
        }
    }
}
