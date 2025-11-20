use async_trait::async_trait;
use uuid::Uuid;

use crate::error::Result;
use crate::watch_status::{
    InProgressItem, UpdateProgressRequest, UserWatchState,
};

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
}
