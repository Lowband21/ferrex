//! Playback state, controls, video provider integration, and update wiring for Ferrex player clients.
//!
//! The crate is intentionally independent of the desktop app's final state facade.
//! Callers provide app-shell adapters for watch-progress persistence, navigation,
//! and view/window side effects while this crate owns the playback state machine.

pub mod constants;
#[cfg(feature = "ui")]
pub mod controls;
pub mod external_mpv;
pub mod messages;
pub mod state;
#[cfg(feature = "ui")]
pub mod theme;
pub mod track_selection;
pub mod update;
pub mod video;
#[cfg(feature = "ui")]
pub mod view;

use ferrex_core::player_prelude::LibraryId;
use ferrex_player_api::services::api::ApiService;
use iced::Task;
use std::sync::Arc;

pub use messages::PlayerMessage;
pub use state::{PlayerDomainState, TrackNotification};

/// Cross-domain event view needed by the playback data domain.
pub trait PlaybackExternalEvent {
    /// Library selection changed.
    fn library_changed(&self) -> Option<LibraryId> {
        None
    }
}

/// Player domain wrapper.
#[derive(Debug)]
pub struct PlayerDomain {
    pub state: PlayerDomainState,
    pub current_library_id: Option<LibraryId>,
    pub api_service: Option<Arc<dyn ApiService>>,
}

impl Default for PlayerDomain {
    fn default() -> Self {
        Self::new(None)
    }
}

impl PlayerDomain {
    pub fn new(api_service: Option<Arc<dyn ApiService>>) -> Self {
        Self {
            state: PlayerDomainState::default(),
            current_library_id: None,
            api_service,
        }
    }

    pub fn handle_event<E>(&mut self, event: &E) -> Task<PlayerMessage>
    where
        E: PlaybackExternalEvent,
    {
        if let Some(library_id) = event.library_changed() {
            self.current_library_id = Some(library_id);
        }
        Task::none()
    }
}

#[cfg(feature = "ui")]
pub mod ui_support {
    //! Small UI helpers kept in this crate so playback overlay rendering does not
    //! depend on the desktop player's helper modules.

    use iced::{Font, widget::text};
    pub use lucide_icons::Icon;

    /// Helper function to create icon text with the default size (20px).
    pub fn icon_text(icon: Icon) -> text::Text<'static> {
        icon_text_with_size(icon, 20.0)
    }

    /// Helper function to create icon text with a custom size.
    pub fn icon_text_with_size(icon: Icon, size: f32) -> text::Text<'static> {
        text(icon.unicode()).font(lucide_font()).size(size)
    }

    /// Get the lucide font.
    pub fn lucide_font() -> Font {
        Font::with_name("lucide")
    }
}
