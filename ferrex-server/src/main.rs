//! # Ferrex Server
//!
//! High-performance media server.
//!
//! ## Overview
//!
//! Ferrex Server is a comprehensive media streaming solution that provides:
//!
//! - **Media Streaming**: Simple direct streaming with transcoding on the way
//! - **User Management**: Opaque session tokens with refresh rotation and device tracking
//! - **Watch Progress**: Automatic progress tracking and "continue watching" features
//! - **Synchronized Playback**: Real-time synchronized viewing sessions via WebSocket (Soon)
//! - **Library Management**: Automatic media scanning and metadata enrichment
//!
//! ## Architecture
//!
//! The server is built on Axum and uses:
//! - PostgreSQL for persistent storage
//! - Redis for caching and session management
//! - FFmpeg for transcoding
//! - TMDB for metadata
// ```

use anyhow::Context;
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode},
    response::{Json, Response},
    routing::get,
};
use chrono::Utc;
use clap::{Args as ClapArgs, Parser, Subcommand};
use ferrex_core::application::unit_of_work::AppUnitOfWork;
use ferrex_core::auth::{
    AuthCrypto,
    domain::repositories::{
        AuthEventRepository, AuthSessionRepository, DeviceChallengeRepository,
        DeviceSessionRepository, RefreshTokenRepository,
        UserAuthenticationRepository,
    },
    domain::services::{
        AuthenticationService, DeviceTrustService, PinManagementService,
    },
    infrastructure::repositories::{
        PostgresAuthEventRepository, PostgresAuthSessionRepository,
        PostgresDeviceChallengeRepository, PostgresDeviceSessionRepository,
        PostgresRefreshTokenRepository, PostgresUserAuthRepository,
    },
};
use ferrex_core::database::ports::media_files::MediaFileFilter;
use ferrex_core::database::{PostgresDatabase, traits::MediaDatabaseTrait};
use ferrex_core::image_service::ImageService;
use ferrex_core::orchestration::LibraryActorConfig;
use ferrex_core::providers::TmdbApiProvider;
use ferrex_core::setup::SetupClaimService;
use ferrex_core::types::LibraryReference;
use ferrex_server::db::validate_primary_database_url;
use ferrex_server::{
    application::auth::AuthApplicationFacade,
    infra::{
        app_context::AppContext,
        app_state::AppState,
        config::{
            Config, ConfigLoad, ConfigLoader, HstsSettings, RateLimitSource,
        },
        orchestration::ScanOrchestrator,
        scan::scan_manager::ScanControlPlane,
        startup::{ProdStartupHooks, StartupHooks},
        websocket,
    },
    media::prep::thumbnail_service::ThumbnailService,
    routes,
};
#[cfg(feature = "demo")]
use ferrex_server::{db::prepare_demo_database, demo::DemoCoordinator};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tower_http::{
    cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;
use uuid::Uuid;

use ferrex_server::infra::config::cli::{
    CheckOptions, InitOptions, run_config_check, run_config_init,
};
use ferrex_server::users::auth::tls::{TlsCertConfig, create_tls_acceptor};

/// CLI entry point
#[derive(Parser, Debug)]
#[command(name = "ferrex-server")]
#[command(
    about = "High-performance media server with real-time transcoding and streaming"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    serve: ServeArgs,
}

#[derive(ClapArgs, Debug, Clone)]
struct ServeArgs {
    /// Path to TLS certificate file (PEM format)
    #[arg(long, env = "TLS_CERT_PATH")]
    cert: Option<PathBuf>,

    /// Path to TLS private key file (PEM format)
    #[arg(long, env = "TLS_KEY_PATH")]
    key: Option<PathBuf>,

    /// Server port (overrides config)
    #[arg(short, long, env = "SERVER_PORT")]
    port: Option<u16>,

    /// Server host (overrides config)
    #[arg(long, env = "SERVER_HOST")]
    host: Option<String>,

    /// Reset any pending setup claim codes and exit
    #[arg(long, env = "FERREX_RESET_CLAIMS", default_value_t = false)]
    claim_reset: bool,

    /// Enable demo mode with synthetic media, demo database, and default user
    #[cfg(feature = "demo")]
    #[arg(long, env = "FERREX_DEMO_MODE", default_value_t = false)]
    demo: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(subcommand)]
    Config(ConfigCommand),
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Init(ConfigInitArgs),
    Check(ConfigCheckArgs),
}

#[derive(ClapArgs, Debug, Clone)]
struct ConfigInitArgs {
    /// Path to write the generated ferrex configuration
    #[arg(long, default_value = "config/ferrex.toml")]
    config_path: PathBuf,

    /// Path to write the companion .env file
    #[arg(long, default_value = ".env")]
    env_path: PathBuf,

    /// Overwrite existing files
    #[arg(long, default_value_t = false)]
    force: bool,

    /// Skip interactive prompts and use defaults
    #[arg(long, default_value_t = false)]
    non_interactive: bool,
}

#[derive(ClapArgs, Debug, Clone)]
struct ConfigCheckArgs {
    /// Explicit path to ferrex.toml
    #[arg(long)]
    config_path: Option<PathBuf>,

    /// Explicit path to .env file
    #[arg(long)]
    env_file: Option<PathBuf>,

    /// TLS certificate path to validate when enforcing HTTPS
    #[arg(long)]
    cert: Option<PathBuf>,

