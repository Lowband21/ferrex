//! Media/watch-state data domain for Ferrex player clients.
//!
//! This crate owns dependency-light state and reducers for the current media
//! selection, season details, cached watch progress, and playback resume hints.
//! UI crates render these values and app shells provide persistence/services.

/// Media domain messages and subscription DTOs.
pub mod messages;
/// Selection helpers for episodes and watch-state decisions.
pub mod selectors;
/// UI-agnostic media reducer logic.
pub mod update;

use ferrex_core::player_prelude::{
    InProgressItem, MediaID, MediaIDLike, SeasonDetails, UserWatchState,
};
use ferrex_player_api::services::api::ApiService;
use ferrex_player_foundation::domain::DomainTask;
use ferrex_player_library::repository::{Accessor, ReadWrite};
use std::{sync::Arc, time::Instant};

/// Cross-domain event view needed by media state.
pub trait MediaExternalEvent {
    /// Whether current show/season details should be cleared.
    fn clear_current_show_data(&self) -> bool {
        false
    }
}

/// Media and watch-state cache owned by the player media domain.
#[derive(Debug)]
pub struct MediaDomainState {
    /// Last playback position sent to the server.
    pub last_progress_sent: f64,
    /// Last instant a progress update was recorded.
    pub last_progress_update: Option<Instant>,
    /// Cached watch state for the current user/session.
    pub user_watch_state: Option<UserWatchState>,
    /// Details for the currently focused season.
    pub current_season_details: Option<SeasonDetails>,
    /// Current media identifier, if a media item is focused/playing.
    pub current_media_id: Option<MediaID>,
    /// Resume position to apply when playback starts.
    pub pending_resume_position: Option<f32>,
    /// Last instant progress was refreshed for the UI.
    pub last_ui_refresh_for_progress: Option<std::time::Instant>,
    /// Repository accessor used to resolve cached media references.
    pub repo_accessor: Accessor<ReadWrite>,
    /// Optional API service for server-backed media operations.
    pub api_service: Option<Arc<dyn ApiService>>,
}

impl MediaDomainState {
    /// Build media domain state from repository and optional API services.
    pub fn new(
        repo_accessor: Accessor<ReadWrite>,
        api_service: Option<Arc<dyn ApiService>>,
    ) -> Self {
        Self {
            last_progress_sent: 0.0,
            last_progress_update: None,
            user_watch_state: None,
            current_season_details: None,
            current_media_id: None,
            pending_resume_position: None,
            last_ui_refresh_for_progress: None,
            repo_accessor,
            api_service,
        }
    }

    /// Borrow the cached watch state, if it has been loaded.
    pub fn get_watch_state(&self) -> &Option<UserWatchState> {
        &self.user_watch_state
    }

    /// Update the in-memory in-progress cache for immediate UI feedback.
    pub fn update_cached_in_progress(
        &mut self,
        id: MediaID,
        position: f32,
        duration: f32,
    ) {
        if let Some(state) = &mut self.user_watch_state {
            state.in_progress.insert(
                id.to_uuid(),
                InProgressItem {
                    media_id: id.to_uuid(),
                    position,
                    duration,
                    last_watched: chrono::Utc::now().timestamp(),
                },
            );
        }
    }

    /// Update the in-memory completed cache for immediate UI feedback.
    pub fn update_cached_watched(self, id: MediaID, _: f32) {
        if let Some(mut state) = self.user_watch_state {
            state.completed.insert(id.to_uuid());
        }
    }

    /// Return normalized progress for a media id from cached watch state.
    pub fn get_media_progress(&self, media_id: &MediaID) -> Option<f32> {
        if let Some(ref watch_state) = self.user_watch_state {
            if let Some(in_progress) =
                watch_state.in_progress.get(media_id.as_uuid())
                && in_progress.duration > 0.0
            {
                return Some(
                    (in_progress.position / in_progress.duration)
                        .clamp(0.0, 1.0),
                );
            }

            if watch_state.completed.contains(media_id.as_uuid()) {
                return Some(1.0);
            }

            Some(0.0)
        } else {
            None
        }
    }

    /// Whether a media id is marked completed in cached watch state.
    pub fn is_watched(&self, media_id: &MediaID) -> bool {
        self.user_watch_state.as_ref().is_some_and(|watch_state| {
            watch_state.completed.contains(media_id.as_uuid())
        })
    }

    /// Whether a media id has an in-progress entry in cached watch state.
    pub fn is_in_progress(&self, media_id: &MediaID) -> bool {
        self.user_watch_state.as_ref().is_some_and(|watch_state| {
            watch_state.in_progress.contains_key(media_id.as_uuid())
        })
    }

    /// Return progress for badge/progress-bar rendering, defaulting to unwatched.
    pub fn get_watch_status(&self, media_id: &MediaID) -> f32 {
        self.get_media_progress(media_id).unwrap_or(0.0)
    }
}

/// Media domain wrapper used by app shells to route cross-domain events.
#[derive(Debug)]
pub struct MediaDomain {
    /// Mutable media domain state.
    pub state: MediaDomainState,
}

impl MediaDomain {
    /// Build a media domain wrapper from existing state.
    pub fn new(state: MediaDomainState) -> Self {
        Self { state }
    }

    /// Handle cross-domain state reset events.
    pub fn handle_event<E>(
        &mut self,
        event: &E,
    ) -> DomainTask<messages::MediaMessage>
    where
        E: MediaExternalEvent,
    {
        if event.clear_current_show_data() {
            self.state.current_season_details = None;
        }
        DomainTask::none()
    }
}
