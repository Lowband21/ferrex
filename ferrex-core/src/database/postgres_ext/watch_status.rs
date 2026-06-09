use crate::{
    database::PostgresDatabase,
    database::repository_ports::watch_status::WatchStatusRepository,
    domain::watch::{
        ContinueWatchingItem, SeriesContinueWatchingItem,
        UpdateProgressRequest, UserWatchState,
    },
    error::Result,
    types::watch::{
        EpisodeKey, NextEpisode, SeasonWatchStatus, SeriesWatchStatus,
    },
};
use ferrex_model::{LibraryId, VideoMediaType};
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
    ) -> Result<Vec<ContinueWatchingItem>> {
        self.watch_status_repository()
            .get_continue_watching(user_id, limit)
            .await
    }

    /// Return series continue/next-up cards scoped to one library.
    pub async fn get_library_series_continue_watching(
        &self,
        user_id: Uuid,
        library_id: LibraryId,
        limit: usize,
    ) -> Result<Vec<SeriesContinueWatchingItem>> {
        self.watch_status_repository()
            .get_library_series_continue_watching(user_id, library_id, limit)
            .await
    }

    /// Return library-scoped series ids with completed or resume-eligible episode state.
    pub async fn list_library_series_ids_with_meaningful_watch_state(
        &self,
        user_id: Uuid,
        library_id: LibraryId,
    ) -> Result<Vec<Uuid>> {
        self.watch_status_repository()
            .list_library_series_ids_with_meaningful_watch_state(
                user_id, library_id,
            )
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

    pub async fn mark_media_watched(
        &self,
        user_id: Uuid,
        media_id: Uuid,
        media_type: VideoMediaType,
        last_media_uuid: Option<Uuid>,
    ) -> Result<()> {
        self.watch_status_repository()
            .mark_media_watched(user_id, media_id, media_type, last_media_uuid)
            .await
    }

    pub async fn mark_media_unwatched(
        &self,
        user_id: Uuid,
        media_id: Uuid,
        media_type: VideoMediaType,
    ) -> Result<()> {
        self.watch_status_repository()
            .mark_media_unwatched(user_id, media_id, media_type)
            .await
    }

    pub async fn mark_series_watched(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
    ) -> Result<()> {
        self.watch_status_repository()
            .mark_series_watched(user_id, tmdb_series_id)
            .await
    }

    pub async fn mark_series_unwatched(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
    ) -> Result<()> {
        self.watch_status_repository()
            .mark_series_unwatched(user_id, tmdb_series_id)
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