    /// TLS key path to validate when enforcing HTTPS
    #[arg(long)]
    key: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        match command {
            Command::Config(ConfigCommand::Init(args)) => {
                let options = InitOptions {
                    config_path: args.config_path.clone(),
                    env_path: args.env_path.clone(),
                    force: args.force,
                    non_interactive: args.non_interactive,
                };
                run_config_init(&options).await?;
                return Ok(());
            }
            Command::Config(ConfigCommand::Check(args)) => {
                let options = CheckOptions {
                    config_path: args.config_path.clone(),
                    env_path: args.env_file.clone(),
                    tls_cert_path: args.cert.clone(),
                    tls_key_path: args.key.clone(),
                };
                run_config_check(&options).await?;
                return Ok(());
            }
        }
    }

    run_server(cli.serve).await
}

fn derive_database_url_from_env() -> Option<String> {
    let database = std::env::var("PGDATABASE")
        .or_else(|_| std::env::var("POSTGRES_DB"))
        .ok()?
        .trim()
        .to_owned();

    if database.is_empty() {
        return None;
    }

    Some(format!("postgresql:///{database}"))
}

fn build_hsts_header(settings: &HstsSettings) -> Option<HeaderValue> {
    let mut directives = vec![format!("max-age={}", settings.max_age)];
    if settings.include_subdomains {
        directives.push("includeSubDomains".to_string());
    }
    if settings.preload {
        directives.push("preload".to_string());
    }

    let header = directives.join("; ");
    HeaderValue::from_str(&header).ok()
}

struct ConfigBootstrap {
    config: Arc<Config>,
    tmdb_provider: Arc<TmdbApiProvider>,
    database_url: String,
    with_cache: bool,
    #[cfg(feature = "demo")]
    demo_coordinator: Option<Arc<DemoCoordinator>>,
}

async fn load_runtime_config(
    args: &ServeArgs,
) -> anyhow::Result<ConfigBootstrap> {
    let ConfigLoad {
        mut config,
        warnings,
    } = ConfigLoader::new()
        .load()
        .context("failed to load configuration")?;
    let config_warnings = warnings;

    if let Some(port) = args.port {
        config.server.port = port;
    }
    if let Some(host) = args.host.clone() {
        config.server.host = host;
    }

    #[cfg(feature = "demo")]
    let mut demo_coordinator: Option<Arc<DemoCoordinator>> = None;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                // Quieter defaults with focused scan summaries. Override via RUST_LOG.
                "info,scan::summary=info,scan::queue=info,scan::seed=info,tower_http=warn".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    if let Some(path) = config.metadata.config_path.as_ref() {
        info!(path = %path.display(), "loaded configuration file");
    } else {
        info!(
            "proceeding without ferrex.toml (environment-only configuration)"
        );
    }

    if config.metadata.env_file_loaded {
        info!("loaded .env file");
    }

    if let Some(source) = config.metadata.rate_limit_source.as_ref() {
        match source {
            RateLimitSource::EnvPath(path) => {
                info!(path = %path.display(), "rate limiter config loaded from env path")
            }
            RateLimitSource::EnvInline => {
                info!("rate limiter config loaded from inline environment json")
            }
            RateLimitSource::FilePath(path) => {
                info!(path = %path.display(), "rate limiter config loaded from file")
            }
            RateLimitSource::FileInline(path) => {
                info!(path = %path.display(), "rate limiter config loaded inline from ferrex.toml")
            }
        }
    }

    if !config_warnings.is_empty() {
        for warning in &config_warnings.items {
            match &warning.hint {
                Some(hint) => {
                    warn!(message = %warning.message, hint = %hint, "configuration warning")
                }
                None => {
                    warn!(message = %warning.message, "configuration warning")
                }
            }
        }
    }

    info!("Server configuration loaded");
    let queue_cfg = &config.scanner.orchestrator.queue;
    let budget_cfg = &config.scanner.orchestrator.budget;
    info!(
        scanner.max_parallel_scans = queue_cfg.max_parallel_scans,
        scanner.max_parallel_scans_per_device =
            queue_cfg.max_parallel_scans_per_device,
        scanner.budget_library_scan_limit = budget_cfg.library_scan_limit,
        scanner.actor_outstanding_cap =
            config.scanner.library_actor_max_outstanding_jobs,
        scanner.quiescence_ms = config.scanner.quiescence_window_ms,
        "scanner configuration in effect"
    );
    if let Some(media_root) = &config.media.root {
        info!("Media root: {}", media_root.display());
    } else {
        warn!(
            "No MEDIA_ROOT configured - will require path parameter for scans"
        );
    }

    info!(
        cache.root = %config.cache_root().display(),
        cache.transcode = %config.transcode_cache_dir().display(),
        cache.thumbnails = %config.thumbnail_cache_dir().display(),
        "cache directories prepared"
    );

    let tmdb_provider = Arc::new(TmdbApiProvider::new());

    #[cfg(feature = "demo")]
    if args.demo {
        let coordinator =
            DemoCoordinator::bootstrap(&mut config, tmdb_provider.clone())
                .await?;
        demo_coordinator = Some(Arc::new(coordinator));
        info!("Demo mode enabled - synthetic media tree prepared");
    }

    #[allow(unused_mut)]
    let (mut database_url, mut url_source) = match config
        .database
        .primary_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        Some(url) => (url.to_owned(), "config"),
        None => match derive_database_url_from_env() {
            Some(url) => (url, "PG env"),
            None => {
                error!(
                    "DATABASE_URL or PGDATABASE must be provided for PostgreSQL connections"
                );
                return Err(anyhow::anyhow!(
                    "No PostgreSQL connection configuration found"
                ));
            }
        },
    };

    validate_primary_database_url(&database_url)?;

    #[cfg(feature = "demo")]
    if demo_coordinator.is_some() {
        database_url = prepare_demo_database(&database_url).await?;
        url_source = "demo";
        config.database.primary_url = Some(database_url.clone());
    }

    if !(database_url.starts_with("postgres://")
        || database_url.starts_with("postgresql://"))
    {
        error!("Only PostgreSQL database URLs are supported");
        return Err(anyhow::anyhow!(
            "Invalid database URL: must start with postgres:// or postgresql://"
        ));
    }

    info!("Connecting to PostgreSQL via {}", url_source);

    config.database.primary_url = Some(database_url.clone());

    let config = Arc::new(config);
    let with_cache = config.redis.is_some();

    Ok(ConfigBootstrap {
        config,
        tmdb_provider,
        database_url,
        with_cache,
        #[cfg(feature = "demo")]
        demo_coordinator,
    })
}

