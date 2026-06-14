//! Display section update adapter.

use super::messages::DisplayMessage;
use crate::{
    common::messages::DomainUpdateResult,
    domains::settings::{messages::SettingsMessage, update::update_settings},
    state::State,
};

/// Route display section messages through the settings reducer.
pub fn handle_message(
    state: &mut State,
    message: DisplayMessage,
) -> DomainUpdateResult {
    update_settings(state, SettingsMessage::Display(message))
}
