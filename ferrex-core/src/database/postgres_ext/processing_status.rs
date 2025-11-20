use std::{fmt, path::PathBuf};

use sqlx::Row;
use uuid::Uuid;

use crate::{
    LibraryID, MediaError, MediaFile, MediaFileMetadata, Result,
    database::{postgres::PostgresDatabase, traits::MediaProcessingStatus},
};

/// Repository handling media processing status persistence.
pub struct ProcessingStatusRepository<'a> {
    db: &'a PostgresDatabase,
}

impl<'a> fmt::Debug for ProcessingStatusRepository<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stats = self.db.pool_stats();
        f.debug_struct("ProcessingStatusRepository")
            .field("pool_size", &stats.size)
            .field("pool_idle", &stats.idle)
            .field("pool_max", &stats.max_size)
            .field("pool_min_idle", &stats.min_idle)
            .finish()
    }
}

impl<'a> ProcessingStatusRepository<'a> {
    pub fn new(db: &'a PostgresDatabase) -> Self {
        Self { db }
    }

    pub async fn create_or_update(&self, status: &MediaProcessingStatus) -> Result<()> {
        let error_details_json = status
            .error_details
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| {
                MediaError::Internal(format!("Failed to serialize error details: {}", e))
            })?;

        sqlx::query!(
            r#"
            INSERT INTO media_processing_status (
                media_file_id, metadata_extracted, metadata_extracted_at,
                tmdb_matched, tmdb_matched_at, images_cached, images_cached_at,
                file_analyzed, file_analyzed_at, last_error, error_details,
                retry_count, next_retry_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (media_file_id) DO UPDATE SET
                metadata_extracted = EXCLUDED.metadata_extracted,
                metadata_extracted_at = EXCLUDED.metadata_extracted_at,
                tmdb_matched = EXCLUDED.tmdb_matched,
                tmdb_matched_at = EXCLUDED.tmdb_matched_at,
                images_cached = EXCLUDED.images_cached,
                images_cached_at = EXCLUDED.images_cached_at,
                file_analyzed = EXCLUDED.file_analyzed,
                file_analyzed_at = EXCLUDED.file_analyzed_at,
                last_error = EXCLUDED.last_error,
                error_details = EXCLUDED.error_details,
                retry_count = EXCLUDED.retry_count,
                next_retry_at = EXCLUDED.next_retry_at,
                updated_at = NOW()
            "#,
            status.media_file_id,
            status.metadata_extracted,
            status.metadata_extracted_at,
            status.tmdb_matched,
            status.tmdb_matched_at,
            status.images_cached,
            status.images_cached_at,
            status.file_analyzed,
            status.file_analyzed_at,
            status.last_error,
            error_details_json,
            status.retry_count,
            status.next_retry_at
        )
        .execute(self.db.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to create/update processing status: {}", e))
        })?;

