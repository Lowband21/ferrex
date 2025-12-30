//! Media playback domain
//!
//! Contains all media playback-related state and logic moved from the monolithic State

pub mod messages;
pub mod selectors;
pub mod update;

use crate::{
    common::messages::{CrossDomainEvent, DomainMessage},
    infra::{
        repository::{Accessor, ReadWrite},
        services::api::ApiService,
    },
};

use ferrex_core::player_prelude::{
    InProgressItem, MediaID, MediaIDLike, SeasonDetails, UserWatchState,
};

use iced::Task;

use std::{sync::Arc, time::Instant};

/// Media domain state - focused on media management, not playback
#[derive(Debug)]
pub struct MediaDomainState {
    pub last_progress_sent: f64,
    pub last_progress_update: Option<Instant>,
    // Media management state
    pub user_watch_state: Option<UserWatchState>,
    pub current_season_details: Option<SeasonDetails>,
    pub current_media_id: Option<MediaID>,
    pub pending_resume_position: Option<f32>, // Resume position for next media to play
    pub last_ui_refresh_for_progress: Option<std::time::Instant>, // Track last UI refresh for debouncing

    pub repo_accessor: Accessor<ReadWrite>,
    pub api_service: Option<Arc<dyn ApiService>>,
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
        repo_accessor: Accessor<ReadWrite>,
        api_service: Option<Arc<dyn ApiService>>,
    ) -> Self {
        //let query_service = Arc::new(MediaQueryService::new(Arc::clone(&media_store)));

        Self {
            last_progress_sent: 0.0,
            last_progress_update: None,
            user_watch_state: None,
            current_season_details: None,
            current_media_id: None,
            pending_resume_position: None,
            last_ui_refresh_for_progress: None,
            //query_service,
            repo_accessor,
            api_service,
        }
    }

    pub fn get_watch_state(&self) -> &Option<UserWatchState> {
        &self.user_watch_state
    }

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

    pub fn update_cached_watched(self, id: MediaID, _: f32) {
        if let Some(mut state) = self.user_watch_state {
            state.completed.insert(id.to_uuid());
        }
    }

    /// Get the watch progress for a specific media item
    /// Returns Some(progress) where progress is 0.0-1.0, or None if no watch state loaded
    pub fn get_media_progress(&self, media_id: &MediaID) -> Option<f32> {
        if let Some(ref watch_state) = self.user_watch_state {
            // Check if it's in progress
            if let Some(in_progress) =
                watch_state.in_progress.get(media_id.as_uuid())
                && in_progress.duration > 0.0
            {
                return Some(
                    (in_progress.position / in_progress.duration)
                        .clamp(0.0, 1.0),
                );
            }

            // Check if it's completed
            if watch_state.completed.contains(media_id.as_uuid()) {
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
    pub fn is_watched(&self, media_id: &MediaID) -> bool {
        if let Some(ref watch_state) = self.user_watch_state {
            watch_state.completed.contains(media_id.as_uuid())
        } else {
            false
        }
    }

    /// Check if a media item is currently in progress
    pub fn is_in_progress(&self, media_id: &MediaID) -> bool {
        if let Some(ref watch_state) = self.user_watch_state {
            watch_state.in_progress.contains_key(media_id.as_uuid())
        } else {
            false
        }
    }

    /// Get watch status for UI display
    /// Returns: 0.0 for unwatched, 0.0-0.95 for in-progress, 1.0 for watched
    pub fn get_watch_status(&self, media_id: &MediaID) -> f32 {
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

    pub fn handle_event(
        &mut self,
        event: &CrossDomainEvent,
    ) -> Task<DomainMessage> {
        match event {
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
