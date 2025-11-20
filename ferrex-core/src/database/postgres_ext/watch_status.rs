use crate::database::PostgresDatabase;
use crate::ports::watch_status::WatchStatusRepository;
use crate::{InProgressItem, Result, UpdateProgressRequest, UserWatchState};
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

    pub async fn get_user_watch_state(&self, user_id: Uuid) -> Result<UserWatchState> {
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

    pub async fn clear_watch_progress(&self, user_id: Uuid, media_id: &Uuid) -> Result<()> {
        self.watch_status_repository()
            .clear_watch_progress(user_id, media_id)
            .await
    }

    pub async fn is_media_completed(&self, user_id: Uuid, media_id: &Uuid) -> Result<bool> {
        self.watch_status_repository()
            .is_media_completed(user_id, media_id)
            .await
    }
}