struct ResourceBootstrap {
    context: Arc<AppContext>,
    state: AppState,
}

#[allow(clippy::too_many_arguments)]
async fn wire_app_resources(
    config: Arc<Config>,
    database_url: &str,
    tmdb_provider: Arc<TmdbApiProvider>,
    with_cache: bool,
    #[cfg(feature = "demo")] demo_coordinator: Option<Arc<DemoCoordinator>>,
) -> anyhow::Result<ResourceBootstrap> {
    let postgres_backend = match PostgresDatabase::new(database_url).await {
        Ok(database) => {
            info!("Successfully connected to PostgreSQL");
            Arc::new(database)
        }
        Err(initial_error) => {
            warn!(
                error = %initial_error,
                "Initial PostgreSQL connection failed; attempting bootstrap"
            );

            match attempt_database_bootstrap(database_url).await {
                Ok(true) => match PostgresDatabase::new(database_url).await {
                    Ok(database) => {
                        info!(
                            "Database connection succeeded after application role bootstrap"
                        );
                        Arc::new(database)
                    }
                    Err(retry_error) => {
                        error!(
                            error = %retry_error,
                            "PostgreSQL connection still failing after bootstrap"
                        );
                        return Err(anyhow::anyhow!(
                            "Database connection failed after bootstrap attempt: {}",
                            retry_error
                        ));
                    }
                },
                Ok(false) => {
                    error!(
                        error = %initial_error,
                        "Bootstrap skipped (admin credentials unavailable)"
                    );
                    return Err(anyhow::anyhow!(
                        "Database connection failed: {}",
                        initial_error
                    ));
                }
                Err(bootstrap_error) => {
                    error!(
                        error = %bootstrap_error,
                        "Database bootstrap attempt failed"
                    );
                    return Err(anyhow::anyhow!(
                        "Database connection failed (bootstrap error): {}",
                        bootstrap_error
                    ));
                }
            }
        }
    };

    match postgres_backend.initialize_schema().await {
        Ok(()) => {
            info!("Database schema initialized successfully");
        }
        Err(e) => {
            error!("Failed to initialize database schema: {}", e);
            return Err(anyhow::anyhow!("Database migration failed: {}", e));
        }
    }

    let unit_of_work = Arc::new(
        AppUnitOfWork::from_postgres(postgres_backend.clone())
            .expect("Failed to compose AppUnitOfWork from Postgres backend"),
    );
    let postgres_pool = postgres_backend.pool().clone();

    #[cfg(feature = "demo")]
    if let Some(coordinator) = demo_coordinator.as_ref() {
        let seeded = coordinator.sync_database(unit_of_work.clone()).await?;
        info!(
            demo_library_count = seeded.len(),
            "Demo libraries synchronised"
        );
    }

    let tmdb_api_key = std::env::var("TMDB_API_KEY").ok();
    match &tmdb_api_key {
        Some(key) => info!("TMDB API key configured (length: {})", key.len()),
        None => {
            warn!("TMDB_API_KEY not set - metadata fetching will be limited")
        }
    }

    let thumbnail_service = Arc::new(
        ThumbnailService::new(
            config.cache_root().to_path_buf(),
            unit_of_work.media_files_read.clone(),
        )
        .expect("Failed to initialize thumbnail service"),
    );

    let image_service = Arc::new(ImageService::new(
        unit_of_work.media_files_read.clone(),
        unit_of_work.images.clone(),
        config.cache_root().to_path_buf(),
    ));

    let orchestrator = Arc::new(
        ScanOrchestrator::postgres(
            config.scanner.orchestrator.clone(),
            postgres_backend.clone(),
            tmdb_provider.clone(),
            image_service.clone(),
            unit_of_work.clone(),
        )
        .await?,
    );

    let libraries = unit_of_work
        .libraries
        .list_libraries()
        .await
        .map_err(|err| anyhow::anyhow!("failed to list libraries: {err}"))?;

    let mut watch_enabled = 0usize;
    for library in &libraries {
        if library.watch_for_changes {
            watch_enabled += 1;
        }

        let actor_config = LibraryActorConfig {
            library: LibraryReference {
                id: library.id,
                name: library.name.clone(),
                library_type: library.library_type,
                paths: library.paths.clone(),
            },
            root_paths: library.paths.clone(),
            max_outstanding_jobs: config
                .scanner
                .library_actor_max_outstanding_jobs,
        };

        orchestrator
            .register_library(actor_config, library.watch_for_changes)
            .await
            .with_context(|| {
                format!("failed to register library {}", library.name)
            })?;
    }

    info!(
        registered = libraries.len(),
        watchers_enabled = watch_enabled,
        watchers_disabled = libraries.len().saturating_sub(watch_enabled),
        "libraries registered with orchestrator"
    );

    orchestrator.start().await?;

    let quiescence =
        Duration::from_millis(config.scanner.quiescence_window_ms.max(1));
    let scan_control = Arc::new(ScanControlPlane::with_quiescence_window(
        unit_of_work.clone(),
        postgres_backend.clone(),
        orchestrator,
        quiescence,
    ));

    let websocket_manager = Arc::new(websocket::ConnectionManager::new());

    let auth_crypto = Arc::new(
        AuthCrypto::new(
            config.auth.password_pepper.as_bytes(),
            config.auth.token_key.as_bytes(),
        )
        .context("failed to initialize authentication crypto helpers")?,
    );

    let user_auth_repository: Arc<dyn UserAuthenticationRepository> =
        Arc::new(PostgresUserAuthRepository::new(postgres_pool.clone()));
    let device_sessions: Arc<dyn DeviceSessionRepository> =
        Arc::new(PostgresDeviceSessionRepository::new(
            postgres_pool.clone(),
            auth_crypto.clone(),
        ));
    let refresh_tokens: Arc<dyn RefreshTokenRepository> =
        Arc::new(PostgresRefreshTokenRepository::new(postgres_pool.clone()));
    let auth_sessions: Arc<dyn AuthSessionRepository> =
        Arc::new(PostgresAuthSessionRepository::new(postgres_pool.clone()));
    let auth_event_repo: Arc<dyn AuthEventRepository> =
        Arc::new(PostgresAuthEventRepository::new(postgres_pool.clone()));
    let device_challenges: Arc<dyn DeviceChallengeRepository> = Arc::new(
        PostgresDeviceChallengeRepository::new(postgres_pool.clone()),
    );

    let auth_service = Arc::new(
        AuthenticationService::new(
            user_auth_repository.clone(),
            device_sessions.clone(),
            refresh_tokens.clone(),
            auth_sessions.clone(),
            auth_crypto.clone(),
        )
        .with_event_repository(auth_event_repo.clone())
        .with_challenge_repository(device_challenges.clone()),
    );

    let device_trust_service = Arc::new(DeviceTrustService::new(
        user_auth_repository.clone(),
        device_sessions.clone(),
        auth_event_repo.clone(),
        auth_sessions.clone(),
        refresh_tokens.clone(),
    ));

    let pin_management_service = Arc::new(PinManagementService::new(
        user_auth_repository.clone(),
        device_sessions.clone(),
        auth_event_repo.clone(),
        auth_crypto.clone(),
    ));

    let auth_facade = Arc::new(AuthApplicationFacade::new(
        auth_service.clone(),
        device_trust_service,
        pin_management_service,
        unit_of_work.clone(),
    ));

    let setup_claim_service = Arc::new(SetupClaimService::new(
        unit_of_work.setup_claims.clone(),
        auth_crypto.clone(),
    ));

    let admin_sessions = Arc::new(Mutex::new(HashMap::new()));
    let app_context = Arc::new(AppContext::new(
        Arc::clone(&config),
        unit_of_work,
        postgres_backend,
        scan_control,
        Arc::clone(&thumbnail_service),
        Arc::clone(&image_service),
        Arc::clone(&websocket_manager),
        Arc::clone(&auth_facade),
        auth_crypto,
        setup_claim_service,
        with_cache,
        #[cfg(feature = "demo")]
        demo_coordinator,
    ));

    let state = AppState::new(Arc::clone(&app_context), admin_sessions);

    Ok(ResourceBootstrap {
        context: app_context,
        state,
    })
}

