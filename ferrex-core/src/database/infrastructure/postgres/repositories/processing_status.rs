use async_trait::async_trait;
use std::path::PathBuf;

use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    database::{
        ports::processing_status::ProcessingStatusRepository,
        traits::MediaProcessingStatus,
    },
    error::{MediaError, Result},
    types::{files::MediaFile, ids::LibraryId},
};

#[derive(Clone, Debug)]
pub struct PostgresProcessingStatusRepository {
    pool: PgPool,
}

impl PostgresProcessingStatusRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn create_or_update(
        &self,
        status: &MediaProcessingStatus,
    ) -> Result<()> {
        let error_details_json = status
            .error_details
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to serialize error details: {}",
                    e
                ))
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
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to create/update processing status: {}",
                e
            ))
        })?;

        Ok(())
    }

    pub async fn get(
        &self,
        media_file_id: Uuid,
    ) -> Result<Option<MediaProcessingStatus>> {
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
        .fetch_optional(self.pool())
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
        library_id: LibraryId,
        status_type: &str,
        limit: i32,
    ) -> Result<Vec<MediaFile>> {
        match status_type {
            "metadata" => {
                let rows = sqlx::query!(
                    r#"
                    SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                           f.technical_metadata, f.parsed_info, f.discovered_at, f.created_at, f.updated_at
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
                    "#,
                    library_id.as_uuid(),
                    limit as i64
                )
                .fetch_all(self.pool())
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to get unprocessed files: {}", e)))?;

                let mut files = Vec::with_capacity(rows.len());
                for row in rows {
                    let metadata = row
                        .technical_metadata
                        .map(serde_json::from_value)
                        .transpose()
                        .ok()
                        .flatten();
                    files.push(MediaFile {
                        id: row.id,
                        library_id: LibraryId(row.library_id),
                        path: PathBuf::from(row.file_path),
                        filename: row.filename,
                        size: row.file_size as u64,
                        discovered_at: row.discovered_at,
                        created_at: row.created_at,
                        media_file_metadata: metadata,
                    });
                }
                Ok(files)
            }
            "tmdb" => {
                let rows = sqlx::query!(
                    r#"
                    SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                           f.technical_metadata, f.parsed_info, f.discovered_at, f.created_at, f.updated_at
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
                    "#,
                    library_id.as_uuid(),
                    limit as i64
                )
                .fetch_all(self.pool())
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to get unprocessed files: {}", e)))?;

                let mut files = Vec::with_capacity(rows.len());
                for row in rows {
                    let metadata = row
                        .technical_metadata
                        .map(serde_json::from_value)
                        .transpose()
                        .ok()
                        .flatten();
                    files.push(MediaFile {
                        id: row.id,
                        library_id: LibraryId(row.library_id),
                        path: PathBuf::from(row.file_path),
                        filename: row.filename,
                        size: row.file_size as u64,
                        discovered_at: row.discovered_at,
                        created_at: row.created_at,
                        media_file_metadata: metadata,
                    });
                }
                Ok(files)
            }
            "images" => {
                let rows = sqlx::query!(
                    r#"
                    SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                           f.technical_metadata, f.parsed_info, f.discovered_at, f.created_at, f.updated_at
                    FROM media_files f
                    LEFT JOIN media_processing_status p ON f.id = p.media_file_id
                    WHERE f.library_id = $1 AND (p.images_cached IS NULL OR p.images_cached = false)
                    LIMIT $2
                    "#,
                    library_id.as_uuid(),
                    limit as i64
                )
                .fetch_all(self.pool())
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to get unprocessed files: {}", e)))?;

                let mut files = Vec::with_capacity(rows.len());
                for row in rows {
                    let metadata = row
                        .technical_metadata
                        .map(serde_json::from_value)
                        .transpose()
                        .ok()
                        .flatten();
                    files.push(MediaFile {
                        id: row.id,
                        library_id: LibraryId(row.library_id),
                        path: PathBuf::from(row.file_path),
                        filename: row.filename,
                        size: row.file_size as u64,
                        discovered_at: row.discovered_at,
                        created_at: row.created_at,
                        media_file_metadata: metadata,
                    });
                }
                Ok(files)
            }
            "analyze" => {
                let rows = sqlx::query!(
                    r#"
                    SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                           f.technical_metadata, f.parsed_info, f.discovered_at, f.created_at, f.updated_at
                    FROM media_files f
                    LEFT JOIN media_processing_status p ON f.id = p.media_file_id
                    WHERE f.library_id = $1 AND (p.file_analyzed IS NULL OR p.file_analyzed = false)
                    LIMIT $2
                    "#,
                    library_id.as_uuid(),
                    limit as i64
                )
                .fetch_all(self.pool())
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to get unprocessed files: {}", e)))?;

                let mut files = Vec::with_capacity(rows.len());
                for row in rows {
                    let metadata = row
                        .technical_metadata
                        .map(serde_json::from_value)
                        .transpose()
                        .ok()
                        .flatten();
                    files.push(MediaFile {
                        id: row.id,
                        library_id: LibraryId(row.library_id),
                        path: PathBuf::from(row.file_path),
                        filename: row.filename,
                        size: row.file_size as u64,
                        discovered_at: row.discovered_at,
                        created_at: row.created_at,
                        media_file_metadata: metadata,
                    });
                }
                Ok(files)
            }
            _ => Err(MediaError::InvalidMedia(format!(
                "Unknown status type: {}",
                status_type
            ))),
        }
    }

    pub async fn fetch_failed(
        &self,
        library_id: LibraryId,
        max_retries: i32,
    ) -> Result<Vec<MediaFile>> {
        let rows = sqlx::query!(
            r#"
            SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                   f.technical_metadata, f.parsed_info, f.discovered_at, f.created_at, f.updated_at
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
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get failed files: {}", e)))?;

        let mut files = Vec::with_capacity(rows.len());
        for row in rows {
            let metadata = row
                .technical_metadata
                .map(serde_json::from_value)
                .transpose()
                .ok()
                .flatten();

            files.push(MediaFile {
                id: row.id,
                library_id: LibraryId(row.library_id),
                path: PathBuf::from(row.file_path),
                filename: row.filename,
                size: row.file_size as u64,
                discovered_at: row.discovered_at,
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
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to reset processing status: {}",
                e
            ))
        })?;

        Ok(())
    }
}

#[async_trait]
impl ProcessingStatusRepository for PostgresProcessingStatusRepository {
    async fn create_or_update_processing_status(
        &self,
        status: &MediaProcessingStatus,
    ) -> Result<()> {
        self.create_or_update(status).await
    }

    async fn get_processing_status(
        &self,
        media_file_id: Uuid,
    ) -> Result<Option<MediaProcessingStatus>> {
        self.get(media_file_id).await
    }

    async fn get_unprocessed_files(
        &self,
        library_id: LibraryId,
        status_type: &str,
        limit: i32,
    ) -> Result<Vec<MediaFile>> {
        self.fetch_unprocessed(library_id, status_type, limit).await
    }

    async fn get_failed_files(
        &self,
        library_id: LibraryId,
        max_retries: i32,
    ) -> Result<Vec<MediaFile>> {
        self.fetch_failed(library_id, max_retries).await
    }

    async fn reset_processing_status(&self, media_file_id: Uuid) -> Result<()> {
        self.reset(media_file_id).await
    }
}
