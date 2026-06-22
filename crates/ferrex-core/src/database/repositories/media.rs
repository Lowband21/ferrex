use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use ferrex_contracts::id::MediaIDLike;
use ferrex_model::MediaID;
use ferrex_model::media_type::VideoMediaType;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::database::repository_ports::media_files::{
    MediaFileFilter, MediaFileSort, MediaFileSortField, MediaFilesReadPort,
    MediaFilesWritePort, Page, PlaybackMediaSource, SortDirection,
    UpsertOutcome,
};
use crate::database::traits::{MediaFilters, MediaStats};
use crate::domain::scan::orchestration::delta::{
    FolderDeltaRepository, StoredMediaFile, is_direct_child_file,
};
use crate::domain::scan::orchestration::job::MediaFingerprint;
use crate::error::{MediaError, Result};
use crate::types::files::{MediaFile, MediaFileMetadata};
use crate::types::ids::LibraryId;

#[derive(Clone, Debug)]
pub struct PostgresMediaRepository {
    pool: PgPool,
}

#[async_trait]
impl MediaFilesReadPort for PostgresMediaRepository {
    async fn get_by_id(&self, id: &Uuid) -> Result<Option<MediaFile>> {
        self.get_media(id).await
    }

    async fn get_playback_source_by_id(
        &self,
        id: &Uuid,
    ) -> Result<Option<PlaybackMediaSource>> {
        self.get_playback_source(id).await
    }

    async fn get_by_media_id(
        &self,
        media_id: &MediaID,
    ) -> Result<Option<MediaFile>> {
        self.get_media_by_media_id(media_id).await
    }

    async fn get_by_path(&self, path: &str) -> Result<Option<MediaFile>> {
        self.get_media_by_path(path).await
    }

    async fn exists_by_path(&self, path: &str) -> Result<bool> {
        self.file_exists(path).await
    }

    async fn list(
        &self,
        filter: MediaFileFilter,
        sort: MediaFileSort,
        page: Page,
    ) -> Result<Vec<MediaFile>> {
        self.list_media_with(filter, sort, page).await
    }

    async fn stats(&self, filter: MediaFileFilter) -> Result<MediaStats> {
        self.stats_with_filter(filter).await
    }
}

#[async_trait]
impl MediaFilesWritePort for PostgresMediaRepository {
    async fn upsert(&self, file: MediaFile) -> Result<UpsertOutcome> {
        self.upsert_media(file).await
    }

    async fn upsert_batch(
        &self,
        files: Vec<MediaFile>,
    ) -> Result<Vec<UpsertOutcome>> {
        self.upsert_media_batch(files).await
    }

    async fn delete_by_id(&self, id: Uuid) -> Result<()> {
        self.delete_media_by_id(id).await
    }

    async fn delete_by_path(
        &self,
        library_id: LibraryId,
        path: &str,
    ) -> Result<()> {
        self.delete_media_by_path(library_id, path).await
    }

    async fn delete_by_path_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
    ) -> Result<u64> {
        self.delete_media_by_path_prefixes(library_id, prefixes)
            .await
    }

    async fn update_technical_metadata(
        &self,
        id: Uuid,
        metadata: &MediaFileMetadata,
    ) -> Result<()> {
        self.update_technical_metadata_by_id(id, metadata).await
    }

    async fn mark_available_with_fingerprint(
        &self,
        library_id: LibraryId,
        path: &str,
        fingerprint: &MediaFingerprint,
    ) -> Result<()> {
        self.mark_available_with_fingerprint_impl(library_id, path, fingerprint)
            .await
    }

    async fn move_by_path(
        &self,
        library_id: LibraryId,
        old_path: &str,
        new_path: &str,
    ) -> Result<Uuid> {
        self.move_media_by_path_impl(
            library_id,
            old_path,
            new_path,
            &MediaFingerprint::default(),
        )
        .await
    }
}

