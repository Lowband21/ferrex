use std::fmt;

use crate::{
    database::{
        PostgresDatabase,
        ports::processing_status::ProcessingStatusRepository as ProcessingStatusRepositoryTrait,
        traits::MediaProcessingStatus,
    },
    error::Result,
    types::{LibraryId, MediaFile},
};

use uuid::Uuid;

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
