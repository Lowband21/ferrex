use crate::common::messages::DomainUpdateResult;
use crate::state_refactored::State;
use iced::Task;

/// Handle update display name
pub fn handle_update_display_name(
    state: &mut State,
    name: String,
) -> DomainUpdateResult {
    // TODO: Store display name in profile state when we add it
    log::debug!("Update display name: {}", name);
    DomainUpdateResult::task(Task::none())
}

/// Handle update email
pub fn handle_update_email(
    state: &mut State,
    email: String,
) -> DomainUpdateResult {
    // TODO: Store email in profile state when we add it
    log::debug!("Update email: {}", email);
    DomainUpdateResult::task(Task::none())
}

/// Handle submit profile changes
pub fn handle_submit_profile_changes(state: &mut State) -> DomainUpdateResult {
    // TODO: Implement when we have profile state
    log::warn!("Profile changes submission not yet implemented");
    DomainUpdateResult::task(Task::none())
}

/// Handle profile change result
pub fn handle_profile_change_result(
    state: &mut State,
    result: Result<(), String>,
) -> DomainUpdateResult {
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
    DomainUpdateResult::task(Task::none())
}
