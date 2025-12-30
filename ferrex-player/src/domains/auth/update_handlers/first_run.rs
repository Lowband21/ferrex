//! First-run admin setup handlers for the setup wizard.

use crate::domains::auth::messages as auth;
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::auth::types::{
    AuthenticationFlow, SetupClaimStatus, SetupStep, TransitionDirection,
};
use crate::infra::api_client::SetupStatus;
use crate::state::State;
use ferrex_core::{
    api::types::setup::{ConfirmClaimResponse, StartClaimResponse},
    domain::users::auth::domain::value_objects::SessionScope,
    player_prelude as core,
};
use iced::Task;
use log::{error, info};

/// Handle check setup status
pub fn handle_check_setup_status(state: &mut State) -> Task<auth::AuthMessage> {
    info!(
        "[Auth] handle_check_setup_status called - checking if first-run setup is needed"
    );

    let api_service = state.domains.auth.state.api_service.clone();

    Task::perform(
        async move {
            info!("[Auth] Calling api_service.check_setup_status()");
            api_service
                .check_setup_status()
                .await
                .map_err(|e| e.to_string())
        },
        |result| match result {
            Ok(status) => {
                info!(
                    "[Auth] Setup status check result: needs_setup = {}",
                    status.needs_setup
                );
                auth::AuthMessage::SetupStatusChecked(status)
            }
            Err(e) => {
                error!("Failed to check setup status: {}", e);
                // Conservative default: if setup status cannot be determined, prefer
                // showing the setup flow rather than presenting stale user lists or
                // an empty login. The subsequent setup attempt will validate against
                // the server and guide the user appropriately.
                auth::AuthMessage::SetupStatusChecked(SetupStatus {
                    needs_setup: true,
                    has_admin: false,
                    requires_setup_token: false,
                    user_count: 0,
                    library_count: 0,
                })
            }
        },
    )
}

/// Handle setup status checked response
pub fn handle_setup_status_checked(
    state: &mut State,
    status: SetupStatus,
) -> Task<auth::AuthMessage> {
    info!(
        "[Auth] handle_setup_status_checked called with needs_setup = {}, requires_setup_token = {}",
        status.needs_setup, status.requires_setup_token
    );

    if status.needs_setup {
        info!("First-run setup needed, showing setup wizard");
        state.is_authenticated = false;
        state.domains.auth.state.is_authenticated = false;
        state.domains.auth.state.user_permissions = None;
        state.domains.auth.state.auth_flow =
            AuthenticationFlow::FirstRunSetup {
                current_step: SetupStep::Welcome,
                username: String::new(),
                password: SecureCredential::new(String::new()),
                confirm_password: SecureCredential::new(String::new()),
                display_name: String::new(),
                setup_token: String::new(),
                show_password: false,
                claim_code: None,
                claim_token: None,
                claim_status: SetupClaimStatus::Idle,
                claim_loading: false,
                pin: SecureCredential::new(String::new()),
                confirm_pin: SecureCredential::new(String::new()),
                error: None,
                loading: false,
                setup_token_required: status.requires_setup_token,
                transition_direction: TransitionDirection::None,
                transition_progress: 0.0,
            };
        Task::none()
    } else {
        // Rationale: When we checked setup due to an empty user list, this is
        // not a first-run server. In that case we should not loop back into the
        // auto-login check which simply re-triggers user loading and keeps the
        // UI stuck on the "Loading users..." view with no cache. Instead, show
        // the pre-auth login form so the user can enter username/password.
        // Auto‑login (if applicable) is handled during bootstrap before we get here.

        state.is_authenticated = false;
        state.domains.auth.state.is_authenticated = false;
        state.domains.auth.state.user_permissions = None;
        state.domains.auth.state.auth_flow = AuthenticationFlow::PreAuthLogin {
            username: String::new(),
            password: SecureCredential::new(String::new()),
            show_password: false,
            // Mirror current device preference as default toggle state
            remember_device: state.domains.auth.state.auto_login_enabled,
            error: None,
            loading: false,
        };
        Task::none()
    }
}

