//! Profile section update handlers
//!
//! Handles all ProfileMessage variants and updates state accordingly.

use super::messages::ProfileMessage;
use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Main message handler for profile section
pub fn handle_message(
    state: &mut State,
    message: ProfileMessage,
) -> DomainUpdateResult {
    match message {
        ProfileMessage::UpdateDisplayName(name) => {
            handle_update_display_name(state, name)
        }
        ProfileMessage::UpdateEmail(email) => handle_update_email(state, email),
        ProfileMessage::UpdateAvatar(avatar) => {
            handle_update_avatar(state, avatar)
        }
        ProfileMessage::SubmitChanges => handle_submit_changes(state),
        ProfileMessage::ChangeResult(result) => {
            handle_change_result(state, result)
        }
        ProfileMessage::Cancel => handle_cancel(state),
        ProfileMessage::Logout => handle_logout(state),
        ProfileMessage::SwitchUser => handle_switch_user(state),
    }
}

fn handle_update_display_name(
    state: &mut State,
    name: String,
) -> DomainUpdateResult {
    // TODO: Update display name in state and mark dirty
    let _ = (state, name);
    DomainUpdateResult::none()
}

fn handle_update_email(state: &mut State, email: String) -> DomainUpdateResult {
    // TODO: Update email in state and mark dirty
    let _ = (state, email);
    DomainUpdateResult::none()
}

fn handle_update_avatar(
    state: &mut State,
    avatar: String,
) -> DomainUpdateResult {
    // TODO: Update avatar in state and mark dirty
    let _ = (state, avatar);
    DomainUpdateResult::none()
}

fn handle_submit_changes(state: &mut State) -> DomainUpdateResult {
    // TODO: Submit profile changes to server
    let _ = state;
    DomainUpdateResult::none()
}

fn handle_change_result(
    state: &mut State,
    result: Result<(), String>,
) -> DomainUpdateResult {
    // TODO: Handle result of profile change submission
    let _ = (state, result);
    DomainUpdateResult::none()
}

fn handle_cancel(state: &mut State) -> DomainUpdateResult {
    // TODO: Revert changes and reset form state
    let _ = state;
    DomainUpdateResult::none()
}

fn handle_logout(state: &mut State) -> DomainUpdateResult {
    // TODO: Trigger logout via auth domain event
    let _ = state;
    DomainUpdateResult::none()
}

fn handle_switch_user(state: &mut State) -> DomainUpdateResult {
    // TODO: Navigate to user selection
    let _ = state;
    DomainUpdateResult::none()
}
