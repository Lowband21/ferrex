//! First-run setup update handlers

use crate::domains::auth::messages::Message;
use crate::domains::ui::types::ViewState;
use crate::infrastructure::services::api::ApiService;
use crate::state_refactored::State;
use iced::Task;

/// Handle username update
pub fn handle_update_username(state: &mut State, username: String) -> Task<Message> {
    state.domains.auth.state.first_run_state.username = username;
    state.domains.auth.state.first_run_state.error = None;
    Task::none()
}

/// Handle display name update
pub fn handle_update_display_name(state: &mut State, display_name: String) -> Task<Message> {
    state.domains.auth.state.first_run_state.display_name = display_name;
    Task::none()
}

/// Handle password update
pub fn handle_update_password(state: &mut State, password: String) -> Task<Message> {
    state.domains.auth.state.first_run_state.password = password;
    state.domains.auth.state.first_run_state.error = None;
    Task::none()
}

/// Handle confirm password update
pub fn handle_update_confirm_password(
    state: &mut State,
    confirm_password: String,
) -> Task<Message> {
    state.domains.auth.state.first_run_state.confirm_password = confirm_password;
    state.domains.auth.state.first_run_state.error = None;
    Task::none()
}

/// Handle password visibility toggle
pub fn handle_toggle_password_visibility(state: &mut State) -> Task<Message> {
    state.domains.auth.state.first_run_state.show_password =
        !state.domains.auth.state.first_run_state.show_password;
    Task::none()
}

/// Handle first-run submit
pub fn handle_submit(state: &mut State) -> Task<Message> {
    // Validate inputs
    if state.domains.auth.state.first_run_state.username.is_empty() {
        state.domains.auth.state.first_run_state.error = Some("Username is required".to_string());
        return Task::none();
    }

    if state
        .domains
        .auth
        .state
        .first_run_state
        .display_name
        .is_empty()
    {
        state.domains.auth.state.first_run_state.error =
            Some("Display name is required".to_string());
        return Task::none();
    }

    if state.domains.auth.state.first_run_state.password
        != state.domains.auth.state.first_run_state.confirm_password
    {
        state.domains.auth.state.first_run_state.error = Some("Passwords do not match".to_string());
        return Task::none();
    }

    // Additional password validation
    if state.domains.auth.state.first_run_state.password.len() < 8 {
        state.domains.auth.state.first_run_state.error =
            Some("Password must be at least 8 characters".to_string());
        return Task::none();
    }

    if !state
        .domains
        .auth
        .state
        .first_run_state
        .password
        .chars()
        .any(|c| c.is_uppercase())
    {
        state.domains.auth.state.first_run_state.error =
            Some("Password must contain at least one uppercase letter".to_string());
        return Task::none();
    }

    if !state
        .domains
        .auth
        .state
        .first_run_state
        .password
        .chars()
        .any(|c| c.is_lowercase())
    {
        state.domains.auth.state.first_run_state.error =
            Some("Password must contain at least one lowercase letter".to_string());
        return Task::none();
    }

    if !state
        .domains
        .auth
        .state
        .first_run_state
        .password
        .chars()
        .any(|c| c.is_numeric())
    {
        state.domains.auth.state.first_run_state.error =
            Some("Password must contain at least one number".to_string());
        return Task::none();
    }

    // All validation passed, create the admin
    state.domains.auth.state.first_run_state.loading = true;
    state.domains.auth.state.first_run_state.error = None;

    let username = state.domains.auth.state.first_run_state.username.clone();
    let display_name = state
        .domains
        .auth
        .state
        .first_run_state
        .display_name
        .clone();
    let password = state.domains.auth.state.first_run_state.password.clone();

    let api_service = state.domains.auth.state.api_service.clone();

    Task::perform(
        async move {
            #[derive(serde::Serialize)]
            struct CreateAdminRequest {
                username: String,
                display_name: String,
                password: String,
            }

            let request = CreateAdminRequest {
                username,
                display_name,
                password,
            };

            api_service
                .post::<ferrex_core::user::AuthToken, _>("/setup/admin", &request)
                .await
        },
        |result| match result {
            Ok(_auth_token) => Message::FirstRunSuccess,
            Err(e) => Message::FirstRunError(e.to_string()),
        },
    )
}

/// Handle first-run success
pub fn handle_success(state: &mut State) -> Task<Message> {
    log::info!("First-run setup completed successfully");

    // Clear first-run state
    state.domains.auth.state.first_run_state = Default::default();

    // Move to user selection to log in with the new admin account
    state.domains.ui.state.view = ViewState::Library;

    // Set auth flow to loading users
    state.domains.auth.state.auth_flow = crate::domains::auth::AuthenticationFlow::LoadingUsers;

    // Load users for selection
    Task::done(Message::LoadUsers)
}

/// Handle first-run error
pub fn handle_error(state: &mut State, error: String) -> Task<Message> {
    log::error!("First-run setup failed: {}", error);

    state.domains.auth.state.first_run_state.loading = false;
    state.domains.auth.state.first_run_state.error = Some(error);

    Task::none()
}
