//! Media playback domain
//!
//! Contains all media playback-related state and logic moved from the monolithic State

pub mod library;
pub mod messages;
pub mod models;
pub mod services;
pub mod store;
pub mod update;
pub mod update_handlers;

use self::services::MediaQueryService;
use self::store::MediaStore;
pub use self::store::{MediaStoreQuerying, MediaStoreSorting};
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::domains::media::messages::Message as MediaMessage;
use crate::domains::media::models::SeasonDetails;
use crate::infrastructure::{
    adapters::api_client_adapter::ApiClientAdapter,
    api_types::{EpisodeReference, SeasonReference, UserWatchState},
};
use ferrex_core::MediaId;
use iced::Task;
use std::sync::{Arc, RwLock as StdRwLock};
use uuid::Uuid;

/// Media domain state - focused on media management, not playback
#[derive(Debug)]
pub struct MediaDomainState {
    // Media management state
    pub user_watch_state: Option<UserWatchState>,
    pub current_season_details: Option<SeasonDetails>,
    pub current_media_id: Option<ferrex_core::api_types::MediaId>,
    // REMOVED: current_show_seasons and current_season_episodes
    // These are now accessed directly from MediaStore to maintain single source of truth

    // Domain services
    pub query_service: Arc<MediaQueryService>,

    // Shared references needed by media domain
    pub media_store: Arc<StdRwLock<MediaStore>>,
    pub api_service: Option<Arc<ApiClientAdapter>>,
    pub current_library_id: Option<Uuid>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl MediaDomainState {
    pub fn new(
        media_store: Arc<StdRwLock<MediaStore>>,
        api_service: Option<Arc<ApiClientAdapter>>,
    ) -> Self {
        let query_service = Arc::new(MediaQueryService::new(Arc::clone(&media_store)));

        Self {
            user_watch_state: None,
            current_season_details: None,
            current_media_id: None,
            query_service,
            media_store,
            api_service,
            current_library_id: None,
        }
    }

    /// Get a media reference by MediaId
    /// Returns the appropriate MediaReference type based on the MediaId variant
    pub fn get_media_by_id(
        &self,
        media_id: &MediaId,
    ) -> Option<crate::infrastructure::api_types::MediaReference> {
        if let Ok(store) = self.media_store.read() {
            store.get(media_id).cloned()
        } else {
            None
        }
    }

    /// Get episode count for a season
    pub fn get_season_episode_count(&self, season_id: &str) -> u32 {
        if let Ok(store) = self.media_store.read() {
            store.get_episodes(season_id).len() as u32
        } else {
            0
        }
    }

    pub fn get_watch_state(&self) -> &Option<UserWatchState> {
        &self.user_watch_state
    }

    /// Get the watch progress for a specific media item
    /// Returns Some(progress) where progress is 0.0-1.0, or None if no watch state loaded
    pub fn get_media_progress(&self, media_id: &MediaId) -> Option<f32> {
        if let Some(ref watch_state) = self.user_watch_state {
            // Check if it's in progress
            if let Some(in_progress) = watch_state
                .in_progress
                .iter()
                .find(|item| &item.media_id == media_id)
            {
                if in_progress.duration > 0.0 {
                    return Some((in_progress.position / in_progress.duration).clamp(0.0, 1.0));
                }
            }

            // Check if it's completed
            if watch_state.completed.contains(media_id) {
                return Some(1.0);
            }

            // If we have watch state but item isn't in progress or completed, it's unwatched
            Some(0.0)
        } else {
            // No watch state loaded yet
            None
        }
    }

    /// Check if a media item has been watched (>= 95% completion)
    pub fn is_watched(&self, media_id: &MediaId) -> bool {
        if let Some(ref watch_state) = self.user_watch_state {
            watch_state.completed.contains(media_id)
        } else {
            false
        }
    }

    /// Check if a media item is currently in progress
    pub fn is_in_progress(&self, media_id: &MediaId) -> bool {
        if let Some(ref watch_state) = self.user_watch_state {
            watch_state
                .in_progress
                .iter()
                .any(|item| &item.media_id == media_id)
        } else {
            false
        }
    }

    /// Get watch status for UI display
    /// Returns: 0.0 for unwatched, 0.0-0.95 for in-progress, 1.0 for watched
    pub fn get_watch_status(&self, media_id: &MediaId) -> f32 {
        self.get_media_progress(media_id).unwrap_or(0.0)
    }
}

#[derive(Debug)]
pub struct MediaDomain {
    pub state: MediaDomainState,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl MediaDomain {
    pub fn new(state: MediaDomainState) -> Self {
        Self { state }
    }

    /// Update function - delegates to existing update_media logic
    pub fn update(&mut self, message: MediaMessage) -> Task<DomainMessage> {
        // This will call the existing update_media function
        // For now, we return Task::none() to make it compile
        Task::none()
    }

    pub fn handle_event(&mut self, event: &CrossDomainEvent) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::UserAuthenticated(_user, _permissions) => {
                // Could load user watch state here
                Task::none()
            }
            CrossDomainEvent::LibraryChanged(library_id) => {
                self.state.current_library_id = Some(*library_id); // TODO: Isn't this handled by the library domain?
                Task::none()
            }
            CrossDomainEvent::ClearMediaStore => {
                // Clear media store data
                if let Ok(mut store) = self.state.media_store.write() {
                    store.clear();
                }
                Task::none()
            }
            CrossDomainEvent::ClearCurrentShowData => {
                // Clear current show data
                // REMOVED: No longer clearing duplicate state fields
                // MediaStore is the single source of truth
                self.state.current_season_details = None;
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