async fn attempt_database_bootstrap(
    database_url: &str,
) -> anyhow::Result<bool> {
    let parsed = match Url::parse(database_url) {
        Ok(url) => url,
        Err(err) => {
            warn!(error = %err, "Unable to parse DATABASE_URL for bootstrap");
            return Ok(false);
        }
    };

    let app_user = parsed.username().to_string();
    if app_user.is_empty() {
        warn!("DATABASE_URL is missing a username; skipping bootstrap");
        return Ok(false);
    }

    let app_password = match parsed.password() {
        Some(password) if !password.is_empty() => password.to_string(),
        _ => match std::env::var("FERREX_APP_PASSWORD") {
            Ok(value) if !value.is_empty() => value,
            _ => {
                warn!("No application password available for bootstrap");
                return Ok(false);
            }
        },
    };

    let db_name = parsed
        .path()
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or_default()
        .trim();
    if db_name.is_empty() {
        warn!("DATABASE_URL is missing a database name; skipping bootstrap");
        return Ok(false);
    }

    let host = parsed.host_str().unwrap_or("localhost").to_string();
    let port = parsed.port().unwrap_or(5432);

    let admin_url = std::env::var("DATABASE_ADMIN_URL").ok().or_else(|| {
        let admin_user = std::env::var("POSTGRES_USER").ok()?;
        let admin_password = std::env::var("POSTGRES_PASSWORD").ok()?;
        let admin_host = std::env::var("POSTGRES_HOST")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or(host.clone());
        let admin_port = std::env::var("POSTGRES_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(port);
        Some(format!(
            "postgresql://{}:{}@{}:{}/postgres",
            admin_user, admin_password, admin_host, admin_port
        ))
    });

    let Some(admin_url) = admin_url else {
        warn!(
            "Admin connection details absent; cannot bootstrap application role"
        );
        return Ok(false);
    };

    let admin_parsed = match Url::parse(&admin_url) {
        Ok(url) => url,
        Err(err) => {
            warn!(error = %err, "Invalid DATABASE_ADMIN_URL; skipping bootstrap");
            return Ok(false);
        }
    };

    let admin_db_url = {
        let mut url = admin_parsed.clone();
        url.set_path(&format!("/{db_name}"));
        url.to_string()
    };

    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&admin_url)
        .await
        .map_err(|err| {
            anyhow::anyhow!("failed to connect with admin credentials: {err}")
        })?;

    sqlx::query(
        "DO $$\nDECLARE\n    role_name text := $1;\n    role_password text := $2;\nBEGIN\n    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN\n        EXECUTE format('CREATE ROLE %I LOGIN PASSWORD %L', role_name, role_password);\n    ELSE\n        EXECUTE format('ALTER ROLE %I WITH PASSWORD %L', role_name, role_password);\n    END IF;\nEND\n$$;",
    )
    .bind(&app_user)
    .bind(&app_password)
    .execute(&admin_pool)
    .await
    .map_err(|err| anyhow::anyhow!("failed to ensure application role: {err}"))?;

    sqlx::query(
        "DO $$\nDECLARE\n    db_name text := $1;\n    role_name text := $2;\nBEGIN\n    IF NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = db_name) THEN\n        EXECUTE format('CREATE DATABASE %I OWNER %I', db_name, role_name);\n    ELSE\n        EXECUTE format('ALTER DATABASE %I OWNER TO %I', db_name, role_name);\n    END IF;\nEND\n$$;",
    )
    .bind(db_name)
    .bind(&app_user)
    .execute(&admin_pool)
    .await
    .map_err(|err| anyhow::anyhow!("failed to ensure database ownership: {err}"))?;

    let target_pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&admin_db_url)
        .await
        .map_err(|err| {
            anyhow::anyhow!(
                "failed to connect to application database as admin: {err}"
            )
        })?;

    sqlx::query(
        "DO $$\nDECLARE\n    role_name text := $1;\nBEGIN\n    EXECUTE format('GRANT ALL PRIVILEGES ON DATABASE %I TO %I', current_database(), role_name);\n    EXECUTE format('GRANT USAGE, CREATE ON SCHEMA public TO %I', role_name);\n    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO %I', role_name);\n    EXECUTE format('GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO %I', role_name);\n    EXECUTE format('ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO %I', role_name);\n    EXECUTE format('ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO %I', role_name);\nEND\n$$;",
    )
    .bind(&app_user)
    .execute(&target_pool)
    .await
    .map_err(|err| anyhow::anyhow!("failed to configure database privileges: {err}"))?;

    info!(
        "Application role '{}' ensured with expected privileges",
        app_user
    );

    Ok(true)
}

