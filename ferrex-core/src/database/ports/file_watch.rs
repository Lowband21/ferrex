use async_trait::async_trait;
use uuid::Uuid;

use crate::database::traits::FileWatchEvent;
use crate::error::Result;
use crate::types::ids::LibraryID;

/// Repository for persisting file system change events detected by watchers.
#[async_trait]
pub trait FileWatchEventRepository: Send + Sync {
    async fn create_event(&self, event: &FileWatchEvent) -> Result<()>;

    async fn get_unprocessed_events(
        &self,
        library_id: LibraryID,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>>;

    async fn mark_processed(&self, event_id: Uuid) -> Result<()>;

    async fn cleanup_processed(&self, days_to_keep: i32) -> Result<u32>;
}
