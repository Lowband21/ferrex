//! Player domain
//!
//! Contains all video player functionality, playback control, and UI

pub mod controls;
pub mod messages;
pub mod state;
pub mod theme;
pub mod track_selection;
pub mod update;
pub mod video;
pub mod view;

pub mod external_mpv;

use self::messages::Message;
use self::state::PlayerDomainState;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::infra::services::api::ApiService;
use ferrex_core::player_prelude::LibraryID;
use iced::Task;
use std::sync::Arc;

// Re-export key types
pub use state::TrackNotification;

/// Player domain wrapper - PlayerState is the actual domain state
#[derive(Debug)]
pub struct PlayerDomain {
    pub state: PlayerDomainState,
    // Cross-domain dependencies
    //pub media_store: Arc<StdRwLock<MediaStore>>,
    pub api_service: Option<Arc<dyn ApiService>>,
    pub current_library_id: Option<LibraryID>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl PlayerDomain {
    pub fn new(
        //media_store: Arc<StdRwLock<MediaStore>>,
        api_service: Option<Arc<dyn ApiService>>,
    ) -> Self {
        Self {
            state: PlayerDomainState::default(),
            //media_store,
            api_service,
            current_library_id: None,
        }
    }

    /// Update function - delegates to player update logic
    /// Note: This method is not currently used as update_player is called directly from main update.rs
    /// If this method is needed, window_size should be passed as a parameter
    pub fn update(&mut self, _message: Message) -> Task<DomainMessage> {
        // Not used in current routing; player updates are handled at root with access to full State
        Task::none()
    }

    /// Handle cross-domain events
    pub fn handle_event(
        &mut self,
        event: &CrossDomainEvent,
    ) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::LibraryChanged(library_id) => {
                self.current_library_id = Some(*library_id);
                Task::none()
            }
            CrossDomainEvent::MediaStarted(_media_id) => {
                // Player domain doesn't need to handle this - it emits it
                Task::none()
            } // CrossDomainEvent::RequestTranscoding(_)
            // | CrossDomainEvent::TranscodingReady(_) => Task::none(),
            //
            _ => Task::none(),
        }
    }
}