impl PostgresMediaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn default_sort() -> MediaFileSort {
        MediaFileSort::descending(MediaFileSortField::DiscoveredAt)
    }

    fn media_id_from_parts(id: Uuid, media_type: &str) -> Result<MediaID> {
        match media_type {
            "movie" => Ok(MediaID::Movie(crate::types::ids::MovieID(id))),
            "episode" => Ok(MediaID::Episode(crate::types::ids::EpisodeID(id))),
            other => Err(MediaError::Internal(format!(
                "Unsupported media_files.media_type for scan delta: {other}"
            ))),
        }
    }

    async fn is_media_file_available(&self, id: Uuid) -> Result<bool> {
        let available = sqlx::query_scalar!(
            "SELECT is_available FROM media_files WHERE id = $1",
            id
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to check media availability: {e}"
            ))
        })?;

        Ok(available.unwrap_or(false))
    }

    fn convert_filters(
        filters: MediaFilters,
    ) -> (MediaFileFilter, MediaFileSort, Page) {
        let filter = MediaFileFilter {
            library_id: filters.library_id,
            ..MediaFileFilter::default()
        };

        let mut sort = Self::default_sort();
        if let Some(order) = filters.order_by.as_deref() {
            let lowered = order.to_ascii_lowercase();
            let (field, direction) = if lowered.contains("filename") {
                (
                    MediaFileSortField::Filename,
                    lowered
                        .contains("desc")
                        .then_some(SortDirection::Descending),
                )
            } else if lowered.contains("file_size") {
                (
                    MediaFileSortField::FileSize,
                    lowered
                        .contains("desc")
                        .then_some(SortDirection::Descending),
                )
            } else if lowered.contains("created_at") {
                (
                    MediaFileSortField::CreatedAt,
                    lowered
                        .contains("desc")
                        .then_some(SortDirection::Descending),
                )
            } else if lowered.contains("discovered_at") {
                (
                    MediaFileSortField::DiscoveredAt,
                    lowered
                        .contains("desc")
                        .then_some(SortDirection::Descending),
                )
            } else {
                (sort.field, None)
            };

            sort.field = field;
            if let Some(dir) = direction {
                sort.direction = dir;
            }
        }

        let requested_limit = filters.limit.unwrap_or(100).clamp(1, 500) as u32;
        let page = Page {
            limit: requested_limit,
            offset: 0,
        };

        (filter, sort, page)
    }

    fn normalized_extensions(filter: &MediaFileFilter) -> Vec<String> {
        filter
            .extension_in
            .iter()
            .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn media_file_from_parts(
        id: Uuid,
        media_id: Uuid,
        media_type: VideoMediaType,
        library_id: Uuid,
        file_path: String,
        filename: String,
        file_size: i64,
        discovered_at: chrono::DateTime<chrono::Utc>,
        created_at: chrono::DateTime<chrono::Utc>,
        technical_metadata: Option<serde_json::Value>,
    ) -> Result<MediaFile> {
        let media_file_metadata = technical_metadata
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to deserialize metadata: {}",
                    e
                ))
            })?;

        Ok(MediaFile {
            id,
            media_id: MediaID::from((media_id, media_type)),
            path: PathBuf::from(file_path),
            filename,
            size: file_size as u64,
            discovered_at,
            created_at,
            media_file_metadata,
            library_id: LibraryId(library_id),
        })
    }

    pub async fn upsert_media(
        &self,
        media_file: MediaFile,
    ) -> Result<UpsertOutcome> {
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Transaction failed: {}", e))
        })?;

        let outcome = self
            .upsert_media_in_transaction(&mut tx, &media_file)
            .await?;

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        Ok(outcome)
    }

    pub async fn upsert_media_batch(
        &self,
        media_files: Vec<MediaFile>,
    ) -> Result<Vec<UpsertOutcome>> {
        if media_files.is_empty() {
            return Ok(Vec::new());
        }

        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Transaction failed: {}", e))
        })?;

        let mut outcomes = Vec::with_capacity(media_files.len());
        const CHUNK_SIZE: usize = 100;
        for chunk in media_files.chunks(CHUNK_SIZE) {
            for media_file in chunk {
                let outcome = self
                    .upsert_media_in_transaction(&mut tx, media_file)
                    .await?;
                outcomes.push(outcome);
            }
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!(
                "Failed to commit batch transaction: {}",
                e
            ))
        })?;

        tracing::info!("Batch stored {} media files", outcomes.len());
        Ok(outcomes)
    }

    pub async fn store_media(&self, media_file: MediaFile) -> Result<Uuid> {
        Ok(self.upsert_media(media_file).await?.id)
    }

    pub async fn store_media_batch(
        &self,
        media_files: Vec<MediaFile>,
    ) -> Result<Vec<Uuid>> {
        let outcomes = self.upsert_media_batch(media_files).await?;
        Ok(outcomes.into_iter().map(|outcome| outcome.id).collect())
    }

    pub async fn get_playback_source(
        &self,
        uuid: &Uuid,
    ) -> Result<Option<PlaybackMediaSource>> {
        let row = sqlx::query!(
            r#"
            SELECT
                id AS "id!",
                file_path AS "file_path!",
                filename AS "filename!",
                file_size AS "file_size!",
                is_available AS "is_available!"
            FROM media_files
            WHERE id = $1
            "#,
            *uuid
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for playback source: {e}"
            ))
        })?;

        Ok(row.map(|row| PlaybackMediaSource {
            id: row.id,
            path: PathBuf::from(row.file_path),
            filename: row.filename,
            size: row.file_size as u64,
            is_available: row.is_available,
        }))
    }

    pub async fn get_media(&self, uuid: &Uuid) -> Result<Option<MediaFile>> {
        let row = sqlx::query!(
            r#"
            SELECT id, media_id, media_type AS "media_type!: VideoMediaType", library_id, file_path, filename, file_size,
                   discovered_at, created_at, technical_metadata, parsed_info
            FROM media_files
            WHERE id = $1
            "#,
            uuid
        )
            .fetch_optional(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Database query failed: {}", e))
            })?;

        let Some(row) = row else {
            return Ok(None);
        };
        if !self.is_media_file_available(row.id).await? {
            return Ok(None);
        }

        let media_file_metadata = row
            .technical_metadata
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to deserialize metadata: {}",
                    e
                ))
            })?;

        let media_id: MediaID = MediaID::from((row.media_id, row.media_type));

        Ok(Some(MediaFile {
            id: row.id,
            media_id,
            path: PathBuf::from(row.file_path),
            filename: row.filename,
            size: row.file_size as u64,
            discovered_at: row.discovered_at,
            created_at: row.created_at,
            media_file_metadata,
            library_id: LibraryId(row.library_id),
        }))
    }

    pub async fn get_media_by_path(
        &self,
        path: &str,
    ) -> Result<Option<MediaFile>> {
        let row = sqlx::query!(
            r#"
            SELECT id, media_id, media_type AS "media_type!: VideoMediaType", library_id, file_path, filename, file_size,
                   discovered_at, created_at, technical_metadata, parsed_info
            FROM media_files
            WHERE file_path = $1
            "#,
            path
        )
            .fetch_optional(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Database query failed: {}", e))
            })?;

        let Some(row) = row else {
            return Ok(None);
        };
        if !self.is_media_file_available(row.id).await? {
            return Ok(None);
        }

        let media_file_metadata = row
            .technical_metadata
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to deserialize metadata: {}",
                    e
                ))
            })?;
        let media_id: MediaID = MediaID::from((row.media_id, row.media_type));

        Ok(Some(MediaFile {
            id: row.id,
            media_id,
            path: PathBuf::from(row.file_path),
            filename: row.filename,
            size: row.file_size as u64,
            discovered_at: row.discovered_at,
            created_at: row.created_at,
            media_file_metadata,
            library_id: LibraryId(row.library_id),
        }))
    }

    pub async fn get_media_by_media_id(
        &self,
        media_id: &MediaID,
    ) -> Result<Option<MediaFile>> {
        let (uuid, media_type) = (media_id.to_uuid(), media_id.media_type());

        let row = sqlx::query!(
            r#"
            SELECT id, media_id, media_type AS "media_type!: VideoMediaType", library_id, file_path, filename, file_size,
                   discovered_at, created_at, technical_metadata, parsed_info
            FROM media_files
            WHERE media_id = $1
              AND media_type = $2
            ORDER BY discovered_at DESC, id ASC
            LIMIT 1
            "#,
            uuid,
            media_type as VideoMediaType
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };
        if !self.is_media_file_available(row.id).await? {
            return Ok(None);
        }

        let media_file_metadata = row
            .technical_metadata
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to deserialize metadata: {}",
                    e
                ))
            })?;

        let media_id: MediaID = MediaID::from((row.media_id, row.media_type));

        Ok(Some(MediaFile {
            id: row.id,
            media_id,
            path: PathBuf::from(row.file_path),
            filename: row.filename,
            size: row.file_size as u64,
            discovered_at: row.discovered_at,
            created_at: row.created_at,
            media_file_metadata,
            library_id: LibraryId(row.library_id),
        }))
    }

    pub async fn list_media(
        &self,
        filters: MediaFilters,
    ) -> Result<Vec<MediaFile>> {
        let (filter, sort, page) = Self::convert_filters(filters);
        self.list_media_with(filter, sort, page).await
    }

    pub async fn list_media_with(
        &self,
        filter: MediaFileFilter,
        sort: MediaFileSort,
        page: Page,
    ) -> Result<Vec<MediaFile>> {
        let library_id = filter.library_id.map(|id| id.to_uuid());
        let path_prefix = filter
            .path_prefix
            .as_ref()
            .map(|prefix| format!("{}%", prefix));
        let extensions = Self::normalized_extensions(&filter);
        let min_size = filter.min_size.map(|size| size as i64);
        let max_size = filter.max_size.map(|size| size as i64);
        let sort_key: i16 = match sort.field {
            MediaFileSortField::DiscoveredAt => 0,
            MediaFileSortField::CreatedAt => 1,
            MediaFileSortField::FileSize => 2,
            MediaFileSortField::Filename => 3,
        };
        let sort_ascending = matches!(sort.direction, SortDirection::Ascending);

        let rows = sqlx::query!(
            r#"
            SELECT id,
                   media_id,
                   media_type AS "media_type!: VideoMediaType",
                   library_id,
                   file_path,
                   filename,
                   file_size,
                   discovered_at,
                   created_at,
                   technical_metadata
            FROM media_files
            WHERE is_available = TRUE
              AND ($1::uuid IS NULL OR library_id = $1)
              AND ($2::text IS NULL OR file_path LIKE $2)
              AND (cardinality($3::text[]) = 0 OR LOWER(split_part(filename, '.', -1)) = ANY($3))
              AND ($4::bigint IS NULL OR file_size >= $4)
              AND ($5::bigint IS NULL OR file_size <= $5)
              AND ($6::timestamptz IS NULL OR discovered_at >= $6)
              AND ($7::timestamptz IS NULL OR discovered_at <= $7)
              AND ($8::timestamptz IS NULL OR created_at >= $8)
              AND ($9::timestamptz IS NULL OR created_at <= $9)
            ORDER BY
              CASE WHEN $10::int2 = 0 AND $11::bool THEN discovered_at END ASC,
              CASE WHEN $10::int2 = 0 AND NOT $11::bool THEN discovered_at END DESC,
              CASE WHEN $10::int2 = 1 AND $11::bool THEN created_at END ASC,
              CASE WHEN $10::int2 = 1 AND NOT $11::bool THEN created_at END DESC,
              CASE WHEN $10::int2 = 2 AND $11::bool THEN file_size END ASC,
              CASE WHEN $10::int2 = 2 AND NOT $11::bool THEN file_size END DESC,
              CASE WHEN $10::int2 = 3 AND $11::bool THEN LOWER(filename) END ASC,
              CASE WHEN $10::int2 = 3 AND NOT $11::bool THEN LOWER(filename) END DESC,
              id ASC
            LIMIT $12 OFFSET $13
            "#,
            library_id,
            path_prefix,
            &extensions,
            min_size,
            max_size,
            filter.discovered_after,
            filter.discovered_before,
            filter.created_after,
            filter.created_before,
            sort_key,
            sort_ascending,
            page.limit as i64,
            page.offset as i64
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        rows.into_iter()
            .map(|row| {
                Self::media_file_from_parts(
                    row.id,
                    row.media_id,
                    row.media_type,
                    row.library_id,
                    row.file_path,
                    row.filename,
                    row.file_size,
                    row.discovered_at,
                    row.created_at,
                    row.technical_metadata,
                )
            })
            .collect()
    }

    pub async fn get_stats(&self) -> Result<MediaStats> {
        self.stats_with_filter(MediaFileFilter::default()).await
    }

    pub async fn stats_with_filter(
        &self,
        filter: MediaFileFilter,
    ) -> Result<MediaStats> {
        let library_id = filter.library_id.map(|id| id.to_uuid());
        let path_prefix = filter
            .path_prefix
            .as_ref()
            .map(|prefix| format!("{}%", prefix));
        let extensions = Self::normalized_extensions(&filter);
        let min_size = filter.min_size.map(|size| size as i64);
        let max_size = filter.max_size.map(|size| size as i64);

        let total_row = sqlx::query!(
            r#"
            SELECT COUNT(*)::bigint AS "count!",
                   COALESCE(SUM(file_size), 0)::bigint AS "total_size!"
            FROM media_files
            WHERE is_available = TRUE
              AND ($1::uuid IS NULL OR library_id = $1)
              AND ($2::text IS NULL OR file_path LIKE $2)
              AND (cardinality($3::text[]) = 0 OR LOWER(split_part(filename, '.', -1)) = ANY($3))
              AND ($4::bigint IS NULL OR file_size >= $4)
              AND ($5::bigint IS NULL OR file_size <= $5)
              AND ($6::timestamptz IS NULL OR discovered_at >= $6)
              AND ($7::timestamptz IS NULL OR discovered_at <= $7)
              AND ($8::timestamptz IS NULL OR created_at >= $8)
              AND ($9::timestamptz IS NULL OR created_at <= $9)
            "#,
            library_id,
            path_prefix.as_deref(),
            &extensions,
            min_size,
            max_size,
            filter.discovered_after,
            filter.discovered_before,
            filter.created_after,
            filter.created_before
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let type_rows = sqlx::query!(
            r#"
            SELECT COALESCE(parsed_info->>'media_type', 'unknown') AS "media_type!",
                   COUNT(*)::bigint AS "count!"
            FROM media_files
            WHERE is_available = TRUE
              AND ($1::uuid IS NULL OR library_id = $1)
              AND ($2::text IS NULL OR file_path LIKE $2)
              AND (cardinality($3::text[]) = 0 OR LOWER(split_part(filename, '.', -1)) = ANY($3))
              AND ($4::bigint IS NULL OR file_size >= $4)
              AND ($5::bigint IS NULL OR file_size <= $5)
              AND ($6::timestamptz IS NULL OR discovered_at >= $6)
              AND ($7::timestamptz IS NULL OR discovered_at <= $7)
              AND ($8::timestamptz IS NULL OR created_at >= $8)
              AND ($9::timestamptz IS NULL OR created_at <= $9)
            GROUP BY COALESCE(parsed_info->>'media_type', 'unknown')
            "#,
            library_id,
            path_prefix.as_deref(),
            &extensions,
            min_size,
            max_size,
            filter.discovered_after,
            filter.discovered_before,
            filter.created_after,
            filter.created_before
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let mut by_type = HashMap::new();
        for row in type_rows {
            by_type.insert(row.media_type, row.count as u64);
        }

        Ok(MediaStats {
            total_files: total_row.count as u64,
            total_size: total_row.total_size as u64,
            by_type,
        })
    }

    pub async fn file_exists(&self, path: &str) -> Result<bool> {
        let count = sqlx::query_scalar!(
            r#"SELECT COUNT(*)::bigint AS "count!" FROM media_files WHERE file_path = $1 AND is_available = TRUE"#,
            path
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        Ok(count > 0)
    }

    pub async fn delete_media_by_id(&self, id: Uuid) -> Result<()> {
        sqlx::query!("DELETE FROM media_files WHERE id = $1", id)
            .execute(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Delete failed: {}", e))
            })?;

        Ok(())
    }

    pub async fn delete_media_by_path(
        &self,
        library_id: LibraryId,
        path: &str,
    ) -> Result<()> {
        sqlx::query!(
            "DELETE FROM media_files WHERE library_id = $1 AND file_path = $2",
            library_id.as_uuid(),
            path
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Delete by path failed: {}", e))
        })?;

        Ok(())
    }

    pub async fn delete_media_by_path_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
    ) -> Result<u64> {
        if prefixes.is_empty() {
            return Ok(0);
        }

        let roots: Vec<String> = prefixes
            .iter()
            .map(|prefix| {
                prefix
                    .trim_end_matches(std::path::MAIN_SEPARATOR)
                    .to_owned()
            })
            .collect();

        let result = sqlx::query!(
            r#"
            WITH target_prefixes AS (
                SELECT root,
                       root || $3::text || '%' AS child_pattern
                FROM UNNEST($2::text[]) AS root
            )
            DELETE FROM media_files AS mf
            USING target_prefixes AS p
            WHERE mf.library_id = $1
              AND (mf.file_path = p.root OR mf.file_path LIKE p.child_pattern)
            "#,
            library_id.as_uuid(),
            &roots,
            std::path::MAIN_SEPARATOR.to_string()
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Delete by prefixes failed for library {}: {}",
                library_id, e
            ))
        })?;

        Ok(result.rows_affected())
    }

    pub async fn delete_media(&self, id: &str) -> Result<()> {
        let uuid = Uuid::parse_str(id).map_err(|e| {
            MediaError::InvalidMedia(format!("Invalid UUID: {}", e))
        })?;
        self.delete_media_by_id(uuid).await
    }

    pub async fn get_all_media(&self) -> Result<Vec<MediaFile>> {
        self.list_media(MediaFilters::default()).await
    }

    pub async fn store_external_metadata(
        &self,
        media_id: &str,
        metadata: &MediaFileMetadata,
    ) -> Result<()> {
        let uuid = Uuid::parse_str(media_id).map_err(|e| {
            MediaError::InvalidMedia(format!("Invalid UUID: {}", e))
        })?;
        self.update_technical_metadata_by_id(uuid, metadata).await
    }

    pub async fn update_technical_metadata_by_id(
        &self,
        id: Uuid,
        metadata: &MediaFileMetadata,
    ) -> Result<()> {
        let metadata_json = serde_json::to_value(metadata).map_err(|e| {
            MediaError::InvalidMedia(format!(
                "Failed to serialize metadata: {}",
                e
            ))
        })?;

        sqlx::query!(
            "UPDATE media_files SET technical_metadata = $1, updated_at = NOW() WHERE id = $2",
            metadata_json,
            id
        )
            .execute(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Update failed: {}", e)))?;

        Ok(())
    }

    async fn upsert_media_in_transaction(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        media_file: &MediaFile,
    ) -> Result<UpsertOutcome> {
        let library_check = sqlx::query!(
            "SELECT id FROM libraries WHERE id = $1",
            media_file.library_id.as_uuid()
        )
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to check library existence: {}",
                e
            ))
        })?;

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
                MediaError::InvalidMedia(format!(
                    "Failed to serialize metadata: {}",
                    e
                ))
            })?;

        let parsed_info = technical_metadata
            .as_ref()
            .and_then(|m| m.get("parsed_info"))
            .cloned();

        let file_path_str = media_file.path.to_string_lossy().to_string();

        let media_type: VideoMediaType = media_file.media_id.media_type();

        let record = sqlx::query!(
            r#"
            INSERT INTO media_files (
                id, media_id, media_type, library_id, file_path, filename, file_size, created_at,
                technical_metadata, parsed_info
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (file_path) DO UPDATE SET
                filename = EXCLUDED.filename,
                file_size = EXCLUDED.file_size,
                technical_metadata = EXCLUDED.technical_metadata,
                parsed_info = EXCLUDED.parsed_info,
                updated_at = NOW()
            RETURNING id, (xmax = 0) as inserted
            "#,
            media_file.id,
            media_file.media_id.as_uuid(),
            media_type as VideoMediaType,
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
            .map_err(|e| {
                MediaError::Internal(format!("Failed to store media file: {}", e))
            })?;

        let actual_id = record.id;
        let created = record.inserted.unwrap_or(false);

        if actual_id != media_file.id {
            tracing::info!(
                "Media file path {} already existed with ID {}, using existing ID instead of {}",
                file_path_str,
                actual_id,
                media_file.id
            );
        }

        sqlx::query!(
            r#"
            UPDATE media_files
            SET is_available = TRUE,
                tombstoned_at = NULL,
                tombstone_reason = NULL,
                updated_at = NOW()
            WHERE id = $1
            "#,
            actual_id
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to mark media file available after upsert: {}",
                e
            ))
        })?;

        Ok(UpsertOutcome {
            id: actual_id,
            created,
        })
    }

    pub async fn mark_available_with_fingerprint_impl(
        &self,
        library_id: LibraryId,
        path: &str,
        fingerprint: &MediaFingerprint,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE media_files
            SET is_available = TRUE,
                tombstoned_at = NULL,
                tombstone_reason = NULL,
                fingerprint_device_id = $3,
                fingerprint_inode = $4,
                fingerprint_size = $5,
                fingerprint_mtime_ms = $6,
                fingerprint_weak_hash = $7,
                updated_at = NOW()
            WHERE library_id = $1 AND file_path = $2
            "#,
            library_id.as_uuid(),
            path,
            fingerprint.device_id.as_deref(),
            fingerprint.inode.map(|value| value as i64),
            fingerprint.size as i64,
            fingerprint.mtime,
            fingerprint.weak_hash.as_deref()
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to update media file fingerprint for {}: {}",
                path, e
            ))
        })?;

        Ok(())
    }

    pub async fn move_media_by_path_impl(
        &self,
        library_id: LibraryId,
        old_path: &str,
        new_path: &str,
        fingerprint: &MediaFingerprint,
    ) -> Result<Uuid> {
        let filename = PathBuf::from(new_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                MediaError::InvalidMedia(format!(
                    "Cannot move media to path without filename: {new_path}"
                ))
            })?;

        let row = sqlx::query!(
            r#"
            UPDATE media_files
            SET file_path = $3,
                filename = $4,
                file_size = CASE WHEN $5::bigint > 0 THEN $5 ELSE file_size END,
                is_available = TRUE,
                tombstoned_at = NULL,
                tombstone_reason = NULL,
                fingerprint_device_id = $6,
                fingerprint_inode = $7,
                fingerprint_size = CASE WHEN $5::bigint > 0 THEN $5 ELSE fingerprint_size END,
                fingerprint_mtime_ms = CASE WHEN $8::bigint > 0 THEN $8 ELSE fingerprint_mtime_ms END,
                fingerprint_weak_hash = $9,
                updated_at = NOW()
            WHERE library_id = $1 AND file_path = $2
            RETURNING id
            "#,
            library_id.as_uuid(),
            old_path,
            new_path,
            filename.as_str(),
            fingerprint.size as i64,
            fingerprint.device_id.as_deref(),
            fingerprint.inode.map(|value| value as i64),
            fingerprint.mtime,
            fingerprint.weak_hash.as_deref()
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to move media from {} to {}: {}",
                old_path, new_path, e
            ))
        })?;

        let Some(row) = row else {
            return Err(MediaError::NotFound(format!(
                "Media path not found for move: {}",
                old_path
            )));
        };

        Ok(row.id)
    }

    #[allow(clippy::too_many_arguments)]
    fn stored_media_from_parts(
        id: Uuid,
        media_uuid: Uuid,
        media_type: String,
        file_path: String,
        file_size: i64,
        is_available: bool,
        fingerprint_device_id: Option<String>,
        fingerprint_inode: Option<i64>,
        fingerprint_size: Option<i64>,
        fingerprint_mtime_ms: Option<i64>,
        fingerprint_weak_hash: Option<String>,
    ) -> Result<StoredMediaFile> {
        let media_id = Self::media_id_from_parts(media_uuid, &media_type)?;
        let fingerprint = MediaFingerprint {
            device_id: fingerprint_device_id,
            inode: fingerprint_inode.map(|value| value as u64),
            size: fingerprint_size.unwrap_or(file_size) as u64,
            mtime: fingerprint_mtime_ms.unwrap_or_default(),
            weak_hash: fingerprint_weak_hash,
        };

        Ok(StoredMediaFile {
            id,
            media_id,
            path_norm: file_path,
            fingerprint,
            is_available,
        })
    }
}

