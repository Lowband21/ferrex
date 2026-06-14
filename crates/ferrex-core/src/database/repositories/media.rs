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
        builder.push(" AND is_available = TRUE");

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

    pub async fn get_playback_source(
        &self,
        uuid: &Uuid,
    ) -> Result<Option<PlaybackMediaSource>> {
        let row = sqlx::query_as::<_, (Uuid, String, String, i64, bool)>(
            r#"
            SELECT id, file_path, filename, file_size, is_available
            FROM media_files
            WHERE id = $1
            "#,
        )
        .bind(*uuid)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for playback source: {e}"
            ))
        })?;

        Ok(row.map(|(id, path, filename, size, is_available)| {
            PlaybackMediaSource {
                id,
                path: PathBuf::from(path),
                filename,
                size: size as u64,
                is_available,
            }
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

    fn stored_media_from_row(
        row: sqlx::postgres::PgRow,
    ) -> Result<StoredMediaFile> {
        Self::stored_media_from_parts(
            row.try_get("id")?,
            row.try_get("media_id")?,
            row.try_get("media_type")?,
            row.try_get("file_path")?,
            row.try_get("file_size")?,
            row.try_get("is_available")?,
            row.try_get("fingerprint_device_id")?,
            row.try_get("fingerprint_inode")?,
            row.try_get("fingerprint_size")?,
            row.try_get("fingerprint_mtime_ms")?,
            row.try_get("fingerprint_weak_hash")?,
        )
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

        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT id, media_id, media_type::text AS media_type, file_path, file_size,
                   is_available, fingerprint_device_id, fingerprint_inode,
                   fingerprint_size, fingerprint_mtime_ms, fingerprint_weak_hash
            FROM media_files
            WHERE library_id = 
            "#,
        );
        builder.push_bind(library_id.as_uuid());
        builder.push(" AND is_available = TRUE AND file_path <> ");
        builder.push_bind(excluding_path_norm);
        builder.push(" AND COALESCE(fingerprint_size, file_size) = ");
        builder.push_bind(fingerprint.size as i64);
        if fingerprint.mtime > 0 {
            builder.push(" AND fingerprint_mtime_ms = ");
            builder.push_bind(fingerprint.mtime);
        }
        if let Some(weak_hash) = fingerprint.weak_hash.as_deref() {
            builder.push(" AND fingerprint_weak_hash = ");
            builder.push_bind(weak_hash);
        }
        builder.push(" ORDER BY updated_at DESC LIMIT 2");

        let rows =
            builder.build().fetch_all(self.pool()).await.map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to find media move candidates for {}: {}",
                    excluding_path_norm, e
                ))
            })?;

        rows.into_iter().map(Self::stored_media_from_row).collect()
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

    async fn mark_unavailable_by_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
        reason: &str,
    ) -> Result<u64> {
        if prefixes.is_empty() {
            return Ok(0);
        }

        let mut builder = QueryBuilder::<Postgres>::new(
            "UPDATE media_files SET is_available = FALSE, tombstoned_at = COALESCE(tombstoned_at, NOW()), tombstone_reason = ",
        );
        builder.push_bind(reason);
        builder.push(", updated_at = NOW() WHERE library_id = ");
        builder.push_bind(library_id.as_uuid());
        builder.push(" AND is_available = TRUE AND (");

        for (idx, prefix) in prefixes.iter().enumerate() {
            if idx > 0 {
                builder.push(" OR ");
            }
            let root = prefix.trim_end_matches('/');
            let child_prefix = format!("{root}/%");
            builder.push("(file_path = ");
            builder.push_bind(root);
            builder.push(" OR file_path LIKE ");
            builder.push_bind(child_prefix);
            builder.push(")");
        }
        builder.push(")");

        let result =
            builder.build().execute(self.pool()).await.map_err(|e| {
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
