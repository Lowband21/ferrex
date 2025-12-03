use super::update_handlers::*;
use crate::common::focus::{FocusArea, FocusMessage};
use crate::common::messages::{
    CrossDomainEvent, DomainMessage, DomainUpdateResult,
};
use crate::domains::auth::messages as auth;
use crate::state::State;
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
pub fn update_auth(
    state: &mut State,
    message: auth::AuthMessage,
) -> DomainUpdateResult {
    match message {
        // Core auth flow
        auth::AuthMessage::CheckAuthStatus => {
            wrap_task!(handle_check_auth_status(state))
        }

        auth::AuthMessage::AuthStatusConfirmedWithPin => {
            wrap_task!(handle_auth_status_confirmed_with_pin(state))
        }

        auth::AuthMessage::CheckSetupStatus => {
            wrap_task!(handle_check_setup_status(state))
        }

        auth::AuthMessage::SetupStatusChecked(status) => {
            let needs_setup = status.needs_setup;
            let setup_task = handle_setup_status_checked(state, status)
                .map(DomainMessage::Auth);

            // Rationale: Activate focus for the appropriate auth form based on setup status.
            // When not first-run, we show the pre-auth login and should enable Tab traversal.
            let focus_task = if needs_setup {
                Task::done(DomainMessage::Focus(FocusMessage::Activate(
                    FocusArea::AuthFirstRunSetup,
                )))
            } else {
                Task::done(DomainMessage::Focus(FocusMessage::Activate(
                    FocusArea::AuthPreAuthLogin,
                )))
            };

            DomainUpdateResult::task(Task::batch(vec![setup_task, focus_task]))
        }

        auth::AuthMessage::AutoLoginCheckComplete => {
            wrap_task!(handle_auto_login_check_complete(state))
        }

        auth::AuthMessage::AutoLoginSuccessful(user) => {
            wrap_task!(handle_auto_login_successful(state, user))
        }

        // User management
        auth::AuthMessage::LoadUsers => wrap_task!(handle_load_users(state)),

        auth::AuthMessage::UsersLoaded(result) => {
            wrap_task!(handle_users_loaded(state, result,))
        }

        // Pre-auth login
        auth::AuthMessage::PreAuthUpdateUsername(value) => {
            wrap_task!(handle_pre_auth_update_username(state, value))
        }
        auth::AuthMessage::PreAuthTogglePasswordVisibility => {
            wrap_task!(handle_pre_auth_toggle_password_visibility(state))
        }
        auth::AuthMessage::PreAuthToggleRememberDevice => {
            wrap_task!(handle_pre_auth_toggle_remember_device(state))
        }
        auth::AuthMessage::PreAuthSubmit => {
            wrap_task!(handle_pre_auth_submit(state))
        }

        auth::AuthMessage::SelectUser(user_id) => {
            wrap_task!(handle_select_user(state, user_id))
        }

        auth::AuthMessage::ShowCreateUser => {
            wrap_task!(handle_show_create_user(state))
        }

        auth::AuthMessage::BackToUserSelection => {
            wrap_task!(handle_back_to_user_selection(state))
        }

        // Login results
        auth::AuthMessage::LoginSuccess(user, permissions) => {
            // Handle login success with cross-domain events
            let task =
                handle_login_success(state, user.clone(), permissions.clone());
            let events = vec![
                CrossDomainEvent::UserAuthenticated(user, permissions),
                CrossDomainEvent::AuthenticationComplete,
            ];
            let focus_clear =
                Task::done(DomainMessage::Focus(FocusMessage::Clear));
            DomainUpdateResult::with_events(
                Task::batch(vec![task.map(DomainMessage::Auth), focus_clear]),
                events,
            )
        }

        auth::AuthMessage::WatchStatusLoaded(result) => {
            // Watch-state improves the UX, but should not control overall auth completion.
            // Auth completion is centralized in the LoginSuccess path only.
            let task = handle_watch_status_loaded(state, result.clone());
            DomainUpdateResult::task(task.map(DomainMessage::Auth))
        }

        auth::AuthMessage::Logout => {
            wrap_task!(handle_logout(state))
        }

        auth::AuthMessage::LogoutComplete => {
            // Handle logout complete with cross-domain events
            let task = handle_logout_complete(state);
            let events = vec![
                CrossDomainEvent::ClearMediaStore,
                CrossDomainEvent::ClearLibraries,
                CrossDomainEvent::ClearCurrentShowData,
                CrossDomainEvent::UserLoggedOut,
            ];
            DomainUpdateResult::with_events(
                task.map(DomainMessage::Auth),
                events,
            )
        }

        // Device auth flow
        auth::AuthMessage::DeviceStatusChecked(user, result) => {
            let device_task = handle_device_status_checked(state, user, result)
                .map(DomainMessage::Auth);

            let focus_task = match &state.domains.auth.state.auth_flow {
                crate::domains::auth::types::AuthenticationFlow::EnteringCredentials {
                    input_type: crate::domains::auth::types::CredentialType::Password,
                    ..
                } => Task::done(DomainMessage::Focus(FocusMessage::Activate(
                    FocusArea::AuthPasswordEntry,
                ))),
                crate::domains::auth::types::AuthenticationFlow::EnteringCredentials { .. } => {
                    Task::done(DomainMessage::Focus(FocusMessage::Clear))
                }
                _ => Task::none(),
            };

            DomainUpdateResult::task(Task::batch(vec![device_task, focus_task]))
        }

        auth::AuthMessage::UpdateCredential(input) => {
            wrap_task!(handle_auth_flow_update_credential(state, input))
        }

        auth::AuthMessage::SubmitCredential => {
            wrap_task!(handle_auth_flow_submit_credential(state))
        }

        auth::AuthMessage::TogglePasswordVisibility => {
            wrap_task!(handle_auth_flow_toggle_password_visibility(state))
        }

        auth::AuthMessage::ToggleRememberDevice => {
            wrap_task!(handle_auth_flow_toggle_remember_device(state))
        }

        auth::AuthMessage::RememberDeviceSynced(enabled) => {
            wrap_task!(handle_remember_device_synced(state, enabled))
        }

        auth::AuthMessage::AuthResult(result) => {
            wrap_task!(handle_auth_flow_auth_result(state, result))
        }

        auth::AuthMessage::SetupPin => {
            wrap_task!(handle_auth_flow_setup_pin(state))
        }

        auth::AuthMessage::UpdatePin(pin) => {
            wrap_task!(handle_auth_flow_update_pin(state, pin))
        }

        auth::AuthMessage::UpdateConfirmPin(pin) => {
            wrap_task!(handle_auth_flow_update_confirm_pin(state, pin))
        }

        auth::AuthMessage::SubmitPin => {
            wrap_task!(handle_auth_flow_submit_pin(state))
        }

        auth::AuthMessage::PinSet(result) => {
            wrap_task!(handle_auth_flow_pin_set(state, result))
        }

        auth::AuthMessage::Retry => wrap_task!(handle_auth_flow_retry(state)),

        auth::AuthMessage::Back => wrap_task!(handle_auth_flow_back(state)),

        // Admin PIN unlock management
        auth::AuthMessage::EnableAdminPinUnlock => {
            wrap_task!(handle_enable_admin_pin_unlock(state))
        }

        auth::AuthMessage::DisableAdminPinUnlock => {
            wrap_task!(handle_disable_admin_pin_unlock(state))
        }

        auth::AuthMessage::AdminPinUnlockToggled(result) => {
            wrap_task!(handle_admin_pin_unlock_toggled(state, result))
        }

        // Admin setup flow
        auth::AuthMessage::UpdateSetupField(field) => {
            wrap_task!(handle_update_setup_field(state, field))
        }

        auth::AuthMessage::ToggleSetupPasswordVisibility => {
            wrap_task!(handle_toggle_setup_password_visibility(state))
        }

        auth::AuthMessage::SubmitSetup => {
            wrap_task!(handle_submit_setup(state))
        }

        auth::AuthMessage::SetupComplete(access_token, refresh_token) => {
            wrap_task!(handle_setup_complete(
                state,
                access_token,
                refresh_token,
            ))
        }

        auth::AuthMessage::SetupError(error) => {
            wrap_task!(handle_setup_error(state, error))
        }

        // Setup wizard navigation
        auth::AuthMessage::SetupNextStep => {
            wrap_task!(handle_setup_next_step(state))
        }

        auth::AuthMessage::SetupPreviousStep => {
            wrap_task!(handle_setup_previous_step(state))
        }

        auth::AuthMessage::SkipPinSetup => {
            wrap_task!(handle_skip_pin_setup(state))
        }

        auth::AuthMessage::SetupAnimationTick(delta) => {
            wrap_task!(handle_setup_animation_tick(state, delta))
        }

        // Device claim flow
        auth::AuthMessage::StartSetupClaim => {
            wrap_task!(handle_start_setup_claim(state))
        }

        auth::AuthMessage::ClaimStarted(result) => {
            wrap_task!(handle_claim_started(state, result))
        }

        auth::AuthMessage::ConfirmSetupClaim => {
            wrap_task!(handle_confirm_setup_claim(state))
        }

        auth::AuthMessage::ClaimConfirmed(result) => {
            wrap_task!(handle_claim_confirmed(state, result))
        }

        // Command execution
        auth::AuthMessage::ExecuteCommand(command) => {
            handle_auth_command(state, command)
        }

        auth::AuthMessage::CommandResult(command, result) => {
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
fn handle_auth_command(
    state: &mut State,
    command: auth::AuthCommand,
) -> DomainUpdateResult {
    info!("Executing auth command: {}", command.sanitized_display());

    // RUS-136: Use trait-based auth_service instead of auth_manager
    let auth_service = state.domains.auth.state.auth_service.clone();

    let task = Task::perform(
        async move {
            let result = execute_auth_command(&auth_service, &command).await;
            (command, result)
        },
        |(cmd, result)| auth::AuthMessage::CommandResult(cmd, result),
    );
    wrap_task!(task)
}

/// Execute an auth command using the auth service
async fn execute_auth_command(
    auth_service: &std::sync::Arc<
        dyn crate::infra::services::auth::AuthService,
    >,
    command: &auth::AuthCommand,
) -> auth::AuthCommandResult {
    match command {
        auth::AuthCommand::ChangePassword {
            old_password: _old_password,
            new_password: _new_password,
        } => {
            // Note: This would require a change_password method on AuthManager
            // For now, return not implemented
            auth::AuthCommandResult::Error(
                "Password change not yet implemented".to_string(),
            )
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
            auth::AuthCommandResult::Error(
                "PIN removal not yet implemented".to_string(),
            )
        }

        auth::AuthCommand::EnableAdminPinUnlock => {
            match auth_service.enable_admin_pin_unlock().await {
                Ok(()) => auth::AuthCommandResult::Success,
                Err(e) => auth::AuthCommandResult::Error(e.to_string()),
            }
        }

        auth::AuthCommand::ChangeUserPin {
            current_pin: _current_pin,
            new_pin: _new_pin,
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