/// Handle setup field updates during first-run admin creation
pub fn handle_update_setup_field(
    state: &mut State,
    field: auth::SetupField,
) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        username,
        password,
        confirm_password,
        display_name,
        setup_token,
        error,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        // Clear error when user starts typing
        *error = None;

        match field {
            auth::SetupField::Username(value) => *username = value,
            auth::SetupField::Password(value) => {
                *password = SecureCredential::new(value)
            }
            auth::SetupField::ConfirmPassword(value) => {
                *confirm_password = SecureCredential::new(value)
            }
            auth::SetupField::DisplayName(value) => *display_name = value,
            auth::SetupField::SetupToken(value) => *setup_token = value,
            auth::SetupField::ClaimToken(_) => {
                // ClaimToken is no longer used (device binding is automatic)
            }
        }
    }

    Task::none()
}

/// Toggle password visibility in first-run setup
pub fn handle_toggle_setup_password_visibility(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup { show_password, .. } =
        &mut state.domains.auth.state.auth_flow
    {
        *show_password = !*show_password;
    }

    Task::none()
}

/// Navigate to the next step in the setup wizard
pub fn handle_setup_next_step(state: &mut State) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        current_step,
        username,
        display_name,
        password,
        confirm_password,
        setup_token,
        setup_token_required,
        claim_token,
        error,
        transition_direction,
        transition_progress,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        // Validate current step before proceeding
        let validation_error = match current_step {
            SetupStep::Welcome => None,
            SetupStep::Account => {
                if username.trim().is_empty() {
                    Some("Username is required")
                } else if display_name.trim().is_empty() {
                    Some("Display name is required")
                } else if password.as_str().is_empty() {
                    Some("Password is required")
                } else if password.as_str() != confirm_password.as_str() {
                    Some("Passwords do not match")
                } else {
                    None
                }
            }
            SetupStep::SetupToken => {
                if setup_token.trim().is_empty() {
                    Some("Setup token is required")
                } else {
                    None
                }
            }
            SetupStep::DeviceClaim => {
                // Must have a confirmed claim token to proceed
                if claim_token.is_none() {
                    Some("Please verify your device first")
                } else {
                    None
                }
            }
            SetupStep::Pin => None, // PIN is optional
            SetupStep::Complete => None,
        };

        if let Some(err) = validation_error {
            *error = Some(err.to_string());
            return Task::none();
        }

        *error = None;

        // Move to next step
        if let Some(next) = current_step.next(*setup_token_required) {
            let entering_claim = matches!(next, SetupStep::DeviceClaim);
            *current_step = next;
            *transition_direction = TransitionDirection::Forward;
            *transition_progress = 0.0;

            // Auto-start claim when entering DeviceClaim step
            if entering_claim {
                return Task::done(auth::AuthMessage::StartSetupClaim);
            }
        }
    }

    Task::none()
}

/// Navigate to the previous step in the setup wizard
pub fn handle_setup_previous_step(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        current_step,
        setup_token_required,
        error,
        transition_direction,
        transition_progress,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *error = None;

        if let Some(prev) = current_step.previous(*setup_token_required) {
            *current_step = prev;
            *transition_direction = TransitionDirection::Backward;
            *transition_progress = 0.0;
        }
    }

    Task::none()
}

/// Skip PIN setup and proceed to completion
pub fn handle_skip_pin_setup(state: &mut State) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        current_step,
        pin,
        confirm_pin,
        transition_direction,
        transition_progress,
        ..
    } = &mut state.domains.auth.state.auth_flow
        && matches!(current_step, SetupStep::Pin)
    {
        // Clear any partial PIN entry
        *pin = SecureCredential::new(String::new());
        *confirm_pin = SecureCredential::new(String::new());
        // Move to complete
        *current_step = SetupStep::Complete;
        *transition_direction = TransitionDirection::Forward;
        *transition_progress = 0.0;
    }

    Task::none()
}

