use async_trait::async_trait;
use std::path::Path;
use uuid::Uuid;

use crate::database::traits::{FolderInventory, FolderProcessingStatus, FolderScanFilters};
use crate::{LibraryID, Result};

#[async_trait]
pub trait FolderInventoryRepository: Send + Sync {
    async fn get_folders_needing_scan(
        &self,
        filters: &FolderScanFilters,
    ) -> Result<Vec<FolderInventory>>;
    async fn update_folder_status(
        &self,
        folder_id: Uuid,
        status: FolderProcessingStatus,
        error: Option<String>,
    ) -> Result<()>;
    async fn record_folder_scan_error(
        &self,
        folder_id: Uuid,
        error: &str,
        next_retry: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()>;
    async fn get_folder_inventory(&self, library_id: LibraryID) -> Result<Vec<FolderInventory>>;
    async fn upsert_folder(&self, folder: &FolderInventory) -> Result<Uuid>;
    async fn cleanup_stale_folders(
        &self,
        library_id: LibraryID,
        stale_after_hours: i32,
    ) -> Result<u32>;
    async fn get_folder_by_path(
        &self,
        library_id: LibraryID,
        path: &Path,
    ) -> Result<Option<FolderInventory>>;
    async fn update_folder_stats(
        &self,
        folder_id: Uuid,
        total_files: i32,
        processed_files: i32,
        total_size_bytes: i64,
        file_types: Vec<String>,
    ) -> Result<()>;
    async fn mark_folder_processed(&self, folder_id: Uuid) -> Result<()>;
    async fn get_child_folders(&self, parent_folder_id: Uuid) -> Result<Vec<FolderInventory>>;
    async fn get_season_folders(&self, parent_folder_id: Uuid) -> Result<Vec<FolderInventory>>;
}
