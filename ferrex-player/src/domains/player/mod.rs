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

use self::messages::Message;
use self::state::PlayerDomainState;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::domains::media::store::MediaStore;
use crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
use iced::Task;
use std::sync::{Arc, RwLock as StdRwLock};
use uuid::Uuid;

// Re-export key types
pub use state::{AspectRatio, TrackNotification};

/// Player domain wrapper - PlayerState is the actual domain state
#[derive(Debug)]
pub struct PlayerDomain {
    pub state: PlayerDomainState,
    // Cross-domain dependencies
    pub media_store: Arc<StdRwLock<MediaStore>>,
    pub api_service: Option<Arc<ApiClientAdapter>>,
    pub current_library_id: Option<Uuid>,
}

impl PlayerDomain {
    pub fn new(media_store: Arc<StdRwLock<MediaStore>>, api_service: Option<Arc<ApiClientAdapter>>) -> Self {
        Self {
            state: PlayerDomainState::default(),
            media_store,
            api_service,
            current_library_id: None,
        }
    }

    /// Update function - delegates to player update logic
    /// Note: This method is not currently used as update_player is called directly from main update.rs
    /// If this method is needed, window_size should be passed as a parameter
    pub fn update(&mut self, message: Message) -> Task<DomainMessage> {
        // Using a default window size - this should be updated if this method is used
        let default_window_size = iced::Size::new(1280.0, 720.0);
        let result = update::update_player(&mut self.state, message, default_window_size);
        // Return the task from the DomainUpdateResult
        // Events are handled by the mediator at a higher level
        result.task
    }

    /// Handle cross-domain events
    pub fn handle_event(&mut self, event: &CrossDomainEvent) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::LibraryChanged(library_id) => {
                self.current_library_id = Some(*library_id);
                Task::none()
            }
            CrossDomainEvent::MediaStarted(_media_id) => {
                // Player domain doesn't need to handle this - it emits it
                Task::none()
            }
            // Legacy transcoding events - no longer used
            CrossDomainEvent::RequestTranscoding(_) | CrossDomainEvent::TranscodingReady(_) => {
                Task::none()
            }
            // NOTE: VideoReadyToPlay moved to direct Media messages in Task 2.7
            _ => Task::none(),
        }
    }
}
