use super::super::messages::Message;
use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::domains::auth::errors::{AuthError, NetworkError};
use crate::infrastructure::services::api::ApiService;
use crate::infrastructure::services::auth::AuthService;
use crate::state_refactored::State;
use iced::Task;

/// Handle toggle auto-login preference
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_toggle_auto_login(state: &mut State, enabled: bool) -> DomainUpdateResult {
    let auth_service = state.domains.settings.auth_service.clone();
    // We need to update both the device-specific setting and synced preference via auth service
    let task = Task::perform(
        async move {
            // First update the device-specific setting
            auth_service
                .set_auto_login(enabled)
                .await
                .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

            Ok(enabled)
        },
        |result| Message::AutoLoginToggled(result.map_err(|e: AuthError| e.to_string())),
    );
    DomainUpdateResult::task(task.map(DomainMessage::Settings))
}

/// Handle auto-login toggled result
pub fn handle_auto_login_toggled(
    state: &mut State,
    result: Result<bool, String>,
) -> DomainUpdateResult {
    match result {
        Ok(enabled) => {
            // Update UI state to reflect the change
            state.domains.settings.preferences.auto_login_enabled = enabled;
            state.domains.auth.state.auto_login_enabled = enabled;

            if let crate::domains::auth::types::AuthenticationFlow::EnteringCredentials {
                remember_device,
                ..
            } = &mut state.domains.auth.state.auth_flow
            {
                *remember_device = enabled;
            }

            log::info!(
                "Auto-login is now {}",
                if enabled { "enabled" } else { "disabled" }
            );
        }
        Err(error) => {
            log::error!("Failed to toggle auto-login: {}", error);
            // TODO: Show error to user
        }
    }

    DomainUpdateResult::task(Task::none())
}
