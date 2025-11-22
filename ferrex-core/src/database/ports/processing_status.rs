use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    database::traits::MediaProcessingStatus,
    error::Result,
    types::{files::MediaFile, ids::LibraryId},
};

#[async_trait]
pub trait ProcessingStatusRepository: Send + Sync {
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
