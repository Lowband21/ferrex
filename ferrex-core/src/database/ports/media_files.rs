use crate::{
    database::traits::MediaStats,
    error::{MediaError, Result},
    types::files::MediaFileMetadata,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::types::files::MediaFile;
use crate::types::ids::LibraryID;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaFileSortField {
    DiscoveredAt,
    CreatedAt,
    FileSize,
    Filename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaFileSort {
    pub field: MediaFileSortField,
    pub direction: SortDirection,
}

impl MediaFileSort {
    pub const fn ascending(field: MediaFileSortField) -> Self {
        Self {
            field,
            direction: SortDirection::Ascending,
        }
    }

    pub const fn descending(field: MediaFileSortField) -> Self {
        Self {
            field,
            direction: SortDirection::Descending,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MediaFileFilter {
    pub library_id: Option<LibraryID>,
    pub path_prefix: Option<String>,
    pub extension_in: Vec<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub discovered_after: Option<DateTime<Utc>>,
    pub discovered_before: Option<DateTime<Utc>>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub limit: u32,
    pub offset: u32,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            limit: 100,
            offset: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpsertOutcome {
    pub id: Uuid,
    pub created: bool,
}

#[async_trait]
pub trait MediaFilesReadPort: Send + Sync {
    async fn get_by_id(&self, id: &Uuid) -> Result<Option<MediaFile>>;
    async fn get_by_path(&self, path: &str) -> Result<Option<MediaFile>>;
    async fn exists_by_path(&self, path: &str) -> Result<bool>;
    async fn list(
        &self,
        filter: MediaFileFilter,
        sort: MediaFileSort,
        page: Page,
    ) -> Result<Vec<MediaFile>>;
    async fn stats(&self, filter: MediaFileFilter) -> Result<MediaStats>;
}

#[async_trait]
pub trait MediaFilesWritePort: Send + Sync {
    async fn upsert(&self, file: MediaFile) -> Result<UpsertOutcome>;
    async fn upsert_batch(&self, files: Vec<MediaFile>) -> Result<Vec<UpsertOutcome>>;
    async fn delete_by_id(&self, id: Uuid) -> Result<()>;
    async fn delete_by_path(&self, library_id: LibraryID, path: &str) -> Result<()>;
    async fn update_technical_metadata(&self, id: Uuid, metadata: &MediaFileMetadata)
    -> Result<()>;
    async fn move_by_path(
        &self,
        _library_id: LibraryID,
        _old_path: &str,
        _new_path: &str,
    ) -> Result<Uuid> {
        Err(MediaError::Internal(
            "move_by_path is not yet implemented".into(),
        ))
    }
}
