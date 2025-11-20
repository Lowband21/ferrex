use super::traits::*;
use crate::{
    auth::{
        AuthEvent, AuthEventType,
        device::{AuthDeviceStatus, AuthenticatedDevice, DeviceUpdateParams, Platform},
    },
    database::{
        infrastructure::postgres::repositories::{
            folder_inventory::PostgresFolderInventoryRepository,
            library::PostgresLibraryRepository, media::PostgresMediaRepository,
            processing_status::PostgresProcessingStatusRepository, rbac::PostgresRbacRepository,
            sync_sessions::PostgresSyncSessionsRepository, users::PostgresUsersRepository,
            watch_status::PostgresWatchStatusRepository,
        },
        ports::{
            folder_inventory::FolderInventoryRepository, library::LibraryRepository,
            processing_status::ProcessingStatusRepository, rbac::RbacRepository,
            sync_sessions::SyncSessionsRepository, users::UsersRepository,
            watch_status::WatchStatusRepository,
        },
        postgres_ext::tmdb_metadata::TmdbMetadataRepository,
    },
    error::{MediaError, Result},
    fs_watch::event_bus::PostgresFileChangeEventBus,
    image::{
        MediaImageKind,
        records::{MediaImageVariantKey, MediaImageVariantRecord},
    },
    query::types::{MediaQuery, MediaWithStatus},
    rbac::{Permission, Role, UserPermissions},
    sync_session::{Participant, PlaybackState, SyncSession},
    traits::prelude::MediaIDLike,
    types::{
        details::{EnhancedMovieDetails, LibraryReference, MediaDetailsOption, TmdbDetails},
        files::{MediaFile, MediaFileMetadata},
        ids::{EpisodeID, LibraryID, MovieID, SeasonID, SeriesID},
        library::{Library, LibraryType},
        media::{EpisodeReference, Media, MovieReference, SeasonReference, SeriesReference},
        titles::{MovieTitle, SeriesTitle},
        urls::{MovieURL, SeriesURL, UrlLike},
    },
    user::{User, UserSession},
    watch_status::{InProgressItem, UpdateProgressRequest, UserWatchState},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rayon::iter::{IntoParallelIterator, ParallelExtend, ParallelIterator};
use serde_json::{self};
use sqlx::{
    PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
};
use std::path::{Path, PathBuf};
use std::{collections::HashMap, fmt};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Statistics about the connection pool
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub size: u32,
    pub idle: u32,
    pub max_size: u32,
    pub min_idle: u32,
}

#[derive(Clone)]
pub struct PostgresDatabase {
    pool: PgPool,
    max_connections: u32,
    min_connections: u32,
    users: PostgresUsersRepository,
    rbac: PostgresRbacRepository,
    watch_status: PostgresWatchStatusRepository,
    sync_sessions: PostgresSyncSessionsRepository,
    folder_inventory: PostgresFolderInventoryRepository,
    libraries: PostgresLibraryRepository,
    media: PostgresMediaRepository,
    processing_status: PostgresProcessingStatusRepository,
}

impl fmt::Debug for PostgresDatabase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pool_size = self.pool.size();
        let idle = self.pool.num_idle();

        f.debug_struct("PostgresDatabase")
            .field("pool_size", &pool_size)
            .field("idle_connections", &idle)
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .finish()
    }
}

