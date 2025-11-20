use async_trait::async_trait;
use std::path::Path;

use crate::database::ports::folder_inventory::FolderInventoryRepository;
use crate::database::traits::{
    FolderDiscoverySource, FolderInventory, FolderProcessingStatus, FolderScanFilters, FolderType,
};
use crate::{LibraryID, MediaError, Result};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};
use tracing::info;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct PostgresFolderInventoryRepository {
    pool: PgPool,
}

impl PostgresFolderInventoryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl FolderInventoryRepository for PostgresFolderInventoryRepository {
    async fn get_folders_needing_scan(
        &self,
        filters: &FolderScanFilters,
    ) -> Result<Vec<FolderInventory>> {
        self.get_folders_needing_scan_impl(filters).await
    }

    async fn update_folder_status(
        &self,
        folder_id: Uuid,
        status: FolderProcessingStatus,
        error: Option<String>,
    ) -> Result<()> {
        self.update_folder_status_impl(folder_id, status, error)
            .await
    }

    async fn record_folder_scan_error(
        &self,
        folder_id: Uuid,
        error: &str,
        next_retry: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.record_folder_scan_error_impl(folder_id, error, next_retry)
            .await
    }

    async fn get_folder_inventory(&self, library_id: LibraryID) -> Result<Vec<FolderInventory>> {
        self.get_folder_inventory_impl(library_id).await
    }

    async fn upsert_folder(&self, folder: &FolderInventory) -> Result<Uuid> {
        self.upsert_folder_impl(folder).await
    }

    async fn cleanup_stale_folders(
        &self,
        library_id: LibraryID,
        stale_after_hours: i32,
    ) -> Result<u32> {
        self.cleanup_stale_folders_impl(library_id, stale_after_hours)
            .await
    }

    async fn get_folder_by_path(
        &self,
        library_id: LibraryID,
        path: &Path,
    ) -> Result<Option<FolderInventory>> {
        self.get_folder_by_path_impl(library_id, path).await
    }

    async fn update_folder_stats(
        &self,
        folder_id: Uuid,
        total_files: i32,
        processed_files: i32,
        total_size_bytes: i64,
        file_types: Vec<String>,
    ) -> Result<()> {
        self.update_folder_stats_impl(
            folder_id,
            total_files,
            processed_files,
            total_size_bytes,
            file_types,
        )
        .await
    }

    async fn mark_folder_processed(&self, folder_id: Uuid) -> Result<()> {
        self.mark_folder_processed_impl(folder_id).await
    }

    async fn get_child_folders(&self, parent_folder_id: Uuid) -> Result<Vec<FolderInventory>> {
        self.get_child_folders_impl(parent_folder_id).await
    }

    async fn get_season_folders(&self, parent_folder_id: Uuid) -> Result<Vec<FolderInventory>> {
        self.get_season_folders_impl(parent_folder_id).await
    }
}

