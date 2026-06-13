//! Playback settings sub-domain.

pub mod messages;
pub mod state;

pub use messages::PlaybackMessage;
pub use state::{PlaybackQuality, PlaybackState, ResumeBehavior};
