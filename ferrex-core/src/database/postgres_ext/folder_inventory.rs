use std::path::Path;

use crate::database::PostgresDatabase;
use crate::database::ports::folder_inventory::FolderInventoryRepository;
use crate::database::traits::{
    FolderInventory, FolderProcessingStatus, FolderScanFilters,
};
use crate::error::Result;
use crate::types::ids::LibraryID;
use chrono::{DateTime, Utc};
use uuid::Uuid;

impl PostgresDatabase {
    pub async fn get_folders_needing_scan(
        &self,
        filters: &FolderScanFilters,
    ) -> Result<Vec<FolderInventory>> {
        self.folder_inventory_repository()
            .get_folders_needing_scan(filters)
            .await
    }

    pub async fn update_folder_status(
        &self,
        folder_id: Uuid,
        status: FolderProcessingStatus,
        error: Option<String>,
    ) -> Result<()> {
        self.folder_inventory_repository()
            .update_folder_status(folder_id, status, error)
            .await
    }

    pub async fn record_folder_scan_error(
        &self,
        folder_id: Uuid,
        error: &str,
        next_retry: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.folder_inventory_repository()
            .record_folder_scan_error(folder_id, error, next_retry)
            .await
    }

    pub async fn get_folder_inventory(
        &self,
        library_id: LibraryID,
    ) -> Result<Vec<FolderInventory>> {
        self.folder_inventory_repository()
            .get_folder_inventory(library_id)
            .await
    }

    pub async fn upsert_folder(
        &self,
        folder: &FolderInventory,
    ) -> Result<Uuid> {
        self.folder_inventory_repository()
            .upsert_folder(folder)
            .await
    }

    pub async fn cleanup_stale_folders(
        &self,
        library_id: LibraryID,
        stale_after_hours: i32,
    ) -> Result<u32> {
        self.folder_inventory_repository()
            .cleanup_stale_folders(library_id, stale_after_hours)
            .await
    }

    pub async fn get_folder_by_path(
        &self,
        library_id: LibraryID,
        path: &Path,
    ) -> Result<Option<FolderInventory>> {
        self.folder_inventory_repository()
            .get_folder_by_path(library_id, path)
            .await
    }

    pub async fn update_folder_stats(
        &self,
        folder_id: Uuid,
        total_files: i32,
        processed_files: i32,
        total_size_bytes: i64,
        file_types: Vec<String>,
    ) -> Result<()> {
        self.folder_inventory_repository()
            .update_folder_stats(
                folder_id,
                total_files,
                processed_files,
                total_size_bytes,
                file_types,
            )
            .await
    }

    pub async fn mark_folder_processed(&self, folder_id: Uuid) -> Result<()> {
        self.folder_inventory_repository()
            .mark_folder_processed(folder_id)
            .await
    }

    pub async fn get_child_folders(
        &self,
        parent_folder_id: Uuid,
    ) -> Result<Vec<FolderInventory>> {
        self.folder_inventory_repository()
            .get_child_folders(parent_folder_id)
            .await
    }

    pub async fn get_season_folders(
        &self,
        parent_folder_id: Uuid,
    ) -> Result<Vec<FolderInventory>> {
        self.folder_inventory_repository()
            .get_season_folders(parent_folder_id)
            .await
    }
}