#[async_trait]
impl FolderDeltaRepository for PostgresMediaRepository {
    async fn list_media_directly_under(
        &self,
        library_id: LibraryId,
        folder_path_norm: &str,
    ) -> Result<Vec<StoredMediaFile>> {
        let root = folder_path_norm.trim_end_matches('/');
        let child_prefix = format!("{root}/%");
        let rows = sqlx::query!(
            r#"
            SELECT id, media_id, media_type::text AS "media_type!", file_path, file_size,
                   is_available, fingerprint_device_id, fingerprint_inode,
                   fingerprint_size, fingerprint_mtime_ms, fingerprint_weak_hash
            FROM media_files
            WHERE library_id = $1
              AND file_path LIKE $2
            "#,
            library_id.as_uuid(),
            child_prefix
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to list media under {}: {}",
                folder_path_norm, e
            ))
        })?;

        rows.into_iter()
            .map(|row| {
                Self::stored_media_from_parts(
                    row.id,
                    row.media_id,
                    row.media_type,
                    row.file_path,
                    row.file_size,
                    row.is_available,
                    row.fingerprint_device_id,
                    row.fingerprint_inode,
                    row.fingerprint_size,
                    row.fingerprint_mtime_ms,
                    row.fingerprint_weak_hash,
                )
            })
            .filter_map(|result| match result {
                Ok(media)
                    if is_direct_child_file(
                        folder_path_norm,
                        &media.path_norm,
                    ) =>
                {
                    Some(Ok(media))
                }
                Ok(_) => None,
                Err(err) => Some(Err(err)),
            })
            .collect()
    }

    async fn find_available_media_by_fingerprint(
        &self,
        library_id: LibraryId,
        fingerprint: &MediaFingerprint,
        excluding_path_norm: &str,
    ) -> Result<Vec<StoredMediaFile>> {
        if fingerprint.size == 0 {
            return Ok(Vec::new());
        }

        let fingerprint_mtime =
            (fingerprint.mtime > 0).then_some(fingerprint.mtime);

        let rows = sqlx::query!(
            r#"
            SELECT id, media_id, media_type::text AS "media_type!", file_path, file_size,
                   is_available, fingerprint_device_id, fingerprint_inode,
                   fingerprint_size, fingerprint_mtime_ms, fingerprint_weak_hash
            FROM media_files
            WHERE library_id = $1
              AND is_available = TRUE
              AND file_path <> $2
              AND COALESCE(fingerprint_size, file_size) = $3
              AND ($4::bigint IS NULL OR fingerprint_mtime_ms = $4)
              AND ($5::text IS NULL OR fingerprint_weak_hash = $5)
            ORDER BY updated_at DESC
            LIMIT 2
            "#,
            library_id.as_uuid(),
            excluding_path_norm,
            fingerprint.size as i64,
            fingerprint_mtime,
            fingerprint.weak_hash.as_deref()
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to find media move candidates for {}: {}",
                excluding_path_norm, e
            ))
        })?;

        rows.into_iter()
            .map(|row| {
                Self::stored_media_from_parts(
                    row.id,
                    row.media_id,
                    row.media_type,
                    row.file_path,
                    row.file_size,
                    row.is_available,
                    row.fingerprint_device_id,
                    row.fingerprint_inode,
                    row.fingerprint_size,
                    row.fingerprint_mtime_ms,
                    row.fingerprint_weak_hash,
                )
            })
            .collect()
    }

    async fn move_media_by_path(
        &self,
        library_id: LibraryId,
        old_path_norm: &str,
        new_path_norm: &str,
        fingerprint: &MediaFingerprint,
    ) -> Result<Uuid> {
        self.move_media_by_path_impl(
            library_id,
            old_path_norm,
            new_path_norm,
            fingerprint,
        )
        .await
    }

    async fn mark_unavailable_by_paths(
        &self,
        library_id: LibraryId,
        paths: Vec<String>,
        reason: &str,
    ) -> Result<u64> {
        if paths.is_empty() {
            return Ok(0);
        }

        let result = sqlx::query!(
            r#"
            UPDATE media_files
            SET is_available = FALSE,
                tombstoned_at = COALESCE(tombstoned_at, NOW()),
                tombstone_reason = $3,
                updated_at = NOW()
            WHERE library_id = $1
              AND file_path = ANY($2)
              AND is_available = TRUE
            "#,
            library_id.as_uuid(),
            &paths,
            reason
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to tombstone media paths for library {}: {}",
                library_id, e
            ))
        })?;

        Ok(result.rows_affected())
    }

    async fn list_media_by_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
    ) -> Result<Vec<MediaID>> {
        if prefixes.is_empty() {
            return Ok(Vec::new());
        }

        let roots: Vec<String> = prefixes
            .iter()
            .map(|prefix| prefix.trim_end_matches('/').to_owned())
            .collect();

        let rows = sqlx::query!(
            r#"
            WITH target_prefixes AS (
                SELECT root,
                       root || '/%' AS child_pattern
                FROM UNNEST($2::text[]) AS root
            )
            SELECT DISTINCT mf.media_id AS "media_id!", mf.media_type::text AS "media_type!"
            FROM media_files AS mf
            JOIN target_prefixes AS p
              ON mf.file_path = p.root OR mf.file_path LIKE p.child_pattern
            WHERE mf.library_id = $1
            "#,
            library_id.as_uuid(),
            &roots
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to list media under prefixes for library {}: {}",
                library_id, e
            ))
        })?;

        rows.into_iter()
            .map(|row| Self::media_id_from_parts(row.media_id, &row.media_type))
            .collect()
    }

    async fn mark_unavailable_by_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
        reason: &str,
    ) -> Result<u64> {
        if prefixes.is_empty() {
            return Ok(0);
        }

        let roots: Vec<String> = prefixes
            .iter()
            .map(|prefix| prefix.trim_end_matches('/').to_owned())
            .collect();

        let result = sqlx::query!(
            r#"
            WITH target_prefixes AS (
                SELECT root,
                       root || '/%' AS child_pattern
                FROM UNNEST($3::text[]) AS root
            )
            UPDATE media_files AS mf
            SET is_available = FALSE,
                tombstoned_at = COALESCE(tombstoned_at, NOW()),
                tombstone_reason = $2,
                updated_at = NOW()
            FROM target_prefixes AS p
            WHERE mf.library_id = $1
              AND mf.is_available = TRUE
              AND (mf.file_path = p.root OR mf.file_path LIKE p.child_pattern)
            "#,
            library_id.as_uuid(),
            reason,
            &roots
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to tombstone media prefixes for library {}: {}",
                library_id, e
            ))
        })?;

        Ok(result.rows_affected())
    }

    async fn delete_folder_inventory_by_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
    ) -> Result<u64> {
        crate::database::repositories::folder_inventory::PostgresFolderInventoryRepository::new(
            self.pool().clone(),
        )
        .delete_by_path_prefixes_impl(library_id, prefixes)
        .await
    }
}
