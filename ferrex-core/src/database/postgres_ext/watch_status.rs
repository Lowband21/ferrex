use crate::{
    database::PostgresDatabase,
    database::ports::watch_status::WatchStatusRepository,
    domain::watch::{InProgressItem, UpdateProgressRequest, UserWatchState},
    types::watch::{EpisodeKey, NextEpisode, SeasonWatchStatus, SeriesWatchStatus},
    error::Result,
};
use uuid::Uuid;

impl PostgresDatabase {
    pub async fn update_watch_progress(
        &self,
        user_id: Uuid,
        progress: &UpdateProgressRequest,
    ) -> Result<()> {
        self.watch_status_repository()
            .update_watch_progress(user_id, progress)
            .await
    }

    pub async fn get_user_watch_state(
        &self,
        user_id: Uuid,
    ) -> Result<UserWatchState> {
        self.watch_status_repository()
            .get_user_watch_state(user_id)
            .await
    }

    pub async fn get_continue_watching(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<InProgressItem>> {
        self.watch_status_repository()
            .get_continue_watching(user_id, limit)
            .await
    }

    pub async fn clear_watch_progress(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<()> {
        self.watch_status_repository()
            .clear_watch_progress(user_id, media_id)
            .await
    }

    pub async fn is_media_completed(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<bool> {
        self.watch_status_repository()
            .is_media_completed(user_id, media_id)
            .await
    }

    // Identity-based helpers
    pub async fn upsert_episode_identity_progress(
        &self,
        user_id: Uuid,
        key: &EpisodeKey,
        position: f32,
        duration: f32,
        last_media_uuid: Option<Uuid>,
    ) -> Result<()> {
        self.watch_status_repository()
            .upsert_episode_identity_progress(
                user_id,
                key,
                position,
                duration,
                last_media_uuid,
            )
            .await
    }

    pub async fn get_series_watch_status(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
    ) -> Result<SeriesWatchStatus> {
        self.watch_status_repository()
            .get_series_watch_status(user_id, tmdb_series_id)
            .await
    }

    pub async fn get_season_watch_status(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
        season_number: u16,
    ) -> Result<SeasonWatchStatus> {
        self.watch_status_repository()
            .get_season_watch_status(user_id, tmdb_series_id, season_number)
            .await
    }

    pub async fn get_next_episode(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
    ) -> Result<Option<NextEpisode>> {
        self.watch_status_repository()
            .get_next_episode(user_id, tmdb_series_id)
            .await
    }

    pub async fn mark_episode_completed(
        &self,
        user_id: Uuid,
        key: &EpisodeKey,
    ) -> Result<()> {
        self.watch_status_repository()
            .mark_episode_completed(user_id, key)
            .await
    }

    pub async fn clear_episode_state(
        &self,
        user_id: Uuid,
        key: &EpisodeKey,
    ) -> Result<()> {
        self.watch_status_repository()
            .clear_episode_state(user_id, key)
            .await
    }
}
