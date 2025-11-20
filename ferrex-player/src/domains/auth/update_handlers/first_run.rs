//! First-run admin setup handlers, including the secure claim workflow.

use std::sync::Arc;

use crate::domains::auth::messages as auth;
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::auth::types::{AuthenticationFlow, SetupClaimStatus, SetupClaimUi};
use crate::infrastructure::services::api::ApiService;
use crate::infrastructure::services::auth::AuthService;
use crate::state_refactored::State;
use ferrex_core::api_routes::v1;
use ferrex_core::api_types::setup::{ConfirmClaimResponse, StartClaimResponse};
use ferrex_core::{auth::domain::value_objects::SessionScope, player_prelude as core};
use iced::Task;
use log::{error, info};

/// Handle check setup status
pub fn handle_check_setup_status(state: &mut State) -> Task<auth::Message> {
    info!("[Auth] handle_check_setup_status called - checking if first-run setup is needed");

    let auth_service = &state.domains.auth.state.auth_service;
    let svc: Arc<dyn AuthService> = Arc::clone(auth_service);

    Task::perform(
        async move {
            info!("[Auth] Calling auth_service.check_setup_status()");
            svc.check_setup_status().await.map_err(|e| e.to_string())
        },
        |result| match result {
            Ok(needs_setup) => {
                info!(
                    "[Auth] Setup status check result: needs_setup = {}",
                    needs_setup
                );
                auth::Message::SetupStatusChecked(needs_setup)
            }
            Err(e) => {
                error!("Failed to check setup status: {}", e);
                auth::Message::SetupStatusChecked(false)
            }
        },
    )
}

/// Handle setup status checked response
pub fn handle_setup_status_checked(state: &mut State, needs_setup: bool) -> Task<auth::Message> {
    info!(
        "[Auth] handle_setup_status_checked called with needs_setup = {}",
        needs_setup
    );

    if needs_setup {
        info!("First-run setup needed, showing admin setup");
        state.is_authenticated = false;
        state.domains.auth.state.is_authenticated = false;
        state.domains.auth.state.user_permissions = None;
        state.domains.auth.state.auth_flow = AuthenticationFlow::FirstRunSetup {
            username: String::new(),
            password: SecureCredential::new(String::new()),
            confirm_password: SecureCredential::new(String::new()),
            display_name: String::new(),
            setup_token: String::new(),
            claim_token: String::new(),
            show_password: false,
            error: None,
            loading: false,
            claim: SetupClaimUi::default(),
        };
        Task::none()
    } else {
        super::auth_flow::transition_to_auto_login_check(state)
    }
}

/// Handle setup field updates during first-run admin creation
pub fn handle_update_setup_field(
    state: &mut State,
    field: auth::SetupField,
) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup {
        username,
        password,
        confirm_password,
        display_name,
        setup_token,
        claim_token,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        match field {
            auth::SetupField::Username(value) => *username = value,
            auth::SetupField::Password(value) => *password = SecureCredential::new(value),
            auth::SetupField::ConfirmPassword(value) => {
                *confirm_password = SecureCredential::new(value)
            }
            auth::SetupField::DisplayName(value) => *display_name = value,
            auth::SetupField::SetupToken(value) => *setup_token = value,
            auth::SetupField::ClaimToken(value) => *claim_token = value,
        }
    }

    Task::none()
}

/// Toggle password visibility in first-run setup
pub fn handle_toggle_setup_password_visibility(state: &mut State) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup { show_password, .. } =
        &mut state.domains.auth.state.auth_flow
    {
        *show_password = !*show_password;
    }

    Task::none()
}

/// Submit the first-run admin setup form
pub fn handle_submit_setup(state: &mut State) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup {
        username,
        password,
        confirm_password,
        display_name,
        setup_token,
        claim_token,
        error,
        loading,
        claim,
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

        if claim.is_expired() {
            claim.mark_expired();
            *error = Some(
                "The binding code has expired. Request a new code before continuing.".to_string(),
            );
            return Task::none();
        }

        if claim_token.trim().is_empty() {
            *error = Some(
                "Secure claim token required. Confirm the binding before creating the admin."
                    .to_string(),
            );
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
        let claim_token = if claim_token.trim().is_empty() {
            None
        } else {
            Some(claim_token.clone())
        };

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
                    auth::Message::SetupComplete(auth_token.access_token, auth_token.refresh_token)
                }
                Err(e) => {
                    error!("[Auth] Admin setup failed: {}", e);
                    auth::Message::SetupError(e.to_string())
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
) -> Task<auth::Message> {
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
                .get(v1::users::CURRENT)
                .await
                .map_err(|e| format!("Failed to get user: {}", e))?;

            let permissions: core::UserPermissions = api_service
                .get(v1::roles::MY_PERMISSIONS)
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
                .authenticate(user.clone(), auth_token, permissions.clone(), server_url)
                .await
            {
                error!("[Auth] Failed to set auth state: {}", e);
            }

            if let Err(e) = svc.save_current_auth().await {
                error!("[Auth] Failed to save auth after setup: {}", e);
            }

            Ok((user, permissions))
        },
        |result: Result<(core::User, core::UserPermissions), String>| match result {
            Ok((user, permissions)) => {
                info!("[Auth] Retrieved admin user: {}", user.username);
                auth::Message::LoginSuccess(user, permissions)
            }
            Err(e) => {
                error!("[Auth] Failed to complete setup: {}", e);
                auth::Message::SetupError(e)
            }
        },
    )
}