impl PostgresDatabase {
    pub async fn new(connection_string: &str) -> Result<Self> {
        // Get pool configuration from environment or use optimized defaults
        let max_connections = std::env::var("DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(num_cpus::get() as u32);

        let min_connections = std::env::var("DB_MIN_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(5);

        // Configure pool for optimal bulk query performance
        let pg_pool = PgPoolOptions::new()
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

        let pool = pg_pool;
        let users = PostgresUsersRepository::new(pool.clone());
        let rbac = PostgresRbacRepository::new(pool.clone());
        let watch_status = PostgresWatchStatusRepository::new(pool.clone());
        let sync_sessions = PostgresSyncSessionsRepository::new(pool.clone());
        let folder_inventory = PostgresFolderInventoryRepository::new(pool.clone());
        let libraries = PostgresLibraryRepository::new(pool.clone());
        let media = PostgresMediaRepository::new(pool.clone());
        let processing_status = PostgresProcessingStatusRepository::new(pool.clone());

        Ok(PostgresDatabase {
            pool,
            max_connections,
            min_connections,
            users,
            rbac,
            watch_status,
            sync_sessions,
            folder_inventory,
            libraries,
            media,
            processing_status,
        })
    }

    /// Create a PostgresDatabase from an existing pool (mainly for testing)
    pub fn from_pool(pool: PgPool) -> Self {
        // Use default values for test pools
        let max_connections = 20;
        let min_connections = 5;

        let users = PostgresUsersRepository::new(pool.clone());
        let rbac = PostgresRbacRepository::new(pool.clone());
        let watch_status = PostgresWatchStatusRepository::new(pool.clone());
        let sync_sessions = PostgresSyncSessionsRepository::new(pool.clone());
        let folder_inventory = PostgresFolderInventoryRepository::new(pool.clone());
        let libraries = PostgresLibraryRepository::new(pool.clone());
        let media = PostgresMediaRepository::new(pool.clone());
        let processing_status = PostgresProcessingStatusRepository::new(pool.clone());

        PostgresDatabase {
            pool,
            max_connections,
            min_connections,
            users,
            rbac,
            watch_status,
            sync_sessions,
            folder_inventory,
            libraries,
            media,
            processing_status,
        }
    }

    fn build_connect_options(connection_string: &str) -> Result<PgConnectOptions> {
        use tracing::debug;

        let trimmed = connection_string.trim();

        let mut options = if trimmed.is_empty() {
            PgConnectOptions::new()
        } else {
            trimmed.parse::<PgConnectOptions>().map_err(|e| {
                MediaError::Internal(format!("Invalid PostgreSQL connection string: {}", e))
            })?
        };

        if let Ok(db_name) = std::env::var("PGDATABASE")
            && !db_name.is_empty()
        {
            options = options.database(&db_name);
        }

        if let Ok(user) = std::env::var("PGUSER")
            && !user.is_empty()
        {
            options = options.username(&user);
        }

        if let Ok(password) = std::env::var("PGPASSWORD")
            && !password.is_empty()
        {
            options = options.password(&password);
        }

        let mut using_socket = false;

        if let Ok(host) = std::env::var("PGHOST") {
            if !host.is_empty() {
                if host.starts_with('/') {
                    options = options.socket(Path::new(&host));
                    using_socket = true;
                    debug!("Using PostgreSQL socket from PGHOST at {}", host);
                } else {
                    options = options.host(&host);
                    debug!("Using PostgreSQL host from PGHOST: {}", host);
                }
            }
        } else if let Ok(socket_dir) = std::env::var("PG_SOCKET_DIR")
            && !socket_dir.is_empty()
        {
            options = options.socket(Path::new(&socket_dir));
            using_socket = true;
            debug!(
                "Using PostgreSQL socket from PG_SOCKET_DIR at {}",
                socket_dir
            );
        }

        if let Ok(port) = std::env::var("PGPORT")
            && let Ok(port) = port.parse::<u16>()
        {
            options = options.port(port);
        }

        if using_socket && std::env::var("PGSSLMODE").is_err() {
            options = options.ssl_mode(PgSslMode::Disable);
        }

        Ok(options)
    }

    /// Get a reference to the connection pool for use in extension modules
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn file_change_event_bus(&self) -> PostgresFileChangeEventBus {
        PostgresFileChangeEventBus::from_postgres(self)
    }

    pub(crate) fn users_repository(&self) -> &PostgresUsersRepository {
        &self.users
    }

    pub(crate) fn rbac_repository(&self) -> &PostgresRbacRepository {
        &self.rbac
    }

    pub(crate) fn watch_status_repository(&self) -> &PostgresWatchStatusRepository {
        &self.watch_status
    }

    pub(crate) fn sync_sessions_repository(&self) -> &PostgresSyncSessionsRepository {
        &self.sync_sessions
    }

    pub(crate) fn folder_inventory_repository(&self) -> &PostgresFolderInventoryRepository {
        &self.folder_inventory
    }

    pub(crate) fn media_repository(&self) -> &PostgresMediaRepository {
        &self.media
    }

    pub(crate) fn processing_status_repository(&self) -> &PostgresProcessingStatusRepository {
        &self.processing_status
    }

    /// Get connection pool statistics for monitoring
    pub fn pool_stats(&self) -> PoolStats {
        PoolStats {
            size: self.pool.size(),
            idle: self.pool.num_idle() as u32,
            max_size: self.max_connections,
            min_idle: self.min_connections,
        }
    }

    // Repository methods for better organization

    /// Store MovieReference within an existing transaction
    /// Get movie with optional full metadata
    pub async fn get_movie_with_metadata(
        &self,
        id: &MovieID,
        include_metadata: bool,
    ) -> Result<Option<(MovieReference, Option<EnhancedMovieDetails>)>> {
        let movie_uuid = id.to_uuid();

        let row = sqlx::query(
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mr.library_id,
                mf.id AS file_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            WHERE mr.id = $1
            "#,
        )
        .bind(movie_uuid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };

        if include_metadata {
            let repository = TmdbMetadataRepository::new(self.pool());
            let movie_ref = repository.load_movie_reference(row).await?;
            let metadata = match &movie_ref.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(details)) => Some(details.clone()),
                _ => None,
            };

            Ok(Some((movie_ref, metadata)))
        } else {
            let library_id = LibraryID(row.try_get("library_id")?);

            let technical_metadata: Option<serde_json::Value> =
                row.try_get("technical_metadata").ok();
            let media_file_metadata = technical_metadata
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to deserialize metadata: {}", e))
                })?;

            let media_file = MediaFile {
                id: row.try_get("file_id")?,
                path: PathBuf::from(row.try_get::<String, _>("file_path")?),
                filename: row.try_get("filename")?,
                size: row.try_get::<i64, _>("file_size")? as u64,
                discovered_at: row.try_get("file_discovered_at")?,
                created_at: row.try_get("file_created_at")?,
                media_file_metadata,
                library_id,
            };

            let tmdb_id: i64 = row.try_get("tmdb_id")?;
            let title: String = row.try_get("title")?;
            let movie_id: Uuid = row.try_get("id")?;
            let file_id: Uuid = row.try_get("file_id")?;
            let theme_color: Option<String> = row.try_get("theme_color")?;

            let movie_ref = MovieReference {
                id: MovieID(movie_id),
                library_id,
                tmdb_id: tmdb_id as u64,
                title: MovieTitle::new(title.clone()).map_err(|e| {
                    MediaError::Internal(format!("Invalid stored movie title '{}': {}", title, e))
                })?,
                details: MediaDetailsOption::Endpoint(format!("/movie/{}", movie_id)),
                endpoint: MovieURL::from_string(format!("/stream/{}", file_id)),
                file: media_file,
                theme_color,
            };

