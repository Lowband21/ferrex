use super::traits::*;
use crate::{
    EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeDetails, EpisodeNumber, EpisodeReference,
    EpisodeURL, LibraryReference, MediaDetailsOption, MediaIDLike, MovieReference, MovieTitle,
    MovieURL, SeasonDetails, SeasonNumber, SeasonReference, SeasonURL, SeriesReference,
    SeriesTitle, SeriesURL, TmdbDetails, UrlLike,
};
use crate::{
    EpisodeID, Library, LibraryID, LibraryType, Media, MediaError, MediaFile, MediaFileMetadata,
    MediaID, MovieID, Result, SeasonID, SeriesID,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rayon::iter::{IntoParallelIterator, ParallelExtend, ParallelIterator};
use serde_json::{self, json};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Statistics about the connection pool
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub size: u32,
    pub idle: u32,
    pub max_size: u32,
    pub min_idle: u32,
}

#[derive(Debug, Clone)]
pub struct PostgresDatabase {
    pool: PgPool,
    max_connections: u32,
    min_connections: u32,
}

impl PostgresDatabase {
    pub async fn new(connection_string: &str) -> Result<Self> {
        // Get pool configuration from environment or use optimized defaults
        let max_connections = std::env::var("DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(20);

        let min_connections = std::env::var("DB_MIN_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(5);

        // Configure pool for optimal bulk query performance
        let pool = PgPoolOptions::new()
            .max_connections(max_connections) // Configurable for different workloads
            .min_connections(min_connections) // Maintain idle connections
            .acquire_timeout(std::time::Duration::from_secs(30)) // Longer timeout for bulk ops
            .max_lifetime(std::time::Duration::from_secs(1800)) // 30 min lifetime
            .idle_timeout(std::time::Duration::from_secs(600)) // 10 min idle timeout
            .test_before_acquire(true) // Ensure connections are healthy
            .connect(connection_string)
            .await
            .map_err(|e| MediaError::Internal(format!("Database connection failed: {}", e)))?;

        info!(
            "Database pool initialized with max_connections={}, min_connections={}",
            max_connections, min_connections
        );

        Ok(PostgresDatabase {
            pool,
            max_connections,
            min_connections,
        })
    }

    /// Create a PostgresDatabase from an existing pool (mainly for testing)
    pub fn from_pool(pool: PgPool) -> Self {
        // Use default values for test pools
        let max_connections = 20;
        let min_connections = 5;

        PostgresDatabase {
            pool,
            max_connections,
            min_connections,
        }
    }

    /// Get a reference to the connection pool for use in extension modules
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Get connection pool statistics for monitoring
    pub fn pool_stats(&self) -> PoolStats {
        PoolStats {
            size: self.pool.size() as u32,
            idle: self.pool.num_idle() as u32,
            max_size: self.max_connections,
            min_idle: self.min_connections,
        }
    }

    // Repository methods for better organization

    /// Store a MediaFile within an existing transaction and return the actual ID
    async fn store_media_file_in_transaction(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        media_file: &MediaFile,
    ) -> Result<Uuid> {
        // First verify the library exists within the transaction
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

        // Serialize metadata to JSONB
        let technical_metadata = media_file
            .media_file_metadata
            .as_ref()
            .map(|m| serde_json::to_value(m))
            .transpose()
            .map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to serialize metadata: {}", e))
            })?;

        let parsed_info = technical_metadata
            .as_ref()
            .and_then(|m| m.get("parsed_info"))
            .cloned();

        let file_path_str = media_file.path.to_string_lossy().to_string();

        // Use RETURNING to get the actual ID after insert/update
        let actual_id = sqlx::query_scalar!(
            r#"
            INSERT INTO media_files (
                id, library_id, file_path, filename, file_size,
                technical_metadata, parsed_info
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
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
            technical_metadata,
            parsed_info
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to store media file: {}", e)))?;

        // If the actual ID differs from what we had, update the reference
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

    /// Store a MediaFile with all metadata
    async fn store_media_file_complete(&self, media_file: &MediaFile) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Transaction failed: {}", e)))?;

        let _ = self
            .store_media_file_in_transaction(&mut tx, media_file)
            .await?;

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit transaction: {}", e)))?;

        Ok(())
    }

    /// Store MovieReference within an existing transaction
    async fn store_movie_reference_in_transaction(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        movie: &MovieReference,
        actual_file_id: Uuid,
        metadata: Option<&EnhancedMovieDetails>,
    ) -> Result<()> {
        let movie_uuid = movie.id.as_uuid();
        let library_id = movie.file.library_id;

        // For movies without TMDB data (tmdb_id = 0), we need different handling
        // since multiple movies can have tmdb_id = 0
        if movie.tmdb_id == 0 {
            // For tmdb_id = 0, check if this specific file already has a movie reference
            let existing = sqlx::query!(
                "SELECT id FROM movie_references WHERE file_id = $1",
                actual_file_id
            )
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to check existing movie reference: {}", e))
            })?;

            if let Some(existing_row) = existing {
                // Update the existing reference
                sqlx::query!(
                    r#"
                    UPDATE movie_references
                    SET title = $1, theme_color = $2, updated_at = NOW()
                    WHERE id = $3
                    "#,
                    movie.title.as_str(),
                    movie.theme_color.as_deref(),
                    existing_row.id
                )
                .execute(&mut **tx)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to update movie reference: {}", e))
                })?;
            } else {
                // Insert new reference
                sqlx::query!(
                    r#"
                    INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title, theme_color)
                    VALUES ($1, $2, $3, $4, $5, $6)
                    "#,
                    movie_uuid,
                    library_id.as_uuid(),
                    actual_file_id,
                    0i64, // tmdb_id = 0 for movies without TMDB data
                    movie.title.as_str(),
                    movie.theme_color.as_deref()
                )
                .execute(&mut **tx)
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to insert movie reference: {}", e)))?;
            }
        } else {
            // For movies with TMDB data, use the normal UPSERT with conflict handling
            sqlx::query!(
                r#"
                INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title, theme_color)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (tmdb_id, library_id) DO UPDATE SET
                    file_id = EXCLUDED.file_id,
                    title = EXCLUDED.title,
                    theme_color = EXCLUDED.theme_color,
                    updated_at = NOW()
                "#,
                movie_uuid,
                library_id.as_uuid(),
                actual_file_id,
                movie.tmdb_id as i64,
                movie.title.as_str(),
                movie.theme_color.as_deref()
            )
            .execute(&mut **tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to store movie reference: {}", e)))?;
        }

        // Store enhanced metadata if provided
        if let Some(meta) = metadata {
            let tmdb_details = serde_json::to_value(meta).map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to serialize movie metadata: {}", e))
            })?;
            let images = serde_json::to_value(&meta.images).map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to serialize images: {}", e))
            })?;

            sqlx::query!(
                r#"
                INSERT INTO movie_metadata (movie_id, tmdb_details, images)
                VALUES ($1, $2, $3)
                ON CONFLICT (movie_id) DO UPDATE SET
                    tmdb_details = EXCLUDED.tmdb_details,
                    images = EXCLUDED.images,
                    updated_at = NOW()
                "#,
                movie_uuid,
                tmdb_details,
                images
            )
            .execute(&mut **tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to store movie metadata: {}", e)))?;
        }

        Ok(())
    }

    /// Store MovieReference with enhanced metadata
    async fn store_movie_reference_complete(
        &self,
        movie: &MovieReference,
        metadata: Option<&EnhancedMovieDetails>,
    ) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Transaction failed: {}", e)))?;

        // Store media file within the transaction and get actual ID
        let actual_file_id = self
            .store_media_file_in_transaction(&mut tx, &movie.file)
            .await?;

        // Store the movie reference with the actual file ID
        let movie_uuid = movie.id.as_uuid();
        let library_id = movie.file.library_id;

        // For movies without TMDB data (tmdb_id = 0), we need different handling
        // since multiple movies can have tmdb_id = 0
        if movie.tmdb_id == 0 {
            // For tmdb_id = 0, check if this specific file already has a movie reference
            let existing = sqlx::query!(
                "SELECT id FROM movie_references WHERE file_id = $1",
                actual_file_id
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to check existing movie reference: {}", e))
            })?;

            if let Some(existing_row) = existing {
                // Update the existing reference
                sqlx::query!(
                    r#"
                    UPDATE movie_references
                    SET title = $1, theme_color = $2, updated_at = NOW()
                    WHERE id = $3
                    "#,
                    movie.title.as_str(),
                    movie.theme_color.as_deref(),
                    existing_row.id
                )
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to update movie reference: {}", e))
                })?;
            } else {
                // Insert new reference
                sqlx::query!(
                    r#"
                    INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title, theme_color)
                    VALUES ($1, $2, $3, $4, $5, $6)
                    "#,
                    movie_uuid,
                    library_id.as_uuid(),
                    actual_file_id,
                    0i64, // tmdb_id = 0 for movies without TMDB data
                    movie.title.as_str(),
                    movie.theme_color.as_deref()
                )
                .execute(&mut *tx)
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to insert movie reference: {}", e)))?;
            }
        } else {
            // For movies with TMDB data, use the normal UPSERT with conflict handling
            sqlx::query!(
                r#"
                INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title, theme_color)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (tmdb_id, library_id) DO UPDATE SET
                    file_id = EXCLUDED.file_id,
                    title = EXCLUDED.title,
                    theme_color = EXCLUDED.theme_color,
                    updated_at = NOW()
                "#,
                movie_uuid,
                library_id.as_uuid(),
                actual_file_id,
                movie.tmdb_id as i64,
                movie.title.as_str(),
                movie.theme_color.as_deref()
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to store movie reference: {}", e)))?;
        }

        // Store enhanced metadata if provided
        if let Some(meta) = metadata {
            let tmdb_details = serde_json::to_value(meta).map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to serialize movie metadata: {}", e))
            })?;
            let images = serde_json::to_value(&meta.images).map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to serialize images: {}", e))
            })?;
            let cast_crew = serde_json::json!({
                "cast": meta.cast,
                "crew": meta.crew
            });
            let videos = serde_json::to_value(&meta.videos).map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to serialize videos: {}", e))
            })?;
            let external_ids = serde_json::to_value(&meta.external_ids).map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to serialize external IDs: {}", e))
            })?;

            sqlx::query!(
                r#"
                INSERT INTO movie_metadata (
                    movie_id, tmdb_details, images, cast_crew, videos, keywords, external_ids
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (movie_id) DO UPDATE SET
                    tmdb_details = EXCLUDED.tmdb_details,
                    images = EXCLUDED.images,
                    cast_crew = EXCLUDED.cast_crew,
                    videos = EXCLUDED.videos,
                    keywords = EXCLUDED.keywords,
                    external_ids = EXCLUDED.external_ids,
                    updated_at = NOW()
                "#,
                movie_uuid,
                tmdb_details,
                images,
                cast_crew,
                videos,
                &meta.keywords,
                external_ids
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to store movie metadata: {}", e)))?;
        }

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit transaction: {}", e)))?;

        Ok(())
    }

    /// Batch store movie references for performance
    pub async fn store_movie_references_batch(
        &self,
        movies: Vec<(MovieReference, Option<EnhancedMovieDetails>)>,
    ) -> Result<()> {
        if movies.is_empty() {
            return Ok(());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Transaction failed: {}", e)))?;

        // Process in chunks to avoid overwhelming the transaction
        const CHUNK_SIZE: usize = 50;
        let total_movies = movies.len();

        for (idx, chunk) in movies.chunks(CHUNK_SIZE).enumerate() {
            for (movie_ref, metadata) in chunk {
                // Store media file within transaction first
                let actual_file_id = self
                    .store_media_file_in_transaction(&mut tx, &movie_ref.file)
                    .await?;

                // Now store movie reference with the actual file ID
                self.store_movie_reference_in_transaction(
                    &mut tx,
                    movie_ref,
                    actual_file_id,
                    metadata.as_ref(),
                )
                .await?;
            }

            if idx > 0 && idx % 5 == 0 {
                info!("Processed {} / {} movies", idx * CHUNK_SIZE, total_movies);
            }
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit batch transaction: {}", e))
        })?;

        info!("Batch stored {} movie references", total_movies);
        Ok(())
    }

    /// Get movie with optional full metadata
    pub async fn get_movie_with_metadata(
        &self,
        id: &MovieID,
        include_metadata: bool,
    ) -> Result<Option<(MovieReference, Option<EnhancedMovieDetails>)>> {
        let movie_uuid = id.as_uuid();

        let query = if include_metadata {
            r#"
            SELECT
                mr.id, mr.tmdb_id, mr.title, mr.theme_color, mr.library_id,
                mf.id as file_id, mf.file_path, mf.filename, mf.file_size, mf.created_at as file_created_at,
                mf.technical_metadata, mf.parsed_info,
                mm.tmdb_details, mm.images, mm.cast_crew, mm.videos, mm.keywords, mm.external_ids
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            LEFT JOIN movie_metadata mm ON mr.id = mm.movie_id
            WHERE mr.id = $1
            "#
        } else {
            r#"
            SELECT
                mr.id, mr.tmdb_id, mr.title, mr.theme_color, mr.library_id,
                mf.id as file_id, mf.file_path, mf.filename, mf.file_size, mf.created_at as file_created_at,
                mf.technical_metadata, mf.parsed_info,
                NULL::jsonb as tmdb_details, NULL::jsonb as images, NULL::jsonb as cast_crew,
                NULL::jsonb as videos, NULL::text[] as keywords, NULL::jsonb as external_ids
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            WHERE mr.id = $1
            "#
        };

        let row = sqlx::query(query)
            .bind(movie_uuid)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let library_id = LibraryID(row.try_get("library_id")?);

        // Build MediaFile
        let technical_metadata: Option<serde_json::Value> = row.try_get("technical_metadata").ok();
        let media_file_metadata = technical_metadata
            .map(|tm| serde_json::from_value(tm))
            .transpose()
            .map_err(|e| MediaError::Internal(format!("Failed to deserialize metadata: {}", e)))?;

        let media_file = MediaFile {
            id: row.try_get("file_id")?,
            path: PathBuf::from(row.try_get::<String, _>("file_path")?),
            filename: row.try_get("filename")?,
            size: row.try_get::<i64, _>("file_size")? as u64,
            created_at: row.try_get("file_created_at")?,
            media_file_metadata,
            library_id,
        };

        // Build metadata if requested and available
        let metadata = if include_metadata {
            let tmdb_json = row.try_get::<Option<serde_json::Value>, _>("tmdb_details")?;

            if tmdb_json.is_none() {
                tracing::debug!(
                    "No TMDB metadata found for movie {}",
                    row.try_get::<Uuid, _>("id")?
                );
            } else {
                tracing::debug!(
                    "Found TMDB metadata for movie {}",
                    row.try_get::<Uuid, _>("id")?
                );
            }

            tmdb_json
                .map(|details| serde_json::from_value::<EnhancedMovieDetails>(details))
                .transpose()
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to deserialize movie metadata: {}", e))
                })?
        } else {
            None
        };

        // Build MovieReference with proper details
        let details = if let Some(ref metadata_details) = metadata {
            // If we have metadata, use it in the details field
            MediaDetailsOption::Details(TmdbDetails::Movie(metadata_details.clone()))
        } else {
            // Otherwise, provide an endpoint to fetch it later
            MediaDetailsOption::Endpoint(format!("/movie/{}", row.try_get::<Uuid, _>("id")?))
        };

        let movie_ref = MovieReference {
            id: MovieID(row.try_get::<Uuid, _>("id")?),
            library_id,
            tmdb_id: row.try_get::<i64, _>("tmdb_id")? as u64,
            title: MovieTitle::new(row.try_get("title")?)?,
            details,
            endpoint: MovieURL::from_string(format!("/stream/{}", media_file.id)),
            file: media_file,
            theme_color: row
                .try_get::<Option<String>, _>("theme_color")
                .unwrap_or(None),
        };

        Ok(Some((movie_ref, metadata)))
    }
}

