use async_trait::async_trait;
use std::fmt;
use uuid::Uuid;

use crate::database::PostgresDatabase;
use crate::{
    database::traits::MediaProcessingStatus,
    error::Result,
    types::{files::MediaFile, ids::LibraryId},
};

#[async_trait]
pub trait ProcessingStatusRepositoryTrait: Send + Sync {
    async fn create_or_update_processing_status(
        &self,
        status: &MediaProcessingStatus,
    ) -> Result<()>;
    async fn get_processing_status(
        &self,
        media_file_id: Uuid,
    ) -> Result<Option<MediaProcessingStatus>>;
    async fn get_unprocessed_files(
        &self,
        library_id: LibraryId,
        status_type: &str,
        limit: i32,
    ) -> Result<Vec<MediaFile>>;
    async fn get_failed_files(
        &self,
        library_id: LibraryId,
        max_retries: i32,
    ) -> Result<Vec<MediaFile>>;
    async fn reset_processing_status(&self, media_file_id: Uuid) -> Result<()>;
}

pub struct ProcessingStatusRepository<'a> {
    db: &'a PostgresDatabase,
}

impl<'a> fmt::Debug for ProcessingStatusRepository<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessingStatusRepository")
            .field("db", &"PostgresDatabase")
            .finish()
    }
}

impl<'a> ProcessingStatusRepository<'a> {
    pub fn new(db: &'a PostgresDatabase) -> Self {
        Self { db }
    }

    pub async fn create_or_update(
        &self,
        status: &MediaProcessingStatus,
    ) -> Result<()> {
        self.db
            .processing_status_repository()
            .create_or_update_processing_status(status)
            .await
    }

    pub async fn get(
        &self,
        media_file_id: Uuid,
    ) -> Result<Option<MediaProcessingStatus>> {
        self.db
            .processing_status_repository()
            .get_processing_status(media_file_id)
            .await
    }

    pub async fn fetch_unprocessed(
        &self,
        library_id: LibraryId,
        status_type: &str,
        limit: i32,
    ) -> Result<Vec<MediaFile>> {
        self.db
            .processing_status_repository()
            .get_unprocessed_files(library_id, status_type, limit)
            .await
    }

    pub async fn fetch_failed(
        &self,
        library_id: LibraryId,
        max_retries: i32,
    ) -> Result<Vec<MediaFile>> {
        self.db
            .processing_status_repository()
            .get_failed_files(library_id, max_retries)
            .await
    }

    pub async fn reset(&self, media_file_id: Uuid) -> Result<()> {
        self.db
            .processing_status_repository()
            .reset_processing_status(media_file_id)
            .await
    }
}