            Ok(Some((movie_ref, None)))
        }
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
        self.media.store_media(media_file).await
    }

    async fn store_media_batch(&self, media_files: Vec<MediaFile>) -> Result<Vec<Uuid>> {
        self.media.store_media_batch(media_files).await
    }

    async fn get_media(&self, uuid: &Uuid) -> Result<Option<MediaFile>> {
        self.media.get_media(uuid).await
    }

    async fn get_media_by_path(&self, path: &str) -> Result<Option<MediaFile>> {
        self.media.get_media_by_path(path).await
    }

    async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>> {
        self.media.list_media(filters).await
    }

    async fn get_stats(&self) -> Result<MediaStats> {
        self.media.get_stats().await
    }

    async fn file_exists(&self, path: &str) -> Result<bool> {
        self.media.file_exists(path).await
    }

    async fn delete_media(&self, id: &str) -> Result<()> {
        self.media.delete_media(id).await
    }

    async fn get_all_media(&self) -> Result<Vec<MediaFile>> {
        self.media.get_all_media().await
    }

    async fn store_external_metadata(
        &self,
        media_id: &str,
        metadata: &MediaFileMetadata,
    ) -> Result<()> {
        self.media.store_external_metadata(media_id, metadata).await
    }

    // Legacy TV show methods - keeping for compatibility but using new reference system internally
    async fn store_tv_show(&self, _show_info: &TvShowInfo) -> Result<String> {
        // TODO: Convert to new reference system
        Ok(Uuid::now_v7().to_string())
    }

    async fn get_tv_show(&self, _tmdb_id: &str) -> Result<Option<TvShowInfo>> {
        // TODO: Convert from new reference system
        Ok(None)
    }

    // Legacy device auth hook; deprecated. Device lockout state is managed by auth domain repos.
    async fn update_device_failed_attempts(
        &self,
        _user_id: Uuid,
        _device_id: Uuid,
        _attempts: i32,
        _locked_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()> {
        Ok(())
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
        Ok(self.libraries.create_library(library).await?.to_string())
    }

    async fn get_library(&self, library_id: &LibraryID) -> Result<Option<Library>> {
        self.libraries.get_library(*library_id).await
    }

    async fn list_libraries(&self) -> Result<Vec<Library>> {
        self.libraries.list_libraries().await
    }

    async fn update_library(&self, id: &str, library: Library) -> Result<()> {
        let uuid = Uuid::parse_str(id)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;
        self.libraries
            .update_library(LibraryID(uuid), library)
            .await
    }

    async fn delete_library(&self, id: &str) -> Result<()> {
        let uuid = Uuid::parse_str(id)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;
        self.libraries.delete_library(LibraryID(uuid)).await
    }

    async fn update_library_last_scan(&self, uuid: &LibraryID) -> Result<()> {
        self.libraries.update_library_last_scan(*uuid).await
    }

    // Reference type methods
    async fn store_movie_reference(&self, movie: &MovieReference) -> Result<()> {
        TmdbMetadataRepository::new(self.pool())
            .store_movie_reference(movie)
            .await
    }

    async fn store_series_reference(&self, series: &SeriesReference) -> Result<()> {
        TmdbMetadataRepository::new(self.pool())
            .store_series_reference(series)
            .await
    }

    async fn store_season_reference(&self, season: &SeasonReference) -> Result<Uuid> {
        TmdbMetadataRepository::new(self.pool())
            .store_season_reference(season)
            .await
    }

    async fn store_episode_reference(&self, episode: &EpisodeReference) -> Result<()> {
        TmdbMetadataRepository::new(self.pool())
            .store_episode_reference(episode)
            .await
    }

    async fn get_all_movie_references(&self) -> Result<Vec<MovieReference>> {
        let repository = TmdbMetadataRepository::new(self.pool());

        let rows = sqlx::query(
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mf.id AS file_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata,
                mf.library_id
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            ORDER BY mr.title
            "#,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut movies = Vec::with_capacity(rows.len());
        for row in rows {
            let movie_ref = repository.load_movie_reference(row).await?;
            movies.push(movie_ref);
        }

        Ok(movies)
    }

    async fn get_series_references(&self) -> Result<Vec<SeriesReference>> {
        // TODO: Implement series references fetching
        Ok(vec![])
    }

    async fn get_series_seasons(&self, series_id: &SeriesID) -> Result<Vec<SeasonReference>> {
        let series_uuid = series_id.to_uuid();

        info!("Getting seasons for series: {}", series_uuid);

        let rows = sqlx::query(
            r#"
            SELECT
                id,
                series_id,
                season_number,
                library_id,
                tmdb_series_id,
                discovered_at,
                created_at,
                theme_color
            FROM season_references
            WHERE series_id = $1
            ORDER BY season_number
            "#,
        )
        .bind(series_uuid)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get series seasons: {}", e)))?;

        let mut buff = Uuid::encode_buffer();

        info!(
            "Found {} season rows for series {}",
            rows.len(),
            series_id.as_str(&mut buff)
        );

        let repository = TmdbMetadataRepository::new(self.pool());
        let mut seasons = Vec::with_capacity(rows.len());

        for row in rows {
            let season = repository.load_season_reference(row).await?;
            seasons.push(season);
        }

        Ok(seasons)
    }

    async fn get_season_episodes(&self, season_id: &SeasonID) -> Result<Vec<EpisodeReference>> {
        let repository = TmdbMetadataRepository::new(self.pool());

        let rows = sqlx::query(
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            WHERE er.season_id = $1
            ORDER BY er.episode_number
            "#,
        )
        .bind(season_id.to_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get season episodes: {}", e)))?;

        let mut episodes = Vec::with_capacity(rows.len());
        for row in rows {
            let episode = repository.load_episode_reference(row).await?;
            episodes.push(episode);
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
        let series_uuid = id.to_uuid();

        let row = sqlx::query(
            r#"
            SELECT id, library_id, tmdb_id, title, theme_color, discovered_at, created_at
            FROM series_references
            WHERE id = $1
            "#,
        )
        .bind(series_uuid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?
        .ok_or_else(|| MediaError::NotFound("Series not found".to_string()))?;

        let repository = TmdbMetadataRepository::new(self.pool());
        let series_ref = repository.load_series_reference(row).await?;

        Ok(series_ref)
    }

    async fn get_season_reference(&self, id: &SeasonID) -> Result<SeasonReference> {
        let season_uuid = id.to_uuid();

        let row = sqlx::query(
            r#"
            SELECT
                id,
                series_id,
                season_number,
                library_id,
                tmdb_series_id,
                discovered_at,
                created_at,
                theme_color
            FROM season_references
            WHERE id = $1
            "#,
        )
        .bind(season_uuid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?
        .ok_or_else(|| MediaError::NotFound("Season not found".to_string()))?;

        let repository = TmdbMetadataRepository::new(self.pool());
        let season_ref = repository.load_season_reference(row).await?;

        Ok(season_ref)
    }

    async fn get_episode_reference(&self, id: &EpisodeID) -> Result<EpisodeReference> {
        let episode_uuid = id.to_uuid();

        let row = sqlx::query(
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            WHERE er.id = $1
            "#,
        )
        .bind(episode_uuid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?
        .ok_or_else(|| MediaError::NotFound("Episode not found".to_string()))?;

        let repository = TmdbMetadataRepository::new(self.pool());
        let episode_ref = repository.load_episode_reference(row).await?;

        Ok(episode_ref)
    }

    async fn update_movie_tmdb_id(&self, id: &MovieID, tmdb_id: u64) -> Result<()> {
        let movie_uuid = id.to_uuid();

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
        let series_uuid = id.to_uuid();

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
        let repository = TmdbMetadataRepository::new(self.pool());

        let row = sqlx::query(
            r#"
            SELECT id, library_id, tmdb_id, title, theme_color, discovered_at, created_at
            FROM series_references
            WHERE library_id = $1 AND tmdb_id = $2
            "#,
        )
        .bind(library_id.as_uuid())
        .bind(tmdb_id as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        match row {
            Some(row) => {
                let series = repository.load_series_reference(row).await?;
                Ok(Some(series))
            }
            None => Ok(None),
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
            SELECT id, library_id, tmdb_id as "tmdb_id?", title, theme_color, discovered_at, created_at
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
                discovered_at: row.discovered_at,
                created_at: row.created_at.unwrap_or(row.discovered_at),
                theme_color: row.theme_color,
            }))
        } else {
            Ok(None)
        }
    }

    async fn list_library_references(&self) -> Result<Vec<LibraryReference>> {
        self.libraries.list_library_references().await
    }

    async fn get_library_reference(&self, id: Uuid) -> Result<LibraryReference> {
        self.libraries.get_library_reference(id).await
    }

    // Image management methods

    async fn create_image(&self, tmdb_path: &str) -> Result<ImageRecord> {
        let id = Uuid::now_v7();
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
        let id = Uuid::now_v7();
        let now = chrono::Utc::now();

        let row = sqlx::query!(
            r#"
            INSERT INTO image_variants (
                id,
                image_id,
                variant,
                file_path,
                file_size,
                width,
                height,
                created_at,
                downloaded_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
            ON CONFLICT (image_id, variant) DO UPDATE SET
                file_path = EXCLUDED.file_path,
                file_size = EXCLUDED.file_size,
                width = EXCLUDED.width,
                height = EXCLUDED.height,
                downloaded_at = NOW()
            RETURNING id, image_id, variant, file_path, file_size, width, height, created_at, downloaded_at
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
            downloaded_at: row.downloaded_at,
        })
    }

    async fn get_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
    ) -> Result<Option<ImageVariant>> {
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                image_id,
                variant,
                file_path,
                file_size,
                width,
                height,
                created_at,
                downloaded_at
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
            downloaded_at: r.downloaded_at,
        }))
    }

    async fn get_image_variants(&self, image_id: Uuid) -> Result<Vec<ImageVariant>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                image_id,
                variant,
                file_path,
                file_size,
                width,
                height,
                created_at,
                downloaded_at
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
                downloaded_at: r.downloaded_at,
            })
            .collect())
    }

    async fn link_media_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_id: Uuid,
        image_type: MediaImageKind,
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
            image_type.as_str(),
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
                image_type: MediaImageKind::parse(&r.image_type),
                order_index: r.order_index,
                is_primary: r.is_primary,
            })
            .collect())
    }

    async fn get_media_primary_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_type: MediaImageKind,
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
            image_type.as_str()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get primary image: {}", e)))?;

        Ok(row.map(|r| MediaImage {
            media_type: r.media_type,
            media_id: r.media_id,
            image_id: r.image_id,
            image_type: MediaImageKind::parse(&r.image_type),
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
            params.image_type.as_str(),
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

    async fn upsert_media_image_variant(
        &self,
        record: &MediaImageVariantRecord,
    ) -> Result<MediaImageVariantRecord> {
        let key = &record.key;
        let row = sqlx::query!(
            r#"
            INSERT INTO media_image_variants (
                media_type,
                media_id,
                image_type,
                order_index,
                variant,
                cached,
                width,
                height,
                content_hash,
                theme_color,
                requested_at,
                cached_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (media_type, media_id, image_type, order_index, variant) DO UPDATE SET
                cached = EXCLUDED.cached,
                width = EXCLUDED.width,
                height = EXCLUDED.height,
                content_hash = EXCLUDED.content_hash,
                theme_color = EXCLUDED.theme_color,
                requested_at = LEAST(media_image_variants.requested_at, EXCLUDED.requested_at),
                cached_at = COALESCE(EXCLUDED.cached_at, media_image_variants.cached_at)
            RETURNING
                media_type,
                media_id,
                image_type,
                order_index,
                variant,
                cached,
                width,
                height,
                content_hash,
                theme_color,
                requested_at,
                cached_at
            "#,
            key.media_type,
            key.media_id,
            key.image_type.as_str(),
            key.order_index,
            key.variant,
            record.cached,
            record.width,
            record.height,
            record.content_hash.as_deref(),
            record.theme_color.as_deref(),
            record.requested_at,
            record.cached_at
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to upsert media image variant: {}", e))
        })?;

        Ok(MediaImageVariantRecord {
            requested_at: row.requested_at,
            cached_at: row.cached_at,
            cached: row.cached,
            width: row.width,
            height: row.height,
            content_hash: row.content_hash,
            theme_color: row.theme_color,
            key: MediaImageVariantKey {
                media_type: row.media_type,
                media_id: row.media_id,
                image_type: MediaImageKind::parse(&row.image_type),
                order_index: row.order_index,
                variant: row.variant,
            },
        })
    }

    async fn mark_media_image_variant_cached(
        &self,
        key: &MediaImageVariantKey,
        width: Option<i32>,
        height: Option<i32>,
        content_hash: Option<&str>,
        theme_color: Option<&str>,
    ) -> Result<MediaImageVariantRecord> {
        let row = sqlx::query!(
            r#"
            UPDATE media_image_variants
            SET
                cached = true,
                width = COALESCE($5, width),
                height = COALESCE($6, height),
                content_hash = COALESCE($7, content_hash),
                theme_color = COALESCE($8, theme_color),
                cached_at = NOW()
            WHERE media_type = $1
              AND media_id = $2
              AND image_type = $3
              AND order_index = $4
              AND variant = $9
            RETURNING
                media_type,
                media_id,
                image_type,
                order_index,
                variant,
                cached,
                width,
                height,
                content_hash,
                theme_color,
                requested_at,
                cached_at
            "#,
            key.media_type,
            key.media_id,
            key.image_type.as_str(),
            key.order_index,
            width,
            height,
            content_hash,
            theme_color,
            key.variant
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to mark media image variant cached: {}", e))
        })?;

        if let Some(row) = row {
            return Ok(MediaImageVariantRecord {
                requested_at: row.requested_at,
                cached_at: row.cached_at,
                cached: row.cached,
                width: row.width,
                height: row.height,
                content_hash: row.content_hash,
                theme_color: row.theme_color,
                key: MediaImageVariantKey {
                    media_type: row.media_type,
                    media_id: row.media_id,
                    image_type: MediaImageKind::parse(&row.image_type),
                    order_index: row.order_index,
                    variant: row.variant,
                },
            });
        }

        let record = MediaImageVariantRecord {
            requested_at: Utc::now(),
            cached_at: Some(Utc::now()),
            cached: true,
            width,
            height,
            content_hash: content_hash.map(|s| s.to_string()),
            theme_color: theme_color.map(|s| s.to_string()),
            key: key.clone(),
        };

        self.upsert_media_image_variant(&record).await
    }

    async fn list_media_image_variants(
        &self,
        media_type: &str,
        media_id: Uuid,
    ) -> Result<Vec<MediaImageVariantRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                media_type,
                media_id,
                image_type,
                order_index,
                variant,
                cached,
                width,
                height,
                content_hash,
                theme_color,
                requested_at,
                cached_at
            FROM media_image_variants
            WHERE media_type = $1 AND media_id = $2
            ORDER BY image_type, order_index, variant
            "#,
            media_type,
            media_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to list media image variants: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|row| MediaImageVariantRecord {
                requested_at: row.requested_at,
                cached_at: row.cached_at,
                cached: row.cached,
                width: row.width,
                height: row.height,
                content_hash: row.content_hash,
                theme_color: row.theme_color,
                key: MediaImageVariantKey {
                    media_type: row.media_type,
                    media_id: row.media_id,
                    image_type: MediaImageKind::parse(&row.image_type),
                    order_index: row.order_index,
                    variant: row.variant,
                },
            })
            .collect())
    }

    async fn update_media_theme_color(
        &self,
        media_type: &str,
        media_id: Uuid,
        theme_color: Option<&str>,
    ) -> Result<()> {
        match media_type {
            "movie" => {
                sqlx::query!(
                    "UPDATE movie_references SET theme_color = $2 WHERE id = $1",
                    media_id,
                    theme_color
                )
                .execute(&self.pool)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to update movie theme color: {}", e))
                })?;
            }
            "series" => {
                sqlx::query!(
                    "UPDATE series_references SET theme_color = $2 WHERE id = $1",
                    media_id,
                    theme_color
                )
                .execute(&self.pool)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to update series theme color: {}", e))
                })?;
            }
            "season" => {
                sqlx::query!(
                    "UPDATE season_references SET theme_color = $2 WHERE id = $1",
                    media_id,
                    theme_color
                )
                .execute(&self.pool)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to update season theme color: {}", e))
                })?;
            }
            _ => {}
        }

        Ok(())
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
        self.processing_status
            .create_or_update_processing_status(status)
            .await
    }

    async fn get_processing_status(
        &self,
        media_file_id: Uuid,
    ) -> Result<Option<MediaProcessingStatus>> {
        self.processing_status
            .get_processing_status(media_file_id)
            .await
    }

    async fn get_unprocessed_files(
        &self,
        library_id: LibraryID,
        status_type: &str,
        limit: i32,
    ) -> Result<Vec<MediaFile>> {
        self.processing_status
            .get_unprocessed_files(library_id, status_type, limit)
            .await
    }

    async fn get_failed_files(
        &self,
        library_id: LibraryID,
        max_retries: i32,
    ) -> Result<Vec<MediaFile>> {
        self.processing_status
            .get_failed_files(library_id, max_retries)
            .await
    }

    async fn reset_processing_status(&self, media_file_id: Uuid) -> Result<()> {
        self.processing_status
            .reset_processing_status(media_file_id)
            .await
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

    async fn create_user(&self, _user: &User) -> Result<()> {
        // The trait method doesn't include password_hash, so we can't create a user through this interface
        // Users should be created through the authentication system which has access to the password
        Err(MediaError::Internal(
            "Use authentication system to create users with password".to_string(),
        ))
    }

    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>> {
        self.users.get_user_by_id(id).await
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        self.users.get_user_by_username(username).await
    }

    async fn get_all_users(&self) -> Result<Vec<User>> {
        self.users.get_all_users().await
    }

    async fn update_user(&self, user: &User) -> Result<()> {
        self.users.update_user(user).await
    }

    async fn delete_user(&self, id: Uuid) -> Result<()> {
        self.users.delete_user(id).await
    }

    async fn get_user_password_hash(&self, user_id: Uuid) -> Result<Option<String>> {
        self.users.get_user_password_hash(user_id).await
    }

    async fn update_user_password(&self, user_id: Uuid, password_hash: &str) -> Result<()> {
        self.users
            .update_user_password(user_id, password_hash)
            .await
    }

    async fn delete_user_atomic(&self, user_id: Uuid, check_last_admin: bool) -> Result<()> {
        self.users
            .delete_user_atomic(user_id, check_last_admin)
            .await
    }

    // ==================== RBAC Methods ====================

    async fn get_user_permissions(&self, user_id: Uuid) -> Result<UserPermissions> {
        self.rbac.get_user_permissions(user_id).await
    }

    async fn get_all_roles(&self) -> Result<Vec<Role>> {
        self.rbac.get_all_roles().await
    }

    async fn get_all_permissions(&self) -> Result<Vec<Permission>> {
        self.rbac.get_all_permissions().await
    }

    async fn assign_user_role(&self, user_id: Uuid, role_id: Uuid, granted_by: Uuid) -> Result<()> {
        self.rbac
            .assign_user_role(user_id, role_id, granted_by)
            .await
    }

    async fn remove_user_role(&self, user_id: Uuid, role_id: Uuid) -> Result<()> {
        self.rbac.remove_user_role(user_id, role_id).await
    }

    async fn remove_user_role_atomic(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        check_last_admin: bool,
    ) -> Result<()> {
        self.rbac
            .remove_user_role_atomic(user_id, role_id, check_last_admin)
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
        self.rbac
            .override_user_permission(user_id, permission, granted, granted_by, reason)
            .await
    }

    async fn get_admin_count(&self, exclude_user_id: Option<Uuid>) -> Result<usize> {
        self.rbac.get_admin_count(exclude_user_id).await
    }

    async fn user_has_role(&self, user_id: Uuid, role_name: &str) -> Result<bool> {
        self.rbac.user_has_role(user_id, role_name).await
    }

    async fn get_users_with_role(&self, role_name: &str) -> Result<Vec<Uuid>> {
        self.rbac.get_users_with_role(role_name).await
    }

    // ==================== Authentication Methods ====================

    async fn store_refresh_token(
        &self,
        token: &str,
        user_id: Uuid,
        device_name: Option<String>,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        self.users
            .store_refresh_token(token, user_id, device_name, expires_at)
            .await
    }

    async fn get_refresh_token(
        &self,
        token: &str,
    ) -> Result<Option<(Uuid, chrono::DateTime<chrono::Utc>)>> {
        self.users.get_refresh_token(token).await
    }

    async fn delete_refresh_token(&self, token: &str) -> Result<()> {
        self.users.delete_refresh_token(token).await
    }

    async fn delete_user_refresh_tokens(&self, user_id: Uuid) -> Result<()> {
        self.users.delete_user_refresh_tokens(user_id).await
    }

    // ==================== Session Management ====================

    async fn create_session(&self, session: &UserSession) -> Result<()> {
        self.users.create_session(session).await
    }

    async fn get_user_sessions(&self, user_id: Uuid) -> Result<Vec<UserSession>> {
        self.users.get_user_sessions(user_id).await
    }

    async fn delete_session(&self, session_id: Uuid) -> Result<()> {
        self.users.delete_session(session_id).await
    }

    // ==================== Watch Status Methods ====================

    async fn update_watch_progress(
        &self,
        user_id: Uuid,
        progress: &UpdateProgressRequest,
    ) -> Result<()> {
        self.watch_status
            .update_watch_progress(user_id, progress)
            .await
    }

    async fn get_user_watch_state(&self, user_id: Uuid) -> Result<UserWatchState> {
        self.watch_status.get_user_watch_state(user_id).await
    }

    async fn get_continue_watching(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<InProgressItem>> {
        self.watch_status
            .get_continue_watching(user_id, limit)
            .await
    }

    async fn clear_watch_progress(&self, user_id: Uuid, media_id: &Uuid) -> Result<()> {
        self.watch_status
            .clear_watch_progress(user_id, media_id)
            .await
    }

    async fn is_media_completed(&self, user_id: Uuid, media_id: &Uuid) -> Result<bool> {
        self.watch_status
            .is_media_completed(user_id, media_id)
            .await
    }

    // ==================== Sync Session Methods ====================

    async fn create_sync_session(&self, session: &SyncSession) -> Result<()> {
        self.sync_sessions.create_sync_session(session).await
    }

    async fn get_sync_session_by_code(&self, room_code: &str) -> Result<Option<SyncSession>> {
        self.sync_sessions.get_sync_session_by_code(room_code).await
    }

    async fn get_sync_session(&self, id: Uuid) -> Result<Option<SyncSession>> {
        self.sync_sessions.get_sync_session(id).await
    }

    async fn update_sync_session_state(&self, id: Uuid, state: &PlaybackState) -> Result<()> {
        self.sync_sessions
            .update_sync_session_state(id, state)
            .await
    }

    async fn add_sync_participant(
        &self,
        session_id: Uuid,
        participant: &Participant,
    ) -> Result<()> {
        self.sync_sessions
            .add_sync_participant(session_id, participant)
            .await
    }

    async fn remove_sync_participant(&self, session_id: Uuid, user_id: Uuid) -> Result<()> {
        self.sync_sessions
            .remove_sync_participant(session_id, user_id)
            .await
    }

    async fn delete_sync_session(&self, id: Uuid) -> Result<()> {
        self.sync_sessions.delete_sync_session(id).await
    }

    async fn update_sync_session(&self, id: Uuid, session: &SyncSession) -> Result<()> {
        self.sync_sessions.update_sync_session(id, session).await
    }

    async fn end_sync_session(&self, id: Uuid) -> Result<()> {
        self.sync_sessions.end_sync_session(id).await
    }

    async fn cleanup_expired_sync_sessions(&self) -> Result<u32> {
        self.sync_sessions.cleanup_expired_sync_sessions().await
    }

    async fn query_media(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>> {
        <PostgresDatabase>::query_media(self, query).await
    }

    // Legacy device auth methods removed; use auth/domain repositories

    // Legacy device auth method removed

    // Legacy device auth methods removed

    // legacy device credential methods removed; use auth domain repositories

    // legacy device credential methods removed

    // legacy device credential methods removed

    // legacy device credential methods removed

    // Legacy device auth methods removed

    // Legacy device auth methods removed

    // Legacy device auth methods removed

    // Legacy device auth methods removed

    // Legacy device auth methods removed

    // Legacy device auth methods removed

    async fn get_library_media_references(
        &self,
        library_id: LibraryID,
        library_type: LibraryType,
    ) -> Result<Vec<Media>> {
        let mut media = Vec::new();
        match library_type {
            LibraryType::Movies => {
                let repository = TmdbMetadataRepository::new(self.pool());
                let rows = sqlx::query(
                    r#"
                    SELECT
                        mr.id,
                        mr.tmdb_id,
                        mr.title,
                        mr.theme_color,
                        mf.id AS file_id,
                        mf.library_id,
                        mf.file_path,
                        mf.filename,
                        mf.file_size,
                        mf.discovered_at AS file_discovered_at,
                        mf.created_at AS file_created_at,
                        mf.technical_metadata
                    FROM movie_references mr
                    JOIN media_files mf ON mr.file_id = mf.id
                    WHERE mf.library_id = $1
                    ORDER BY mr.title
                    "#,
                )
                .bind(library_id.as_uuid())
                .fetch_all(&self.pool)
                .await
                .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

                for row in rows {
                    let movie = repository.load_movie_reference(row).await?;
                    media.push(Media::Movie(movie));
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
                    media.par_extend(series.into_par_iter().map(Media::Series));
                }
                if let Ok(seasons) = seasons_result {
                    media.par_extend(seasons.into_par_iter().map(Media::Season));
                }
                if let Ok(episodes) = episodes_result {
                    media.par_extend(episodes.into_par_iter().map(Media::Episode));
                }
            }
        }

        Ok(media)
    }

    // Lookup a single movie by file path
    async fn get_movie_reference_by_path(&self, path: &str) -> Result<Option<MovieReference>> {
        let repository = TmdbMetadataRepository::new(self.pool());
        let row = sqlx::query(
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            WHERE mf.file_path = $1
            LIMIT 1
            "#,
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        if let Some(row) = row {
            let movie = repository.load_movie_reference(row).await?;
            Ok(Some(movie))
        } else {
            Ok(None)
        }
    }

    // Bulk reference retrieval methods for performance
    async fn get_movie_references_bulk(&self, ids: &[&MovieID]) -> Result<Vec<MovieReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();
        let repository = TmdbMetadataRepository::new(self.pool());

        let rows = sqlx::query(
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            WHERE mr.id = ANY($1)
            "#,
        )
        .bind(uuids.as_slice())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut movies = Vec::with_capacity(rows.len());
        for row in rows {
            let movie = repository.load_movie_reference(row).await?;
            movies.push(movie);
        }

        Ok(movies)
    }

    async fn get_series_references_bulk(&self, ids: &[&SeriesID]) -> Result<Vec<SeriesReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();

        let repository = TmdbMetadataRepository::new(self.pool());

        let rows = sqlx::query(
            r#"
            SELECT
                sr.id,
                sr.library_id,
                sr.tmdb_id,
                sr.title,
                sr.theme_color,
                sr.discovered_at,
                sr.created_at
            FROM series_references sr
            WHERE sr.id = ANY($1)
            "#,
        )
        .bind(uuids.as_slice())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut series_list = Vec::with_capacity(rows.len());
        for row in rows {
            let series = repository.load_series_reference(row).await?;
            series_list.push(series);
        }

        Ok(series_list)
    }

    async fn get_library_series(&self, library_id: &LibraryID) -> Result<Vec<SeriesReference>> {
        let repository = TmdbMetadataRepository::new(self.pool());

        let rows = sqlx::query(
            r#"
            SELECT
                sr.id,
                sr.library_id,
                sr.tmdb_id,
                sr.title,
                sr.theme_color,
                sr.discovered_at,
                sr.created_at
            FROM series_references sr
            WHERE sr.library_id = $1
            ORDER BY sr.title
            "#,
        )
        .bind(library_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut series_list = Vec::with_capacity(rows.len());
        for row in rows {
            let series = repository.load_series_reference(row).await?;
            series_list.push(series);
        }

        Ok(series_list)
    }

    async fn get_season_references_bulk(&self, ids: &[&SeasonID]) -> Result<Vec<SeasonReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();

        let repository = TmdbMetadataRepository::new(self.pool());

        let rows = sqlx::query(
            r#"
            SELECT
                sr.id,
                sr.series_id,
                sr.season_number,
                sr.library_id,
                sr.tmdb_series_id,
                sr.discovered_at,
                sr.created_at,
                sr.theme_color
            FROM season_references sr
            WHERE sr.id = ANY($1)
            "#,
        )
        .bind(uuids.as_slice())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get seasons: {}", e)))?;

        let mut seasons = Vec::with_capacity(rows.len());
        for row in rows {
            let season = repository.load_season_reference(row).await?;
            seasons.push(season);
        }

        Ok(seasons)
    }

    async fn get_library_seasons(&self, library_id: &LibraryID) -> Result<Vec<SeasonReference>> {
        let repository = TmdbMetadataRepository::new(self.pool());

        let rows = sqlx::query(
            r#"
            SELECT
                sr.id,
                sr.series_id,
                sr.season_number,
                sr.library_id,
                sr.tmdb_series_id,
                sr.discovered_at,
                sr.created_at,
                sr.theme_color
            FROM season_references sr
            WHERE sr.library_id = $1
            ORDER BY sr.series_id, sr.season_number
            "#,
        )
        .bind(library_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get seasons: {}", e)))?;

        let mut seasons = Vec::with_capacity(rows.len());
        for row in rows {
            let season = repository.load_season_reference(row).await?;
            seasons.push(season);
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

        let repository = TmdbMetadataRepository::new(self.pool());

        let rows = sqlx::query(
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
                er.discovered_at AS episode_discovered_at,
                er.created_at AS episode_created_at,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            WHERE er.id = ANY($1)
            "#,
        )
        .bind(uuids.as_slice())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get episodes: {}", e)))?;

        let mut episodes = Vec::with_capacity(rows.len());
        for row in rows {
            let episode = repository.load_episode_reference(row).await?;
            episodes.push(episode);
        }

        Ok(episodes)
    }

    async fn get_library_episodes(&self, library_id: &LibraryID) -> Result<Vec<EpisodeReference>> {
        let repository = TmdbMetadataRepository::new(self.pool());

        let rows = sqlx::query(
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
                er.discovered_at AS episode_discovered_at,
                er.created_at AS episode_created_at,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            WHERE mf.library_id = $1
            ORDER BY er.series_id ASC, er.season_number ASC, er.episode_number ASC
            "#,
        )
        .bind(library_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get episodes: {}", e)))?;

        let mut episodes = Vec::with_capacity(rows.len());
        for row in rows {
            let episode = repository.load_episode_reference(row).await?;
            episodes.push(episode);
        }

        Ok(episodes)
    }

    // ==================== Folder Inventory Methods ====================

    async fn get_folders_needing_scan(
        &self,
        filters: &FolderScanFilters,
    ) -> Result<Vec<FolderInventory>> {
        self.folder_inventory
            .get_folders_needing_scan(filters)
            .await
    }

    async fn update_folder_status(
        &self,
        folder_id: Uuid,
        status: FolderProcessingStatus,
        error: Option<String>,
    ) -> Result<()> {
        self.folder_inventory
            .update_folder_status(folder_id, status, error)
            .await
    }

    async fn record_folder_scan_error(
        &self,
        folder_id: Uuid,
        error: &str,
        next_retry: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.folder_inventory
            .record_folder_scan_error(folder_id, error, next_retry)
            .await
    }

    async fn get_folder_inventory(&self, library_id: LibraryID) -> Result<Vec<FolderInventory>> {
        self.folder_inventory.get_folder_inventory(library_id).await
    }

    async fn upsert_folder(&self, folder: &FolderInventory) -> Result<Uuid> {
        self.folder_inventory.upsert_folder(folder).await
    }

    async fn cleanup_stale_folders(
        &self,
        library_id: LibraryID,
        stale_after_hours: i32,
    ) -> Result<u32> {
        self.folder_inventory
            .cleanup_stale_folders(library_id, stale_after_hours)
            .await
    }

    async fn get_folder_by_path(
        &self,
        library_id: LibraryID,
        path: &Path,
    ) -> Result<Option<FolderInventory>> {
        self.folder_inventory
            .get_folder_by_path(library_id, path)
            .await
    }

    async fn update_folder_stats(
        &self,
        folder_id: Uuid,
        total_files: i32,
        processed_files: i32,
        total_size_bytes: i64,
        file_types: Vec<String>,
    ) -> Result<()> {
        self.folder_inventory
            .update_folder_stats(
                folder_id,
                total_files,
                processed_files,
                total_size_bytes,
                file_types,
            )
            .await
    }

    async fn mark_folder_processed(&self, folder_id: Uuid) -> Result<()> {
        self.folder_inventory.mark_folder_processed(folder_id).await
    }

    async fn get_child_folders(&self, parent_folder_id: Uuid) -> Result<Vec<FolderInventory>> {
        self.folder_inventory
            .get_child_folders(parent_folder_id)
            .await
    }

    async fn get_season_folders(&self, parent_folder_id: Uuid) -> Result<Vec<FolderInventory>> {
        self.folder_inventory
            .get_season_folders(parent_folder_id)
            .await
    }
}
