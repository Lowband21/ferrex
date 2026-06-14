//! Media/watch-state data domain for Ferrex player clients.

pub mod messages;
pub mod selectors;
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
    fn clear_current_show_data(&self) -> bool {
        false
    }
}

#[derive(Debug)]
pub struct MediaDomainState {
    pub last_progress_sent: f64,
    pub last_progress_update: Option<Instant>,
    pub user_watch_state: Option<UserWatchState>,
    pub current_season_details: Option<SeasonDetails>,
    pub current_media_id: Option<MediaID>,
    pub pending_resume_position: Option<f32>,
    pub last_ui_refresh_for_progress: Option<std::time::Instant>,
    pub repo_accessor: Accessor<ReadWrite>,
    pub api_service: Option<Arc<dyn ApiService>>,
}

impl MediaDomainState {
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

    pub fn is_watched(&self, media_id: &MediaID) -> bool {
        self.user_watch_state.as_ref().is_some_and(|watch_state| {
            watch_state.completed.contains(media_id.as_uuid())
        })
    }

    pub fn is_in_progress(&self, media_id: &MediaID) -> bool {
        self.user_watch_state.as_ref().is_some_and(|watch_state| {
            watch_state.in_progress.contains_key(media_id.as_uuid())
        })
    }

    pub fn get_watch_status(&self, media_id: &MediaID) -> f32 {
        self.get_media_progress(media_id).unwrap_or(0.0)
    }
}

#[derive(Debug)]
pub struct MediaDomain {
    pub state: MediaDomainState,
}

impl MediaDomain {
    pub fn new(state: MediaDomainState) -> Self {
        Self { state }
    }

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