#[derive(Debug, Default, Clone)]
struct ResolvedTlsPaths {
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
}

impl ResolvedTlsPaths {
    fn terminates_here(&self) -> bool {
        self.cert.is_some() && self.key.is_some()
    }
}

fn resolve_tls_paths(args: &ServeArgs) -> ResolvedTlsPaths {
    let cert = args
        .cert
        .clone()
        .or_else(|| std::env::var("TLS_CERT_PATH").ok().map(PathBuf::from));
    let key = args
        .key
        .clone()
        .or_else(|| std::env::var("TLS_KEY_PATH").ok().map(PathBuf::from));

    ResolvedTlsPaths { cert, key }
}

fn determine_server_mode(port: u16, tls: &ResolvedTlsPaths) -> ServerMode {
    match (&tls.cert, &tls.key) {
        (Some(cert_path), Some(key_path)) => ServerMode::Https {
            addr: SocketAddr::from(([0, 0, 0, 0], port)),
            tls: TlsCertConfig {
                cert_path: cert_path.clone(),
                key_path: key_path.clone(),
                ..Default::default()
            },
        },
        _ => ServerMode::Http {
            addr: SocketAddr::from(([0, 0, 0, 0], port)),
        },
    }
}

#[derive(Debug)]
enum ServerMode {
    Https {
        addr: SocketAddr,
        tls: TlsCertConfig,
    },
    Http {
        addr: SocketAddr,
    },
}

struct ServerSetup {
    router: Router,
    mode: ServerMode,
}

fn build_server_setup(
    state: AppState,
    config: Arc<Config>,
    args: &ServeArgs,
) -> ServerSetup {
    let tls = resolve_tls_paths(args);
    let router = create_app(state, tls.terminates_here());
    let mode = determine_server_mode(config.server.port, &tls);

    ServerSetup { router, mode }
}

async fn run_server(args: ServeArgs) -> anyhow::Result<()> {
    run_server_with_hooks(args, &ProdStartupHooks).await
}

