//! Playback settings sub-domain
//!
//! Handles playback-related settings including:
//! - General: Auto-play next, resume behavior, preferred quality
//! - Seeking: Forward/backward coarse and fine seek amounts
//! - Skip: Intro and credits skip durations
//! - Subtitles: Enabled by default, preferred language, font scale

pub mod messages;
pub mod state;
pub mod update;

pub use messages::PlaybackMessage;
pub use state::PlaybackState;

use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Playback section marker for type-safe section identification
#[derive(Debug)]
pub struct PlaybackSection;

/// Update playback settings state
pub fn update(
    state: &mut State,
    message: PlaybackMessage,
) -> DomainUpdateResult {
    update::handle_message(state, message)
}
