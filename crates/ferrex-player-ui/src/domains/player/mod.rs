//! Compatibility shim for the extracted `ferrex-player-playback` domain.

pub mod messages;
pub mod state;
pub mod update;
pub mod view;

use crate::common::messages::CrossDomainEvent;
use ferrex_core::player_prelude::LibraryId;
use ferrex_player_playback::PlaybackExternalEvent;

pub use ferrex_player_playback::{PlayerDomain, TrackNotification};

impl PlaybackExternalEvent for CrossDomainEvent {
    fn library_changed(&self) -> Option<LibraryId> {
        match self {
            CrossDomainEvent::LibraryChanged(library_id) => Some(*library_id),
            _ => None,
        }
    }
}
