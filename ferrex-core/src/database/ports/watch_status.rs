use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::watch::{
    EpisodeKey, InProgressItem, NextEpisode, SeasonWatchStatus,
    SeriesWatchStatus, UpdateProgressRequest, UserWatchState,
};
use crate::error::Result;

#[async_trait]
pub trait WatchStatusRepository: Send + Sync {
    async fn update_watch_progress(
        &self,
        user_id: Uuid,
        progress: &UpdateProgressRequest,
    ) -> Result<()>;
    async fn get_user_watch_state(
        &self,
        user_id: Uuid,
    ) -> Result<UserWatchState>;
    async fn get_continue_watching(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<InProgressItem>>;
    async fn clear_watch_progress(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<()>;
    async fn is_media_completed(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<bool>;

    // Identity-based episode progress (Option B)
    async fn upsert_episode_identity_progress(
        &self,
        user_id: Uuid,
        key: &EpisodeKey,
        position: f32,
        duration: f32,
        last_media_uuid: Option<Uuid>,
    ) -> Result<()>;

    async fn get_series_watch_status(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
    ) -> Result<SeriesWatchStatus>;

    async fn get_season_watch_status(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
        season_number: u16,
    ) -> Result<SeasonWatchStatus>;

    async fn get_next_episode(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
    ) -> Result<Option<NextEpisode>>;

    async fn mark_episode_completed(
        &self,
        user_id: Uuid,
        key: &EpisodeKey,
    ) -> Result<()>;

    async fn clear_episode_state(
        &self,
        user_id: Uuid,
        key: &EpisodeKey,
    ) -> Result<()>;
}
