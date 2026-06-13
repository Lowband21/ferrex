use async_trait::async_trait;
use uuid::Uuid;

use crate::database::traits::FileWatchEvent;
use crate::error::Result;
use crate::types::ids::LibraryId;

/// Repository for persisting file system change events detected by watchers.
#[async_trait]
pub trait FileWatchEventRepository: Send + Sync {
    /// Persist a normalized watcher event.
    ///
    /// Returns `true` when a new row was inserted and `false` when an
    /// existing row already owns the same idempotency key.
    async fn create_event(&self, event: &FileWatchEvent) -> Result<bool>;

    async fn get_unprocessed_events(
        &self,
        library_id: LibraryId,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>>;

    async fn mark_processed(&self, event_id: Uuid) -> Result<()>;

    async fn cleanup_processed(&self, days_to_keep: i32) -> Result<u32>;
}
