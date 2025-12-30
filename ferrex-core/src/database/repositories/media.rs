use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use ferrex_contracts::id::MediaIDLike;
use ferrex_model::MediaID;
use ferrex_model::media_type::VideoMediaType;
use sqlx::{PgPool, Postgres, QueryBuilder, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

use crate::database::repository_ports::media_files::{
    MediaFileFilter, MediaFileSort, MediaFileSortField, MediaFilesReadPort,
    MediaFilesWritePort, Page, SortDirection, UpsertOutcome,
};
use crate::database::traits::{MediaFilters, MediaStats};
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

    fn map_sort_field(field: MediaFileSortField) -> &'static str {
        match field {
            MediaFileSortField::DiscoveredAt => "discovered_at",
            MediaFileSortField::CreatedAt => "created_at",
            MediaFileSortField::FileSize => "file_size",
            MediaFileSortField::Filename => "LOWER(filename)",
        }
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

    fn apply_filter(
        builder: &mut QueryBuilder<Postgres>,
        filter: &MediaFileFilter,
    ) {
        if let Some(library) = filter.library_id {
            builder.push(" AND library_id = ");
            builder.push_bind(library.to_uuid());
        }

        if let Some(prefix) = &filter.path_prefix {
            builder.push(" AND file_path LIKE ");
            builder.push_bind(format!("{}%", prefix));
        }

        if !filter.extension_in.is_empty() {
            let lowered: Vec<String> = filter
                .extension_in
                .iter()
                .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
                .collect();
            builder.push(" AND LOWER(split_part(filename, '.', -1)) = ANY(");
            builder.push_bind(lowered);
            builder.push(")");
        }

        if let Some(min_size) = filter.min_size {
            builder.push(" AND file_size >= ");
            builder.push_bind(min_size as i64);
        }

        if let Some(max_size) = filter.max_size {
            builder.push(" AND file_size <= ");
            builder.push_bind(max_size as i64);
        }

        if let Some(after) = filter.discovered_after {
            builder.push(" AND discovered_at >= ");
            builder.push_bind(after);
        }

        if let Some(before) = filter.discovered_before {
            builder.push(" AND discovered_at <= ");
            builder.push_bind(before);
        }

        if let Some(after) = filter.created_after {
            builder.push(" AND created_at >= ");
            builder.push_bind(after);
        }

        if let Some(before) = filter.created_before {
            builder.push(" AND created_at <= ");
            builder.push_bind(before);
        }
    }

    fn hydrate_media_file(row: &PgRow) -> Result<MediaFile> {
        let technical_metadata: Option<serde_json::Value> =
            row.try_get("technical_metadata")?;
        let media_file_metadata = technical_metadata
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to deserialize metadata: {}",
                    e
                ))
            })?;

        let id: Uuid = row.try_get("media_id")?;
        let imt: i16 = row.try_get("media_type")?;
        let media_id: MediaID =
            MediaID::from((id, VideoMediaType::from(imt as u16)));

        Ok(MediaFile {
            id: row.try_get("id")?,
            media_id,
            path: PathBuf::from(row.try_get::<String, _>("file_path")?),
            filename: row.try_get("filename")?,
            size: row.try_get::<i64, _>("file_size")? as u64,
            discovered_at: row.try_get("discovered_at")?,
            created_at: row.try_get("created_at")?,
            media_file_metadata,
            library_id: LibraryId(row.try_get("library_id")?),
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
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT id, media_id, media_type, library_id, file_path, filename, file_size, discovered_at, created_at, technical_metadata, parsed_info FROM media_files WHERE 1=1",
        );

        Self::apply_filter(&mut builder, &filter);

        builder.push(" ORDER BY ");
        builder.push(Self::map_sort_field(sort.field));
        builder.push(match sort.direction {
            SortDirection::Ascending => " ASC",
            SortDirection::Descending => " DESC",
        });

        builder.push(", id ASC");

        builder.push(" LIMIT ");
        builder.push_bind(page.limit as i64);
        builder.push(" OFFSET ");
        builder.push_bind(page.offset as i64);

        let rows =
            builder.build().fetch_all(self.pool()).await.map_err(|e| {
                MediaError::Internal(format!("Database query failed: {}", e))
            })?;

        rows.into_iter()
            .map(|row| Self::hydrate_media_file(&row))
            .collect()
    }

    pub async fn get_stats(&self) -> Result<MediaStats> {
        self.stats_with_filter(MediaFileFilter::default()).await
    }

    pub async fn stats_with_filter(
        &self,
        filter: MediaFileFilter,
    ) -> Result<MediaStats> {
        let mut totals_builder = QueryBuilder::<Postgres>::new(
            "SELECT COUNT(*) as count, COALESCE(SUM(file_size), 0) as total_size FROM media_files WHERE 1=1",
        );
        Self::apply_filter(&mut totals_builder, &filter);

        let total_row = totals_builder
            .build()
            .fetch_one(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Database query failed: {}", e))
            })?;

        let mut type_builder = QueryBuilder::<Postgres>::new(
            "SELECT COALESCE(parsed_info->>'media_type', 'unknown') as media_type, COUNT(*) as count FROM media_files WHERE 1=1",
        );
        Self::apply_filter(&mut type_builder, &filter);
        type_builder
            .push(" GROUP BY COALESCE(parsed_info->>'media_type', 'unknown')");

        let type_rows = type_builder
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Database query failed: {}", e))
            })?;

        let mut by_type = HashMap::new();
        for row in type_rows {
            let media_type: Option<String> = row.try_get("media_type").ok();
            let count: i64 = row.try_get("count").unwrap_or(0);
            by_type.insert(
                media_type.unwrap_or_else(|| "unknown".to_string()),
                count as u64,
            );
        }

        let total_files: i64 = total_row.try_get("count").unwrap_or(0);
        let total_size: i64 = total_row.try_get("total_size").unwrap_or(0);

        Ok(MediaStats {
            total_files: total_files as u64,
            total_size: total_size as u64,
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
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        Ok(count.unwrap_or(0) > 0)
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

        let mut builder = QueryBuilder::<Postgres>::new(
            "DELETE FROM media_files WHERE library_id = ",
        );
        builder.push_bind(library_id.as_uuid());
        builder.push(" AND (");

        for (idx, prefix) in prefixes.iter().enumerate() {
            if idx > 0 {
                builder.push(" OR ");
            }

            let root = prefix.trim_end_matches(std::path::MAIN_SEPARATOR);
            let mut children_prefix = root.to_string();
            children_prefix.push(std::path::MAIN_SEPARATOR);

            builder.push("(");
            builder.push("file_path = ");
            builder.push_bind(root);
            builder.push(" OR file_path LIKE ");
            builder.push_bind(format!("{}%", children_prefix));
            builder.push(")");
        }

        builder.push(")");

        let result =
            builder.build().execute(self.pool()).await.map_err(|e| {
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

        Ok(UpsertOutcome {
            id: actual_id,
            created,
        })
    }
}