async fn run_server_with_hooks<H>(
    args: ServeArgs,
    hooks: &H,
) -> anyhow::Result<()>
where
    H: StartupHooks,
{
    let ConfigBootstrap {
        config,
        tmdb_provider,
        database_url,
        with_cache,
        #[cfg(feature = "demo")]
        demo_coordinator,
    } = load_runtime_config(&args).await?;

    let ResourceBootstrap { context, state } = wire_app_resources(
        Arc::clone(&config),
        &database_url,
        tmdb_provider,
        with_cache,
        #[cfg(feature = "demo")]
        demo_coordinator.clone(),
    )
    .await?;

    if args.claim_reset {
        let setup_claim_service = context.setup_claim_service();
        let revoked = setup_claim_service
            .revoke_all(Some("operator reset"))
            .await
            .context("failed to revoke setup claim records")?;
        let purged = setup_claim_service
            .purge_stale(Utc::now())
            .await
            .context("failed to purge stale setup claim records")?;
        info!(revoked, purged, "setup claim records reset");
        return Ok(());
    }

    hooks
        .run(
            Arc::clone(&context),
            &state,
            #[cfg(feature = "demo")]
            demo_coordinator,
        )
        .await?;

    let ServerSetup { router, mode } =
        build_server_setup(state, Arc::clone(&config), &args);

    match mode {
        ServerMode::Https { addr, tls } => {
            info!("TLS enabled - starting HTTPS server");
            info!("Certificate path: {:?}", tls.cert_path);
            info!("Private key path: {:?}", tls.key_path);
            info!(
                "Starting Ferrex Media Server (HTTPS) on {}:{}",
                config.server.host, config.server.port
            );
            let rustls_config = create_tls_acceptor(tls).await?;
            axum_server::bind_rustls(addr, rustls_config)
                .serve(
                    router.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await?;
        }
        ServerMode::Http { addr } => {
            info!(
                "Starting Ferrex Media Server (HTTP) on {}:{}",
                config.server.host, config.server.port
            );
            warn!(
                "TLS is not configured. For production use, set TLS_CERT_PATH and TLS_KEY_PATH environment variables."
            );

            let listener = tokio::net::TcpListener::bind(addr).await?;
            let make_service =
                router.into_make_service_with_connect_info::<SocketAddr>();
            axum::serve(listener, make_service).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ResolvedTlsPaths, ServeArgs, ServerMode, determine_server_mode,
        resolve_tls_paths,
    };
    use std::{ffi::OsString, path::PathBuf};

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn unset(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests run in isolation and restore previous environment state on drop.
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }

        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests run in isolation and restore previous environment state on drop.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: we reinstate the environment variable to its prior state.
            unsafe {
                match &self.previous {
                    Some(prev) => std::env::set_var(self.key, prev),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn sample_args() -> ServeArgs {
        ServeArgs {
            cert: None,
            key: None,
            port: None,
            host: None,
            claim_reset: false,
            #[cfg(feature = "demo")]
            demo: false,
        }
    }

    #[test]
    fn resolve_tls_prefers_args_over_env() {
        let _cert_clear = EnvVarGuard::unset("TLS_CERT_PATH");
        let _key_clear = EnvVarGuard::unset("TLS_KEY_PATH");

        let mut args = sample_args();
        args.cert = Some(PathBuf::from("cert-from-args.pem"));
        args.key = Some(PathBuf::from("key-from-args.pem"));

        let _cert_scope = EnvVarGuard::set("TLS_CERT_PATH", "env-cert.pem");
        let _key_scope = EnvVarGuard::set("TLS_KEY_PATH", "env-key.pem");

        let resolved = resolve_tls_paths(&args);
        assert_eq!(resolved.cert, Some(PathBuf::from("cert-from-args.pem")));
        assert_eq!(resolved.key, Some(PathBuf::from("key-from-args.pem")));
    }

    #[test]
    fn determine_server_mode_returns_https_when_paths_present() {
        let tls = ResolvedTlsPaths {
            cert: Some(PathBuf::from("cert.pem")),
            key: Some(PathBuf::from("key.pem")),
        };

        match determine_server_mode(9443, &tls) {
            ServerMode::Https { addr, tls } => {
                assert_eq!(addr.port(), 9443);
                assert_eq!(tls.cert_path, PathBuf::from("cert.pem"));
                assert_eq!(tls.key_path, PathBuf::from("key.pem"));
            }
            other => panic!("expected HTTPS mode, got {other:?}"),
        }
    }

    #[test]
    fn determine_server_mode_returns_http_when_missing_key() {
        let tls = ResolvedTlsPaths {
            cert: Some(PathBuf::from("cert.pem")),
            key: None,
        };

        match determine_server_mode(8080, &tls) {
            ServerMode::Http { addr } => assert_eq!(addr.port(), 8080),
            other => panic!("expected HTTP mode, got {other:?}"),
        }
    }
}

pub fn create_app(state: AppState, https_terminates_here: bool) -> Router {
    // Create versioned API routes
    let versioned_api = routes::create_api_router(state.clone());

    // Global rate limiting layer using MatchedPath classification
    let rate_limit_layer = {
        use axum::extract::{ConnectInfo, MatchedPath};
        use axum::http::header::{HeaderName, HeaderValue, RETRY_AFTER};
        use ferrex_core::api_routes::v1;
        use ferrex_core::auth::rate_limit::{
            EndpointLimits, RateLimitKey, RateLimiter,
        };
        use ferrex_server::infra::middleware::create_rate_limiter;
        use std::net::SocketAddr;
        use std::time::{SystemTime, UNIX_EPOCH};

        let config = state.config_handle();

        match (config.rate_limiter.as_ref(), config.redis.as_ref()) {
            (Some(settings), Some(redis)) => {
                let configured_limits = settings.config.endpoint_limits.clone();
                match create_rate_limiter(&redis.url, settings.config.clone()) {
                    Ok(limiter) => {
                        let limiter = limiter.clone();
                        Some(axum::middleware::from_fn(move |req: Request<Body>, next: axum::middleware::Next| {
                            let limiter = limiter.clone();
                            let configured_limits = configured_limits.clone();
                            async move {
                                let matched = req
                                    .extensions()
                                    .get::<MatchedPath>()
                                    .map(|m: &MatchedPath| m.as_str().to_string());
                                let limits = configured_limits.clone();
                                let rule_opt = matched.as_deref().and_then(|p| {
                                    if p == v1::auth::LOGIN || p == v1::auth::device::LOGIN { Some(limits.login) }
                                    else if p == v1::auth::REGISTER { Some(limits.register) }
                                    else if p == v1::auth::REFRESH { Some(limits.token_refresh) }
                                    else if p == v1::auth::device::PIN_LOGIN || p == v1::auth::device::PIN_CHALLENGE { Some(limits.pin_auth) }
                                    else if p == v1::setup::CLAIM_START { Some(limits.setup_start) }
                                    else if p == v1::setup::CLAIM_CONFIRM { Some(limits.setup_confirm) }
                                    else if p == v1::setup::CREATE_ADMIN { Some(limits.setup_create_admin) }
                                    else { None }
                                });

                                let Some(rule) = rule_opt else { return Ok::<_, StatusCode>(next.run(req).await); };

                                let mut key = None;
                                if let Some(dev_id) = req.headers().get("X-Device-ID").and_then(|v| v.to_str().ok()).and_then(|s| Uuid::parse_str(s).ok()) {
                                    key = Some(RateLimitKey::DeviceId(dev_id));
                                }
                                if key.is_none() {
                                    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
                                        key = Some(RateLimitKey::IpAddress(addr.ip().to_string()));
                                    } else if let Some(forwarded) = req.headers().get("X-Forwarded-For").and_then(|v| v.to_str().ok()).and_then(|s| s.split(',').next()) {
                                        key = Some(RateLimitKey::IpAddress(forwarded.trim().to_string()));
                                    }
                                }
                                let key = key.unwrap_or_else(|| RateLimitKey::Custom("unknown".to_string()));

                                match limiter.check_and_update(&key, &rule).await {
                                    Ok(decision) if decision.allowed => {
                                        let mut response = next.run(req).await;
                                        let headers = response.headers_mut();
                                        headers.insert(HeaderName::from_static("x-ratelimit-limit"), HeaderValue::from_str(&decision.limit.to_string()).unwrap_or(HeaderValue::from_static("0")));
                                        headers.insert(HeaderName::from_static("x-ratelimit-remaining"), HeaderValue::from_str(&decision.limit.saturating_sub(decision.current_count).to_string()).unwrap_or(HeaderValue::from_static("0")));
                                        let reset = SystemTime::now().checked_add(decision.reset_after).unwrap_or(SystemTime::now()).duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                                        headers.insert(HeaderName::from_static("x-ratelimit-reset"), HeaderValue::from_str(&reset.to_string()).unwrap_or(HeaderValue::from_static("0")));
                                        Ok::<_, StatusCode>(response)
                                    }
                                    Ok(_) => {
                                        let response = Response::builder().status(StatusCode::TOO_MANY_REQUESTS).body(Body::empty()).unwrap();
                                        Ok::<_, StatusCode>(response)
                                    }
                                    Err(ferrex_core::auth::rate_limit::RateLimitError::RateLimitExceeded { retry_after, .. }) => {
                                        let response = Response::builder()
                                            .status(StatusCode::TOO_MANY_REQUESTS)
                                            .header(RETRY_AFTER, HeaderValue::from_str(&retry_after.as_secs().to_string()).unwrap_or(HeaderValue::from_static("60")))
                                            .body(Body::empty())
                                            .unwrap();
                                        Ok::<_, StatusCode>(response)
                                    }
                                    Err(_) => Ok::<_, StatusCode>(next.run(req).await),
                                }
                            }
                        }))
                    }
                    Err(_) => None,
                }
            }
            _ => None,
        }
    };

    // Build CORS layer (permissive in dev, allow-list in prod)
    let cors_layer = if state.config().dev_mode {
        CorsLayer::permissive()
    } else {
        let origins: Vec<axum::http::HeaderValue> = state
            .config()
            .cors
            .allowed_origins
            .iter()
            .filter_map(|s| axum::http::HeaderValue::from_str(s).ok())
            .collect();
        let allow_origin = if origins.is_empty() {
            AllowOrigin::any()
        } else {
            AllowOrigin::list(origins)
        };

        let methods: Vec<Method> = state
            .config()
            .cors
            .allowed_methods
            .iter()
            .map(|m| {
                Method::from_bytes(m.as_bytes())
                    .expect("CORS methods validated during config load")
            })
            .collect();
        let allow_methods = AllowMethods::list(methods);

        let headers: Vec<axum::http::HeaderName> = state
            .config()
            .cors
            .allowed_headers
            .iter()
            .map(|h| {
                axum::http::HeaderName::from_bytes(h.as_bytes())
                    .expect("CORS headers validated during config load")
            })
            .collect();
        let allow_headers = AllowHeaders::list(headers);

        let mut layer = CorsLayer::new()
            .allow_origin(allow_origin)
            .allow_methods(allow_methods)
            .allow_headers(allow_headers);

        if state.config().cors.allow_credentials {
            layer = layer.allow_credentials(true);
        }

        layer
    };

    let hsts_header_value = build_hsts_header(&state.config().security.hsts);
    let trust_proxy_for_hsts = state.config().security.trust_proxy_headers;
    let hsts_layer = axum::middleware::from_fn(
        move |req: Request<Body>, next: axum::middleware::Next| {
            let header_value = hsts_header_value.clone();
            async move {
                use axum::http::header;

                let is_https = if trust_proxy_for_hsts {
                    req.headers()
                        .get("x-forwarded-proto")
                        .and_then(|v| v.to_str().ok())
                        .map(|v| v.eq_ignore_ascii_case("https"))
                        .unwrap_or(false)
                } else {
                    req.uri()
                        .scheme()
                        .map(|s| s.as_str().eq_ignore_ascii_case("https"))
                        .unwrap_or(false)
                };

                let mut response: Response<Body> = next.run(req).await;
                if is_https {
                    if let Some(value) = &header_value {
                        response.headers_mut().insert(
                            header::STRICT_TRANSPORT_SECURITY,
                            value.clone(),
                        );
                    }
                }

                Ok::<Response<Body>, std::convert::Infallible>(response)
            }
        },
    );

    // Public routes
    let mut app = Router::new()
        .route("/ping", get(ping_handler))
        .route("/health", get(health_handler))
        // Add versioned API routes
        .merge(versioned_api)
        // Add middleware layers in correct order (outer to inner):
        // 1. CORS (outermost)
        .layer(cors_layer)
        // 2. Tracing
        .layer(TraceLayer::new_for_http())
        // 3. HSTS header for HTTPS responses only
        .layer(hsts_layer)
        // 3. HTTPS enforcement (redirects before processing) when requested
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            move |State(app_state): State<AppState>,
                  req: Request<Body>,
                  next: axum::middleware::Next| async move {
                use axum::http::header;
                use std::convert::Infallible;

                let enforce_https = app_state.config().security.enforce_https && !app_state.config().dev_mode;

                if !enforce_https || https_terminates_here {
                    return Ok::<_, Infallible>(next.run(req).await);
                }

                // Check if request is HTTPS
                let trust_proxy = app_state.config().security.trust_proxy_headers;

                let is_https = if trust_proxy {
                    req.headers()
                        .get("x-forwarded-proto")
                        .and_then(|v| v.to_str().ok())
                        .map(|v| v.eq_ignore_ascii_case("https"))
                        .unwrap_or(false)
                } else {
                    req.uri()
                        .scheme()
                        .map(|s| s.as_str().eq_ignore_ascii_case("https"))
                        .unwrap_or(false)
                };

                if !is_https {
                    // Build HTTPS URL
                    let uri = req.uri();
                    let host = req
                        .headers()
                        .get(header::HOST)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("localhost");

                    let https_url = format!(
                        "https://{}{}{}",
                        host,
                        uri.path(),
                        uri.query().map(|q| format!("?{}", q)).unwrap_or_default()
                    );

                    return Ok::<_, Infallible>(
                        Response::builder()
                            .status(StatusCode::MOVED_PERMANENTLY)
                            .header(header::LOCATION, https_url)
                            .body(Body::empty())
                            .unwrap(),
                    );
                }

                // Pass through HTTPS requests
                Ok::<_, Infallible>(next.run(req).await)
            },
        ))
        // 4. Rate limiting (before auth to protect auth endpoints)
        ;

    if let Some(layer) = rate_limit_layer {
        app = app.layer(layer);
    }

    app.with_state(state)
}

async fn ping_handler() -> Result<Json<Value>, StatusCode> {
    info!("Ping endpoint called");
    Ok(Json(json!({
        "status": "ok",
        "message": "Rusty Media Server is running",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION")
    })))
}

async fn health_handler(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let mut health_status = json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION"),
        "checks": {}
    });

    // Check database connectivity
    let mut is_unhealthy = false;

    match state
        .unit_of_work()
        .media_files_read
        .stats(MediaFileFilter::default())
        .await
    {
        Ok(stats) => {
            health_status["checks"]["database"] = json!({
                "status": "healthy",
                "total_files": stats.total_files,
                "total_size": stats.total_size
            });
        }
        Err(e) => {
            health_status["checks"]["database"] = json!({
                "status": "unhealthy",
                "error": e.to_string()
            });
            is_unhealthy = true;
        }
    }

    // Check cache if available
    if state.cache_enabled() {
        health_status["checks"]["cache"] = json!({
            "status": "healthy",
            "type": "redis"
        });
    }

    // Check disk space for cache directories
    health_status["checks"]["cache_directories"] = json!({
        "status": "healthy",
        "thumbnail_cache": state.config().thumbnail_cache_dir().exists(),
        "transcode_cache": state.config().transcode_cache_dir().exists()
    });

    if is_unhealthy {
        health_status["status"] = json!("unhealthy");
        Err(StatusCode::SERVICE_UNAVAILABLE)
    } else {
        Ok(Json(health_status))
    }
}