/// Handle animation tick for carousel transitions
pub fn handle_setup_animation_tick(
    state: &mut State,
    delta: f32,
) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        transition_progress,
        transition_direction,
        ..
    } = &mut state.domains.auth.state.auth_flow
        && !matches!(transition_direction, TransitionDirection::None)
    {
        *transition_progress = (*transition_progress + delta).min(1.0);
        if *transition_progress >= 1.0 {
            *transition_direction = TransitionDirection::None;
            *transition_progress = 0.0;
        }
    }

    Task::none()
}

/// Submit the first-run admin setup form
pub fn handle_submit_setup(state: &mut State) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        username,
        password,
        confirm_password,
        display_name,
        setup_token,
        claim_token,
        error,
        loading,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *error = None;

        if username.trim().is_empty() {
            *error = Some("Username is required".to_string());
            return Task::none();
        }

        if display_name.trim().is_empty() {
            *error = Some("Display name is required".to_string());
            return Task::none();
        }

        if password.as_str().is_empty() {
            *error = Some("Password is required".to_string());
            return Task::none();
        }

        if password.as_str() != confirm_password.as_str() {
            *error = Some("Passwords do not match".to_string());
            return Task::none();
        }

        if claim_token.is_none() {
            *error = Some("Device verification is required".to_string());
            return Task::none();
        }

        *loading = true;

        let api_service = state.domains.auth.state.api_service.clone();
        let username = username.clone();
        let password = password.as_str().to_string();
        let display_name = if display_name.trim().is_empty() {
            None
        } else {
            Some(display_name.clone())
        };
        let setup_token = if setup_token.trim().is_empty() {
            None
        } else {
            Some(setup_token.clone())
        };
        let claim_token = claim_token.clone();

        return Task::perform(
            async move {
                api_service
                    .create_initial_admin(
                        username,
                        password,
                        display_name,
                        setup_token,
                        claim_token,
                    )
                    .await
                    .map_err(|e| e.to_string())
                    .map(|(_user, token)| core::AuthToken {
                        access_token: token.access_token,
                        refresh_token: token.refresh_token,
                        expires_in: 900,
                        session_id: None,
                        device_session_id: None,
                        user_id: None,
                        scope: token.scope,
                    })
            },
            |result| match result {
                Ok(auth_token) => {
                    info!("[Auth] Admin setup successful");
                    auth::AuthMessage::SetupComplete(
                        auth_token.access_token,
                        auth_token.refresh_token,
                    )
                }
                Err(e) => {
                    error!("[Auth] Admin setup failed: {}", e);
                    auth::AuthMessage::SetupError(e.to_string())
                }
            },
        );
    }

    Task::none()
}

/// Handle successful admin setup completion
pub fn handle_setup_complete(
    state: &mut State,
    access_token: String,
    refresh_token: String,
) -> Task<auth::AuthMessage> {
    info!("[Auth] Admin setup complete, storing tokens");

    let svc = state.domains.auth.state.auth_service.clone();
    let api_service = state.domains.auth.state.api_service.clone();
    let server_url = api_service.build_url("");

    let auth_token = core::AuthToken {
        access_token,
        refresh_token,
        expires_in: 900,
        session_id: None,
        device_session_id: None,
        user_id: None,
        scope: SessionScope::Full,
    };

    Task::perform(
        async move {
            api_service.set_token(Some(auth_token.clone())).await;

            let user: core::User = api_service
                .fetch_current_user()
                .await
                .map_err(|e| format!("Failed to get user: {}", e))?;

            let permissions: core::UserPermissions = api_service
                .fetch_my_permissions()
                .await
                .unwrap_or_else(|e| {
                    info!(
                        "[Auth] Failed to get permissions, using default admin permissions: {}",
                        e
                    );
                    core::UserPermissions {
                        user_id: user.id,
                        roles: vec![],
                        permissions: std::collections::HashMap::new(),
                        permission_details: None,
                    }
                });

            if let Err(e) = svc
                .authenticate(
                    user.clone(),
                    auth_token,
                    permissions.clone(),
                    server_url,
                )
                .await
            {
                error!("[Auth] Failed to set auth state: {}", e);
            }

            if let Err(e) = svc.save_current_auth().await {
                error!("[Auth] Failed to save auth after setup: {}", e);
            }

            Ok((user, permissions))
        },
        |result: Result<(core::User, core::UserPermissions), String>| {
            match result {
                Ok((user, permissions)) => {
                    info!("[Auth] Retrieved admin user: {}", user.username);
                    auth::AuthMessage::LoginSuccess(user, permissions)
                }
                Err(e) => {
                    error!("[Auth] Failed to complete setup: {}", e);
                    auth::AuthMessage::SetupError(e)
                }
            }
        },
    )
}