        Ok(())
    }

    pub async fn get(&self, media_file_id: Uuid) -> Result<Option<MediaProcessingStatus>> {
        let row = sqlx::query!(
            r#"
            SELECT media_file_id, metadata_extracted, metadata_extracted_at,
                   tmdb_matched, tmdb_matched_at, images_cached, images_cached_at,
                   file_analyzed, file_analyzed_at, last_error, error_details,
                   retry_count, next_retry_at, created_at, updated_at
            FROM media_processing_status
            WHERE media_file_id = $1
            "#,
            media_file_id
        )
        .fetch_optional(self.db.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get processing status: {}", e)))?;

        if let Some(row) = row {
            Ok(Some(MediaProcessingStatus {
                media_file_id: row.media_file_id,
                metadata_extracted: row.metadata_extracted,
                metadata_extracted_at: row.metadata_extracted_at,
                tmdb_matched: row.tmdb_matched,
                tmdb_matched_at: row.tmdb_matched_at,
                images_cached: row.images_cached,
                images_cached_at: row.images_cached_at,
                file_analyzed: row.file_analyzed,
                file_analyzed_at: row.file_analyzed_at,
                last_error: row.last_error,
                error_details: row.error_details,
                retry_count: row.retry_count,
                next_retry_at: row.next_retry_at,
                created_at: row.created_at,
                updated_at: row.updated_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn fetch_unprocessed(
        &self,
        library_id: LibraryID,
        status_type: &str,
        limit: i32,
    ) -> Result<Vec<MediaFile>> {
        let sql = match status_type {
            "metadata" => {
                r#"
                SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                       f.technical_metadata, f.parsed_info, f.created_at, f.updated_at
                FROM media_files f
                LEFT JOIN media_processing_status p ON f.id = p.media_file_id
                WHERE f.library_id = $1
                  AND (
                        p.media_file_id IS NULL
                        OR (
                            COALESCE(p.metadata_extracted, false) = false
                            AND (p.next_retry_at IS NULL OR p.next_retry_at <= NOW())
                        )
                    )
                LIMIT $2
                "#
            }
            "tmdb" => {
                r#"
                SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                       f.technical_metadata, f.parsed_info, f.created_at, f.updated_at
                FROM media_files f
                LEFT JOIN media_processing_status p ON f.id = p.media_file_id
                WHERE f.library_id = $1
                  AND COALESCE(p.metadata_extracted, false) = true
                  AND (
                        (
                            COALESCE(p.tmdb_matched, false) = false
                            OR p.last_error IS NOT NULL
                        )
                        AND (p.next_retry_at IS NULL OR p.next_retry_at <= NOW())
                    )
                LIMIT $2
                "#
            }
            "images" => {
                r#"
                SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                       f.technical_metadata, f.parsed_info, f.created_at, f.updated_at
                FROM media_files f
                LEFT JOIN media_processing_status p ON f.id = p.media_file_id
                WHERE f.library_id = $1 AND (p.images_cached IS NULL OR p.images_cached = false)
                LIMIT $2
                "#
            }
            "analyze" => {
                r#"
                SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                       f.technical_metadata, f.parsed_info, f.created_at, f.updated_at
                FROM media_files f
                LEFT JOIN media_processing_status p ON f.id = p.media_file_id
                WHERE f.library_id = $1 AND (p.file_analyzed IS NULL OR p.file_analyzed = false)
                LIMIT $2
                "#
            }
            _ => {
                return Err(MediaError::InvalidMedia(format!(
                    "Unknown status type: {}",
                    status_type
                )));
            }
        };

        let rows = sqlx::query(sql)
            .bind(library_id.as_uuid())
            .bind(limit as i64)
            .fetch_all(self.db.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to get unprocessed files: {}", e)))?;

        let mut files = Vec::with_capacity(rows.len());
        for row in rows {
            let metadata: Option<MediaFileMetadata> = if let Some(meta_json) =
                row.try_get::<Option<serde_json::Value>, _>("technical_metadata")?
            {
                serde_json::from_value(meta_json).ok()
            } else {
                None
            };

            files.push(MediaFile {
                id: row.try_get("id")?,
                library_id: LibraryID(row.try_get("library_id")?),
                path: PathBuf::from(row.try_get::<String, _>("file_path")?),
                filename: row.try_get("filename")?,
                size: row.try_get::<i64, _>("file_size")? as u64,
                created_at: row.try_get("created_at")?,
                media_file_metadata: metadata,
            });
        }

        Ok(files)
    }

    pub async fn fetch_failed(
        &self,
        library_id: LibraryID,
        max_retries: i32,
    ) -> Result<Vec<MediaFile>> {
        let rows = sqlx::query!(
            r#"
            SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                   f.technical_metadata, f.parsed_info, f.created_at, f.updated_at
            FROM media_files f
            JOIN media_processing_status p ON f.id = p.media_file_id
            WHERE f.library_id = $1
              AND p.retry_count > 0
              AND p.retry_count <= $2
              AND (p.next_retry_at IS NULL OR p.next_retry_at <= NOW())
            "#,
            library_id.as_uuid(),
            max_retries
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get failed files: {}", e)))?;

        let mut files = Vec::with_capacity(rows.len());
        for row in rows {
            let metadata = if let Some(meta_json) = row.technical_metadata {
                serde_json::from_value(meta_json).ok()
            } else {
                None
            };

            files.push(MediaFile {
                id: row.id,
                library_id: LibraryID(row.library_id),
                path: PathBuf::from(&row.file_path),
                filename: row.filename,
                size: row.file_size as u64,
                created_at: row.created_at,
                media_file_metadata: metadata,
            });
        }

        Ok(files)
    }

    pub async fn reset(&self, media_file_id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM media_processing_status WHERE media_file_id = $1
            "#,
            media_file_id
        )
        .execute(self.db.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to reset processing status: {}", e)))?;

        Ok(())
    }
}
