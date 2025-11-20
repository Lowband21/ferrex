use super::traits::*;
use crate::{
    database::{
        infrastructure::postgres::repositories::{
            folder_inventory::PostgresFolderInventoryRepository,
            library::PostgresLibraryRepository, media::PostgresMediaRepository,
            processing_status::PostgresProcessingStatusRepository,
            rbac::PostgresRbacRepository,
            sync_sessions::PostgresSyncSessionsRepository,
            users::PostgresUsersRepository,
            watch_status::PostgresWatchStatusRepository,
        },
        ports::{
            folder_inventory::FolderInventoryRepository,
            library::LibraryRepository,
            processing_status::ProcessingStatusRepository,
            rbac::RbacRepository, sync_sessions::SyncSessionsRepository,
            users::UsersRepository, watch_status::WatchStatusRepository,
        },
    },
    error::{MediaError, Result},
    fs_watch::event_bus::PostgresFileChangeEventBus,
    query::types::{MediaQuery, MediaWithStatus},
    rbac::{Permission, Role, UserPermissions},
    sync_session::{Participant, PlaybackState, SyncSession},
    types::{
        LibraryID,
        details::LibraryReference,
        files::{MediaFile, MediaFileMetadata},
        library::Library,
    },
    user::User,
    watch_status::{InProgressItem, UpdateProgressRequest, UserWatchState},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{self};
use sqlx::{
    PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
};
use std::{fmt, path::Path};
use tracing::{info, warn};
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
            // Ensure unqualified names resolve to application schema first.
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    // Safe to set even if schema doesn't yet exist; Postgres allows arbitrary names in search_path.
                    let _ = sqlx::query("SET search_path = ferrex, public")
                        .execute(conn)
                        .await;
                    Ok(())
                })
            })
            .connect(connection_string)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Database connection failed: {}",
                    e
                ))
            })?;

        info!(
            "Database pool initialized with max_connections={}, min_connections={}",
            max_connections, min_connections
        );

        let pool = pg_pool;
        let users = PostgresUsersRepository::new(pool.clone());
        let rbac = PostgresRbacRepository::new(pool.clone());
        let watch_status = PostgresWatchStatusRepository::new(pool.clone());
        let sync_sessions = PostgresSyncSessionsRepository::new(pool.clone());
        let folder_inventory =
            PostgresFolderInventoryRepository::new(pool.clone());
        let libraries = PostgresLibraryRepository::new(pool.clone());
        let media = PostgresMediaRepository::new(pool.clone());
        let processing_status =
            PostgresProcessingStatusRepository::new(pool.clone());

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

    /// Run only the preflight checks without applying migrations.
    pub async fn preflight_only(&self) -> Result<()> {
        self.preflight_check().await
    }

    /// Preflight checks for schema privileges and required extensions.
    ///
    /// Rationale: surface clear, actionable errors (GRANTs/CREATE EXTENSION)
    /// instead of a generic "permission denied for schema" during
    /// migrations. This helps operators fix DB permissions without guessing.
    async fn preflight_check(&self) -> Result<()> {
        // Determine target schema for privileges: prefer `ferrex` if present, otherwise `public`.
        let ferrex_exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM pg_namespace WHERE nspname = 'ferrex') AS \"exists!: bool\"",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Schema existence check failed: {}", e)))?
        .exists;

        let target_schema = if ferrex_exists { "ferrex" } else { "public" };

        // Check required privileges on target schema.
        let privs = sqlx::query!(
            r#"
            SELECT
              has_schema_privilege(current_user, $1, 'USAGE')  AS "usage!: bool",
              has_schema_privilege(current_user, $1, 'CREATE') AS "create!: bool"
            "#,
            target_schema
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Privilege preflight failed: {}", e)))?;

        // Detect presence of required extensions; migrations create them IF NOT EXISTS.
        let ext_citext = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'citext') AS \"exists!: bool\""
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Extension check (citext) failed: {}", e)))?
        .exists;

        let ext_trgm = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'pg_trgm') AS \"exists!: bool\""
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Extension check (pg_trgm) failed: {}", e)))?
        .exists;

        let ext_pgcrypto = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'pgcrypto') AS \"exists!: bool\""
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Extension check (pgcrypto) failed: {}", e)))?
        .exists;

        // Determine whether current_user can CREATE EXTENSION (db owner or superuser).
        let db_info = sqlx::query!(
            r#"
            SELECT current_database() AS "db!: String",
                   pg_get_userbyid(datdba) AS "owner!: String"
            FROM pg_database
            WHERE datname = current_database()
            "#
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database owner lookup failed: {}", e))
        })?;

        let current_user =
            sqlx::query!("SELECT current_user AS \"u!: String\"")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "Current user lookup failed: {}",
                        e
                    ))
                })?
                .u;

        let is_db_owner = current_user == db_info.owner;
        // Avoid Rust keyword field name by aliasing to `is_superuser`.
        let is_superuser = sqlx::query!(
            r#"SELECT rolsuper AS "is_superuser!: bool" FROM pg_roles WHERE rolname = current_user"#
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Role check failed: {}", e)))?
        .is_superuser;
        let can_create_extension = is_db_owner || is_superuser;

        // Build actionable errors if prerequisites are missing.
        let mut problems: Vec<String> = Vec::new();
        if !privs.create {
            problems.push(format!(
                "Role '{current_user}' lacks CREATE on schema {schema}.",
                schema = target_schema
            ));
        }

        let mut missing_exts: Vec<&str> = Vec::new();
        if !ext_citext {
            missing_exts.push("citext");
        }
        if !ext_trgm {
            missing_exts.push("pg_trgm");
        }
        if !ext_pgcrypto {
            missing_exts.push("pgcrypto");
        }

        // If extensions are missing and we cannot create them, fail with guidance.
        if !missing_exts.is_empty() && !can_create_extension {
            problems.push(format!(
                "Missing extensions ({}) and role '{current_user}' cannot CREATE EXTENSION; database owner is '{}'",
                missing_exts.join(", "),
                db_info.owner
            ));
        }

        if !problems.is_empty() {
            let grants = format!(
                r#"Recommended fixes (run as a superuser/DB owner):
                -- Create and grant within the application schema
                CREATE SCHEMA IF NOT EXISTS ferrex AUTHORIZATION {current_user};
                GRANT USAGE, CREATE ON SCHEMA {schema} TO {current_user};
                GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA {schema} TO {current_user};
                GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA {schema} TO {current_user};
                GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA {schema} TO {current_user};
                ALTER DEFAULT PRIVILEGES FOR ROLE {current_user} IN SCHEMA {schema}
                  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {current_user};
                ALTER DEFAULT PRIVILEGES FOR ROLE {current_user} IN SCHEMA {schema}
                  GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO {current_user};
                ALTER DEFAULT PRIVILEGES FOR ROLE {current_user} IN SCHEMA {schema}
                  GRANT EXECUTE ON FUNCTIONS TO {current_user};
                  
                -- Optional: set search_path so unqualified names resolve to the app schema
                -- Replace your_database with the actual DB name
                ALTER ROLE {current_user} IN DATABASE your_database SET search_path = ferrex, public;
                
                -- If extensions are missing, install into public (requires superuser or DB owner '{owner}')
                -- Only run those that are missing:
                CREATE EXTENSION IF NOT EXISTS citext    WITH SCHEMA public;
                CREATE EXTENSION IF NOT EXISTS pg_trgm   WITH SCHEMA public;
                CREATE EXTENSION IF NOT EXISTS pgcrypto  WITH SCHEMA public;
                "#,
                current_user = current_user,
                owner = db_info.owner,
                schema = target_schema
            );

            let detail = format!(
                "Database preflight failed:\n- {}\n\n{}",
                problems.join("\n- "),
                grants
            );

            return Err(MediaError::Internal(detail));
        }

        // Optional: if extensions are missing but can be created, migrations will install them.
        if !missing_exts.is_empty() && can_create_extension {
            warn!(
                missing = %missing_exts.join(", "),
                owner = %db_info.owner,
                "Required extensions missing; migrations will attempt to create them"
            );
        }

        // Ensure CREATE on target schema; otherwise object creation will fail even if migrations run.
        if !privs.create {
            return Err(MediaError::Internal(format!(
                "Role '{current_user}' lacks CREATE on schema {schema}; apply the GRANTs above and restart.",
                schema = target_schema
            )));
        }

        Ok(())
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
        let folder_inventory =
            PostgresFolderInventoryRepository::new(pool.clone());
        let libraries = PostgresLibraryRepository::new(pool.clone());
        let media = PostgresMediaRepository::new(pool.clone());
        let processing_status =
            PostgresProcessingStatusRepository::new(pool.clone());

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

    fn build_connect_options(
        connection_string: &str,
    ) -> Result<PgConnectOptions> {
        use tracing::debug;

        let trimmed = connection_string.trim();

        let mut options = if trimmed.is_empty() {
            PgConnectOptions::new()
        } else {
            trimmed.parse::<PgConnectOptions>().map_err(|e| {
                MediaError::Internal(format!(
                    "Invalid PostgreSQL connection string: {}",
                    e
                ))
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

    pub(crate) fn watch_status_repository(
        &self,
    ) -> &PostgresWatchStatusRepository {
        &self.watch_status
    }

    pub(crate) fn sync_sessions_repository(
        &self,
    ) -> &PostgresSyncSessionsRepository {
        &self.sync_sessions
    }

    pub(crate) fn folder_inventory_repository(
        &self,
    ) -> &PostgresFolderInventoryRepository {
        &self.folder_inventory
    }

    pub(crate) fn processing_status_repository(
        &self,
    ) -> &PostgresProcessingStatusRepository {
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
}

#[async_trait]
impl MediaDatabaseTrait for PostgresDatabase {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn initialize_schema(&self) -> Result<()> {
        // Preflight: verify privileges and required extensions for clearer errors
        self.preflight_check().await?;

        // Run migrations using sqlx migrate
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Migration failed: {}", e))
            })?;

        Ok(())
    }

    async fn store_media(&self, media_file: MediaFile) -> Result<Uuid> {
        self.media.store_media(media_file).await
    }

    async fn store_media_batch(
        &self,
        media_files: Vec<MediaFile>,
    ) -> Result<Vec<Uuid>> {
        self.media.store_media_batch(media_files).await
    }

    async fn get_media(&self, uuid: &Uuid) -> Result<Option<MediaFile>> {
        self.media.get_media(uuid).await
    }

    async fn get_media_by_path(&self, path: &str) -> Result<Option<MediaFile>> {
        self.media.get_media_by_path(path).await
    }

    async fn list_media(
        &self,
        filters: MediaFilters,
    ) -> Result<Vec<MediaFile>> {
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

    async fn get_library(
        &self,
        library_id: &LibraryID,
    ) -> Result<Option<Library>> {
        self.libraries.get_library(*library_id).await
    }

    async fn list_libraries(&self) -> Result<Vec<Library>> {
        self.libraries.list_libraries().await
    }

    async fn update_library(&self, id: &str, library: Library) -> Result<()> {
        let uuid = Uuid::parse_str(id).map_err(|e| {
            MediaError::InvalidMedia(format!("Invalid UUID: {}", e))
        })?;
        self.libraries
            .update_library(LibraryID(uuid), library)
            .await
    }

    async fn delete_library(&self, id: &str) -> Result<()> {
        let uuid = Uuid::parse_str(id).map_err(|e| {
            MediaError::InvalidMedia(format!("Invalid UUID: {}", e))
        })?;
        self.libraries.delete_library(LibraryID(uuid)).await
    }

    async fn update_library_last_scan(&self, uuid: &LibraryID) -> Result<()> {
        self.libraries.update_library_last_scan(*uuid).await
    }

    async fn list_library_references(&self) -> Result<Vec<LibraryReference>> {
        self.libraries.list_library_references().await
    }

    async fn get_library_reference(
        &self,
        id: Uuid,
    ) -> Result<LibraryReference> {
        self.libraries.get_library_reference(id).await
    }

    // Image management methods

    // Scan state management methods
    async fn create_scan_state(&self, scan_state: &ScanState) -> Result<()> {
        let options_json =
            serde_json::to_value(&scan_state.options).map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to serialize scan options: {}",
                    e
                ))
            })?;

        let errors_json =
            serde_json::to_value(&scan_state.errors).map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to serialize errors: {}",
                    e
                ))
            })?;

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
        let options_json =
            serde_json::to_value(&scan_state.options).map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to serialize scan options: {}",
                    e
                ))
            })?;

        let errors_json =
            serde_json::to_value(&scan_state.errors).map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to serialize errors: {}",
                    e
                ))
            })?;

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
        .map_err(|e| {
            MediaError::Internal(format!("Failed to update scan state: {}", e))
        })?;

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

    async fn get_active_scans(
        &self,
        library_id: Option<Uuid>,
    ) -> Result<Vec<ScanState>> {
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
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get active scans: {}", e))
        })?;

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

            let errors_json: Option<serde_json::Value> =
                row.try_get("errors")?;
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
                total_folders: row
                    .try_get::<Option<i32>, _>("total_folders")?
                    .unwrap_or(0),
                processed_folders: row
                    .try_get::<Option<i32>, _>("processed_folders")?
                    .unwrap_or(0),
                total_files: row
                    .try_get::<Option<i32>, _>("total_files")?
                    .unwrap_or(0),
                processed_files: row
                    .try_get::<Option<i32>, _>("processed_files")?
                    .unwrap_or(0),
                current_path: row.try_get("current_path")?,
                error_count: row
                    .try_get::<Option<i32>, _>("error_count")?
                    .unwrap_or(0),
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
    async fn create_file_watch_event(
        &self,
        event: &FileWatchEvent,
    ) -> Result<()> {
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
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to mark event processed: {}",
                e
            ))
        })?;

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
            "Use authentication system to create users with password"
                .to_string(),
        ))
    }

    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>> {
        self.users.get_user_by_id(id).await
    }

    async fn get_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<User>> {
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

    async fn get_user_password_hash(
        &self,
        user_id: Uuid,
    ) -> Result<Option<String>> {
        self.users.get_user_password_hash(user_id).await
    }

    async fn update_user_password(
        &self,
        user_id: Uuid,
        password_hash: &str,
    ) -> Result<()> {
        self.users
            .update_user_password(user_id, password_hash)
            .await
    }

    async fn delete_user_atomic(
        &self,
        user_id: Uuid,
        check_last_admin: bool,
    ) -> Result<()> {
        self.users
            .delete_user_atomic(user_id, check_last_admin)
            .await
    }

    // ==================== RBAC Methods ====================

    async fn get_user_permissions(
        &self,
        user_id: Uuid,
    ) -> Result<UserPermissions> {
        self.rbac.get_user_permissions(user_id).await
    }

    async fn get_all_roles(&self) -> Result<Vec<Role>> {
        self.rbac.get_all_roles().await
    }

    async fn get_all_permissions(&self) -> Result<Vec<Permission>> {
        self.rbac.get_all_permissions().await
    }

    async fn assign_user_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        granted_by: Uuid,
    ) -> Result<()> {
        self.rbac
            .assign_user_role(user_id, role_id, granted_by)
            .await
    }

    async fn remove_user_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
    ) -> Result<()> {
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
            .override_user_permission(
                user_id, permission, granted, granted_by, reason,
            )
            .await
    }

    async fn get_admin_count(
        &self,
        exclude_user_id: Option<Uuid>,
    ) -> Result<usize> {
        self.rbac.get_admin_count(exclude_user_id).await
    }

    async fn user_has_role(
        &self,
        user_id: Uuid,
        role_name: &str,
    ) -> Result<bool> {
        self.rbac.user_has_role(user_id, role_name).await
    }

    async fn get_users_with_role(&self, role_name: &str) -> Result<Vec<Uuid>> {
        self.rbac.get_users_with_role(role_name).await
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

    async fn get_user_watch_state(
        &self,
        user_id: Uuid,
    ) -> Result<UserWatchState> {
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

    async fn clear_watch_progress(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<()> {
        self.watch_status
            .clear_watch_progress(user_id, media_id)
            .await
    }

    async fn is_media_completed(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<bool> {
        self.watch_status
            .is_media_completed(user_id, media_id)
            .await
    }

    // ==================== Sync Session Methods ====================

    async fn create_sync_session(&self, session: &SyncSession) -> Result<()> {
        self.sync_sessions.create_sync_session(session).await
    }

    async fn get_sync_session_by_code(
        &self,
        room_code: &str,
    ) -> Result<Option<SyncSession>> {
        self.sync_sessions.get_sync_session_by_code(room_code).await
    }

    async fn get_sync_session(&self, id: Uuid) -> Result<Option<SyncSession>> {
        self.sync_sessions.get_sync_session(id).await
    }

    async fn update_sync_session_state(
        &self,
        id: Uuid,
        state: &PlaybackState,
    ) -> Result<()> {
        self.sync_sessions
            .update_sync_session_state(id, state)
            .await
    }

    async fn update_sync_session(
        &self,
        id: Uuid,
        session: &SyncSession,
    ) -> Result<()> {
        self.sync_sessions.update_sync_session(id, session).await
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

    async fn remove_sync_participant(
        &self,
        session_id: Uuid,
        user_id: Uuid,
    ) -> Result<()> {
        self.sync_sessions
            .remove_sync_participant(session_id, user_id)
            .await
    }

    async fn delete_sync_session(&self, id: Uuid) -> Result<()> {
        self.sync_sessions.delete_sync_session(id).await
    }

    async fn end_sync_session(&self, id: Uuid) -> Result<()> {
        self.sync_sessions.end_sync_session(id).await
    }

    async fn cleanup_expired_sync_sessions(&self) -> Result<u32> {
        self.sync_sessions.cleanup_expired_sync_sessions().await
    }

    async fn query_media(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        <PostgresDatabase>::query_media(self, query).await
    }

    // Legacy device auth methods removed; use auth/domain repositories

    // Legacy device auth method removed

    // Legacy device auth methods removed

    // legacy device credential methods removed; use auth domain repositories

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

    async fn get_folder_inventory(
        &self,
        library_id: LibraryID,
    ) -> Result<Vec<FolderInventory>> {
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

    async fn get_child_folders(
        &self,
        parent_folder_id: Uuid,
    ) -> Result<Vec<FolderInventory>> {
        self.folder_inventory
            .get_child_folders(parent_folder_id)
            .await
    }

    async fn get_season_folders(
        &self,
        parent_folder_id: Uuid,
    ) -> Result<Vec<FolderInventory>> {
        self.folder_inventory
            .get_season_folders(parent_folder_id)
            .await
    }
}
