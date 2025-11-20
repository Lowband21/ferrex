use crate::{
    messages::settings,
    state::State,
};
use iced::Task;

/// Handle update display name
pub fn handle_update_display_name(state: &mut State, name: String) -> Task<settings::Message> {
    // TODO: Store display name in profile state when we add it
    log::debug!("Update display name: {}", name);
    Task::none()
}

/// Handle update email
pub fn handle_update_email(state: &mut State, email: String) -> Task<settings::Message> {
    // TODO: Store email in profile state when we add it
    log::debug!("Update email: {}", email);
    Task::none()
}

/// Handle submit profile changes
pub fn handle_submit_profile_changes(state: &mut State) -> Task<settings::Message> {
    // TODO: Implement when we have profile state
    log::warn!("Profile changes submission not yet implemented");
    Task::none()
}

/// Handle profile change result
pub fn handle_profile_change_result(state: &mut State, result: Result<(), String>) -> Task<settings::Message> {
    match result {
        Ok(()) => {
            log::info!("Profile updated successfully");
            // TODO: Clear any error state
        }
        Err(error) => {
            log::error!("Failed to update profile: {}", error);
            // TODO: Store error in state
        }
    }
    Task::none()
}