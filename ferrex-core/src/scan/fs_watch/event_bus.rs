use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::Stream;
use uuid::Uuid;

use crate::database::MediaDatabase;
use crate::database::traits::FileWatchEvent;
use crate::error::{MediaError, Result};
use crate::types::ids::LibraryID;

pub mod postgres;
pub use postgres::PostgresFileChangeEventBus;

/// Stream of file change events for a subscriber group.
pub type FileChangeEventStream =
    Pin<Box<dyn Stream<Item = FileWatchEvent> + Send>>;

/// Represents a durable cursor for a subscriber group.
#[derive(Debug, Clone)]
pub struct FileChangeCursor {
    pub group: String,
    pub library_id: LibraryID,
    pub last_event_id: Option<Uuid>,
    pub last_detected_at: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait FileChangeEventBus: Send + Sync {
    async fn publish(&self, event: FileWatchEvent) -> Result<()>;

    async fn subscribe(
        &self,
        _group: &str,
        _library_id: LibraryID,
    ) -> Result<FileChangeEventStream> {
        Err(MediaError::Internal(
            "FileChangeEventBus::subscribe not implemented".into(),
        ))
    }

    async fn ack(&self, _group: &str, _event_id: Uuid) -> Result<()> {
        Err(MediaError::Internal(
            "FileChangeEventBus::ack not implemented".into(),
        ))
    }

    async fn commit_cursor(&self, _cursor: FileChangeCursor) -> Result<()> {
        Err(MediaError::Internal(
            "FileChangeEventBus::commit_cursor not implemented".into(),
        ))
    }

    async fn get_cursor(
        &self,
        _group: &str,
        _library_id: LibraryID,
    ) -> Result<Option<FileChangeCursor>> {
        Err(MediaError::Internal(
            "FileChangeEventBus::get_cursor not implemented".into(),
        ))
    }

    async fn get_unprocessed_events(
        &self,
        library_id: LibraryID,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>>;

    async fn mark_processed(&self, event_id: Uuid) -> Result<()>;

    async fn cleanup_retention(&self, days_to_keep: i32) -> Result<u32>;
}

#[derive(Clone)]
pub struct LegacyDatabaseFileChangeEventBus {
    db: Arc<MediaDatabase>,
}

impl LegacyDatabaseFileChangeEventBus {
    pub fn new(db: Arc<MediaDatabase>) -> Self {
        Self { db }
    }
}

impl fmt::Debug for LegacyDatabaseFileChangeEventBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LegacyDatabaseFileChangeEventBus")
            .field("db", &self.db)
            .finish()
    }
}

#[async_trait]
impl FileChangeEventBus for LegacyDatabaseFileChangeEventBus {
    async fn publish(&self, event: FileWatchEvent) -> Result<()> {
        self.db.backend().create_file_watch_event(&event).await
    }

    async fn subscribe(
        &self,
        _group: &str,
        _library_id: LibraryID,
    ) -> Result<FileChangeEventStream> {
        Err(MediaError::Internal(
            "LegacyDatabaseFileChangeEventBus does not support durable subscribe".into(),
        ))
    }

    async fn ack(&self, _group: &str, _event_id: Uuid) -> Result<()> {
        Err(MediaError::Internal(
            "LegacyDatabaseFileChangeEventBus does not support durable ack"
                .into(),
        ))
    }

    async fn commit_cursor(&self, _cursor: FileChangeCursor) -> Result<()> {
        Err(MediaError::Internal(
            "LegacyDatabaseFileChangeEventBus does not support cursor commits"
                .into(),
        ))
    }

    async fn get_cursor(
        &self,
        _group: &str,
        _library_id: LibraryID,
    ) -> Result<Option<FileChangeCursor>> {
        Ok(None)
    }

    async fn get_unprocessed_events(
        &self,
        library_id: LibraryID,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>> {
        self.db
            .backend()
            .get_unprocessed_events(library_id, limit)
            .await
    }

    async fn mark_processed(&self, event_id: Uuid) -> Result<()> {
        self.db.backend().mark_event_processed(event_id).await
    }

    async fn cleanup_retention(&self, days_to_keep: i32) -> Result<u32> {
        self.db.backend().cleanup_old_events(days_to_keep).await
    }
}
