use std::collections::HashMap;
use std::path::PathBuf;

use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::database::traits::{MediaFilters, MediaStats};
use crate::{LibraryID, MediaError, MediaFile, MediaFileMetadata, Result};

#[derive(Clone, Debug)]
pub struct PostgresMediaRepository {
    pool: PgPool,
}

impl PostgresMediaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn store_media(&self, media_file: MediaFile) -> Result<Uuid> {
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Transaction failed: {}", e)))?;

        let actual_id = self
            .store_media_file_in_transaction(&mut tx, &media_file)
            .await?;

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit transaction: {}", e)))?;

        Ok(actual_id)
    }

    pub async fn store_media_batch(&self, media_files: Vec<MediaFile>) -> Result<Vec<Uuid>> {
        if media_files.is_empty() {
            return Ok(Vec::new());
        }

        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Transaction failed: {}", e)))?;

        let mut ids = Vec::new();
        const CHUNK_SIZE: usize = 100;
        for chunk in media_files.chunks(CHUNK_SIZE) {
            for media_file in chunk {
                let actual_id = self
                    .store_media_file_in_transaction(&mut tx, media_file)
                    .await?;
                ids.push(actual_id);
            }
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit batch transaction: {}", e))
        })?;

        tracing::info!("Batch stored {} media files", ids.len());
        Ok(ids)
    }

    pub async fn get_media(&self, uuid: &Uuid) -> Result<Option<MediaFile>> {
        let row = sqlx::query!(
            r#"
            SELECT id, library_id, file_path, filename, file_size,
                   discovered_at, created_at, technical_metadata, parsed_info
            FROM media_files
            WHERE id = $1
            "#,
            uuid
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let media_file_metadata = row
            .technical_metadata
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| MediaError::Internal(format!("Failed to deserialize metadata: {}", e)))?;

        Ok(Some(MediaFile {
            id: row.id,
            path: PathBuf::from(row.file_path),
            filename: row.filename,
            size: row.file_size as u64,
            discovered_at: row.discovered_at,
            created_at: row.created_at,
            media_file_metadata,
            library_id: LibraryID(row.library_id),
        }))
    }

    pub async fn get_media_by_path(&self, path: &str) -> Result<Option<MediaFile>> {
        let row = sqlx::query!(
            r#"
            SELECT id, library_id, file_path, filename, file_size,
                   discovered_at, created_at, technical_metadata, parsed_info
            FROM media_files
            WHERE file_path = $1
            "#,
            path
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let media_file_metadata = row
            .technical_metadata
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| MediaError::Internal(format!("Failed to deserialize metadata: {}", e)))?;

        Ok(Some(MediaFile {
            id: row.id,
            path: PathBuf::from(row.file_path),
            filename: row.filename,
            size: row.file_size as u64,
            discovered_at: row.discovered_at,
            created_at: row.created_at,
            media_file_metadata,
            library_id: LibraryID(row.library_id),
        }))
    }

    pub async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>> {
        let mut query = "SELECT id, library_id, file_path, filename, file_size, discovered_at, created_at, technical_metadata, parsed_info FROM media_files".to_string();
        let mut conditions = Vec::new();

        if let Some(media_type) = &filters.media_type {
            conditions.push(format!("parsed_info->>'media_type' = '{}'", media_type));
        }

        if let Some(show_name) = &filters.show_name {
            conditions.push(format!(
                "parsed_info->>'show' = '{}'",
                show_name.replace("'", "''")
            ));
        }

        if let Some(season) = filters.season {
            conditions.push(format!("(parsed_info->>'season')::int = {}", season));
        }

        if let Some(library_id) = filters.library_id {
            conditions.push(format!("library_id = '{}'", library_id.as_uuid()));
        }

        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }

        if let Some(order_by) = filters.order_by {
            query.push_str(" ORDER BY ");
            query.push_str(&order_by);
        } else {
            query.push_str(" ORDER BY discovered_at DESC");
        }

        if let Some(limit) = filters.limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }

        let rows = sqlx::query(&query)
            .fetch_all(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut media_files = Vec::new();
        for row in rows {
            let technical_metadata: Option<serde_json::Value> =
                row.try_get("technical_metadata").ok();
            let media_file_metadata = technical_metadata
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to deserialize metadata: {}", e))
                })?;

            media_files.push(MediaFile {
                id: row.try_get("id")?,
                path: PathBuf::from(row.try_get::<String, _>("file_path")?),
                filename: row.try_get("filename")?,
                size: row.try_get::<i64, _>("file_size")? as u64,
                discovered_at: row.try_get("discovered_at")?,
                created_at: row.try_get("created_at")?,
                media_file_metadata,
                library_id: LibraryID(row.try_get("library_id")?),
            });
        }

        Ok(media_files)
    }

    pub async fn get_stats(&self) -> Result<MediaStats> {
        let total_row = sqlx::query!(
            "SELECT COUNT(*) as count, COALESCE(SUM(file_size), 0) as total_size FROM media_files"
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let type_rows = sqlx::query!(
            r#"
            SELECT
                CASE
                    WHEN parsed_info->>'media_type' IS NOT NULL THEN parsed_info->>'media_type'
                    ELSE 'unknown'
                END as media_type,
                COUNT(*) as count
            FROM media_files
            GROUP BY parsed_info->>'media_type'
            "#
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut by_type = HashMap::new();
        for row in type_rows {
            by_type.insert(
                row.media_type.unwrap_or_else(|| "unknown".to_string()),
                row.count.unwrap_or(0) as u64,
            );
        }

        Ok(MediaStats {
            total_files: total_row.count.unwrap_or(0) as u64,
            total_size: total_row
                .total_size
                .and_then(|size| size.to_string().parse::<u64>().ok())
                .unwrap_or(0),
            by_type,
        })
    }

    pub async fn file_exists(&self, path: &str) -> Result<bool> {
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM media_files WHERE file_path = $1",
            path
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        Ok(count.unwrap_or(0) > 0)
    }

    pub async fn delete_media(&self, id: &str) -> Result<()> {
        let uuid = Uuid::parse_str(id)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;

        sqlx::query!("DELETE FROM media_files WHERE id = $1", uuid)
            .execute(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Delete failed: {}", e)))?;

        Ok(())
    }

    pub async fn get_all_media(&self) -> Result<Vec<MediaFile>> {
        self.list_media(MediaFilters::default()).await
    }

    pub async fn store_external_metadata(
        &self,
        media_id: &str,
        metadata: &MediaFileMetadata,
    ) -> Result<()> {
        let uuid = Uuid::parse_str(media_id)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;

        let metadata_json = serde_json::to_value(metadata).map_err(|e| {
            MediaError::InvalidMedia(format!("Failed to serialize metadata: {}", e))
        })?;

        sqlx::query!(
            "UPDATE media_files SET technical_metadata = $1, updated_at = NOW() WHERE id = $2",
            metadata_json,
            uuid
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Update failed: {}", e)))?;

        Ok(())
    }

    async fn store_media_file_in_transaction(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        media_file: &MediaFile,
    ) -> Result<Uuid> {
        let library_check = sqlx::query!(
            "SELECT id FROM libraries WHERE id = $1",
            media_file.library_id.as_uuid()
        )
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to check library existence: {}", e)))?;

        if library_check.is_none() {
            return Err(MediaError::InvalidMedia(format!(
                "Library with ID {} does not exist",
                media_file.library_id
            )));
        }

        let technical_metadata = media_file
            .media_file_metadata
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to serialize metadata: {}", e))
            })?;

        let parsed_info = technical_metadata
            .as_ref()
            .and_then(|m| m.get("parsed_info"))
            .cloned();

        let file_path_str = media_file.path.to_string_lossy().to_string();

        let actual_id = sqlx::query_scalar!(
            r#"
            INSERT INTO media_files (
                id, library_id, file_path, filename, file_size, created_at,
                technical_metadata, parsed_info
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (file_path) DO UPDATE SET
                filename = EXCLUDED.filename,
                file_size = EXCLUDED.file_size,
                technical_metadata = EXCLUDED.technical_metadata,
                parsed_info = EXCLUDED.parsed_info,
                updated_at = NOW()
            RETURNING id
            "#,
            media_file.id,
            media_file.library_id.as_uuid(),
            file_path_str,
            media_file.filename,
            media_file.size as i64,
            media_file.created_at,
            technical_metadata,
            parsed_info
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to store media file: {}", e)))?;

        if actual_id != media_file.id {
            tracing::info!(
                "Media file path {} already existed with ID {}, using existing ID instead of {}",
                file_path_str,
                actual_id,
                media_file.id
            );
        }

        Ok(actual_id)
    }
}
