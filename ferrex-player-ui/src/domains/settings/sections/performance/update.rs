//! Performance section update adapter.

use super::messages::PerformanceMessage;
use crate::{
    common::messages::DomainUpdateResult,
    domains::settings::{messages::SettingsMessage, update::update_settings},
    state::State,
};

/// Route performance section messages through the settings reducer.
pub fn handle_message(
    state: &mut State,
    message: PerformanceMessage,
) -> DomainUpdateResult {
    update_settings(state, SettingsMessage::Performance(message))
}