/// Folder inventory management extensions implemented for the PostgreSQL repository.
impl PostgresFolderInventoryRepository {
    /// Get folders that need scanning based on filters
    pub async fn get_folders_needing_scan_impl(
        &self,
        filters: &FolderScanFilters,
    ) -> Result<Vec<FolderInventory>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT
                id, library_id, folder_path, folder_type, parent_folder_id,
                discovered_at, last_seen_at, discovery_source,
                processing_status, last_processed_at, processing_error,
                processing_attempts, next_retry_at,
                total_files, processed_files, total_size_bytes,
                file_types, last_modified,
                metadata, created_at, updated_at
            FROM folder_inventory
            WHERE 1=1
            "#,
        );

        if let Some(library_id) = filters.library_id {
            builder.push(" AND library_id = ");
            builder.push_bind(library_id.as_uuid());
        }

        if let Some(status) = filters.processing_status {
            let status_str = serde_json::to_string(&status)
                .unwrap()
                .trim_matches('"')
                .to_string();
            builder.push(" AND processing_status = ");
            builder.push_bind(status_str);
        }

        if let Some(folder_type) = filters.folder_type {
            let type_str = serde_json::to_string(&folder_type)
                .unwrap()
                .trim_matches('"')
                .to_string();
            builder.push(" AND folder_type = ");
            builder.push_bind(type_str);
        }

        if let Some(max_attempts) = filters.max_attempts {
            builder.push(" AND processing_attempts < ");
            builder.push_bind(max_attempts);
        }

        if let Some(stale_hours) = filters.stale_after_hours {
            builder.push(" AND last_seen_at < NOW() - ");
            builder.push_bind(format!("{} hours", stale_hours));
            builder.push("::interval");
        }

        // Add retry condition - only get folders that are ready for retry
        builder.push(" AND (next_retry_at IS NULL OR next_retry_at <= NOW())");

        // Add prioritized ordering:
        // 1. Pending (unscanned) folders first
        // 2. Failed folders with retry attempts remaining
        // 3. Everything else by oldest scan time
        let retry_threshold = filters.error_retry_threshold.unwrap_or(3);
        builder.push(
            r#"
            ORDER BY
                CASE
                    WHEN processing_status = 'pending' THEN 1
                    WHEN processing_status = 'failed' AND processing_attempts < "#,
        );
        builder.push_bind(retry_threshold);
        builder.push(
            r#" THEN 2
                    ELSE 3
                END,
                processing_attempts ASC,
                last_seen_at ASC"#,
        );

        // Apply batch size limit if specified
        let limit = filters.max_batch_size.or(filters.limit).unwrap_or(100);
        builder.push(" LIMIT ");
        builder.push_bind(limit);

        let rows = builder
            .build_query_as::<FolderInventoryRow>()
            .fetch_all(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to get folders needing scan: {}", e))
            })?;

        Ok(rows.into_iter().map(|row| row.into()).collect())
    }

    /// Update folder processing status
    pub async fn update_folder_status_impl(
        &self,
        folder_id: Uuid,
        status: FolderProcessingStatus,
        error: Option<String>,
    ) -> Result<()> {
        let status_str = serde_json::to_string(&status)
            .unwrap()
            .trim_matches('"')
            .to_string();

        let last_processed_at = if status == FolderProcessingStatus::Completed {
            Some(Utc::now())
        } else {
            None
        };

        sqlx::query!(
            r#"
            UPDATE folder_inventory
            SET processing_status = $1,
                processing_error = $2,
                last_processed_at = $3,
                updated_at = NOW()
            WHERE id = $4
            "#,
            status_str,
            error,
            last_processed_at,
            folder_id
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update folder status: {}", e)))?;

        Ok(())
    }

    /// Record a folder scan error and update retry information
    pub async fn record_folder_scan_error_impl(
        &self,
        folder_id: Uuid,
        error: &str,
        next_retry: Option<DateTime<Utc>>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE folder_inventory
            SET processing_status = 'failed',
                processing_error = $1,
                processing_attempts = processing_attempts + 1,
                next_retry_at = $2,
                updated_at = NOW()
            WHERE id = $3
            "#,
            error,
            next_retry,
            folder_id
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to record folder scan error: {}", e)))?;

        Ok(())
    }

    /// Get complete folder inventory for a library
    pub async fn get_folder_inventory_impl(
        &self,
        library_id: LibraryID,
    ) -> Result<Vec<FolderInventory>> {
        let rows = sqlx::query_as::<_, FolderInventoryRow>(
            r#"
            SELECT
                id, library_id, folder_path, folder_type, parent_folder_id,
                discovered_at, last_seen_at, discovery_source,
                processing_status, last_processed_at, processing_error,
                processing_attempts, next_retry_at,
                total_files, processed_files, total_size_bytes,
                file_types, last_modified,
                metadata, created_at, updated_at
            FROM folder_inventory
            WHERE library_id = $1
            ORDER BY folder_path
            "#,
        )
        .bind(library_id.as_uuid())
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get folder inventory: {}", e)))?;

        Ok(rows.into_iter().map(|row| row.into()).collect())
    }

    /// Upsert a folder (insert or update if exists)
    pub async fn upsert_folder_impl(&self, folder: &FolderInventory) -> Result<Uuid> {
        let folder_type_str = serde_json::to_string(&folder.folder_type)
            .unwrap()
            .trim_matches('"')
            .to_string();
        let discovery_source_str = serde_json::to_string(&folder.discovery_source)
            .unwrap()
            .trim_matches('"')
            .to_string();
        let processing_status_str = serde_json::to_string(&folder.processing_status)
            .unwrap()
            .trim_matches('"')
            .to_string();

        let file_types_json =
            serde_json::to_value(&folder.file_types).unwrap_or_else(|_| serde_json::json!([]));

        let result = sqlx::query!(
            r#"
            INSERT INTO folder_inventory (
                id, library_id, folder_path, folder_type, parent_folder_id,
                discovered_at, last_seen_at, discovery_source,
                processing_status, last_processed_at, processing_error,
                processing_attempts, next_retry_at,
                total_files, processed_files, total_size_bytes,
                file_types, last_modified, metadata
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19
            )
            ON CONFLICT (library_id, folder_path)
            DO UPDATE SET
                folder_type = EXCLUDED.folder_type,
                parent_folder_id = EXCLUDED.parent_folder_id,
                last_seen_at = EXCLUDED.last_seen_at,
                processing_status = EXCLUDED.processing_status,
                last_processed_at = EXCLUDED.last_processed_at,
                processing_error = EXCLUDED.processing_error,
                processing_attempts = EXCLUDED.processing_attempts,
                next_retry_at = EXCLUDED.next_retry_at,
                total_files = EXCLUDED.total_files,
                processed_files = EXCLUDED.processed_files,
                total_size_bytes = EXCLUDED.total_size_bytes,
                file_types = EXCLUDED.file_types,
                last_modified = EXCLUDED.last_modified,
                metadata = EXCLUDED.metadata,
                updated_at = NOW()
            RETURNING id
            "#,
            folder.id,
            folder.library_id.as_uuid(),
            folder.folder_path,
            folder_type_str,
            folder.parent_folder_id,
            folder.discovered_at,
            folder.last_seen_at,
            discovery_source_str,
            processing_status_str,
            folder.last_processed_at,
            folder.processing_error,
            folder.processing_attempts,
            folder.next_retry_at,
            folder.total_files,
            folder.processed_files,
            folder.total_size_bytes,
            file_types_json,
            folder.last_modified,
            folder.metadata
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to upsert folder: {}", e)))?;

        Ok(result.id)
    }

    /// Cleanup stale folders that haven't been seen in the specified time
    pub async fn cleanup_stale_folders_impl(
        &self,
        library_id: LibraryID,
        stale_after_hours: i32,
    ) -> Result<u32> {
        let result = sqlx::query!(
            r#"
            DELETE FROM folder_inventory
            WHERE library_id = $1
            AND last_seen_at < NOW() - INTERVAL '1 hour' * $2
            "#,
            library_id.as_uuid(),
            stale_after_hours as f64
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to cleanup stale folders: {}", e)))?;

        let deleted_count = result.rows_affected() as u32;

        if deleted_count > 0 {
            info!(
                "Cleaned up {} stale folders from library {}",
                deleted_count, library_id
            );
        }

        Ok(deleted_count)
    }

    /// Get folder by path
    pub async fn get_folder_by_path_impl(
        &self,
        library_id: LibraryID,
        path: &Path,
    ) -> Result<Option<FolderInventory>> {
        let row = sqlx::query_as::<_, FolderInventoryRow>(
            r#"
            SELECT
                id, library_id, folder_path, folder_type, parent_folder_id,
                discovered_at, last_seen_at, discovery_source,
                processing_status, last_processed_at, processing_error,
                processing_attempts, next_retry_at,
                total_files, processed_files, total_size_bytes,
                file_types, last_modified,
                metadata, created_at, updated_at
            FROM folder_inventory
            WHERE library_id = $1 AND folder_path = $2
            "#,
        )
        .bind(library_id.as_uuid())
        .bind(path.to_string_lossy().to_string())
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get folder by path: {}", e)))?;

        Ok(row.map(|r| r.into()))
    }

    /// Update folder content statistics
    pub async fn update_folder_stats_impl(
        &self,
        folder_id: Uuid,
        total_files: i32,
        processed_files: i32,
        total_size_bytes: i64,
        file_types: Vec<String>,
    ) -> Result<()> {
        let file_types_json =
            serde_json::to_value(&file_types).unwrap_or_else(|_| serde_json::json!([]));

        sqlx::query!(
            r#"
            UPDATE folder_inventory
            SET total_files = $1,
                processed_files = $2,
                total_size_bytes = $3,
                file_types = $4,
                updated_at = NOW()
            WHERE id = $5
            "#,
            total_files,
            processed_files,
            total_size_bytes,
            file_types_json,
            folder_id
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update folder stats: {}", e)))?;

        Ok(())
    }

    /// Mark folder as processed
    pub async fn mark_folder_processed_impl(&self, folder_id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE folder_inventory
            SET processing_status = 'completed',
                last_processed_at = NOW(),
                processing_error = NULL,
                updated_at = NOW()
            WHERE id = $1
            "#,
            folder_id
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to mark folder as processed: {}", e)))?;

        Ok(())
    }

    /// Get child folders of a parent folder
    pub async fn get_child_folders_impl(
        &self,
        parent_folder_id: Uuid,
    ) -> Result<Vec<FolderInventory>> {
        let rows = sqlx::query_as::<_, FolderInventoryRow>(
            r#"
            SELECT
                id, library_id, folder_path, folder_type, parent_folder_id,
                discovered_at, last_seen_at, discovery_source,
                processing_status, last_processed_at, processing_error,
                processing_attempts, next_retry_at,
                total_files, processed_files, total_size_bytes,
                file_types, last_modified,
                metadata, created_at, updated_at
            FROM folder_inventory
            WHERE parent_folder_id = $1
            ORDER BY folder_path
            "#,
        )
        .bind(parent_folder_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get child folders: {}", e)))?;

        Ok(rows.into_iter().map(|row| row.into()).collect())
    }

    /// Get season folders under a series folder
    pub async fn get_season_folders_impl(
        &self,
        parent_folder_id: Uuid,
    ) -> Result<Vec<FolderInventory>> {
        let rows = sqlx::query_as::<_, FolderInventoryRow>(
            r#"
            SELECT
                id, library_id, folder_path, folder_type, parent_folder_id,
                discovered_at, last_seen_at, discovery_source,
                processing_status, last_processed_at, processing_error,
                processing_attempts, next_retry_at,
                total_files, processed_files, total_size_bytes,
                file_types, last_modified,
                metadata, created_at, updated_at
            FROM folder_inventory
            WHERE parent_folder_id = $1 AND folder_type = 'season'
            ORDER BY folder_path
            "#,
        )
        .bind(parent_folder_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get season folders: {}", e)))?;

        Ok(rows.into_iter().map(|row| row.into()).collect())
    }
}

// Database row type for folder inventory
#[derive(sqlx::FromRow)]
struct FolderInventoryRow {
    id: Uuid,
    library_id: Uuid,
    folder_path: String,
    folder_type: String,
    parent_folder_id: Option<Uuid>,
    discovered_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    discovery_source: String,
    processing_status: String,
    last_processed_at: Option<DateTime<Utc>>,
    processing_error: Option<String>,
    processing_attempts: i32,
    next_retry_at: Option<DateTime<Utc>>,
    total_files: i32,
    processed_files: i32,
    total_size_bytes: i64,
    file_types: serde_json::Value,
    last_modified: Option<DateTime<Utc>>,
    metadata: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<FolderInventoryRow> for FolderInventory {
    fn from(row: FolderInventoryRow) -> Self {
        let folder_type = match row.folder_type.as_str() {
            "root" => FolderType::Root,
            "movie" => FolderType::Movie,
            "tv_show" => FolderType::TvShow,
            "season" => FolderType::Season,
            "extra" => FolderType::Extra,
            _ => FolderType::Unknown,
        };

        let discovery_source = match row.discovery_source.as_str() {
            "scan" => FolderDiscoverySource::Scan,
            "watch" => FolderDiscoverySource::Watch,
            "manual" => FolderDiscoverySource::Manual,
            "import" => FolderDiscoverySource::Import,
            _ => FolderDiscoverySource::Scan,
        };

        let processing_status = match row.processing_status.as_str() {
            "pending" => FolderProcessingStatus::Pending,
            "processing" => FolderProcessingStatus::Processing,
            "completed" => FolderProcessingStatus::Completed,
            "failed" => FolderProcessingStatus::Failed,
            "skipped" => FolderProcessingStatus::Skipped,
            "queued" => FolderProcessingStatus::Queued,
            _ => FolderProcessingStatus::Pending,
        };

        let file_types: Vec<String> = row
            .file_types
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        FolderInventory {
            id: row.id,
            library_id: LibraryID(row.library_id),
            folder_path: row.folder_path,
            folder_type,
            parent_folder_id: row.parent_folder_id,
            discovered_at: row.discovered_at,
            last_seen_at: row.last_seen_at,
            discovery_source,
            processing_status,
            last_processed_at: row.last_processed_at,
            processing_error: row.processing_error,
            processing_attempts: row.processing_attempts,
            next_retry_at: row.next_retry_at,
            total_files: row.total_files,
            processed_files: row.processed_files,
            total_size_bytes: row.total_size_bytes,
            file_types,
            last_modified: row.last_modified,
            metadata: row.metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