/// Handle setup error during first-run admin create
pub fn handle_setup_error(
    state: &mut State,
    error: String,
) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        error: view_error,
        loading,
        claim_loading,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *view_error = Some(error);
        *loading = false;
        *claim_loading = false;
    }

    Task::none()
}

/// Start the secure setup claim workflow
pub fn handle_start_setup_claim(state: &mut State) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        claim_loading,
        claim_status,
        claim_code,
        error,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        info!("[Auth] Starting setup claim workflow");
        *claim_loading = true;
        *claim_status = SetupClaimStatus::Pending;
        *claim_code = None;
        *error = None;

        let api_service = state.domains.auth.state.api_service.clone();

        return Task::perform(
            async move {
                api_service
                    .start_setup_claim(None) // Let server determine device name
                    .await
                    .map_err(|e| e.to_string())
            },
            auth::AuthMessage::ClaimStarted,
        );
    }

    Task::none()
}

/// Handle the result of starting a claim
pub fn handle_claim_started(
    state: &mut State,
    result: Result<StartClaimResponse, String>,
) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        claim_loading,
        claim_status,
        claim_code,
        error,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *claim_loading = false;

        match result {
            Ok(response) => {
                info!(
                    "[Auth] Claim started successfully, code: {}",
                    response.claim_code
                );
                *claim_code = Some(response.claim_code);
                *claim_status = SetupClaimStatus::Pending;
                *error = None;
            }
            Err(e) => {
                error!("[Auth] Failed to start claim: {}", e);
                *claim_status = SetupClaimStatus::Idle;
                *error = Some(format!("Failed to start verification: {}", e));
            }
        }
    }

    Task::none()
}

/// Confirm the setup claim after user verifies on server
pub fn handle_confirm_setup_claim(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        claim_code,
        claim_loading,
        error,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        if let Some(code) = claim_code.clone() {
            info!("[Auth] Confirming setup claim with code: {}", code);
            *claim_loading = true;
            *error = None;

            let api_service = state.domains.auth.state.api_service.clone();

            return Task::perform(
                async move {
                    api_service
                        .confirm_setup_claim(code)
                        .await
                        .map_err(|e| e.to_string())
                },
                auth::AuthMessage::ClaimConfirmed,
            );
        } else {
            *error = Some("No claim code available".to_string());
        }
    }

    Task::none()
}

/// Handle the result of confirming a claim
pub fn handle_claim_confirmed(
    state: &mut State,
    result: Result<ConfirmClaimResponse, String>,
) -> Task<auth::AuthMessage> {
    if let AuthenticationFlow::FirstRunSetup {
        claim_loading,
        claim_status,
        claim_token,
        error,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *claim_loading = false;

        match result {
            Ok(response) => {
                info!("[Auth] Claim confirmed successfully");
                *claim_token = Some(response.claim_token);
                *claim_status = SetupClaimStatus::Confirmed;
                *error = None;
            }
            Err(e) => {
                error!("[Auth] Failed to confirm claim: {}", e);
                // Keep status as pending so user can retry
                *error = Some("Verification not confirmed yet. Please run the confirm command on your server.".to_string());
            }
        }
    }

    Task::none()
}