/// Handle setup error during first-run admin create
pub fn handle_setup_error(state: &mut State, error: String) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup {
        error: view_error,
        loading,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *view_error = Some(error);
        *loading = false;
    }

    Task::none()
}

/// Update the device name used when starting the claim flow
pub fn handle_update_claim_device_name(state: &mut State, name: String) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup { claim, .. } = &mut state.domains.auth.state.auth_flow
    {
        claim.device_name = name;
    }

    Task::none()
}

/// Start the secure claim flow
pub fn handle_start_setup_claim(state: &mut State) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup {
        claim, claim_token, ..
    } = &mut state.domains.auth.state.auth_flow
    {
        if claim.is_requesting {
            info!("[Auth] Ignoring duplicate claim start request");
            return Task::none();
        }

        info!("[Auth] Starting secure claim flow");

        claim.is_requesting = true;
        claim.last_error = None;
        claim.claim_id = None;
        claim.claim_code = None;
        claim.expires_at = None;
        claim.claim_token = None;
        claim.status = SetupClaimStatus::Idle;
        claim_token.clear();

        let api_service = state.domains.auth.state.api_service.clone();
        let device_name = if claim.device_name.trim().is_empty() {
            None
        } else {
            Some(claim.device_name.trim().to_string())
        };

        return Task::perform(
            async move {
                api_service
                    .start_setup_claim(device_name)
                    .await
                    .map_err(|e| e.to_string())
            },
            |result| match result {
                Ok(response) => auth::Message::SetupClaimStarted(response),
                Err(err) => auth::Message::SetupClaimFailed(err),
            },
        );
    }

    Task::none()
}

/// Handle successful claim start
pub fn handle_setup_claim_started(
    state: &mut State,
    response: StartClaimResponse,
) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup { claim, .. } = &mut state.domains.auth.state.auth_flow
    {
        info!(
            "[Auth] Secure claim code issued; expires at {}",
            response.expires_at
        );
        claim.is_requesting = false;
        claim.last_error = None;
        claim.claim_id = Some(response.claim_id);
        claim.claim_code = Some(response.claim_code);
        claim.expires_at = Some(response.expires_at);
        claim.lan_only = response.lan_only;
        claim.status = SetupClaimStatus::Pending;
    }

    Task::none()
}

/// Handle claim start failure
pub fn handle_setup_claim_failed(state: &mut State, error: String) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup { claim, .. } = &mut state.domains.auth.state.auth_flow
    {
        error!("[Auth] Failed to start secure claim: {}", error);
        claim.is_requesting = false;
        claim.last_error = Some(error);
    }

    Task::none()
}

/// Confirm the secure claim
pub fn handle_confirm_setup_claim(state: &mut State) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup {
        claim, claim_token, ..
    } = &mut state.domains.auth.state.auth_flow
    {
        if claim.is_confirming {
            info!("[Auth] Ignoring duplicate claim confirm request");
            return Task::none();
        }

        if !matches!(claim.status, SetupClaimStatus::Pending) {
            claim.last_error =
                Some("Request a binding code before attempting confirmation.".to_string());
            return Task::none();
        }

        if claim.is_expired() {
            claim.mark_expired();
            claim.last_error =
                Some("The binding code has expired. Request a new code.".to_string());
            return Task::none();
        }

        let Some(code) = claim.claim_code.clone() else {
            claim.last_error = Some("Binding code missing. Start the claim again.".to_string());
            return Task::none();
        };

        info!("[Auth] Confirming secure claim");

        claim.is_confirming = true;
        claim.last_error = None;
        claim.claim_token = None;
        claim_token.clear();

        let api_service = state.domains.auth.state.api_service.clone();

        return Task::perform(
            async move {
                api_service
                    .confirm_setup_claim(code)
                    .await
                    .map_err(|e| e.to_string())
            },
            |result| match result {
                Ok(response) => auth::Message::SetupClaimConfirmed(response),
                Err(err) => auth::Message::SetupClaimConfirmFailed(err),
            },
        );
    }

    Task::none()
}

/// Handle successful claim confirmation
pub fn handle_setup_claim_confirmed(
    state: &mut State,
    response: ConfirmClaimResponse,
) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup {
        claim, claim_token, ..
    } = &mut state.domains.auth.state.auth_flow
    {
        info!("[Auth] Secure claim confirmed; token issued");
        claim.is_confirming = false;
        claim.last_error = None;
        claim.claim_id = Some(response.claim_id);
        claim.claim_token = Some(response.claim_token.clone());
        claim.expires_at = Some(response.expires_at);
        claim.status = SetupClaimStatus::Confirmed;
        claim_token.clear();
        claim_token.push_str(&response.claim_token);
    }

    Task::none()
}

/// Handle claim confirmation failure
pub fn handle_setup_claim_confirm_failed(state: &mut State, error: String) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup { claim, .. } = &mut state.domains.auth.state.auth_flow
    {
        error!("[Auth] Failed to confirm secure claim: {}", error);
        claim.is_confirming = false;
        claim.last_error = Some(error);
    }

    Task::none()
}

/// Reset the secure claim state
pub fn handle_reset_setup_claim(state: &mut State) -> Task<auth::Message> {
    if let AuthenticationFlow::FirstRunSetup {
        claim, claim_token, ..
    } = &mut state.domains.auth.state.auth_flow
    {
        info!("[Auth] Resetting secure claim state");
        claim.reset();
        claim_token.clear();
    }

    Task::none()
}
