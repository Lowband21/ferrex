//! Profile section update adapter.

use super::messages::ProfileMessage;
use crate::{
    common::messages::{CrossDomainEvent, DomainUpdateResult},
    domains::settings::{messages::SettingsMessage, update::update_settings},
    state::State,
};
use iced::Task;

/// Route profile section messages through the settings reducer or shell events.
pub fn handle_message(
    state: &mut State,
    message: ProfileMessage,
) -> DomainUpdateResult {
    match message {
        ProfileMessage::UpdateDisplayName(name) => {
            update_settings(state, SettingsMessage::UpdateDisplayName(name))
        }
        ProfileMessage::UpdateEmail(email) => {
            update_settings(state, SettingsMessage::UpdateEmail(email))
        }
        ProfileMessage::UpdateAvatar(avatar) => {
            state.domains.settings.profile.success_message = None;
            state.domains.settings.profile.error = if avatar.trim().is_empty() {
                None
            } else {
                Some(
                    "Avatar changes are not supported by the current API"
                        .to_string(),
                )
            };
            DomainUpdateResult::task(Task::none())
        }
        ProfileMessage::SubmitChanges => {
            update_settings(state, SettingsMessage::SubmitProfileChanges)
        }
        ProfileMessage::ChangeResult(result) => {
            update_settings(state, SettingsMessage::ProfileChangeResult(result))
        }
        ProfileMessage::Cancel => {
            state.domains.settings.profile.loading = false;
            state.domains.settings.profile.error = None;
            state.domains.settings.profile.success_message = None;
            DomainUpdateResult::task(Task::none())
        }
        ProfileMessage::Logout | ProfileMessage::SwitchUser => {
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::UserLoggedOut],
            )
        }
    }
}