#[async_trait]
impl MediaDatabaseTrait for PostgresDatabase {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn initialize_schema(&self) -> Result<()> {
        // Run migrations using sqlx migrate
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Migration failed: {}", e)))?;

        Ok(())
    }

    async fn store_media(&self, media_file: MediaFile) -> Result<Uuid> {
        // Use a transaction to get the actual ID
        let mut tx = self
            .pool
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

    async fn store_media_batch(&self, media_files: Vec<MediaFile>) -> Result<Vec<Uuid>> {
        if media_files.is_empty() {
            return Ok(Vec::new());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Transaction failed: {}", e)))?;

        let mut ids = Vec::new();

        // Process in chunks to avoid overwhelming the connection
        const CHUNK_SIZE: usize = 100;
        for chunk in media_files.chunks(CHUNK_SIZE) {
            for media_file in chunk {
                // Use the transaction-aware method instead of creating new transactions
                let actual_id = self
                    .store_media_file_in_transaction(&mut tx, media_file)
                    .await?;
                ids.push(actual_id);
            }
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit batch transaction: {}", e))
        })?;

        info!("Batch stored {} media files", ids.len());
        Ok(ids)
    }

    async fn get_media(&self, uuid: &Uuid) -> Result<Option<MediaFile>> {
        let row = sqlx::query!(
            r#"
            SELECT id, library_id, file_path, filename, file_size, created_at, technical_metadata, parsed_info
            FROM media_files
            WHERE id = $1
            "#,
            uuid
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let media_file_metadata = row
            .technical_metadata
            .map(|tm| serde_json::from_value(tm))
            .transpose()
            .map_err(|e| MediaError::Internal(format!("Failed to deserialize metadata: {}", e)))?;

        Ok(Some(MediaFile {
            id: row.id,
            path: PathBuf::from(row.file_path),
            filename: row.filename,
            size: row.file_size as u64,
            created_at: row.created_at,
            media_file_metadata,
            library_id: LibraryID(row.library_id),
        }))
    }

    async fn get_media_by_path(&self, path: &str) -> Result<Option<MediaFile>> {
        let row = sqlx::query!(
            r#"
            SELECT id, library_id, file_path, filename, file_size, created_at, technical_metadata, parsed_info
            FROM media_files
            WHERE file_path = $1
            "#,
            path
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let media_file_metadata = row
            .technical_metadata
            .map(|tm| serde_json::from_value(tm))
            .transpose()
            .map_err(|e| MediaError::Internal(format!("Failed to deserialize metadata: {}", e)))?;

        Ok(Some(MediaFile {
            id: row.id,
            path: PathBuf::from(row.file_path),
            filename: row.filename,
            size: row.file_size as u64,
            created_at: row.created_at,
            media_file_metadata,
            library_id: LibraryID(row.library_id),
        }))
    }

    async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>> {
        let mut query = "SELECT id, library_id, file_path, filename, file_size, created_at, technical_metadata, parsed_info FROM media_files".to_string();
        let mut conditions = Vec::new();
        let mut bind_count = 0;

        if let Some(library_id) = filters.library_id {
            bind_count += 1;
            conditions.push(format!("library_id = ${}", bind_count));
        }

        if !conditions.is_empty() {
            query.push_str(&format!(" WHERE {}", conditions.join(" AND ")));
        }

        query.push_str(" ORDER BY created_at DESC");

        if let Some(limit) = filters.limit {
            bind_count += 1;
            query.push_str(&format!(" LIMIT ${}", bind_count));
        }

        let mut sql_query = sqlx::query(&query);

        if let Some(library_id) = filters.library_id {
            sql_query = sql_query.bind(library_id.as_uuid());
        }

        if let Some(limit) = filters.limit {
            sql_query = sql_query.bind(limit as i64);
        }

        let rows = sql_query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut media_files = Vec::new();
        for row in rows {
            let technical_metadata: Option<serde_json::Value> =
                row.try_get("technical_metadata").ok();
            let media_file_metadata = technical_metadata
                .map(|tm| serde_json::from_value(tm))
                .transpose()
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to deserialize metadata: {}", e))
                })?;

            media_files.push(MediaFile {
                id: row.try_get("id")?,
                path: PathBuf::from(row.try_get::<String, _>("file_path")?),
                filename: row.try_get("filename")?,
                size: row.try_get::<i64, _>("file_size")? as u64,
                created_at: row.try_get("created_at")?,
                media_file_metadata,
                library_id: LibraryID(row.try_get("library_id")?),
            });
        }

        Ok(media_files)
    }

    async fn get_stats(&self) -> Result<MediaStats> {
        let total_row = sqlx::query!(
            "SELECT COUNT(*) as count, COALESCE(SUM(file_size), 0) as total_size FROM media_files"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        // For by_type, we'll extract from parsed_info JSON
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
        .fetch_all(&self.pool)
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

    async fn file_exists(&self, path: &str) -> Result<bool> {
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM media_files WHERE file_path = $1",
            path
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        Ok(count.unwrap_or(0) > 0)
    }

    async fn delete_media(&self, id: &str) -> Result<()> {
        let uuid = Uuid::parse_str(id)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;

        sqlx::query!("DELETE FROM media_files WHERE id = $1", uuid)
            .execute(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Delete failed: {}", e)))?;

        Ok(())
    }

    async fn get_all_media(&self) -> Result<Vec<MediaFile>> {
        self.list_media(MediaFilters::default()).await
    }

    async fn store_external_metadata(
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
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Update failed: {}", e)))?;

        Ok(())
    }

    // Legacy TV show methods - keeping for compatibility but using new reference system internally
    async fn store_tv_show(&self, _show_info: &TvShowInfo) -> Result<String> {
        // TODO: Convert to new reference system
        Ok(Uuid::new_v4().to_string())
    }

    async fn get_tv_show(&self, _tmdb_id: &str) -> Result<Option<TvShowInfo>> {
        // TODO: Convert from new reference system
        Ok(None)
    }

    async fn link_episode_to_file(
        &self,
        _media_file_id: &str,
        _show_tmdb_id: &str,
        _season: i32,
        _episode: i32,
    ) -> Result<()> {
        // TODO: Use new reference system
        Ok(())
    }

    // Library management
    async fn create_library(&self, library: Library) -> Result<String> {
        let paths: Vec<String> = library
            .paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        let library_type = match library.library_type {
            crate::LibraryType::Movies => "movies",
            crate::LibraryType::Series => "tvshows",
        };

        sqlx::query!(
            r#"
            INSERT INTO libraries (id, name, library_type, paths, scan_interval_minutes, enabled)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            library.id.as_uuid(),
            library.name,
            library_type,
            &paths,
            library.scan_interval_minutes as i32,
            library.enabled
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to create library: {}", e)))?;

        Ok(library.id.to_string())
    }

    async fn get_library(&self, library_id: &LibraryID) -> Result<Option<Library>> {
        let row = sqlx::query!(
            "SELECT id, name, library_type, paths, scan_interval_minutes, last_scan, enabled, auto_scan, watch_for_changes, analyze_on_scan, max_retry_attempts, created_at, updated_at FROM libraries WHERE id = $1",
            library_id.as_uuid()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let library_type = match row.library_type.as_str() {
            "movies" => crate::LibraryType::Movies,
            "tvshows" => crate::LibraryType::Series,
            _ => return Err(MediaError::InvalidMedia("Unknown library type".to_string())),
        };

        Ok(Some(Library {
            id: LibraryID(row.id),
            name: row.name,
            library_type,
            paths: row.paths.into_iter().map(PathBuf::from).collect(),
            scan_interval_minutes: row.scan_interval_minutes as u32,
            last_scan: row.last_scan,
            enabled: row.enabled,
            auto_scan: row.auto_scan,
            watch_for_changes: row.watch_for_changes,
            analyze_on_scan: row.analyze_on_scan,
            max_retry_attempts: row.max_retry_attempts as u32,
            created_at: row.created_at,
            updated_at: row.updated_at,
            media: None,
        }))
    }

    async fn list_libraries(&self) -> Result<Vec<Library>> {
        let rows = sqlx::query!(
            "SELECT id, name, library_type, paths, scan_interval_minutes, last_scan, enabled, auto_scan, watch_for_changes, analyze_on_scan, max_retry_attempts, created_at, updated_at FROM libraries ORDER BY name"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut libraries = Vec::new();
        for row in rows {
            let library_type = match row.library_type.as_str() {
                "movies" => crate::LibraryType::Movies,
                "tvshows" => crate::LibraryType::Series,
                _ => continue,
            };

            libraries.push(Library {
                id: LibraryID(row.id),
                name: row.name,
                library_type,
                paths: row.paths.into_iter().map(PathBuf::from).collect(),
                scan_interval_minutes: row.scan_interval_minutes as u32,
                last_scan: row.last_scan,
                enabled: row.enabled,
                auto_scan: row.auto_scan,
                watch_for_changes: row.watch_for_changes,
                analyze_on_scan: row.analyze_on_scan,
                max_retry_attempts: row.max_retry_attempts as u32,
                created_at: row.created_at,
                updated_at: row.updated_at,
                media: None, // Deprecated field
            });
        }

        Ok(libraries)
    }

    async fn update_library(&self, id: &str, library: Library) -> Result<()> {
        let uuid = Uuid::parse_str(id)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;

        let paths: Vec<String> = library
            .paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        let library_type = match library.library_type {
            crate::LibraryType::Movies => "movies",
            crate::LibraryType::Series => "tvshows",
        };

        sqlx::query!(
            r#"
            UPDATE libraries
            SET name = $1, library_type = $2, paths = $3, scan_interval_minutes = $4, enabled = $5, updated_at = NOW()
            WHERE id = $6
            "#,
            library.name,
            library_type,
            &paths,
            library.scan_interval_minutes as i32,
            library.enabled,
            uuid
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update library: {}", e)))?;

        Ok(())
    }

    async fn delete_library(&self, id: &str) -> Result<()> {
        let uuid = Uuid::parse_str(id)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;

        sqlx::query!("DELETE FROM libraries WHERE id = $1", uuid)
            .execute(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Delete failed: {}", e)))?;

        Ok(())
    }

    async fn update_library_last_scan(&self, id: &str) -> Result<()> {
        let uuid = Uuid::parse_str(id)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;

        sqlx::query!(
            "UPDATE libraries SET last_scan = NOW(), updated_at = NOW() WHERE id = $1",
            uuid
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Update failed: {}", e)))?;

        Ok(())
    }

    // Reference type methods
    async fn store_movie_reference(&self, movie: &MovieReference) -> Result<()> {
        // Extract metadata from the details field if available
        let metadata = match &movie.details {
            MediaDetailsOption::Details(TmdbDetails::Movie(details)) => {
                tracing::info!("Storing movie {} with TMDB metadata", movie.title.as_str());
                Some(details)
            }
            MediaDetailsOption::Details(_) => {
                tracing::warn!(
                    "Movie {} has non-movie TMDB details type",
                    movie.title.as_str()
                );
                None
            }
            MediaDetailsOption::Endpoint(_) => {
                tracing::info!(
                    "Storing movie {} without metadata (endpoint only)",
                    movie.title.as_str()
                );
                None
            }
        };

        self.store_movie_reference_complete(movie, metadata).await
    }

    async fn store_series_reference(&self, series: &SeriesReference) -> Result<()> {
        let mut buff = Uuid::encode_buffer();
        info!(
            "store_series_reference called for series: {} (ID: {}, TMDB: {}, Library: {})",
            series.title.as_str(),
            series.id.as_str(&mut buff),
            series.tmdb_id,
            series.library_id
        );

        // Extract metadata from the details field if available
        let metadata = match &series.details {
            MediaDetailsOption::Details(TmdbDetails::Series(details)) => {
                info!(
                    "Storing series {} with TMDB metadata",
                    series.title.as_str()
                );
                Some(details)
            }
            MediaDetailsOption::Details(_) => {
                error!(
                    "Series {} has non-series TMDB details type",
                    series.title.as_str()
                );
                None
            }
            MediaDetailsOption::Endpoint(_) => {
                info!(
                    "Storing series {} without metadata (endpoint only)",
                    series.title.as_str()
                );
                None
            }
        };

        // Store the series reference and metadata together
        self.store_series_reference_complete(series, metadata).await
    }

    async fn store_season_reference(&self, season: &SeasonReference) -> Result<Uuid> {
        // Extract metadata if available
        let metadata = match &season.details {
            MediaDetailsOption::Details(TmdbDetails::Season(details)) => {
                Some(serde_json::to_value(details).map_err(|e| {
                    MediaError::Internal(format!("Failed to serialize season metadata: {}", e))
                })?)
            }
            _ => None,
        };

        // Store the season reference
        let season_uuid = season.id.as_uuid();
        let series_uuid = season.series_id.as_uuid();

        // Use RETURNING to get the actual ID (either new or existing)
        let actual_season_id = sqlx::query_scalar!(
            r#"
            INSERT INTO season_references (id, season_number, series_id, library_id, tmdb_series_id, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (series_id, season_number) DO UPDATE
            SET tmdb_series_id = EXCLUDED.tmdb_series_id,
                library_id = EXCLUDED.library_id,
                updated_at = NOW()
            RETURNING id
            "#,
            season_uuid,
            season.season_number.value() as i32,
            series_uuid,
            season.library_id.as_uuid(),
            season.tmdb_series_id as i64,
            season.created_at
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to store season reference: {}", e)))?;

        // Store metadata if available
        if let Some(meta) = metadata {
            // Create empty images object if not present
            let images = json!({"posters": []});

            sqlx::query!(
                r#"
                INSERT INTO season_metadata (season_id, tmdb_details, images)
                VALUES ($1, $2, $3)
                ON CONFLICT (season_id) DO UPDATE
                SET tmdb_details = EXCLUDED.tmdb_details,
                    images = EXCLUDED.images,
                    updated_at = NOW()
                "#,
                actual_season_id,
                meta,
                images
            )
            .execute(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to store season metadata: {}", e)))?;
        }

        Ok(actual_season_id)
    }

    async fn store_episode_reference(&self, episode: &EpisodeReference) -> Result<()> {
        let mut buff = Uuid::encode_buffer();
        info!(
            "Storing episode reference: {} S{}E{}",
            episode.id.as_str(&mut buff),
            episode.season_number.value(),
            episode.episode_number.value()
        );

        // Parse IDs
        let episode_uuid = episode.id.as_uuid();
        let series_uuid = episode.series_id.as_uuid();
        let season_uuid = episode.season_id.as_uuid();
        let file_uuid = episode.id.as_uuid();

        // Start transaction
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to start transaction: {}", e)))?;

        // First, check if episode already exists for this series/season/episode number
        let existing_episode_id: Option<Uuid> = sqlx::query_scalar!(
            r#"
            SELECT id
            FROM episode_references
            WHERE series_id = $1
              AND season_number = $2
              AND episode_number = $3
            "#,
            series_uuid,
            episode.season_number.value() as i16,
            episode.episode_number.value() as i16
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to check existing episode: {}", e)))?;

        let actual_episode_id = if let Some(existing_id) = existing_episode_id {
            // Episode exists, update it
            info!(
                "Episode already exists with ID {}, updating instead of creating new",
                existing_id
            );

            sqlx::query!(
                r#"
                UPDATE episode_references
                SET season_id = $1,
                    file_id = $2,
                    tmdb_series_id = $3,
                    updated_at = NOW()
                WHERE id = $4
                "#,
                season_uuid,
                file_uuid,
                episode.tmdb_series_id as i64,
                existing_id
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to update episode reference: {}", e))
            })?;

            existing_id
        } else {
            // Episode doesn't exist, insert it
            sqlx::query!(
                r#"
                INSERT INTO episode_references (
                    id, series_id, season_id, file_id,
                    season_number, episode_number, tmdb_series_id
                ) VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
                episode_uuid,
                series_uuid,
                season_uuid,
                file_uuid,
                episode.season_number.value() as i16,
                episode.episode_number.value() as i16,
                episode.tmdb_series_id as i64
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to insert episode reference: {}", e))
            })?;

            *episode_uuid
        };

        // Log if we're using a different ID than expected (conflict occurred)
        if &actual_episode_id != episode_uuid {
            info!(
                "Episode already exists with ID {}, updating instead of creating new",
                actual_episode_id
            );
        }

        // Store metadata if available
        if let MediaDetailsOption::Details(TmdbDetails::Episode(details)) = &episode.details {
            let tmdb_details_json = serde_json::to_value(details).map_err(|e| {
                MediaError::Internal(format!("Failed to serialize episode details: {}", e))
            })?;

            sqlx::query!(
                r#"
                INSERT INTO episode_metadata (
                    episode_id, tmdb_details, still_images
                ) VALUES ($1, $2, $3)
                ON CONFLICT (episode_id) DO UPDATE SET
                    tmdb_details = EXCLUDED.tmdb_details,
                    updated_at = NOW()
                "#,
                actual_episode_id, // Use the actual ID from the database
                tmdb_details_json,
                serde_json::json!([])
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to insert episode metadata: {}", e))
            })?;
        }

        // Commit transaction
        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit transaction: {}", e)))?;

        let mut buff = Uuid::encode_buffer();

        info!(
            "Successfully stored episode reference: {} S{}E{} (actual ID: {})",
            episode.id.as_str(&mut buff),
            episode.season_number.value(),
            episode.episode_number.value(),
            actual_episode_id
        );

        Ok(())
    }

    async fn get_all_movie_references(&self) -> Result<Vec<MovieReference>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                mr.id, mr.tmdb_id, mr.title, mr.theme_color,
                mf.id as file_id, mf.file_path, mf.filename, mf.file_size,
                mf.created_at as file_created_at, mf.technical_metadata, mf.library_id
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            ORDER BY mr.title
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut movies = Vec::new();
        for row in rows {
            let technical_metadata: Option<serde_json::Value> = row.technical_metadata;
            let media_file_metadata = technical_metadata
                .map(|tm| serde_json::from_value(tm))
                .transpose()
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to deserialize metadata: {}", e))
                })?;

            let media_file = MediaFile {
                id: row.file_id,
                path: PathBuf::from(row.file_path),
                filename: row.filename,
                size: row.file_size as u64,
                created_at: row.file_created_at,
                media_file_metadata,
                library_id: LibraryID(row.library_id),
            };

            let movie_ref = MovieReference {
                id: MovieID(row.id),
                library_id: LibraryID(row.library_id),
                tmdb_id: row.tmdb_id as u64,
                title: MovieTitle::new(row.title)?,
                details: MediaDetailsOption::Endpoint(format!("/movie/{}", row.id)),
                endpoint: MovieURL::from_string(format!("/stream/{}", row.file_id)),
                file: media_file,
                theme_color: row.theme_color,
            };

            movies.push(movie_ref);
        }

        Ok(movies)
    }

    async fn get_series_references(&self) -> Result<Vec<SeriesReference>> {
        // TODO: Implement series references fetching
        Ok(vec![])
    }

    async fn get_series_seasons(&self, series_id: &SeriesID) -> Result<Vec<SeasonReference>> {
        let series_uuid = series_id.as_uuid();

        info!("Getting seasons for series: {}", series_uuid);

        let rows = sqlx::query!(
            r#"
            SELECT
                sr.id, sr.series_id, sr.season_number, sr.library_id, sr.tmdb_series_id, sr.created_at,
                sm.tmdb_details
            FROM season_references sr
            LEFT JOIN season_metadata sm ON sr.id = sm.season_id
            WHERE sr.series_id = $1
            ORDER BY sr.season_number
            "#,
            series_uuid
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get series seasons: {}", e)))?;

        let mut buff = Uuid::encode_buffer();

        info!(
            "Found {} season rows for series {}",
            rows.len(),
            series_id.as_str(&mut buff)
        );

        let mut seasons = Vec::new();
        for row in rows {
            // Parse TMDB details if available
            let details = if row.tmdb_details.is_null() {
                MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
            } else {
                match serde_json::from_value::<SeasonDetails>(row.tmdb_details) {
                    Ok(season_details) => {
                        MediaDetailsOption::Details(TmdbDetails::Season(season_details))
                    }
                    Err(e) => {
                        warn!("Failed to parse season TMDB details: {}", e);
                        MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
                    }
                }
            };

            seasons.push(SeasonReference {
                id: SeasonID(row.id),
                season_number: SeasonNumber::new(row.season_number as u8),
                series_id: SeriesID(row.series_id),
                library_id: LibraryID(row.library_id),
                tmdb_series_id: row.tmdb_series_id as u64,
                details,
                endpoint: SeasonURL::from_string(format!("/media/{}", row.id)),
                created_at: row.created_at,
                theme_color: None, // Seasons typically inherit theme color from the series
            });
        }

        Ok(seasons)
    }

    async fn get_season_episodes(&self, season_id: &SeasonID) -> Result<Vec<EpisodeReference>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                er.id, er.episode_number, er.season_number, er.season_id, er.series_id,
                er.tmdb_series_id, er.file_id,
                em.tmdb_details,
                mf.id as media_file_id, mf.file_path, mf.filename, mf.file_size,
                mf.created_at as file_created_at, mf.technical_metadata, mf.library_id
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            LEFT JOIN episode_metadata em ON er.id = em.episode_id
            WHERE er.season_id = $1
            ORDER BY er.episode_number
            "#,
            season_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get season episodes: {}", e)))?;

        let mut episodes = Vec::new();
        for row in rows {
            // Parse technical metadata
            let technical_metadata: Option<serde_json::Value> = row.technical_metadata;
            let parsed_metadata = technical_metadata
                .and_then(|tm| serde_json::from_value::<MediaFileMetadata>(tm).ok());

            // Create media file
            let media_file = MediaFile {
                id: row.media_file_id,
                path: PathBuf::from(&row.file_path),
                filename: row.filename.clone(),
                size: row.file_size as u64,
                created_at: row.file_created_at,
                media_file_metadata: parsed_metadata,
                library_id: LibraryID(row.library_id),
            };

            // Parse TMDB details if available
            let details = if row.tmdb_details.is_null() {
                MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
            } else {
                match serde_json::from_value::<EpisodeDetails>(row.tmdb_details) {
                    Ok(episode_details) => {
                        MediaDetailsOption::Details(TmdbDetails::Episode(episode_details))
                    }
                    Err(e) => {
                        warn!("Failed to parse episode TMDB details: {}", e);
                        MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
                    }
                }
            };

            episodes.push(EpisodeReference {
                id: EpisodeID(row.id),
                library_id: LibraryID(row.library_id),
                episode_number: EpisodeNumber::new(row.episode_number as u8),
                season_number: SeasonNumber::new(row.season_number as u8),
                season_id: SeasonID(row.season_id),
                series_id: SeriesID(row.series_id),
                tmdb_series_id: row.tmdb_series_id as u64,
                details,
                endpoint: EpisodeURL::from_string(format!("/stream/{}", row.file_id)),
                file: media_file,
            });
        }

        Ok(episodes)
    }

    async fn get_movie_reference(&self, id: &MovieID) -> Result<MovieReference> {
        // Include full metadata when fetching individual movie references
        // This is used by the /media endpoint to provide complete data
        match self.get_movie_with_metadata(id, true).await? {
            Some((movie_ref, _)) => Ok(movie_ref),
            None => Err(MediaError::NotFound("Movie not found".to_string())),
        }
    }

    async fn get_series_reference(&self, id: &SeriesID) -> Result<SeriesReference> {
        let series_uuid = id.as_uuid();

        // First get the series reference
        let series_row = sqlx::query!(
            r#"
            SELECT id, library_id, tmdb_id as "tmdb_id?", title, theme_color, created_at
            FROM series_references
            WHERE id = $1
            "#,
            series_uuid
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?
        .ok_or_else(|| MediaError::NotFound("Series not found".to_string()))?;

        // Try to get metadata if available
        let metadata_row = sqlx::query!(
            r#"
            SELECT tmdb_details
            FROM series_metadata
            WHERE series_id = $1
            "#,
            series_uuid
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        // Build the details field
        let details = if let Some(metadata) = metadata_row {
            match serde_json::from_value::<EnhancedSeriesDetails>(metadata.tmdb_details) {
                Ok(series_details) => {
                    MediaDetailsOption::Details(TmdbDetails::Series(series_details))
                }
                Err(e) => {
                    tracing::warn!("Failed to deserialize series metadata: {}", e);
                    MediaDetailsOption::Endpoint(format!("/series/{}", series_uuid))
                }
            }
        } else {
            MediaDetailsOption::Endpoint(format!("/series/{}", series_uuid))
        };

        // Handle nullable tmdb_id - use 0 if null (indicates no TMDB match)
        let tmdb_id = series_row.tmdb_id.unwrap_or(0) as u64;

        Ok(SeriesReference {
            id: SeriesID(series_row.id),
            library_id: LibraryID(series_row.library_id),
            tmdb_id,
            title: SeriesTitle::new(series_row.title)?,
            details,
            endpoint: SeriesURL::from_string(format!("/series/{}", series_uuid)),
            created_at: series_row.created_at,
            theme_color: series_row.theme_color,
        })
    }

    async fn get_season_reference(&self, id: &SeasonID) -> Result<SeasonReference> {
        let season_uuid = id.as_uuid();

        let row = sqlx::query!(
            r#"
            SELECT
                sr.id, sr.series_id, sr.season_number, sr.library_id, sr.tmdb_series_id, sr.created_at,
                sm.tmdb_details
            FROM season_references sr
            LEFT JOIN season_metadata sm ON sr.id = sm.season_id
            WHERE sr.id = $1
            "#,
            season_uuid
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?
        .ok_or_else(|| MediaError::NotFound("Season not found".to_string()))?;

        // Parse TMDB details if available
        let details = if row.tmdb_details.is_null() {
            MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
        } else {
            match serde_json::from_value::<SeasonDetails>(row.tmdb_details) {
                Ok(season_details) => {
                    MediaDetailsOption::Details(TmdbDetails::Season(season_details))
                }
                Err(e) => {
                    warn!("Failed to parse season TMDB details: {}", e);
                    MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
                }
            }
        };

        Ok(SeasonReference {
            id: SeasonID(row.id),
            season_number: SeasonNumber::new(row.season_number as u8),
            series_id: SeriesID(row.series_id),
            library_id: LibraryID(row.library_id),
            tmdb_series_id: row.tmdb_series_id as u64,
            details,
            endpoint: SeasonURL::from_string(format!("/media/{}", row.id)),
            created_at: row.created_at,
            theme_color: None, // Seasons typically inherit theme color from the series
        })
    }

    async fn get_episode_reference(&self, id: &EpisodeID) -> Result<EpisodeReference> {
        let episode_uuid = id.as_uuid();

        let row = sqlx::query!(
            r#"
            SELECT
                er.id, er.episode_number, er.season_number, er.season_id, er.series_id,
                er.tmdb_series_id, er.file_id,
                em.tmdb_details,
                mf.id as media_file_id, mf.file_path, mf.filename, mf.file_size,
                mf.created_at as file_created_at, mf.technical_metadata, mf.library_id
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            LEFT JOIN episode_metadata em ON er.id = em.episode_id
            WHERE er.id = $1
            "#,
            episode_uuid
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?
        .ok_or_else(|| MediaError::NotFound("Episode not found".to_string()))?;

        // Parse technical metadata
        let technical_metadata: Option<serde_json::Value> = row.technical_metadata;
        let parsed_metadata =
            technical_metadata.and_then(|tm| serde_json::from_value::<MediaFileMetadata>(tm).ok());

        // Create media file
        let media_file = MediaFile {
            id: row.media_file_id,
            path: PathBuf::from(&row.file_path),
            filename: row.filename.clone(),
            size: row.file_size as u64,
            created_at: row.file_created_at,
            media_file_metadata: parsed_metadata,
            library_id: LibraryID(row.library_id),
        };

        // Parse TMDB details if available
        let details = if row.tmdb_details.is_null() {
            MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
        } else {
            match serde_json::from_value::<EpisodeDetails>(row.tmdb_details) {
                Ok(episode_details) => {
                    MediaDetailsOption::Details(TmdbDetails::Episode(episode_details))
                }
                Err(e) => {
                    warn!("Failed to parse episode TMDB details: {}", e);
                    MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
                }
            }
        };

        Ok(EpisodeReference {
            id: EpisodeID(row.id),
            library_id: LibraryID(row.library_id),
            series_id: SeriesID(row.series_id),
            season_id: SeasonID(row.season_id),
            season_number: SeasonNumber::new(row.season_number as u8),
            episode_number: EpisodeNumber::new(row.episode_number as u8),
            tmdb_series_id: row.tmdb_series_id as u64,
            details,
            endpoint: EpisodeURL::from_string(format!("/stream/{}", row.file_id)),
            file: media_file,
        })
    }

    async fn update_movie_tmdb_id(&self, id: &MovieID, tmdb_id: u64) -> Result<()> {
        let movie_uuid = id.as_uuid();

        sqlx::query!(
            "UPDATE movie_references SET tmdb_id = $1, updated_at = NOW() WHERE id = $2",
            tmdb_id as i64,
            movie_uuid
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Update failed: {}", e)))?;

        Ok(())
    }

    async fn update_series_tmdb_id(&self, id: &SeriesID, tmdb_id: u64) -> Result<()> {
        let series_uuid = id.as_uuid();

        sqlx::query!(
            "UPDATE series_references SET tmdb_id = $1, updated_at = NOW() WHERE id = $2",
            tmdb_id as i64,
            series_uuid
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Update failed: {}", e)))?;

        Ok(())
    }

    async fn get_series_by_tmdb_id(
        &self,
        library_id: LibraryID,
        tmdb_id: u64,
    ) -> Result<Option<SeriesReference>> {
        let row = sqlx::query!(
            r#"
            SELECT id, library_id, tmdb_id as "tmdb_id?", title, theme_color, created_at
            FROM series_references
            WHERE library_id = $1 AND tmdb_id = $2
            "#,
            library_id.as_uuid(),
            tmdb_id as i64
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        if let Some(row) = row {
            // tmdb_id should not be null when querying by tmdb_id
            let tmdb_id = row.tmdb_id.ok_or_else(|| {
                MediaError::Internal("Series found by tmdb_id but tmdb_id is null".to_string())
            })?;

            Ok(Some(SeriesReference {
                id: SeriesID(row.id),
                library_id: LibraryID(row.library_id),
                tmdb_id: tmdb_id as u64,
                title: SeriesTitle::new(row.title)?,
                details: MediaDetailsOption::Endpoint(format!("/series/{}", row.id)),
                endpoint: SeriesURL::from_string(format!("/series/{}", row.id)),
                created_at: row.created_at,
                theme_color: row.theme_color,
            }))
        } else {
            Ok(None)
        }
    }

    async fn find_series_by_name(
        &self,
        library_id: LibraryID,
        name: &str,
    ) -> Result<Option<SeriesReference>> {
        // Use ILIKE for case-insensitive search with fuzzy matching
        let search_pattern = format!("%{}%", name);

        let row = sqlx::query!(
            r#"
            SELECT id, library_id, tmdb_id as "tmdb_id?", title, theme_color, created_at
            FROM series_references
            WHERE library_id = $1 AND title ILIKE $2
            ORDER BY
                CASE
                    WHEN LOWER(title) = LOWER($3) THEN 0
                    WHEN LOWER(title) LIKE LOWER($3 || '%') THEN 1
                    ELSE 2
                END,
                LENGTH(title)
            LIMIT 1
            "#,
            library_id.as_uuid(),
            search_pattern,
            name
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        if let Some(row) = row {
            // Handle nullable tmdb_id - use 0 if null (indicates no TMDB match)
            let tmdb_id = row.tmdb_id.unwrap_or(0) as u64;

            Ok(Some(SeriesReference {
                id: SeriesID(row.id),
                library_id: LibraryID(row.library_id),
                tmdb_id,
                title: SeriesTitle::new(row.title)?,
                details: MediaDetailsOption::Endpoint(format!("/series/{}", row.id)),
                endpoint: SeriesURL::from_string(format!("/series/{}", row.id)),
                created_at: row.created_at,
                theme_color: row.theme_color,
            }))
        } else {
            Ok(None)
        }
    }

    async fn list_library_references(&self) -> Result<Vec<LibraryReference>> {
        let rows = sqlx::query!(
            "SELECT id, name, library_type, paths FROM libraries WHERE enabled = true ORDER BY name"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut libraries = Vec::new();
        for row in rows {
            let library_type = match row.library_type.as_str() {
                "movies" => crate::LibraryType::Movies,
                "tvshows" => crate::LibraryType::Series,
                _ => continue,
            };

            libraries.push(LibraryReference {
                id: LibraryID(row.id),
                name: row.name,
                library_type,
                paths: row.paths.into_iter().map(PathBuf::from).collect(),
            });
        }

        Ok(libraries)
    }

    async fn get_library_reference(&self, id: Uuid) -> Result<LibraryReference> {
        let row = sqlx::query!(
            "SELECT id, name, library_type, paths FROM libraries WHERE id = $1",
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        match row {
            Some(row) => {
                let library_type = match row.library_type.as_str() {
                    "movies" => crate::LibraryType::Movies,
                    "tvshows" => crate::LibraryType::Series,
                    _ => return Err(MediaError::InvalidMedia("Unknown library type".to_string())),
                };

                Ok(LibraryReference {
                    id: LibraryID(row.id),
                    name: row.name,
                    library_type,
                    paths: row.paths.into_iter().map(PathBuf::from).collect(),
                })
            }
            None => Err(MediaError::NotFound("Library not found".to_string())),
        }
    }

    // Image management methods

    async fn create_image(&self, tmdb_path: &str) -> Result<ImageRecord> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let row = sqlx::query!(
            r#"
            INSERT INTO images (id, tmdb_path, created_at, updated_at)
            VALUES ($1, $2, $3, $3)
            RETURNING id, tmdb_path, file_hash, file_size, width, height, format, created_at
            "#,
            id,
            tmdb_path,
            now
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to create image: {}", e)))?;

        Ok(ImageRecord {
            id: row.id,
            tmdb_path: row.tmdb_path,
            file_hash: row.file_hash,
            file_size: row.file_size,
            width: row.width,
            height: row.height,
            format: row.format,
            created_at: row.created_at,
        })
    }

    async fn get_image_by_tmdb_path(&self, tmdb_path: &str) -> Result<Option<ImageRecord>> {
        let row = sqlx::query!(
            r#"
            SELECT id, tmdb_path, file_hash, file_size, width, height, format, created_at
            FROM images
            WHERE tmdb_path = $1
            "#,
            tmdb_path
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get image: {}", e)))?;

        Ok(row.map(|r| ImageRecord {
            id: r.id,
            tmdb_path: r.tmdb_path,
            file_hash: r.file_hash,
            file_size: r.file_size,
            width: r.width,
            height: r.height,
            format: r.format,
            created_at: r.created_at,
        }))
    }

    async fn get_image_by_hash(&self, hash: &str) -> Result<Option<ImageRecord>> {
        let row = sqlx::query!(
            r#"
            SELECT id, tmdb_path, file_hash, file_size, width, height, format, created_at
            FROM images
            WHERE file_hash = $1
            "#,
            hash
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get image by hash: {}", e)))?;

        Ok(row.map(|r| ImageRecord {
            id: r.id,
            tmdb_path: r.tmdb_path,
            file_hash: r.file_hash,
            file_size: r.file_size,
            width: r.width,
            height: r.height,
            format: r.format,
            created_at: r.created_at,
        }))
    }

    async fn update_image_metadata(
        &self,
        image_id: Uuid,
        hash: &str,
        size: i32,
        width: i32,
        height: i32,
        format: &str,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE images
            SET file_hash = $2, file_size = $3, width = $4, height = $5, format = $6, updated_at = NOW()
            WHERE id = $1
            "#,
            image_id,
            hash,
            size,
            width,
            height,
            format
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update image metadata: {}", e)))?;

        Ok(())
    }

    async fn create_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
        file_path: &str,
        size: i32,
        width: Option<i32>,
        height: Option<i32>,
    ) -> Result<ImageVariant> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let row = sqlx::query!(
            r#"
            INSERT INTO image_variants (id, image_id, variant, file_path, file_size, width, height, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (image_id, variant) DO UPDATE SET
                file_path = EXCLUDED.file_path,
                file_size = EXCLUDED.file_size,
                width = EXCLUDED.width,
                height = EXCLUDED.height
            RETURNING id, image_id, variant, file_path, file_size, width, height, created_at
            "#,
            id,
            image_id,
            variant,
            file_path,
            size,
            width,
            height,
            now
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to create image variant: {}", e)))?;

        Ok(ImageVariant {
            id: row.id,
            image_id: row.image_id,
            variant: row.variant,
            file_path: row.file_path,
            file_size: row.file_size,
            width: row.width,
            height: row.height,
            created_at: row.created_at,
        })
    }

    async fn get_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
    ) -> Result<Option<ImageVariant>> {
        let row = sqlx::query!(
            r#"
            SELECT id, image_id, variant, file_path, file_size, width, height, created_at
            FROM image_variants
            WHERE image_id = $1 AND variant = $2
            "#,
            image_id,
            variant
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get image variant: {}", e)))?;

        Ok(row.map(|r| ImageVariant {
            id: r.id,
            image_id: r.image_id,
            variant: r.variant,
            file_path: r.file_path,
            file_size: r.file_size,
            width: r.width,
            height: r.height,
            created_at: r.created_at,
        }))
    }

    async fn get_image_variants(&self, image_id: Uuid) -> Result<Vec<ImageVariant>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, image_id, variant, file_path, file_size, width, height, created_at
            FROM image_variants
            WHERE image_id = $1
            ORDER BY variant
            "#,
            image_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get image variants: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|r| ImageVariant {
                id: r.id,
                image_id: r.image_id,
                variant: r.variant,
                file_path: r.file_path,
                file_size: r.file_size,
                width: r.width,
                height: r.height,
                created_at: r.created_at,
            })
            .collect())
    }

    async fn link_media_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_id: Uuid,
        image_type: &str,
        order_index: i32,
        is_primary: bool,
    ) -> Result<()> {
        info!(
            "link_media_image: type={}, media_id={}, image_id={}, image_type={}, index={}",
            media_type, media_id, image_id, image_type, order_index
        );

        sqlx::query!(
            r#"
            INSERT INTO media_images (media_type, media_id, image_id, image_type, order_index, is_primary)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (media_type, media_id, image_type, order_index) DO UPDATE SET
                image_id = EXCLUDED.image_id,
                is_primary = EXCLUDED.is_primary
            "#,
            media_type,
            media_id,
            image_id,
            image_type,
            order_index,
            is_primary
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to link media image: {}", e)))?;

        Ok(())
    }

    async fn get_media_images(&self, media_type: &str, media_id: Uuid) -> Result<Vec<MediaImage>> {
        let rows = sqlx::query!(
            r#"
            SELECT media_type, media_id, image_id, image_type, order_index, is_primary
            FROM media_images
            WHERE media_type = $1 AND media_id = $2
            ORDER BY image_type, order_index
            "#,
            media_type,
            media_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get media images: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|r| MediaImage {
                media_type: r.media_type,
                media_id: r.media_id,
                image_id: r.image_id,
                image_type: r.image_type,
                order_index: r.order_index,
                is_primary: r.is_primary,
            })
            .collect())
    }

    async fn get_media_primary_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_type: &str,
    ) -> Result<Option<MediaImage>> {
        let row = sqlx::query!(
            r#"
            SELECT media_type, media_id, image_id, image_type, order_index, is_primary
            FROM media_images
            WHERE media_type = $1 AND media_id = $2 AND image_type = $3 AND is_primary = true
            LIMIT 1
            "#,
            media_type,
            media_id,
            image_type
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get primary image: {}", e)))?;

        Ok(row.map(|r| MediaImage {
            media_type: r.media_type,
            media_id: r.media_id,
            image_id: r.image_id,
            image_type: r.image_type,
            order_index: r.order_index,
            is_primary: r.is_primary,
        }))
    }

    async fn lookup_image_variant(
        &self,
        params: &ImageLookupParams,
    ) -> Result<Option<(ImageRecord, Option<ImageVariant>)>> {
        info!(
            "lookup_image_variant: type={}, id='{}', image_type={}, index={}",
            params.media_type, params.media_id, params.image_type, params.index
        );

        // Parse media_id to UUID
        let media_id = match Uuid::parse_str(&params.media_id) {
            Ok(uuid) => uuid,
            Err(e) => {
                warn!(
                    "Failed to parse media_id '{}' as UUID: {}",
                    params.media_id, e
                );
                return Err(MediaError::InvalidMedia(format!(
                    "Invalid media ID '{}': {}",
                    params.media_id, e
                )));
            }
        };

        // First find the media image link
        info!(
            "Querying media_images table: type={}, media_id={}, image_type={}, index={}",
            &params.media_type, media_id, &params.image_type, params.index
        );

        let media_image = sqlx::query!(
            r#"
            SELECT image_id
            FROM media_images
            WHERE media_type = $1 AND media_id = $2 AND image_type = $3 AND order_index = $4
            "#,
            &params.media_type,
            media_id,
            &params.image_type,
            params.index as i32
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to lookup media image: {}", e)))?;

        if let Some(media_image) = media_image {
            // Get the image record
            let image = sqlx::query!(
                r#"
                SELECT id, tmdb_path, file_hash, file_size, width, height, format, created_at
                FROM images
                WHERE id = $1
                "#,
                media_image.image_id
            )
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to get image: {}", e)))?;

            if let Some(image_row) = image {
                let image_record = ImageRecord {
                    id: image_row.id,
                    tmdb_path: image_row.tmdb_path,
                    file_hash: image_row.file_hash,
                    file_size: image_row.file_size,
                    width: image_row.width,
                    height: image_row.height,
                    format: image_row.format,
                    created_at: image_row.created_at,
                };

                // Get the variant if requested
                let variant = if let Some(variant_name) = &params.variant {
                    self.get_image_variant(image_row.id, variant_name).await?
                } else {
                    None
                };

                return Ok(Some((image_record, variant)));
            }
        }

        Ok(None)
    }

    async fn cleanup_orphaned_images(&self) -> Result<u32> {
        let result = sqlx::query!(
            r#"
            DELETE FROM images
            WHERE id NOT IN (
                SELECT DISTINCT image_id FROM media_images
            )
            "#
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to cleanup orphaned images: {}", e)))?;

        Ok(result.rows_affected() as u32)
    }

    async fn get_image_cache_stats(&self) -> Result<HashMap<String, u64>> {
        let mut stats = HashMap::new();

        // Total images
        let total_images = sqlx::query!("SELECT COUNT(*) as count FROM images")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to count images: {}", e)))?;
        stats.insert(
            "total_images".to_string(),
            total_images.count.unwrap_or(0) as u64,
        );

        // Total variants
        let total_variants = sqlx::query!("SELECT COUNT(*) as count FROM image_variants")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to count variants: {}", e)))?;
        stats.insert(
            "total_variants".to_string(),
            total_variants.count.unwrap_or(0) as u64,
        );

        // Total size
        let total_size =
            sqlx::query!("SELECT COALESCE(SUM(file_size), 0) as total FROM image_variants")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to sum sizes: {}", e)))?;
        stats.insert(
            "total_size_bytes".to_string(),
            total_size.total.unwrap_or(0) as u64,
        );

        // Variants by type
        let variant_counts = sqlx::query!(
            r#"
            SELECT variant, COUNT(*) as count
            FROM image_variants
            GROUP BY variant
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to count by variant: {}", e)))?;

        for row in variant_counts {
            stats.insert(
                format!("variant_{}", row.variant),
                row.count.unwrap_or(0) as u64,
            );
        }

        Ok(stats)
    }

    // Scan state management methods
    async fn create_scan_state(&self, scan_state: &ScanState) -> Result<()> {
        let options_json = serde_json::to_value(&scan_state.options).map_err(|e| {
            MediaError::Internal(format!("Failed to serialize scan options: {}", e))
        })?;

        let errors_json = serde_json::to_value(&scan_state.errors)
            .map_err(|e| MediaError::Internal(format!("Failed to serialize errors: {}", e)))?;

        sqlx::query!(
            r#"
            INSERT INTO scan_state (
                id, library_id, scan_type, status, total_folders, processed_folders,
                total_files, processed_files, current_path, error_count, errors,
                started_at, updated_at, completed_at, options
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
            scan_state.id,
            scan_state.library_id.as_uuid(),
            format!("{:?}", scan_state.scan_type).to_lowercase(),
            format!("{:?}", scan_state.status).to_lowercase(),
            scan_state.total_folders,
            scan_state.processed_folders,
            scan_state.total_files,
            scan_state.processed_files,
            scan_state.current_path,
            scan_state.error_count,
            errors_json,
            scan_state.started_at,
            scan_state.updated_at,
            scan_state.completed_at,
            options_json
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to create scan state: {}", e)))?;

        Ok(())
    }

    async fn update_scan_state(&self, scan_state: &ScanState) -> Result<()> {
        let options_json = serde_json::to_value(&scan_state.options).map_err(|e| {
            MediaError::Internal(format!("Failed to serialize scan options: {}", e))
        })?;

        let errors_json = serde_json::to_value(&scan_state.errors)
            .map_err(|e| MediaError::Internal(format!("Failed to serialize errors: {}", e)))?;

        sqlx::query!(
            r#"
            UPDATE scan_state SET
                status = $2, total_folders = $3, processed_folders = $4,
                total_files = $5, processed_files = $6, current_path = $7,
                error_count = $8, errors = $9, updated_at = $10,
                completed_at = $11, options = $12
            WHERE id = $1
            "#,
            scan_state.id,
            format!("{:?}", scan_state.status).to_lowercase(),
            scan_state.total_folders,
            scan_state.processed_folders,
            scan_state.total_files,
            scan_state.processed_files,
            scan_state.current_path,
            scan_state.error_count,
            errors_json,
            scan_state.updated_at,
            scan_state.completed_at,
            options_json
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update scan state: {}", e)))?;

        Ok(())
    }

    async fn get_scan_state(&self, id: Uuid) -> Result<Option<ScanState>> {
        let row = sqlx::query!(
            r#"
            SELECT id, library_id, scan_type, status, total_folders, processed_folders,
                   total_files, processed_files, current_path, error_count, errors,
                   started_at, updated_at, completed_at, options
            FROM scan_state
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get scan state: {}", e)))?;

        if let Some(row) = row {
            let scan_type = match row.scan_type.as_str() {
                "full" => ScanType::Full,
                "incremental" => ScanType::Incremental,
                "refresh_metadata" => ScanType::RefreshMetadata,
                "analyze" => ScanType::Analyze,
                _ => {
                    return Err(MediaError::Internal(format!(
                        "Unknown scan type: {}",
                        row.scan_type
                    )));
                }
            };

            let status = match row.status.as_str() {
                "pending" => ScanStatus::Pending,
                "running" => ScanStatus::Running,
                "paused" => ScanStatus::Paused,
                "completed" => ScanStatus::Completed,
                "failed" => ScanStatus::Failed,
                "cancelled" => ScanStatus::Cancelled,
                _ => {
                    return Err(MediaError::Internal(format!(
                        "Unknown scan status: {}",
                        row.status
                    )));
                }
            };

            let errors: Vec<String> = if let Some(errors_json) = row.errors {
                serde_json::from_value(errors_json).unwrap_or_else(|_| vec![])
            } else {
                vec![]
            };

            Ok(Some(ScanState {
                id: row.id,
                library_id: LibraryID(row.library_id),
                scan_type,
                status,
                total_folders: row.total_folders.unwrap_or(0),
                processed_folders: row.processed_folders.unwrap_or(0),
                total_files: row.total_files.unwrap_or(0),
                processed_files: row.processed_files.unwrap_or(0),
                current_path: row.current_path,
                error_count: row.error_count.unwrap_or(0),
                errors,
                started_at: row.started_at,
                updated_at: row.updated_at,
                completed_at: row.completed_at,
                options: row.options,
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_active_scans(&self, library_id: Option<Uuid>) -> Result<Vec<ScanState>> {
        // Build the query dynamically to avoid type mismatches
        let sql = if library_id.is_some() {
            r#"
            SELECT id, library_id, scan_type, status, total_folders, processed_folders,
                   total_files, processed_files, current_path, error_count, errors,
                   started_at, updated_at, completed_at, options
            FROM scan_state
            WHERE library_id = $1 AND status IN ('pending', 'running', 'paused')
            ORDER BY started_at DESC
            "#
        } else {
            r#"
            SELECT id, library_id, scan_type, status, total_folders, processed_folders,
                   total_files, processed_files, current_path, error_count, errors,
                   started_at, updated_at, completed_at, options
            FROM scan_state
            WHERE status IN ('pending', 'running', 'paused')
            ORDER BY started_at DESC
            "#
        };

        let rows = if let Some(lib_id) = library_id {
            sqlx::query(sql).bind(lib_id).fetch_all(&self.pool).await
        } else {
            sqlx::query(sql).fetch_all(&self.pool).await
        }
        .map_err(|e| MediaError::Internal(format!("Failed to get active scans: {}", e)))?;

        let mut scans = Vec::new();
        for row in rows {
            let scan_type_str: String = row.try_get("scan_type")?;
            let scan_type = match scan_type_str.as_str() {
                "full" => ScanType::Full,
                "incremental" => ScanType::Incremental,
                "refresh_metadata" => ScanType::RefreshMetadata,
                "analyze" => ScanType::Analyze,
                _ => continue,
            };

            let status_str: String = row.try_get("status")?;
            let status = match status_str.as_str() {
                "pending" => ScanStatus::Pending,
                "running" => ScanStatus::Running,
                "paused" => ScanStatus::Paused,
                "completed" => ScanStatus::Completed,
                "failed" => ScanStatus::Failed,
                "cancelled" => ScanStatus::Cancelled,
                _ => continue,
            };

            let errors_json: Option<serde_json::Value> = row.try_get("errors")?;
            let errors: Vec<String> = if let Some(json) = errors_json {
                serde_json::from_value(json).unwrap_or_else(|_| vec![])
            } else {
                vec![]
            };

            scans.push(ScanState {
                id: row.try_get("id")?,
                library_id: LibraryID(row.try_get("library_id")?),
                scan_type,
                status,
                total_folders: row.try_get::<Option<i32>, _>("total_folders")?.unwrap_or(0),
                processed_folders: row
                    .try_get::<Option<i32>, _>("processed_folders")?
                    .unwrap_or(0),
                total_files: row.try_get::<Option<i32>, _>("total_files")?.unwrap_or(0),
                processed_files: row
                    .try_get::<Option<i32>, _>("processed_files")?
                    .unwrap_or(0),
                current_path: row.try_get("current_path")?,
                error_count: row.try_get::<Option<i32>, _>("error_count")?.unwrap_or(0),
                errors,
                started_at: row.try_get("started_at")?,
                updated_at: row.try_get("updated_at")?,
                completed_at: row.try_get("completed_at")?,
                options: row.try_get("options")?,
            });
        }

        Ok(scans)
    }

    async fn get_latest_scan(
        &self,
        library_id: LibraryID,
        scan_type: ScanType,
    ) -> Result<Option<ScanState>> {
        let scan_type_str = format!("{:?}", scan_type).to_lowercase();

        let row = sqlx::query!(
            r#"
            SELECT id, library_id, scan_type, status, total_folders, processed_folders,
                   total_files, processed_files, current_path, error_count, errors,
                   started_at, updated_at, completed_at, options
            FROM scan_state
            WHERE library_id = $1 AND scan_type = $2
            ORDER BY started_at DESC
            LIMIT 1
            "#,
            library_id.as_uuid(),
            scan_type_str
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get latest scan: {}", e)))?;

        if let Some(row) = row {
            let status = match row.status.as_str() {
                "pending" => ScanStatus::Pending,
                "running" => ScanStatus::Running,
                "paused" => ScanStatus::Paused,
                "completed" => ScanStatus::Completed,
                "failed" => ScanStatus::Failed,
                "cancelled" => ScanStatus::Cancelled,
                _ => {
                    return Err(MediaError::Internal(format!(
                        "Unknown scan status: {}",
                        row.status
                    )));
                }
            };

            let errors: Vec<String> = if let Some(errors_json) = row.errors {
                serde_json::from_value(errors_json).unwrap_or_else(|_| vec![])
            } else {
                vec![]
            };

            Ok(Some(ScanState {
                id: row.id,
                library_id: LibraryID(row.library_id),
                scan_type,
                status,
                total_folders: row.total_folders.unwrap_or(0),
                processed_folders: row.processed_folders.unwrap_or(0),
                total_files: row.total_files.unwrap_or(0),
                processed_files: row.processed_files.unwrap_or(0),
                current_path: row.current_path,
                error_count: row.error_count.unwrap_or(0),
                errors,
                started_at: row.started_at,
                updated_at: row.updated_at,
                completed_at: row.completed_at,
                options: row.options,
            }))
        } else {
            Ok(None)
        }
    }

    // Media processing status methods
    async fn create_or_update_processing_status(
        &self,
        status: &MediaProcessingStatus,
    ) -> Result<()> {
        let error_details_json = status
            .error_details
            .as_ref()
            .map(|d| serde_json::to_value(d))
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
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to create/update processing status: {}", e))
        })?;

        Ok(())
    }

    async fn get_processing_status(
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
        .fetch_optional(&self.pool)
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

    async fn get_unprocessed_files(
        &self,
        library_id: LibraryID,
        status_type: &str,
        limit: i32,
    ) -> Result<Vec<MediaFile>> {
        // Build the query dynamically based on status type
        let sql = match status_type {
            "metadata" => {
                r#"
                SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                       f.technical_metadata, f.parsed_info, f.created_at, f.updated_at
                FROM media_files f
                LEFT JOIN media_processing_status p ON f.id = p.media_file_id
                WHERE f.library_id = $1 AND (p.metadata_extracted IS NULL OR p.metadata_extracted = false)
                LIMIT $2
            "#
            }
            "tmdb" => {
                r#"
                SELECT f.id, f.library_id, f.file_path, f.filename, f.file_size,
                       f.technical_metadata, f.parsed_info, f.created_at, f.updated_at
                FROM media_files f
                LEFT JOIN media_processing_status p ON f.id = p.media_file_id
                WHERE f.library_id = $1 AND (p.tmdb_matched IS NULL OR p.tmdb_matched = false)
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
            .fetch_all(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to get unprocessed files: {}", e)))?;

        let mut files = Vec::new();
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

    async fn get_failed_files(
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
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get failed files: {}", e)))?;

        let mut files = Vec::new();
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

    async fn reset_processing_status(&self, media_file_id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM media_processing_status WHERE media_file_id = $1
            "#,
            media_file_id
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to reset processing status: {}", e)))?;

        Ok(())
    }

    // File watch event methods
    async fn create_file_watch_event(&self, event: &FileWatchEvent) -> Result<()> {
        let event_type_str = format!("{:?}", event.event_type).to_lowercase();

        sqlx::query!(
            r#"
            INSERT INTO file_watch_events (
                id, library_id, event_type, file_path, old_path, file_size,
                detected_at, processed, processed_at, processing_attempts, last_error
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
            event.id,
            event.library_id.as_uuid(),
            event_type_str,
            event.file_path,
            event.old_path,
            event.file_size,
            event.detected_at,
            event.processed,
            event.processed_at,
            event.processing_attempts,
            event.last_error
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to create file watch event: {}", e)))?;

        Ok(())
    }

    async fn get_unprocessed_events(
        &self,
        library_id: LibraryID,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, library_id, event_type, file_path, old_path, file_size,
                   detected_at, processed, processed_at, processing_attempts, last_error
            FROM file_watch_events
            WHERE library_id = $1 AND processed = false
            ORDER BY detected_at ASC
            LIMIT $2
            "#,
            library_id.as_uuid(),
            limit as i64
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get unprocessed events: {}", e)))?;

        let mut events = Vec::new();
        for row in rows {
            let event_type = match row.event_type.as_str() {
                "created" => FileWatchEventType::Created,
                "modified" => FileWatchEventType::Modified,
                "deleted" => FileWatchEventType::Deleted,
                "moved" => FileWatchEventType::Moved,
                _ => continue,
            };

            events.push(FileWatchEvent {
                id: row.id,
                library_id: LibraryID(row.library_id),
                event_type,
                file_path: row.file_path,
                old_path: row.old_path,
                file_size: row.file_size,
                detected_at: row.detected_at,
                processed: row.processed,
                processed_at: row.processed_at,
                processing_attempts: row.processing_attempts,
                last_error: row.last_error,
            });
        }

        Ok(events)
    }

    async fn mark_event_processed(&self, event_id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE file_watch_events
            SET processed = true, processed_at = NOW()
            WHERE id = $1
            "#,
            event_id
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to mark event processed: {}", e)))?;

        Ok(())
    }

    async fn cleanup_old_events(&self, days_to_keep: i32) -> Result<u32> {
        let result = sqlx::query!(
            r#"
            DELETE FROM file_watch_events
            WHERE processed = true AND processed_at < NOW() - CAST($1 || ' days' AS INTERVAL)
            "#,
            days_to_keep.to_string()
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to cleanup old events: {}", e)))?;

        Ok(result.rows_affected() as u32)
    }

    // ==================== User Management Methods ====================

    async fn create_user(&self, user: &crate::User) -> Result<()> {
        // The trait method doesn't include password_hash, so we can't create a user through this interface
        // Users should be created through the authentication system which has access to the password
        Err(MediaError::Internal(
            "Use authentication system to create users with password".to_string(),
        ))
    }

    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<crate::User>> {
        self.get_user_by_id(id).await
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<crate::User>> {
        self.get_user_by_username(username).await
    }

    async fn get_all_users(&self) -> Result<Vec<crate::User>> {
        self.get_all_users().await
    }

    async fn update_user(&self, user: &crate::User) -> Result<()> {
        self.update_user(user).await
    }

    async fn delete_user(&self, id: Uuid) -> Result<()> {
        self.delete_user(id).await
    }

    async fn get_user_password_hash(&self, user_id: Uuid) -> Result<Option<String>> {
        self.get_user_password_hash(user_id).await
    }

    async fn update_user_password(&self, user_id: Uuid, password_hash: &str) -> Result<()> {
        self.update_user_password(user_id, password_hash).await
    }

    async fn delete_user_atomic(&self, user_id: Uuid, check_last_admin: bool) -> Result<()> {
        self.delete_user_atomic(user_id, check_last_admin).await
    }

    // ==================== RBAC Methods ====================

    async fn get_user_permissions(&self, user_id: Uuid) -> Result<crate::rbac::UserPermissions> {
        self.rbac_get_user_permissions(user_id).await
    }

    async fn get_all_roles(&self) -> Result<Vec<crate::rbac::Role>> {
        self.rbac_get_all_roles().await
    }

    async fn get_all_permissions(&self) -> Result<Vec<crate::rbac::Permission>> {
        self.rbac_get_all_permissions().await
    }

    async fn assign_user_role(&self, user_id: Uuid, role_id: Uuid, granted_by: Uuid) -> Result<()> {
        self.rbac_assign_user_role(user_id, role_id, granted_by)
            .await
    }

    async fn remove_user_role(&self, user_id: Uuid, role_id: Uuid) -> Result<()> {
        self.rbac_remove_user_role(user_id, role_id).await
    }

    async fn remove_user_role_atomic(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        check_last_admin: bool,
    ) -> Result<()> {
        self.rbac_remove_user_role_atomic(user_id, role_id, check_last_admin)
            .await
    }

    async fn override_user_permission(
        &self,
        user_id: Uuid,
        permission: &str,
        granted: bool,
        granted_by: Uuid,
        reason: Option<String>,
    ) -> Result<()> {
        self.rbac_override_user_permission(user_id, permission, granted, granted_by, reason)
            .await
    }

    async fn get_admin_count(&self, exclude_user_id: Option<Uuid>) -> Result<usize> {
        self.get_admin_count(exclude_user_id).await
    }

    async fn user_has_role(&self, user_id: Uuid, role_name: &str) -> Result<bool> {
        self.user_has_role(user_id, role_name).await
    }

    async fn get_users_with_role(&self, role_name: &str) -> Result<Vec<Uuid>> {
        self.get_users_with_role(role_name).await
    }

    // ==================== Authentication Methods ====================

    async fn store_refresh_token(
        &self,
        token: &str,
        user_id: Uuid,
        device_name: Option<String>,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        self.store_refresh_token(token, user_id, device_name, expires_at)
            .await
    }

    async fn get_refresh_token(
        &self,
        token: &str,
    ) -> Result<Option<(Uuid, chrono::DateTime<chrono::Utc>)>> {
        self.get_refresh_token(token).await
    }

    async fn delete_refresh_token(&self, token: &str) -> Result<()> {
        self.delete_refresh_token(token).await
    }

    async fn delete_user_refresh_tokens(&self, user_id: Uuid) -> Result<()> {
        self.delete_user_refresh_tokens(user_id).await
    }

    // ==================== Session Management ====================

    async fn create_session(&self, session: &crate::UserSession) -> Result<()> {
        self.create_session(session).await
    }

    async fn get_user_sessions(&self, user_id: Uuid) -> Result<Vec<crate::UserSession>> {
        self.get_user_sessions(user_id).await
    }

    async fn delete_session(&self, session_id: Uuid) -> Result<()> {
        self.delete_session(session_id).await
    }

    // ==================== Watch Status Methods ====================

    async fn update_watch_progress(
        &self,
        user_id: Uuid,
        progress: &crate::UpdateProgressRequest,
    ) -> Result<()> {
        self.update_watch_progress(user_id, progress).await
    }

    async fn get_user_watch_state(&self, user_id: Uuid) -> Result<crate::UserWatchState> {
        self.get_user_watch_state(user_id).await
    }

    async fn get_continue_watching(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<crate::InProgressItem>> {
        self.get_continue_watching(user_id, limit).await
    }

    async fn clear_watch_progress(&self, user_id: Uuid, media_id: &Uuid) -> Result<()> {
        self.clear_watch_progress(user_id, media_id).await
    }

    async fn is_media_completed(&self, user_id: Uuid, media_id: &Uuid) -> Result<bool> {
        self.is_media_completed(user_id, media_id).await
    }

    // ==================== Sync Session Methods ====================

    async fn create_sync_session(&self, session: &crate::SyncSession) -> Result<()> {
        self.create_sync_session(session).await
    }

    async fn get_sync_session_by_code(
        &self,
        room_code: &str,
    ) -> Result<Option<crate::SyncSession>> {
        self.get_sync_session_by_code(room_code).await
    }

    async fn get_sync_session(&self, id: Uuid) -> Result<Option<crate::SyncSession>> {
        self.get_sync_session(id).await
    }

    async fn update_sync_session_state(
        &self,
        id: Uuid,
        state: &crate::PlaybackState,
    ) -> Result<()> {
        self.update_sync_session_state(id, state).await
    }

    async fn add_sync_participant(
        &self,
        session_id: Uuid,
        participant: &crate::Participant,
    ) -> Result<()> {
        self.add_sync_participant(session_id, participant).await
    }

    async fn remove_sync_participant(&self, session_id: Uuid, user_id: Uuid) -> Result<()> {
        self.remove_sync_participant(session_id, user_id).await
    }

    async fn delete_sync_session(&self, id: Uuid) -> Result<()> {
        self.delete_sync_session(id).await
    }

    async fn update_sync_session(&self, id: Uuid, session: &crate::SyncSession) -> Result<()> {
        self.update_sync_session(id, session).await
    }

    async fn end_sync_session(&self, id: Uuid) -> Result<()> {
        // For now, ending a session is the same as deleting it
        self.delete_sync_session(id).await
    }

    async fn cleanup_expired_sync_sessions(&self) -> Result<u32> {
        self.cleanup_expired_sync_sessions().await
    }

    async fn query_media(
        &self,
        query: &crate::query::MediaQuery,
    ) -> Result<Vec<crate::query::MediaWithStatus>> {
        self.query_media(query).await
    }

    // Device authentication methods

    async fn register_device(&self, device: &crate::auth::AuthenticatedDevice) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO authenticated_devices
            (id, fingerprint, name, platform, app_version, first_authenticated_by,
             first_authenticated_at, trusted_until, last_seen_at, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            device.id,
            device.fingerprint,
            device.name,
            serde_json::to_string(&device.platform)?,
            device.app_version,
            device.first_authenticated_by,
            device.first_authenticated_at,
            device.trusted_until,
            device.last_seen_at,
            device.metadata
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_device_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Option<crate::auth::AuthenticatedDevice>> {
        let row = sqlx::query!(
            r#"
            SELECT id, fingerprint, name, platform, app_version, first_authenticated_by,
                   first_authenticated_at, trusted_until, last_seen_at, revoked,
                   revoked_by, revoked_at, metadata
            FROM authenticated_devices
            WHERE fingerprint = $1
            "#,
            fingerprint
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| crate::auth::AuthenticatedDevice {
            id: r.id,
            fingerprint: r.fingerprint,
            name: r.name,
            platform: serde_json::from_str(&r.platform).unwrap_or(crate::auth::Platform::Unknown),
            app_version: r.app_version,
            first_authenticated_by: r.first_authenticated_by,
            first_authenticated_at: r.first_authenticated_at,
            trusted_until: r.trusted_until,
            last_seen_at: r.last_seen_at,
            revoked: r.revoked,
            revoked_by: r.revoked_by,
            revoked_at: r.revoked_at,
            metadata: r.metadata.unwrap_or(serde_json::json!({})),
        }))
    }

    async fn get_user_devices(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<crate::auth::AuthenticatedDevice>> {
        let rows = sqlx::query!(
            r#"
            SELECT ad.id, ad.fingerprint, ad.name, ad.platform, ad.app_version,
                   ad.first_authenticated_by, ad.first_authenticated_at, ad.trusted_until,
                   ad.last_seen_at, ad.revoked, ad.revoked_by, ad.revoked_at, ad.metadata
            FROM authenticated_devices ad
            INNER JOIN device_user_credentials duc ON ad.id = duc.device_id
            WHERE duc.user_id = $1
            AND ad.revoked = false
            ORDER BY ad.last_seen_at DESC
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| crate::auth::AuthenticatedDevice {
                id: r.id,
                fingerprint: r.fingerprint,
                name: r.name,
                platform: serde_json::from_str(&r.platform)
                    .unwrap_or(crate::auth::Platform::Unknown),
                app_version: r.app_version,
                first_authenticated_by: r.first_authenticated_by,
                first_authenticated_at: r.first_authenticated_at,
                trusted_until: r.trusted_until,
                last_seen_at: r.last_seen_at,
                revoked: r.revoked,
                revoked_by: r.revoked_by,
                revoked_at: r.revoked_at,
                metadata: r.metadata.unwrap_or(serde_json::json!({})),
            })
            .collect())
    }

    async fn update_device(
        &self,
        device_id: Uuid,
        updates: &crate::auth::DeviceUpdateParams,
    ) -> Result<()> {
        let mut query_builder = sqlx::QueryBuilder::new("UPDATE authenticated_devices SET ");
        let mut first = true;

        if let Some(name) = &updates.name {
            if !first {
                query_builder.push(", ");
            }
            query_builder.push("name = ");
            query_builder.push_bind(name);
            first = false;
        }

        if let Some(app_version) = &updates.app_version {
            if !first {
                query_builder.push(", ");
            }
            query_builder.push("app_version = ");
            query_builder.push_bind(app_version);
            first = false;
        }

        if let Some(last_seen_at) = &updates.last_seen_at {
            if !first {
                query_builder.push(", ");
            }
            query_builder.push("last_seen_at = ");
            query_builder.push_bind(last_seen_at);
            first = false;
        }

        if let Some(trusted_until) = &updates.trusted_until {
            if !first {
                query_builder.push(", ");
            }
            query_builder.push("trusted_until = ");
            query_builder.push_bind(trusted_until);
            first = false;
        }

        if !first {
            query_builder.push(", updated_at = NOW()");
        }

        query_builder.push(" WHERE id = ");
        query_builder.push_bind(device_id);

        query_builder.build().execute(&self.pool).await?;

        Ok(())
    }

    async fn revoke_device(&self, device_id: Uuid, revoked_by: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE authenticated_devices
            SET revoked = true, revoked_by = $2, revoked_at = NOW()
            WHERE id = $1
            "#,
            device_id,
            revoked_by
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn upsert_device_credential(
        &self,
        credential: &crate::auth::DeviceUserCredential,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO device_user_credentials
            (user_id, device_id, pin_hash, pin_set_at, pin_last_used_at,
             failed_attempts, locked_until, auto_login_enabled, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (user_id, device_id) DO UPDATE SET
                pin_hash = EXCLUDED.pin_hash,
                pin_set_at = EXCLUDED.pin_set_at,
                pin_last_used_at = EXCLUDED.pin_last_used_at,
                failed_attempts = EXCLUDED.failed_attempts,
                locked_until = EXCLUDED.locked_until,
                auto_login_enabled = EXCLUDED.auto_login_enabled,
                updated_at = EXCLUDED.updated_at
            "#,
            credential.user_id,
            credential.device_id,
            credential.pin_hash,
            credential.pin_set_at,
            credential.pin_last_used_at,
            credential.failed_attempts,
            credential.locked_until,
            credential.auto_login_enabled,
            credential.created_at,
            credential.updated_at
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_device_credential(
        &self,
        user_id: Uuid,
        device_id: Uuid,
    ) -> Result<Option<crate::auth::DeviceUserCredential>> {
        let row = sqlx::query!(
            r#"
            SELECT user_id, device_id, pin_hash, pin_set_at, pin_last_used_at,
                   failed_attempts, locked_until, auto_login_enabled, created_at, updated_at
            FROM device_user_credentials
            WHERE user_id = $1 AND device_id = $2
            "#,
            user_id,
            device_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| crate::auth::DeviceUserCredential {
            user_id: r.user_id,
            device_id: r.device_id,
            pin_hash: r.pin_hash,
            pin_set_at: r.pin_set_at,
            pin_last_used_at: r.pin_last_used_at,
            failed_attempts: r.failed_attempts,
            locked_until: r.locked_until,
            auto_login_enabled: r.auto_login_enabled,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    async fn update_device_pin(
        &self,
        user_id: Uuid,
        device_id: Uuid,
        pin_hash: &str,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE device_user_credentials
            SET pin_hash = $3, pin_set_at = NOW(), failed_attempts = 0, locked_until = NULL, updated_at = NOW()
            WHERE user_id = $1 AND device_id = $2
            "#,
            user_id,
            device_id,
            pin_hash
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_device_failed_attempts(
        &self,
        user_id: Uuid,
        device_id: Uuid,
        attempts: i32,
        locked_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE device_user_credentials
            SET failed_attempts = $3, locked_until = $4, updated_at = NOW()
            WHERE user_id = $1 AND device_id = $2
            "#,
            user_id,
            device_id,
            attempts,
            locked_until
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn create_device_session(
        &self,
        session: &crate::auth::SessionDeviceSession,
    ) -> Result<()> {
        // Token is already hashed by the caller
        sqlx::query!(
            r#"
            INSERT INTO sessions
            (id, token_hash, user_id, device_id, created_at, expires_at,
             last_activity, ip_address, user_agent, revoked, revoked_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
            session.id,
            session.session_token, // Already hashed by caller
            session.user_id,
            session.device_id,
            session.created_at,
            session.expires_at,
            session.last_activity,
            session
                .ip_address
                .as_ref()
                .and_then(|ip| sqlx::types::ipnetwork::IpNetwork::from_str(ip).ok()),
            session.user_agent,
            session.revoked,
            session.revoked_at
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_device_sessions(
        &self,
        device_id: Uuid,
    ) -> Result<Vec<crate::auth::SessionDeviceSession>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, user_id, device_id, created_at, expires_at,
                   last_activity, ip_address, user_agent, revoked, revoked_at
            FROM sessions
            WHERE device_id = $1
            ORDER BY created_at DESC
            "#,
            device_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| crate::auth::SessionDeviceSession {
                id: r.id,
                user_id: r.user_id,
                device_id: r.device_id,
                session_token: String::new(), // Token hash is not returned for security
                created_at: r.created_at,
                expires_at: r.expires_at,
                last_activity: r.last_activity,
                ip_address: r.ip_address.map(|ip| ip.to_string()),
                user_agent: r.user_agent,
                revoked: r.revoked,
                revoked_at: r.revoked_at,
            })
            .collect())
    }

    async fn revoke_device_sessions(&self, device_id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE sessions
            SET revoked = true, revoked_at = NOW()
            WHERE device_id = $1 AND NOT revoked
            "#,
            device_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn log_auth_event(&self, event: &crate::auth::AuthEvent) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO auth_events
            (id, user_id, device_id, event_type, success, failure_reason,
             ip_address, user_agent, metadata, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            event.id,
            event.user_id,
            event.device_id,
            event.event_type.as_str(),
            event.success,
            event.failure_reason,
            event
                .ip_address
                .as_ref()
                .and_then(|ip| sqlx::types::ipnetwork::IpNetwork::from_str(ip).ok()),
            event.user_agent,
            event.metadata,
            event.created_at
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_user_auth_events(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<crate::auth::AuthEvent>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, user_id, device_id, event_type, success, failure_reason,
                   ip_address, user_agent, metadata, created_at
            FROM auth_events
            WHERE user_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
            user_id,
            limit as i64
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                crate::auth::AuthEventType::from_str(&r.event_type).map(|event_type| {
                    crate::auth::AuthEvent {
                        id: r.id,
                        user_id: r.user_id,
                        device_id: r.device_id,
                        event_type,
                        success: r.success,
                        failure_reason: r.failure_reason,
                        ip_address: r.ip_address.map(|ip| ip.to_string()),
                        user_agent: r.user_agent,
                        metadata: r.metadata.unwrap_or(serde_json::json!({})),
                        created_at: r.created_at,
                    }
                })
            })
            .collect())
    }

    async fn get_device_auth_events(
        &self,
        device_id: Uuid,
        limit: usize,
    ) -> Result<Vec<crate::auth::AuthEvent>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, user_id, device_id, event_type, success, failure_reason,
                   ip_address, user_agent, metadata, created_at
            FROM auth_events
            WHERE device_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
            device_id,
            limit as i64
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                crate::auth::AuthEventType::from_str(&r.event_type).map(|event_type| {
                    crate::auth::AuthEvent {
                        id: r.id,
                        user_id: r.user_id,
                        device_id: r.device_id,
                        event_type,
                        success: r.success,
                        failure_reason: r.failure_reason,
                        ip_address: r.ip_address.map(|ip| ip.to_string()),
                        user_agent: r.user_agent,
                        metadata: r.metadata.unwrap_or(serde_json::json!({})),
                        created_at: r.created_at,
                    }
                })
            })
            .collect())
    }

    async fn get_library_media_references(
        &self,
        library_id: LibraryID,
        library_type: LibraryType,
    ) -> Result<Vec<Media>> {
        let mut media = Vec::new();
        match library_type {
            LibraryType::Movies => {
                let rows = sqlx::query!(
                    r#"
                    SELECT
                        mr.id, mr.tmdb_id, mr.title, mr.theme_color,
                        mf.id as file_id, mf.file_path, mf.filename, mf.file_size, mf.created_at as file_created_at,
                        mf.technical_metadata, mf.parsed_info,
                        mm.tmdb_details as "tmdb_details?"
                    FROM movie_references mr
                    JOIN media_files mf ON mr.file_id = mf.id
                    LEFT JOIN movie_metadata mm ON mr.id = mm.movie_id
                    WHERE mr.library_id = $1
                    ORDER BY mr.title
                    "#,
                    library_id.as_uuid()
                )
                .fetch_all(&self.pool)
                .await
                .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

                for row in rows {
                    let technical_metadata: Option<serde_json::Value> = row.technical_metadata;
                    let media_file_metadata = technical_metadata
                        .map(|tm| serde_json::from_value(tm))
                        .transpose()
                        .map_err(|e| {
                            MediaError::Internal(format!("Failed to deserialize metadata: {}", e))
                        })?;

                    let media_file = MediaFile {
                        id: row.file_id,
                        path: PathBuf::from(row.file_path),
                        filename: row.filename,
                        size: row.file_size as u64,
                        created_at: row.file_created_at,
                        media_file_metadata,
                        library_id: library_id,
                    };

                    // Build metadata if available
                    let details = if let Some(tmdb_json) = row.tmdb_details {
                        match serde_json::from_value::<EnhancedMovieDetails>(tmdb_json) {
                            Ok(metadata_details) => {
                                MediaDetailsOption::Details(TmdbDetails::Movie(metadata_details))
                            }
                            Err(e) => {
                                warn!("Failed to deserialize movie metadata: {}", e);
                                MediaDetailsOption::Endpoint(format!("/movie/{}", row.id))
                            }
                        }
                    } else {
                        MediaDetailsOption::Endpoint(format!("/movie/{}", row.id))
                    };

                    let movie_ref = Media::Movie(MovieReference {
                        id: MovieID(row.id),
                        library_id,
                        tmdb_id: row.tmdb_id as u64,
                        title: MovieTitle::new(row.title)?,
                        details,
                        endpoint: MovieURL::from_string(format!("/stream/{}", row.file_id)),
                        file: media_file,
                        theme_color: row.theme_color,
                    });

                    media.push(movie_ref);
                }
            }
            LibraryType::Series => {
                // Execute bulk queries in parallel using tokio::join!
                let (series_result, seasons_result, episodes_result) = tokio::join!(
                    self.get_library_series(&library_id),
                    self.get_library_seasons(&library_id),
                    self.get_library_episodes(&library_id)
                );
                if let Ok(series) = series_result {
                    media.par_extend(
                        series
                            .into_par_iter()
                            .map(|series_ref| Media::Series(series_ref)),
                    );
                }
                if let Ok(seasons) = seasons_result {
                    media.par_extend(
                        seasons
                            .into_par_iter()
                            .map(|season_ref| Media::Season(season_ref)),
                    );
                }
                if let Ok(episodes) = episodes_result {
                    media.par_extend(
                        episodes
                            .into_par_iter()
                            .map(|episode_ref| Media::Episode(episode_ref)),
                    );
                }
            }
        }

        Ok(media)
    }

    // Bulk reference retrieval methods for performance
    async fn get_movie_references_bulk(&self, ids: &[&MovieID]) -> Result<Vec<MovieReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();
        let uuids = uuids;

        // Build query with ANY clause
        let rows = sqlx::query!(
            r#"
            SELECT
                mr.id, mr.tmdb_id, mr.title, mr.theme_color, mr.library_id,
                mf.id as file_id, mf.file_path, mf.filename, mf.file_size, mf.created_at as file_created_at,
                mf.technical_metadata, mf.parsed_info,
                mm.tmdb_details as "tmdb_details?"
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            LEFT JOIN movie_metadata mm ON mr.id = mm.movie_id
            WHERE mr.id = ANY($1)
            "#,
            &uuids
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut movies = Vec::new();
        for row in rows {
            // Build MediaFile
            let technical_metadata: Option<serde_json::Value> = row.technical_metadata; // TODO: See if we can optimize this with rkyv
            let media_file_metadata = technical_metadata
                .map(|tm| serde_json::from_value(tm))
                .transpose()
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to deserialize metadata: {}", e))
                })?;

            let media_file = MediaFile {
                id: row.file_id,
                path: PathBuf::from(row.file_path),
                filename: row.filename,
                size: row.file_size as u64,
                created_at: row.file_created_at,
                media_file_metadata,
                library_id: LibraryID(row.library_id),
            };

            // Build metadata if available
            let details = if let Some(tmdb_json) = row.tmdb_details {
                match serde_json::from_value::<EnhancedMovieDetails>(tmdb_json) {
                    Ok(metadata_details) => {
                        MediaDetailsOption::Details(TmdbDetails::Movie(metadata_details))
                    }
                    Err(e) => {
                        warn!("Failed to deserialize movie metadata: {}", e);
                        MediaDetailsOption::Endpoint(format!("/movie/{}", row.id))
                    }
                }
            } else {
                MediaDetailsOption::Endpoint(format!("/movie/{}", row.id))
            };

            let movie_ref = MovieReference {
                id: MovieID(row.id),
                library_id: LibraryID(row.library_id),
                tmdb_id: row.tmdb_id as u64,
                title: MovieTitle::new(row.title)?,
                details,
                endpoint: MovieURL::from_string(format!("/stream/{}", row.file_id)),
                file: media_file,
                theme_color: row.theme_color,
            };

            movies.push(movie_ref);
        }

        Ok(movies)
    }

    async fn get_series_references_bulk(&self, ids: &[&SeriesID]) -> Result<Vec<SeriesReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();

        // Fetch series references with metadata
        let rows = sqlx::query!(
            r#"
            SELECT
                sr.id, sr.library_id, sr.tmdb_id as "tmdb_id?", sr.title, sr.theme_color, sr.created_at,
                sm.tmdb_details as "tmdb_details?"
            FROM series_references sr
            LEFT JOIN series_metadata sm ON sr.id = sm.series_id
            WHERE sr.id = ANY($1)
            "#,
            &uuids
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut series_list = Vec::new();
        for row in rows {
            // Extract required fields - these are non-nullable in the query
            let row_id = row.id;
            let library_id = row.library_id;
            let title = row.title;
            let created_at = row.created_at;

            // Build the details field
            let details = match row.tmdb_details {
                Some(metadata) if !metadata.is_null() => {
                    match serde_json::from_value::<EnhancedSeriesDetails>(metadata) {
                        Ok(series_details) => {
                            MediaDetailsOption::Details(TmdbDetails::Series(series_details))
                        }
                        Err(e) => {
                            warn!("Failed to deserialize series metadata: {}", e);
                            MediaDetailsOption::Endpoint(format!("/series/{}", row_id))
                        }
                    }
                }
                _ => MediaDetailsOption::Endpoint(format!("/series/{}", row_id)),
            };

            let tmdb_id = row.tmdb_id.unwrap_or(0) as u64;

            series_list.push(SeriesReference {
                id: SeriesID(row_id),
                library_id: LibraryID(library_id),
                tmdb_id,
                title: SeriesTitle::new(title)?,
                details,
                endpoint: SeriesURL::from_string(format!("/series/{}", row_id)),
                created_at,
                theme_color: row.theme_color,
            });
        }

        Ok(series_list)
    }

    async fn get_library_series(&self, library_id: &LibraryID) -> Result<Vec<SeriesReference>> {
        // Build query with ANY clause
        let rows = sqlx::query!(
            r#"
            SELECT
                sr.id, sr.library_id, sr.tmdb_id as "tmdb_id?", sr.title, sr.theme_color, sr.created_at,
                sm.tmdb_details as "tmdb_details?"
            FROM series_references sr
            LEFT JOIN series_metadata sm ON sr.id = sm.series_id
            WHERE sr.library_id = $1
            ORDER BY sr.title
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut series_list = Vec::new();
        for row in rows {
            // Extract required fields - these are non-nullable in the query
            let row_id = row.id;
            let library_id = row.library_id;
            let title = row.title;
            let created_at = row.created_at;

            // Build the details field
            let details = match row.tmdb_details {
                Some(metadata) if !metadata.is_null() => {
                    match serde_json::from_value::<EnhancedSeriesDetails>(metadata) {
                        Ok(series_details) => {
                            MediaDetailsOption::Details(TmdbDetails::Series(series_details))
                        }
                        Err(e) => {
                            warn!("Failed to deserialize series metadata: {}", e);
                            MediaDetailsOption::Endpoint(format!("/series/{}", row_id))
                        }
                    }
                }
                _ => MediaDetailsOption::Endpoint(format!("/series/{}", row_id)),
            };

            let tmdb_id = row.tmdb_id.unwrap_or(0) as u64;

            series_list.push(SeriesReference {
                id: SeriesID(row_id),
                library_id: LibraryID(library_id),
                tmdb_id,
                title: SeriesTitle::new(title)?,
                details,
                endpoint: SeriesURL::from_string(format!("/series/{}", row_id)),
                created_at,
                theme_color: row.theme_color,
            });
        }

        Ok(series_list)
    }

    async fn get_season_references_bulk(&self, ids: &[&SeasonID]) -> Result<Vec<SeasonReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();

        let rows = sqlx::query!(
            r#"
            SELECT
                sr.id, sr.series_id, sr.season_number, sr.library_id, sr.tmdb_series_id, sr.created_at,
                sm.tmdb_details
            FROM season_references sr
            LEFT JOIN season_metadata sm ON sr.id = sm.season_id
            WHERE sr.id = ANY($1)
            "#,
            &uuids
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get seasons: {}", e)))?;

        let mut seasons = Vec::new();
        for row in rows {
            // Parse TMDB details if available
            let details = match row.tmdb_details {
                None => MediaDetailsOption::Endpoint(format!("/media/{}", row.id)),
                Some(tmdb_json) => match serde_json::from_value::<SeasonDetails>(tmdb_json) {
                    Ok(season_details) => {
                        MediaDetailsOption::Details(TmdbDetails::Season(season_details))
                    }
                    Err(e) => {
                        warn!("Failed to parse season TMDB details: {}", e);
                        MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
                    }
                },
            };

            seasons.push(SeasonReference {
                id: SeasonID(row.id),
                season_number: SeasonNumber::new(row.season_number as u8),
                series_id: SeriesID(row.series_id),
                library_id: LibraryID(row.library_id),
                tmdb_series_id: row.tmdb_series_id as u64,
                details,
                endpoint: SeasonURL::from_string(format!("/media/{}", row.id)),
                created_at: row.created_at,
                theme_color: None,
            });
        }

        Ok(seasons)
    }

    async fn get_library_seasons(&self, library_id: &LibraryID) -> Result<Vec<SeasonReference>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                sr.id, sr.series_id, sr.season_number, sr.library_id, sr.tmdb_series_id, sr.created_at,
                sm.tmdb_details as "tmdb_details?"
            FROM season_references sr
            LEFT JOIN season_metadata sm ON sr.id = sm.season_id
            WHERE sr.library_id = $1
            ORDER BY sr.series_id, sr.season_number
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get seasons: {}", e)))?;

        let mut seasons = Vec::new();
        for row in rows {
            // Parse TMDB details if available
            let details = match row.tmdb_details {
                None => MediaDetailsOption::Endpoint(format!("/media/{}", row.id)),
                Some(tmdb_json) => match serde_json::from_value::<SeasonDetails>(tmdb_json) {
                    Ok(season_details) => {
                        MediaDetailsOption::Details(TmdbDetails::Season(season_details))
                    }
                    Err(e) => {
                        warn!("Failed to parse season TMDB details: {}", e);
                        MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
                    }
                },
            };

            seasons.push(SeasonReference {
                id: SeasonID(row.id),
                season_number: SeasonNumber::new(row.season_number as u8),
                series_id: SeriesID(row.series_id),
                library_id: LibraryID(row.library_id),
                tmdb_series_id: row.tmdb_series_id as u64,
                details,
                endpoint: SeasonURL::from_string(format!("/media/{}", row.id)),
                created_at: row.created_at,
                theme_color: None,
            });
        }

        Ok(seasons)
    }

    async fn get_episode_references_bulk(
        &self,
        ids: &[&EpisodeID],
    ) -> Result<Vec<EpisodeReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();

        let rows = sqlx::query!(
            r#"
            SELECT
                er.id, er.episode_number, er.season_number, er.season_id, er.series_id,
                er.tmdb_series_id, er.file_id,
                em.tmdb_details,
                mf.id as media_file_id, mf.file_path, mf.filename, mf.file_size,
                mf.created_at as file_created_at, mf.technical_metadata, mf.library_id
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            LEFT JOIN episode_metadata em ON er.id = em.episode_id
            WHERE er.id = ANY($1)
            "#,
            &uuids
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get episodes: {}", e)))?;

        let mut episodes = Vec::new();
        for row in rows {
            // Parse technical metadata
            let technical_metadata: Option<serde_json::Value> = row.technical_metadata;
            let parsed_metadata = technical_metadata
                .and_then(|tm| serde_json::from_value::<MediaFileMetadata>(tm).ok());

            // Create media file
            let media_file = MediaFile {
                id: row.media_file_id,
                path: PathBuf::from(&row.file_path),
                filename: row.filename.clone(),
                size: row.file_size as u64,
                created_at: row.file_created_at,
                media_file_metadata: parsed_metadata,
                library_id: LibraryID(row.library_id),
            };

            // Parse TMDB details if available
            let details = match row.tmdb_details {
                None => MediaDetailsOption::Endpoint(format!("/media/{}", row.id)),
                Some(tmdb_json) => match serde_json::from_value::<EpisodeDetails>(tmdb_json) {
                    Ok(episode_details) => {
                        MediaDetailsOption::Details(TmdbDetails::Episode(episode_details))
                    }
                    Err(e) => {
                        warn!("Failed to parse episode TMDB details: {}", e);
                        MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
                    }
                },
            };

            episodes.push(EpisodeReference {
                id: EpisodeID(row.id),
                library_id: LibraryID(row.library_id),
                series_id: SeriesID(row.series_id),
                season_id: SeasonID(row.season_id),
                season_number: SeasonNumber::new(row.season_number as u8),
                episode_number: EpisodeNumber::new(row.episode_number as u8),
                tmdb_series_id: row.tmdb_series_id as u64,
                details,
                endpoint: EpisodeURL::from_string(format!("/stream/{}", row.file_id)),
                file: media_file,
            });
        }

        Ok(episodes)
    }

    async fn get_library_episodes(&self, library_id: &LibraryID) -> Result<Vec<EpisodeReference>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                er.id, er.episode_number, er.season_number, er.season_id, er.series_id,
                er.tmdb_series_id, er.file_id,
                em.tmdb_details as "tmdb_details?",
                mf.id as media_file_id, mf.file_path, mf.filename, mf.file_size,
                mf.created_at as file_created_at, mf.technical_metadata, mf.library_id
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            LEFT JOIN episode_metadata em ON er.id = em.episode_id
            WHERE mf.library_id = $1
            ORDER BY er.series_id ASC, er.season_number ASC, er.episode_number ASC
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get episodes: {}", e)))?;

        let mut episodes = Vec::new();
        for row in rows {
            // Parse technical metadata
            let technical_metadata: Option<serde_json::Value> = row.technical_metadata;
            let parsed_metadata = technical_metadata
                .and_then(|tm| serde_json::from_value::<MediaFileMetadata>(tm).ok());

            // Create media file
            let media_file = MediaFile {
                id: row.media_file_id,
                path: PathBuf::from(&row.file_path),
                filename: row.filename.clone(),
                size: row.file_size as u64,
                created_at: row.file_created_at,
                media_file_metadata: parsed_metadata,
                library_id: LibraryID(row.library_id),
            };

            // Parse TMDB details if available
            let details = match row.tmdb_details {
                None => MediaDetailsOption::Endpoint(format!("/media/{}", row.id)),
                Some(tmdb_json) => match serde_json::from_value::<EpisodeDetails>(tmdb_json) {
                    Ok(episode_details) => {
                        MediaDetailsOption::Details(TmdbDetails::Episode(episode_details))
                    }
                    Err(e) => {
                        warn!("Failed to parse episode TMDB details: {}", e);
                        MediaDetailsOption::Endpoint(format!("/media/{}", row.id))
                    }
                },
            };

            episodes.push(EpisodeReference {
                id: EpisodeID(row.id),
                library_id: LibraryID(row.library_id),
                series_id: SeriesID(row.series_id),
                season_id: SeasonID(row.season_id),
                season_number: SeasonNumber::new(row.season_number as u8),
                episode_number: EpisodeNumber::new(row.episode_number as u8),
                tmdb_series_id: row.tmdb_series_id as u64,
                details,
                endpoint: EpisodeURL::from_string(format!("/stream/{}", row.file_id)),
                file: media_file,
            });
        }

        Ok(episodes)
    }

    // Folder inventory management methods
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

impl PostgresDatabase {
    /// Store SeriesReference with enhanced metadata (helper method)
    async fn store_series_reference_complete(
        &self,
        series: &SeriesReference,
        metadata: Option<&EnhancedSeriesDetails>,
    ) -> Result<()> {
        let series_uuid = series.id.as_uuid();

        // Convert tmdb_id, treating 0 as None (no TMDB match)
        let tmdb_id = if series.tmdb_id > 0 {
            Some(series.tmdb_id as i64)
        } else {
            None
        };

        // Start a transaction
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Transaction failed: {}", e)))?;

        // Handle the different conflict scenarios:
        // 1. If tmdb_id is None, only conflict on id
        // 2. If tmdb_id is Some, we need to handle both id and (tmdb_id, library_id) conflicts
        if tmdb_id.is_none() {
            // Series without TMDB match - simple insert/update on id conflict
            info!("Storing series without TMDB ID: {}", series.title.as_str());
            sqlx::query!(
                r#"
                INSERT INTO series_references (id, library_id, tmdb_id, title, theme_color, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
                ON CONFLICT (id) DO UPDATE SET
                    library_id = EXCLUDED.library_id,
                    title = EXCLUDED.title,
                    theme_color = EXCLUDED.theme_color,
                    updated_at = NOW()
                "#,
                series_uuid,
                series.library_id.as_uuid(),
                tmdb_id,
                series.title.as_str(),
                series.theme_color.as_deref()
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                error!("SQL error storing series without TMDB ID: {}", e);
                MediaError::Internal(format!("Failed to store series reference: {}", e))
            })?;
            info!(
                "Successfully stored series without TMDB ID: {}",
                series.title.as_str()
            );
        } else {
            // Series with TMDB match - handle both possible conflicts
            info!(
                "Storing series with TMDB ID {}: {}",
                tmdb_id.unwrap(),
                series.title.as_str()
            );
            // First check if a series with this tmdb_id already exists
            let existing = sqlx::query!(
                r#"
                SELECT id FROM series_references
                WHERE tmdb_id = $1 AND library_id = $2
                "#,
                tmdb_id,
                series.library_id.as_uuid()
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to check existing series: {}", e)))?;

            if let Some(existing_row) = existing {
                if &existing_row.id != series_uuid {
                    // Different ID - this means find_or_create_series didn't find the existing series
                    // This is a logic error - the scanner should have used the existing series ID
                    return Err(MediaError::Internal(format!(
                        "Series with TMDB ID {} already exists in library {} with different ID. Scanner should use existing series.",
                        tmdb_id.unwrap_or(0),
                        series.library_id
                    )));
                } else {
                    // Same ID - regular upsert
                    sqlx::query!(
                        r#"
                        INSERT INTO series_references (id, library_id, tmdb_id, title, theme_color, created_at, updated_at)
                        VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
                        ON CONFLICT (id) DO UPDATE SET
                            library_id = EXCLUDED.library_id,
                            tmdb_id = EXCLUDED.tmdb_id,
                            title = EXCLUDED.title,
                            theme_color = EXCLUDED.theme_color,
                            updated_at = NOW()
                        "#,
                        series_uuid,
                        series.library_id.as_uuid(),
                        tmdb_id,
                        series.title.as_str(),
                        series.theme_color.as_deref()
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| MediaError::Internal(format!("Failed to store series reference: {}", e)))?;
                }
            } else {
                // No existing series with this tmdb_id - safe to insert
                sqlx::query!(
                    r#"
                    INSERT INTO series_references (id, library_id, tmdb_id, title, theme_color, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
                    ON CONFLICT (id) DO UPDATE SET
                        library_id = EXCLUDED.library_id,
                        tmdb_id = EXCLUDED.tmdb_id,
                        title = EXCLUDED.title,
                        theme_color = EXCLUDED.theme_color,
                        updated_at = NOW()
                    "#,
                    series_uuid,
                    series.library_id.as_uuid(),
                    tmdb_id,
                    series.title.as_str(),
                    series.theme_color.as_deref()
                )
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    error!("SQL error storing series with TMDB ID: {}", e);
                    MediaError::Internal(format!("Failed to store series reference: {}", e))
                })?;
                info!(
                    "Successfully stored series with TMDB ID {}: {}",
                    tmdb_id.unwrap(),
                    series.title.as_str()
                );
            }
        }

        // Store metadata if provided
        if let Some(details) = metadata {
            info!(
                "Storing TMDB metadata for series: {} (overview: {})",
                series.title.as_str(),
                details.overview.as_deref().unwrap_or("None")
            );

            let tmdb_json = serde_json::to_value(details).map_err(|e| {
                MediaError::Internal(format!("Failed to serialize metadata: {}", e))
            })?;

            // Extract arrays from the details structure
            let images_json = serde_json::json!({
                "posters": details.images.posters,
                "backdrops": details.images.backdrops,
                "logos": details.images.logos
            });

            let cast_crew_json = serde_json::json!({
                "cast": details.cast,
                "crew": details.crew
            });

            let videos_json = serde_json::to_value(&details.videos)
                .map_err(|e| MediaError::Internal(format!("Failed to serialize videos: {}", e)))?;

            let keywords: Vec<String> = details.keywords.clone();

            let external_ids_json = serde_json::to_value(&details.external_ids).map_err(|e| {
                MediaError::Internal(format!("Failed to serialize external IDs: {}", e))
            })?;

            sqlx::query!(
                r#"
                INSERT INTO series_metadata (
                    series_id, tmdb_details, images, cast_crew, videos, keywords, external_ids,
                    created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())
                ON CONFLICT (series_id) DO UPDATE SET
                    tmdb_details = EXCLUDED.tmdb_details,
                    images = EXCLUDED.images,
                    cast_crew = EXCLUDED.cast_crew,
                    videos = EXCLUDED.videos,
                    keywords = EXCLUDED.keywords,
                    external_ids = EXCLUDED.external_ids,
                    updated_at = NOW()
                "#,
                series_uuid,
                tmdb_json,
                images_json,
                cast_crew_json,
                videos_json,
                &keywords,
                external_ids_json
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                error!("SQL error storing series metadata: {}", e);
                MediaError::Internal(format!("Failed to store series metadata: {}", e))
            })?;

            info!(
                "Successfully stored TMDB metadata for series: {}",
                series.title.as_str()
            );
        }

        // Commit the transaction
        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit transaction: {}", e)))?;

        Ok(())
    }
}
