use crate::{
    database::infrastructure::postgres::repositories::{
        folder_inventory::PostgresFolderInventoryRepository,
        processing_status::PostgresProcessingStatusRepository,
        rbac::PostgresRbacRepository,
        sync_sessions::PostgresSyncSessionsRepository,
        users::PostgresUsersRepository,
        watch_status::PostgresWatchStatusRepository,
    },
    error::{MediaError, Result},
    scan::fs_watch::event_bus::PostgresFileChangeEventBus,
};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
};
use std::{fmt, path::Path};
use tracing::{info, warn};

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
        // Integrate env-aware connection option builder to reduce duplication and centralize DSN parsing.
        let connect_options = Self::build_connect_options(connection_string)?;
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
            .connect_with(connect_options)
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
        //let libraries = PostgresLibraryRepository::new(pool.clone());
        //let media = PostgresMediaRepository::new(pool.clone());
        let processing_status =
            PostgresProcessingStatusRepository::new(pool.clone());
        //let file_watch = PostgresFileWatchRepository::new(pool.clone());

        Ok(PostgresDatabase {
            pool,
            max_connections,
            min_connections,
            users,
            rbac,
            watch_status,
            sync_sessions,
            folder_inventory,
            processing_status,
            //file_watch,
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
        let processing_status =
            PostgresProcessingStatusRepository::new(pool.clone());
        //let file_watch = PostgresFileWatchRepository::new(pool.clone());

        PostgresDatabase {
            pool,
            max_connections,
            min_connections,
            users,
            rbac,
            watch_status,
            sync_sessions,
            folder_inventory,
            processing_status,
            //file_watch,
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

        if let Ok(db_name) = std::env::var("DATABASE_NAME")
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

    // // Reintegrate?
    // pub(crate) fn file_watch_repository(&self) -> &PostgresFileWatchRepository {
    //&self.file_watch
    // }

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

    /// Run migrations after performing preflight checks.
    pub async fn initialize_schema(&self) -> Result<()> {
        self.preflight_check().await?;

        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Migration failed: {}", e))
            })?;

        Ok(())
    }
}
