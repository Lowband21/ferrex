//! Theme section update adapter.

use super::messages::ThemeMessage;
use crate::{
    common::messages::DomainUpdateResult,
    domains::settings::{messages::SettingsMessage, update::update_settings},
    state::State,
};

/// Route theme section messages through the settings reducer.
pub fn handle_message(
    state: &mut State,
    message: ThemeMessage,
) -> DomainUpdateResult {
    update_settings(state, SettingsMessage::Theme(message))
}
