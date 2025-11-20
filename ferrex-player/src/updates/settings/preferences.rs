use crate::{
    messages::settings,
    state::State,
    auth_errors::{AuthError, NetworkError},
};
use iced::Task;

/// Handle toggle auto-login preference
pub fn handle_toggle_auto_login(state: &mut State, enabled: bool) -> Task<settings::Message> {
    if let (Some(auth_manager), Some(api_client)) = (&state.auth_manager, &state.api_client) {
        let auth_manager = auth_manager.clone();
        let api_client = api_client.clone();
        
        // We need to update both the device-specific setting AND the user preference
        return Task::perform(
            async move {
                // First update the device-specific setting
                auth_manager.set_auto_login(enabled).await?;
                
                // Then update the user preference in the database
                let request = serde_json::json!({
                    "auto_login_enabled": enabled
                });
                
                api_client.put::<serde_json::Value, serde_json::Value>(
                    "/api/users/me/preferences", 
                    &request
                ).await
                .map_err(|e| AuthError::Network(
                    NetworkError::RequestFailed(e.to_string())
                ))?;
                
                Ok(enabled)
            },
            |result| settings::Message::AutoLoginToggled(
                result.map_err(|e: AuthError| e.to_string())
            ),
        );
    }
    
    Task::done(settings::Message::AutoLoginToggled(
        Err("Auth manager or API client not available".to_string())
    ))
}

/// Handle auto-login toggled result
pub fn handle_auto_login_toggled(state: &mut State, result: Result<bool, String>) -> Task<settings::Message> {
    match result {
        Ok(enabled) => {
            // Update UI state to reflect the change
            state.auto_login_enabled = enabled;
            log::info!("Auto-login is now {}", if enabled { "enabled" } else { "disabled" });
        }
        Err(error) => {
            log::error!("Failed to toggle auto-login: {}", error);
            // TODO: Show error to user
        }
    }
    
    Task::none()
}