//! Playback section update adapter.

use super::messages::PlaybackMessage;
use crate::{
    common::messages::DomainUpdateResult,
    domains::settings::{messages::SettingsMessage, update::update_settings},
    state::State,
};

/// Route playback section messages through the settings reducer.
pub fn handle_message(
    state: &mut State,
    message: PlaybackMessage,
) -> DomainUpdateResult {
    update_settings(state, SettingsMessage::Playback(message))
}
