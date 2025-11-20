//! # Ferrex Server
//!
//! High-performance media server with real-time transcoding, streaming, and synchronized playback support.
//!
//! ## Overview
//!
//! Ferrex Server is a comprehensive media streaming solution that provides:
//!
//! - **Media Streaming**: HLS adaptive bitrate streaming with on-the-fly transcoding
//! - **User Management**: JWT-based authentication with session tracking
//! - **Watch Progress**: Automatic progress tracking and "continue watching" features
//! - **Synchronized Playback**: Real-time synchronized viewing sessions via WebSocket
//! - **Library Management**: Automatic media scanning and metadata enrichment
//! - **API Access**: RESTful API for all features
//!
//! ## Architecture
//!
//! The server is built on Axum and uses:
//! - PostgreSQL for persistent storage
//! - Redis for caching and session management
//! - FFmpeg for transcoding
//! - TMDB for metadata
//!
//! ## API Endpoints
//!
//! ### Authentication
//! - `POST /api/auth/register` - Create new user account
//! - `POST /api/auth/login` - Authenticate and receive tokens
//! - `POST /api/auth/refresh` - Refresh access token
//! - `POST /api/auth/logout` - Invalidate session
//!
//! ### Media
//! - `GET /api/media/:id` - Get media details
//! - `POST /api/media/batch` - Batch fetch multiple media items
//! - `POST /api/media/query` - Query media with filters
//! - `GET /api/stream/:id` - Stream media file
//!
//! ### Watch Status
//! - `POST /api/watch/progress` - Update viewing progress
//! - `GET /api/watch/state` - Get user's watch state
//! - `GET /api/watch/continue` - Get continue watching list
//!
//! ### Synchronized Playback
//! - `POST /api/sync/sessions` - Create sync session
//! - `GET /api/sync/sessions/:code` - Join sync session
//! - `GET /api/sync/ws` - WebSocket for real-time sync
//!
//! ## Configuration
//!
//! Server configuration is loaded from environment variables and config files.

#[cfg(test)]
mod tests;
// See [`config`] module for details.
//
// ## Example Usage
//
// ```bash
// # Start the server
// cargo run --bin ferrex-server
//
// # Register a user
// curl -X POST http://localhost:3000/api/auth/register \
//   -H "Content-Type: application/json" \
//   -d '{"username":"alice","password":"password123","display_name":"Alice"}'
//
// # Login
// curl -X POST http://localhost:3000/api/auth/login \
//   -H "Content-Type: application/json" \
//   -d '{"username":"alice","password":"password123"}'
// ```

//#![warn(missing_docs)]

/// Admin-only management handlers
pub mod admin_handlers;
/// API endpoint handlers organized by functionality
pub mod api;
/// Authentication and JWT token management
pub mod auth;
/// Server configuration and settings
pub mod config;
/// Development utilities (only available in debug builds)
pub mod dev_handlers;
/// Error types and handling
pub mod errors;
/// First-run setup and other handlers
pub mod handlers;
/// Image serving and caching handlers
pub mod image_handlers;
/// Library management API handlers
pub mod library_handlers_v2;
/// Unified media reference API handlers
pub mod media_reference_handlers;
/// External metadata provider integration (TMDB)
pub mod metadata_service;
/// Middleware implementations
pub mod middleware;
/// Movie specific API handlers
pub mod movie_handlers;
/// Media query API handlers
pub mod query_handlers;
/// Role and permission management handlers
pub mod role_handlers;
/// Versioned route organization
pub mod routes;
/// Media scanning API handlers
pub mod scan_handlers;
/// Media library scanning and indexing
pub mod scan_manager;
/// Centralized services for common operations
pub mod services;
/// Session management handlers
pub mod session_handlers;
/// Media streaming handlers
pub mod stream_handlers;
/// Synchronized playback session handlers
pub mod sync_handlers;
/// Development and testing endpoints
pub mod test_endpoints;
/// Integration and unit tests
#[cfg(test)]
pub mod tests;
/// Thumbnail generation and caching
pub mod thumbnail_service;
/// TLS configuration and certificate management
pub mod tls;
/// Video transcoding and HLS streaming
pub mod transcoding;
/// TV show specific API handlers
pub mod tv_handlers;
/// User profile management handlers
pub mod user_handlers;
/// API versioning infrastructure
pub mod versioning;
/// Watch progress tracking handlers
pub mod watch_status_handlers;
/// WebSocket connection management
pub mod websocket;

use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware as axum_middleware,
    response::{Json, Response},
    routing::{get, post, put},
    Router,
};
use chrono;
use clap::Parser;
use config::Config;
use ferrex_core::{
    auth::domain::services::{create_authentication_service, AuthenticationService},
    database::traits::MediaFilters,
    database::PostgresDatabase,
    scanner::FolderMonitor,
    Library, MediaDatabase, MediaEvent, MediaFileMetadata, ParsedMediaInfo, ScanRequest,
};
use futures;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::io::ReaderStream;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use transcoding::handlers::*;
use uuid::Uuid;

use library_handlers_v2::*;
use scan_handlers::*;

/// Command line arguments for the Ferrex media server
#[derive(Parser, Debug)]
#[command(name = "ferrex-server")]
#[command(about = "High-performance media server with real-time transcoding and streaming")]
struct Args {
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
}

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<MediaDatabase>,
    pub database: Arc<MediaDatabase>, // Alias for compatibility
    pub config: Arc<Config>,
    pub metadata_service: Arc<metadata_service::MetadataService>,
    pub thumbnail_service: Arc<thumbnail_service::ThumbnailService>,
    pub scan_manager: Arc<scan_manager::ScanManager>,
    pub transcoding_service: Arc<transcoding::TranscodingService>,
    pub image_service: Arc<ferrex_core::ImageService>,
    pub websocket_manager: Arc<websocket::ConnectionManager>,
    pub auth_service: Arc<AuthenticationService>,
    pub folder_monitor: Arc<FolderMonitor>,
    /// Track admin sessions per device for PIN authentication eligibility
    pub admin_sessions: Arc<Mutex<HashMap<Uuid, AdminSessionInfo>>>,
}

/// Admin session information for PIN authentication tracking
#[derive(Debug, Clone)]
pub struct AdminSessionInfo {
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub authenticated_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub session_token: String,
}

impl AdminSessionInfo {
    pub fn new(user_id: Uuid, device_id: Uuid, session_token: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            user_id,
            device_id,
            authenticated_at: now,
            expires_at: now + chrono::Duration::hours(24), // Admin sessions expire after 24 hours
            session_token,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.expires_at > chrono::Utc::now()
    }
}

impl AppState {
    /// Check if an admin is authenticated on the given device
    pub async fn is_admin_authenticated_on_device(&self, device_id: Uuid) -> bool {
        let admin_sessions = self.admin_sessions.lock().await;
        admin_sessions
            .get(&device_id)
            .map(|session| session.is_valid())
            .unwrap_or(false)
    }

    /// Register an admin session for a device
    pub async fn register_admin_session(
        &self,
        user_id: Uuid,
        device_id: Uuid,
        session_token: String,
    ) -> Result<(), anyhow::Error> {
        // Verify the user is actually an admin
        let user = self
            .db
            .backend()
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User not found"))?;

        // TODO: Add proper admin role checking once role system is implemented
        // For now, assume the caller has already verified admin status

        let mut admin_sessions = self.admin_sessions.lock().await;
        let session_info = AdminSessionInfo::new(user_id, device_id, session_token);
        admin_sessions.insert(device_id, session_info);

        tracing::info!(
            "Admin session registered for device {} by user {}",
            device_id,
            user_id
        );
        Ok(())
    }

    /// Remove admin session for a device
    pub async fn remove_admin_session(&self, device_id: Uuid) {
        let mut admin_sessions = self.admin_sessions.lock().await;
        if admin_sessions.remove(&device_id).is_some() {
            tracing::info!("Admin session removed for device {}", device_id);
        }
    }

    /// Clean up expired admin sessions
    pub async fn cleanup_expired_admin_sessions(&self) {
        let mut admin_sessions = self.admin_sessions.lock().await;
        let initial_count = admin_sessions.len();
        admin_sessions.retain(|_, session| session.is_valid());
        let removed_count = initial_count - admin_sessions.len();

        if removed_count > 0 {
            tracing::info!("Cleaned up {} expired admin sessions", removed_count);
        }
    }

    /// Get admin session info for a device
    pub async fn get_admin_session(&self, device_id: Uuid) -> Option<AdminSessionInfo> {
        let admin_sessions = self.admin_sessions.lock().await;
        admin_sessions
            .get(&device_id)
            .filter(|session| session.is_valid())
            .cloned()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Load configuration from environment
    let mut config = Config::from_env()?;

    // Override config with CLI arguments if provided
    if let Some(port) = args.port {
        config.server_port = port;
    }
    if let Some(host) = args.host {
        config.server_host = host;
    }

    let config = Arc::new(config);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ferrex_server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Log configuration
    info!("Server configuration loaded");
    if let Some(media_root) = &config.media_root {
        info!("Media root: {}", media_root.display());
    } else {
        warn!("No MEDIA_ROOT configured - will require path parameter for scans");
    }

    // Ensure cache directories exist
    config.ensure_directories()?;
    info!("Cache directories created");

    // Create database instance based on configuration
    let db = if let Some(db_url) = &config.database_url {
        // Check if the URL looks like PostgreSQL
        if db_url.starts_with("postgres://") || db_url.starts_with("postgresql://") {
            info!("Connecting to PostgreSQL database");

            // Use Redis cache if available
            let with_cache = config.redis_url.is_some();
            match MediaDatabase::new_postgres(db_url, with_cache).await {
                Ok(database) => {
                    info!("Successfully connected to PostgreSQL");
                    Arc::new(database)
                }
                Err(e) => {
                    error!("Failed to connect to PostgreSQL: {}", e);
                    return Err(anyhow::anyhow!("Database connection failed: {}", e));
                }
            }
        } else {
            error!("Only PostgreSQL database URLs are supported");
            return Err(anyhow::anyhow!(
                "Invalid database URL: must start with postgres:// or postgresql://"
            ));
        }
    } else {
        error!("DATABASE_URL environment variable is required");
        return Err(anyhow::anyhow!("DATABASE_URL not set"));
    };

    if let Err(e) = db.backend().initialize_schema().await {
        warn!("Failed to initialize database schema: {}", e);
    }
    info!("Database initialized successfully");

    // Initialize metadata service
    let tmdb_api_key = std::env::var("TMDB_API_KEY").ok();
    match &tmdb_api_key {
        Some(key) => info!("TMDB API key configured (length: {})", key.len()),
        None => warn!("TMDB_API_KEY not set - metadata fetching will be limited"),
    }
    let metadata_service = Arc::new(metadata_service::MetadataService::new(
        tmdb_api_key,
        config.cache_dir.clone(),
    ));

    let thumbnail_service = Arc::new(
        thumbnail_service::ThumbnailService::new(config.cache_dir.clone(), db.clone())
            .expect("Failed to initialize thumbnail service"),
    );

    let scan_manager = Arc::new(scan_manager::ScanManager::new(
        db.clone(),
        metadata_service.clone(),
        thumbnail_service.clone(),
    ));

    // Initialize transcoding service
    let transcoding_config = transcoding::config::TranscodingConfig {
        ffmpeg_path: config.ffmpeg_path.clone(),
        ffprobe_path: config.ffprobe_path.clone(),
        transcode_cache_dir: config.transcode_cache_dir.clone(),
        ..Default::default()
    };

    let transcoding_service = Arc::new(
        transcoding::TranscodingService::new(transcoding_config, db.clone())
            .await
            .expect("Failed to initialize transcoding service"),
    );

    // Initialize image service
    let image_service = Arc::new(ferrex_core::ImageService::new(
        db.clone(),
        config.cache_dir.clone(),
    ));

    let websocket_manager = Arc::new(websocket::ConnectionManager::new());

    // Initialize authentication service
    let auth_service = {
        // Get the PostgreSQL pool from the MediaDatabase
        let postgres_backend = db
            .as_any()
            .downcast_ref::<PostgresDatabase>()
            .expect("Expected PostgreSQL backend for authentication service");

        Arc::new(create_authentication_service(Arc::new(
            postgres_backend.pool().clone(),
        )))
    };

    // Initialize FolderMonitor
    let folder_monitor = {
        // Load libraries from database
        let libraries = match db.backend().list_libraries().await {
            Ok(libs) => libs,
            Err(e) => {
                warn!("Failed to load libraries for FolderMonitor: {}", e);
                Vec::new()
            }
        };

        // Get the backend as a trait object for FolderMonitor
        let postgres_backend = db
            .as_any()
            .downcast_ref::<PostgresDatabase>()
            .expect("Expected PostgreSQL backend for FolderMonitor");

        // Create FolderMonitorConfig with 60-second scan interval
        let folder_monitor_config = ferrex_core::scanner::FolderMonitorConfig {
            scan_interval_secs: 60,
            max_retry_attempts: 3,
            stale_folder_hours: 24,
            batch_size: 100,
            error_retry_threshold: 3,
        };

        let monitor = Arc::new(FolderMonitor::new(
            Arc::new(postgres_backend.clone())
                as Arc<dyn ferrex_core::database::traits::MediaDatabaseTrait>,
            Arc::new(tokio::sync::RwLock::new(libraries)),
            folder_monitor_config,
        ));

        // Start the folder monitor background task
        if let Err(e) = monitor.clone().start().await {
            warn!("Failed to start FolderMonitor background task: {}", e);
        } else {
            info!("FolderMonitor background task started with 60-second scan interval");
        }

        monitor
    };

    let state = AppState {
        db: db.clone(),
        database: db,
        config: config.clone(),
        metadata_service,
        thumbnail_service,
        scan_manager,
        transcoding_service,
        image_service,
        websocket_manager,
        auth_service,
        folder_monitor,
        admin_sessions: Arc::new(Mutex::new(HashMap::new())),
    };

    // Start periodic cleanup of expired admin sessions
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300)); // Clean up every 5 minutes
        loop {
            interval.tick().await;
            cleanup_state.cleanup_expired_admin_sessions().await;
        }
    });

    let app = create_app(state);

    // Check if TLS is enabled via CLI args or environment variables
    let tls_cert_path = args
        .cert
        .or_else(|| std::env::var("TLS_CERT_PATH").ok().map(PathBuf::from));
    let tls_key_path = args
        .key
        .or_else(|| std::env::var("TLS_KEY_PATH").ok().map(PathBuf::from));

    match (tls_cert_path, tls_key_path) {
        (Some(cert_path), Some(key_path)) => {
            // HTTPS mode with TLS
            info!("TLS enabled - starting HTTPS server");
            info!("Certificate path: {:?}", cert_path);
            info!("Private key path: {:?}", key_path);

            let tls_config = crate::tls::TlsCertConfig {
                cert_path,
                key_path,
                ..Default::default()
            };

            // Create TLS acceptor
            let rustls_config = crate::tls::create_tls_acceptor(tls_config).await?;

            let addr = SocketAddr::from(([0, 0, 0, 0], config.server_port));
            info!(
                "Starting Ferrex Media Server (HTTPS) on {}:{}",
                config.server_host, config.server_port
            );

            axum_server::bind_rustls(addr, rustls_config)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await?;
        }
        _ => {
            // HTTP mode (development/behind reverse proxy)
            let addr = SocketAddr::from(([0, 0, 0, 0], config.server_port));
            info!(
                "Starting Ferrex Media Server (HTTP) on {}:{}",
                config.server_host, config.server_port
            );
            warn!("TLS is not configured. For production use, set TLS_CERT_PATH and TLS_KEY_PATH environment variables.");

            let listener = tokio::net::TcpListener::bind(addr).await?;

            // Create the service with ConnectInfo for client IP tracking
            let make_service = app.into_make_service_with_connect_info::<SocketAddr>();

            axum::serve(listener, make_service).await?;
        }
    }

    Ok(())
}

pub fn create_app(state: AppState) -> Router {
    // Create versioned API routes
    let mut versioned_api = routes::create_api_router(state.clone());

    // Apply rate limiting to API routes
    let rate_limit_config = middleware::rate_limit_setup::RateLimitConfig::default();
    versioned_api =
        middleware::rate_limit_setup::apply_auth_rate_limits(versioned_api, &rate_limit_config);
    versioned_api =
        middleware::rate_limit_setup::apply_public_rate_limits(versioned_api, &rate_limit_config);
    versioned_api =
        middleware::rate_limit_setup::apply_api_rate_limits(versioned_api, &rate_limit_config);

    // Create backward compatibility layer for old API paths
    // This will redirect old paths to v1 endpoints during migration period
    let compatibility_routes = create_compatibility_routes(state.clone());

    // Routes that require authentication (legacy - to be removed)
    let protected_routes = Router::new()
        .route("/api/auth/logout", post(auth::handlers::logout))
        .route("/api/users/me", get(auth::handlers::get_current_user))
        // User management
        .route(
            "/api/user/password",
            axum::routing::put(user_handlers::change_password_handler),
        )
        // Device authentication management (requires auth)
        .route(
            "/api/auth/device/pin/set",
            post(auth::device_handlers::set_device_pin),
        )
        .route(
            "/api/auth/device/pin/change",
            post(auth::device_handlers::change_device_pin),
        )
        .route(
            "/api/auth/device/list",
            get(auth::device_handlers::list_user_devices),
        )
        .route(
            "/api/auth/device/revoke",
            post(auth::device_handlers::revoke_device),
        )
        // Watch status endpoints
        .route(
            "/api/watch/progress",
            post(watch_status_handlers::update_progress_handler),
        )
        .route(
            "/api/watch/state",
            get(watch_status_handlers::get_watch_state_handler),
        )
        .route(
            "/api/watch/continue",
            get(watch_status_handlers::get_continue_watching_handler),
        )
        .route(
            "/api/watch/progress/:media_id",
            axum::routing::delete(watch_status_handlers::clear_progress_handler),
        )
        .route(
            "/api/media/:id/progress",
            get(watch_status_handlers::get_media_progress_handler),
        )
        .route(
            "/api/media/:id/complete",
            post(watch_status_handlers::mark_completed_handler),
        )
        .route(
            "/api/media/:id/is-completed",
            get(watch_status_handlers::is_completed_handler),
        )
        // Protected streaming endpoints with progress tracking
        .route(
            "/api/stream/:id",
            get(stream_handlers::stream_with_progress_handler),
        )
        .route(
            "/api/stream/:id/progress",
            post(stream_handlers::report_progress_handler),
        )
        // Sync session endpoints
        .route(
            "/api/sync/sessions",
            post(sync_handlers::create_sync_session_handler),
        )
        .route(
            "/api/sync/sessions/join/:code",
            get(sync_handlers::join_sync_session_handler),
        )
        .route(
            "/api/sync/sessions/:id",
            axum::routing::delete(sync_handlers::leave_sync_session_handler),
        )
        .route(
            "/api/sync/sessions/:id/state",
            get(sync_handlers::get_sync_session_state_handler),
        )
        // WebSocket endpoint for sync sessions
        .route("/api/sync/ws", get(websocket::handler::websocket_handler))
        // User profile management
        .route("/api/users/:id", get(user_handlers::get_user_handler))
        .route(
            "/api/users/:id",
            axum::routing::put(user_handlers::update_user_handler),
        )
        .route(
            "/api/users/:id",
            axum::routing::delete(user_handlers::delete_user_handler),
        )
        // Session management
        .route(
            "/api/users/sessions",
            get(session_handlers::get_user_sessions_handler),
        )
        .route(
            "/api/users/sessions/:id",
            axum::routing::delete(session_handlers::delete_session_handler),
        )
        .route(
            "/api/users/sessions",
            axum::routing::delete(session_handlers::delete_all_sessions_handler),
        )
        // Folder inventory monitoring
        .route(
            "/api/folders/inventory/:library_id",
            get(handlers::get_folder_inventory),
        )
        .route(
            "/api/folders/progress/:library_id",
            get(handlers::get_scan_progress),
        )
        .route(
            "/api/folders/rescan/:folder_id",
            post(handlers::trigger_folder_rescan),
        )
        // Query system
        .route(
            "/api/media/query",
            post(query_handlers::query_media_handler),
        )
        // PIN authentication routes (require admin session on device)
        .route(
            "/api/auth/pin/authenticate",
            post(auth::pin_handlers::authenticate_with_pin),
        )
        .route("/api/auth/pin/set", post(auth::pin_handlers::set_pin))
        .route(
            "/api/auth/pin/remove/:device_id",
            axum::routing::delete(auth::pin_handlers::remove_pin),
        )
        .route(
            "/api/auth/pin/available/:device_id",
            get(auth::pin_handlers::check_pin_availability),
        )
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ));

    // Admin routes - require both auth and admin middleware
    // Note: Middleware runs in reverse order - auth_middleware runs first, then admin_middleware
    // TODO: Migrate to permission-based middleware with specific permissions per endpoint:
    // - /admin/users: users:read
    // - /admin/users/:id/roles: users:manage_roles
    // - /admin/users/:id (DELETE): users:delete
    // - /admin/users/:id/sessions: users:read
    // - /admin/users/:user_id/sessions/:session_id: users:update
    // - /admin/stats: server:read_settings
    let admin_routes = Router::new()
        .route("/admin/users", get(admin_handlers::list_all_users))
        .route(
            "/admin/users/:id/roles",
            put(admin_handlers::assign_user_roles),
        )
        .route(
            "/admin/users/:id",
            axum::routing::delete(admin_handlers::delete_user_admin),
        )
        .route(
            "/admin/users/:id/sessions",
            get(admin_handlers::get_user_sessions_admin),
        )
        .route(
            "/admin/users/:user_id/sessions/:session_id",
            axum::routing::delete(admin_handlers::revoke_user_session_admin),
        )
        .route("/admin/stats", get(admin_handlers::get_admin_stats))
        // Development/reset endpoints (admin only)
        .route(
            "/admin/dev/reset/check",
            get(dev_handlers::check_reset_status),
        )
        .route(
            "/admin/dev/reset/database",
            post(dev_handlers::reset_database),
        )
        .route("/admin/dev/seed", post(dev_handlers::seed_database))
        // Admin session management for PIN authentication
        .route(
            "/admin/sessions/register",
            post(auth::pin_handlers::register_admin_session),
        )
        .route(
            "/admin/sessions/:device_id",
            axum::routing::delete(auth::pin_handlers::remove_admin_session),
        )
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
        .layer(axum_middleware::from_fn(auth::middleware::admin_middleware));

    // Role management routes
    let role_routes = Router::new()
        .route("/roles", get(role_handlers::list_roles_handler))
        .route("/permissions", get(role_handlers::list_permissions_handler))
        .route(
            "/users/:id/permissions",
            get(role_handlers::get_user_permissions_handler),
        )
        .route(
            "/users/:id/roles",
            put(role_handlers::assign_user_roles_handler),
        )
        .route(
            "/users/:id/permissions/override",
            post(role_handlers::override_user_permission_handler),
        )
        .route(
            "/users/me/permissions",
            get(role_handlers::get_my_permissions_handler),
        )
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ));

    // Public routes
    Router::new()
        .route("/ping", get(ping_handler))
        .route("/health", get(health_handler))
        // Public setup endpoints (for first-run)
        .route("/api/setup/status", get(handlers::check_setup_status))
        .route("/api/setup/admin", post(handlers::create_initial_admin))
        // Public authentication endpoints
        .route("/api/auth/register", post(auth::handlers::register))
        .route("/api/auth/login", post(auth::handlers::login))
        .route("/api/auth/refresh", post(auth::handlers::refresh))
        // Device authentication endpoints
        .route(
            "/api/auth/device/login",
            post(auth::device_handlers::device_login),
        )
        .route(
            "/api/auth/device/pin",
            post(auth::device_handlers::pin_login),
        )
        .route(
            "/api/auth/device/status",
            get(auth::device_handlers::check_device_status),
        )
        // Public user endpoints (for user selection screen)
        .route("/api/users", get(user_handlers::list_users_handler))
        // Add protected routes
        .merge(protected_routes)
        // Add admin routes with /api prefix
        .nest("/api", admin_routes)
        // Add role management routes with /api prefix
        .nest("/api", role_routes)
        .route("/scan", post(scan_handler))
        .route("/scan", get(scan_status_handler))
        // .route("/metadata", post(metadata_handler)) // Old endpoint - deprecated
        // Old library endpoint (returns MediaFiles)
        // .route(
        //     "/library",
        //     get(library_get_handler).post(library_post_handler),
        // )
        .route("/scan/start", post(start_scan_handler))
        .route("/scan/all", post(scan_all_libraries_handler))
        .route("/scan/progress/:id", get(scan_progress_handler))
        .route("/scan/progress/:id/sse", get(scan_progress_sse_handler))
        .route("/scan/active", get(active_scans_handler))
        .route("/scan/history", get(scan_history_handler))
        .route("/scan/cancel/:id", post(cancel_scan_handler))
        .route("/library/events/sse", get(media_events_sse_handler))
        .route("/stream/:id", get(stream_handler))
        // HLS streaming endpoints
        .route("/stream/:id/hls/playlist.m3u8", get(hls_playlist_handler))
        .route("/stream/:id/hls/:segment", get(hls_segment_handler))
        .route("/stream/:id/transcode", get(stream_transcode_handler))
        .route("/transcode/:id", post(start_transcode_handler))
        .route("/transcode/status/:job_id", get(transcode_status_handler))
        // New production transcoding endpoints
        .route(
            "/transcode/:id/adaptive",
            post(start_adaptive_transcode_handler),
        )
        .route(
            "/transcode/:id/segment/:segment_number",
            get(get_segment_handler),
        )
        .route(
            "/transcode/:id/master.m3u8",
            get(get_master_playlist_handler),
        )
        .route(
            "/transcode/:id/variant/:profile/playlist.m3u8",
            get(get_variant_playlist_handler),
        )
        .route(
            "/transcode/:id/variant/:profile/:segment",
            get(get_variant_segment_handler),
        )
        .route("/transcode/cancel/:job_id", post(cancel_transcode_handler))
        .route("/transcode/profiles", get(list_transcode_profiles_handler))
        .route("/transcode/cache/stats", get(transcode_cache_stats_handler))
        .route(
            "/transcode/:id/clear-cache",
            post(clear_transcode_cache_handler),
        )
        .route("/library/status", get(library_status_handler))
        .route("/media/:id/availability", get(media_availability_handler))
        .route("/config", get(config_handler))
        // .route("/metadata/fetch/:id", post(fetch_metadata_handler)) // Old endpoint - deprecated
        // .route("/metadata/fetch-show/:show_name", post(fetch_show_metadata_handler)) // Old endpoint - deprecated
        .route("/poster/:id", get(poster_handler))
        .route("/thumbnail/:id", get(thumbnail_handler))
        // New unified media reference endpoint
        .route(
            "/api/media/:id",
            get(media_reference_handlers::get_media_reference_handler),
        )
        // Batch media fetch endpoint
        .route(
            "/api/media/batch",
            post(media_reference_handlers::get_media_batch_handler),
        )
        // Image serving endpoint (public but client sends auth headers)
        .route(
            "/images/:type/:id/:category/:index",
            get(image_handlers::serve_image_handler),
        )
        .route(
            "/season-poster/:show_name/:season_num",
            get(season_poster_handler),
        )
        // TV Show endpoints (old - using MediaFile)
        // .route("/shows", get(list_shows_handler))
        // .route("/shows/:show_name", get(show_details_handler))
        // .route("/shows/:show_name/episodes", get(show_episodes_handler))
        // .route(
        //     "/shows/:show_name/seasons/:season_num",
        //     get(season_details_handler),
        // )
        // Movie endpoints (old - using MediaFile)
        // .route("/movies", get(list_movies_handler))
        // .route("/movies/:id", get(movie_details_handler))
        // Library management endpoints (old - commented out)
        // .route("/libraries", get(list_libraries_handler).post(create_library_handler))
        // .route("/libraries/:id", get(get_library_handler))
        // .route("/libraries/:id", axum::routing::put(update_library_handler))
        // .route("/libraries/:id", axum::routing::delete(delete_library_handler))
        // .route("/libraries/:id/scan", post(scan_library_handler))
        // New library-centric endpoints
        .route(
            "/libraries",
            get(list_libraries_handler).post(create_library_handler),
        )
        .route("/libraries/:id", get(get_library_handler))
        .route("/libraries/:id", axum::routing::put(update_library_handler))
        .route(
            "/libraries/:id",
            axum::routing::delete(delete_library_handler),
        )
        .route("/libraries/:id/scan", post(scan_library_handler))
        .route("/libraries/:id/media", get(get_library_media_handler))
        .route("/media", post(fetch_media_handler))
        .route("/media/match", post(manual_match_media_handler))
        // Temporary maintenance endpoint
        .route(
            "/maintenance/delete-by-title/:title",
            axum::routing::delete(delete_by_title_handler),
        )
        .route("/metadata/fetch-batch", post(fetch_metadata_batch_handler))
        .route("/posters/batch", post(fetch_posters_batch_handler))
        // .route(
        //     "/metadata/queue-missing",
        //     post(queue_missing_metadata_handler),
        // ) // Old endpoint - deprecated
        // Database maintenance endpoints (for testing/debugging)
        .route("/maintenance/clear-database", post(clear_database_handler))
        // Test endpoints for metadata extraction and transcoding
        .route(
            "/test/metadata/:path",
            get(test_endpoints::test_metadata_extraction),
        )
        .route(
            "/test/transcode/:path",
            post(test_endpoints::test_transcoding),
        )
        .route(
            "/test/transcode/status/:job_id",
            get(test_endpoints::test_transcode_status),
        )
        .route("/test/hls/:path", post(test_endpoints::test_hls_streaming))
        // Add versioned API routes
        .merge(versioned_api)
        // Add compatibility routes (temporary)
        .merge(compatibility_routes)
        // Add middleware layers in correct order (outer to inner):
        // 1. CORS (outermost)
        .layer(CorsLayer::permissive())
        // 2. Tracing
        .layer(TraceLayer::new_for_http())
        // 3. HTTPS enforcement (redirects before processing) - DISABLED IN DEV
        // NOTE: Completely disabled for now to debug connection issues
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            |State(app_state): State<AppState>,
             req: Request<Body>,
             next: axum::middleware::Next| async move {
                use axum::http::header;
                use std::convert::Infallible;

                // Skip HTTPS enforcement in development mode
                if app_state.config.dev_mode || cfg!(debug_assertions) {
                    return Ok::<_, Infallible>(next.run(req).await);
                }

                // Check if request is HTTPS
                let is_https = req
                    .headers()
                    .get("x-forwarded-proto")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v == "https")
                    .unwrap_or_else(|| {
                        req.uri()
                            .scheme()
                            .map(|s| s.as_str() == "https")
                            .unwrap_or(false)
                    });

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
        .layer(axum::middleware::from_fn(
            |request: Request<Body>, next: axum::middleware::Next| async move {
                // Simple pass-through for now - rate limiting can be enhanced later
                // In production, this would check Redis/memory cache for rate limits
                next.run(request).await
            },
        ))
        // 5. Version negotiation (after security, before business logic)
        .layer(axum_middleware::from_fn(versioning::version_middleware))
        .with_state(state)
}

/// Create backward compatibility routes that redirect to v1 endpoints
fn create_compatibility_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // These routes will forward to the v1 equivalents
        // We'll implement actual redirects or proxies later
        // For now, this serves as documentation of what needs compatibility
        .with_state(state)
}

// These types are now in ferrex_core::api_types
// #[derive(Deserialize)]
#[derive(Deserialize)]
struct MetadataRequest {
    path: String,
}

// #[derive(Deserialize)]
// struct CreateLibraryRequest {
//     name: String,
//     library_type: String,
//     paths: Vec<String>,
//     #[serde(default = "default_scan_interval")]
//     scan_interval_minutes: u32,
//     #[serde(default = "default_enabled")]
//     enabled: bool,
// }

// #[derive(Deserialize)]
// struct UpdateLibraryRequest {
//     name: Option<String>,
//     paths: Option<Vec<String>>,
//     scan_interval_minutes: Option<u32>,
//     enabled: Option<bool>,
// }

// fn default_scan_interval() -> u32 {
//     60
// }

// fn default_enabled() -> bool {
//     true
// }

// #[derive(Debug, Deserialize)]
// struct LibraryFilters {
//     media_type: Option<String>,
//     show_name: Option<String>,
//     season: Option<u32>,
//     order_by: Option<String>,
//     limit: Option<u64>,
//     library_id: Option<String>,
// }

// ScanAndStoreRequest - old type, not needed with new scanner
// #[derive(Deserialize)]
// struct ScanAndStoreRequest {
//     #[serde(default)]
//     path: Option<String>,
//     #[serde(default)]
//     max_depth: Option<usize>,
//     #[serde(default)]
//     follow_links: bool,
//     #[serde(default = "default_extract_metadata")]
//     extract_metadata: bool,
// }

// fn default_extract_metadata() -> bool {
//     true
// }

// ScanRequest is now in ferrex_core::api_types
// #[derive(Deserialize)]
// struct ScanRequest {
//     path: String,
//     #[serde(default)]
//     max_depth: Option<usize>,
//     #[serde(default)]
//     follow_links: bool,
// }

// ScanResponse struct removed - using new scan management API

async fn ping_handler() -> Result<Json<Value>, StatusCode> {
    info!("Ping endpoint called");
    Ok(Json(json!({
        "status": "ok",
        "message": "Rusty Media Server is running",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION")
    })))
}

async fn health_handler(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let mut health_status = json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION"),
        "checks": {}
    });

    // Check database connectivity
    let mut is_unhealthy = false;

    match state.db.backend().get_stats().await {
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
    if let Some(_cache) = state.db.cache() {
        health_status["checks"]["cache"] = json!({
            "status": "healthy",
            "type": "redis"
        });
    }

    // Check disk space for cache directories
    health_status["checks"]["cache_directories"] = json!({
        "status": "healthy",
        "thumbnail_cache": state.config.thumbnail_cache_dir.exists(),
        "transcode_cache": state.config.transcode_cache_dir.exists()
    });

    if is_unhealthy {
        health_status["status"] = json!("unhealthy");
        Err(StatusCode::SERVICE_UNAVAILABLE)
    } else {
        Ok(Json(health_status))
    }
}

// Deprecated scan handler - use start_scan_handler instead
async fn scan_handler(_request: Json<ScanRequest>) -> Result<Json<Value>, StatusCode> {
    warn!("Deprecated /scan endpoint called. Use /scan/start instead");
    Ok(Json(json!({
        "status": "error",
        "message": "This endpoint is deprecated. Use POST /scan/start instead",
        "error": "deprecated_endpoint"
    })))
}

async fn scan_status_handler() -> Result<Json<Value>, StatusCode> {
    info!("Scan status requested");
    Ok(Json(json!({
        "status": "ready",
        "message": "Media scanner is ready",
        "supported_extensions": [
            "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv",
            "m4v", "mpg", "mpeg", "3gp", "ogv", "ts", "mts", "m2ts"
        ]
    })))
}

// Deprecated - will be removed
async fn metadata_handler(Json(request): Json<MetadataRequest>) -> Result<Json<Value>, StatusCode> {
    warn!("Deprecated metadata handler called for: {}", request.path);
    Err(StatusCode::NOT_IMPLEMENTED)
}

async fn stream_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    info!("=== STREAM REQUEST DEBUG ===");
    info!("Requested media ID: {}", id);
    info!("Request headers: {:?}", headers);

    // Decode the percent-encoded ID
    let decoded_id = urlencoding::decode(&id).map_err(|e| {
        error!("Failed to decode media ID: {}", e);
        StatusCode::BAD_REQUEST
    })?;

    // Use the decoded ID for database lookup
    let db_id = Uuid::parse_str(&decoded_id).map_err(|e| {
        error!("Failed to parse media ID: {}", e);
        StatusCode::BAD_REQUEST
    })?;

    info!("Database ID to query: {}", db_id);

    // Get media file from database
    let media_file = match state.db.backend().get_media(&db_id).await {
        Ok(Some(media)) => {
            info!("Found media file: {:?}", media.filename);
            info!("Media path: {:?}", media.path);
            media
        }
        Ok(None) => {
            warn!(
                "Media file not found in database for ID: {} (db_id: {})",
                id, db_id
            );
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            warn!(
                "Database error retrieving media {} (db_id: {}): {}",
                id, db_id, e
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Check if file exists on disk
    if !media_file.path.exists() {
        warn!("Media file not found on disk: {:?}", media_file.path);

        // Check if the entire library is offline
        if let Some(media_root) = &state.config.media_root {
            if !media_root.exists() {
                warn!("Media library root is offline: {:?}", media_root);
                // Return 503 Service Unavailable with custom header
                return Ok(Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .header("X-Media-Error", "library-offline")
                    .body(axum::body::Body::empty())
                    .unwrap());
            }
        }

        // Otherwise, it's just this file that's missing
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("X-Media-Error", "file-missing")
            .body(axum::body::Body::empty())
            .unwrap());
    }
    info!("File exists on disk, size: {} bytes", media_file.size);

    // Open file for streaming
    let file = match tokio::fs::File::open(&media_file.path).await {
        Ok(file) => file,
        Err(e) => {
            warn!("Failed to open file {:?}: {}", media_file.path, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get file size
    let file_size = media_file.size;

    // Determine content type based on file extension
    let extension = media_file.path.extension().and_then(|ext| ext.to_str());
    info!("File extension: {:?}", extension);

    let content_type = match extension {
        Some("mp4") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        Some("avi") => "video/x-msvideo",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        Some("flv") => "video/x-flv",
        Some("wmv") => "video/x-ms-wmv",
        Some("m4v") => "video/x-m4v",
        Some("mpg") | Some("mpeg") => "video/mpeg",
        Some("3gp") => "video/3gpp",
        Some("ogv") => "video/ogg",
        Some("ts") => "video/mp2t",
        Some("mts") | Some("m2ts") => "video/mp2t",
        _ => "application/octet-stream",
    };
    info!("Content-Type: {}", content_type);

    // Check for range request (for video seeking)
    if let Some(range_header) = headers.get(header::RANGE) {
        if let Ok(range_str) = range_header.to_str() {
            if let Some(range) = parse_range_header(range_str, file_size) {
                info!("Range request: {}-{}/{}", range.start, range.end, file_size);

                // Seek to the requested position
                use tokio::io::{AsyncReadExt, AsyncSeekExt};
                let mut file = file;
                if let Err(e) = file.seek(std::io::SeekFrom::Start(range.start)).await {
                    warn!("Failed to seek in file: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }

                // Stream the requested range
                let content_length = range.end - range.start + 1;
                info!(
                    "Serving range {}-{} ({} bytes)",
                    range.start, range.end, content_length
                );

                // Use file.take() to limit the read to the requested range
                use tokio_util::io::ReaderStream;
                let limited_file = file.take(content_length);
                let stream = ReaderStream::new(limited_file);

                return Ok(Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_TYPE, content_type)
                    .header(header::CONTENT_LENGTH, content_length.to_string())
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {}-{}/{}", range.start, range.end, file_size),
                    )
                    .header(header::ACCEPT_RANGES, "bytes")
                    // Add cache headers to help with seeking performance
                    .header("Cache-Control", "public, max-age=3600")
                    .header("Connection", "keep-alive")
                    .body(axum::body::Body::from_stream(stream))
                    .unwrap());
            }
        }
    }

    // Stream entire file
    info!(
        "Streaming entire file: {} ({} bytes)",
        media_file.filename, file_size
    );

    let stream = ReaderStream::new(file);
    let body = axum::body::Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        // Add cache headers to help with seeking performance
        .header("Cache-Control", "public, max-age=3600")
        .header("Connection", "keep-alive")
        .body(body)
        .unwrap())
}

#[derive(Debug)]
struct ByteRange {
    start: u64,
    end: u64,
}

fn parse_range_header(range_str: &str, file_size: u64) -> Option<ByteRange> {
    // Parse "bytes=start-end" format
    if !range_str.starts_with("bytes=") {
        return None;
    }

    let range_part = &range_str[6..]; // Remove "bytes="
    let parts: Vec<&str> = range_part.split('-').collect();

    if parts.len() != 2 {
        return None;
    }

    let start = if parts[0].is_empty() {
        // Suffix range: "-1000" (last 1000 bytes)
        if let Ok(suffix_len) = parts[1].parse::<u64>() {
            if suffix_len >= file_size {
                0
            } else {
                file_size - suffix_len
            }
        } else {
            return None;
        }
    } else if let Ok(start) = parts[0].parse::<u64>() {
        start
    } else {
        return None;
    };

    let end = if parts[1].is_empty() {
        // Prefix range: "1000-" (from byte 1000 to end)
        file_size - 1
    } else if let Ok(end) = parts[1].parse::<u64>() {
        std::cmp::min(end, file_size - 1)
    } else {
        return None;
    };

    if start <= end && start < file_size {
        Some(ByteRange { start, end })
    } else {
        None
    }
}

async fn config_handler(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    info!("Config request");

    Ok(Json(json!({
        "status": "success",
        "config": {
            "server_host": state.config.server_host,
            "server_port": state.config.server_port,
            "media_root": state.config.media_root.as_ref().map(|p| p.display().to_string()),
            "dev_mode": state.config.dev_mode,
            "database_configured": state.config.database_url.is_some(),
            "redis_configured": state.config.redis_url.is_some(),
            "transcode_cache_dir": state.config.transcode_cache_dir.display().to_string(),
            "thumbnail_cache_dir": state.config.thumbnail_cache_dir.display().to_string(),
        }
    })))
}

async fn fetch_metadata_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    info!("Metadata fetch request for media ID: {}", id);

    // Get media file from database
    let media_file = match state.db.backend().get_media(&id).await {
        Ok(Some(media)) => media,
        Ok(None) => {
            warn!("Media file not found: {}", id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            warn!("Database error: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // First, re-extract technical metadata from the video file
    let mut updated_media = media_file.clone();
    let mut technical_metadata_extracted = false;

    if media_file.path.exists() {
        info!(
            "Re-extracting technical metadata from: {:?}",
            media_file.path
        );
        let mut extractor = ferrex_core::MetadataExtractor::new();

        match extractor.extract_metadata(&media_file.path) {
            Ok(new_metadata) => {
                info!("Technical metadata extracted successfully:");
                info!("  Color transfer: {:?}", new_metadata.color_transfer);
                info!("  Color space: {:?}", new_metadata.color_space);
                info!("  Color primaries: {:?}", new_metadata.color_primaries);
                info!("  Bit depth: {:?}", new_metadata.bit_depth);

                // Update media with new technical metadata
                if let Some(ref mut existing_metadata) = updated_media.media_file_metadata {
                    // Update technical fields while preserving parsed info
                    let parsed_info = existing_metadata.parsed_info.clone();
                    *existing_metadata = new_metadata;
                    existing_metadata.parsed_info = parsed_info;
                } else {
                    updated_media.media_file_metadata = Some(new_metadata);
                }
                technical_metadata_extracted = true;
            }
            Err(e) => {
                warn!("Failed to extract technical metadata: {}", e);
            }
        }
    } else {
        warn!("Media file not found on disk: {:?}", media_file.path);
    }

    // Store updated media if technical metadata was extracted
    if technical_metadata_extracted {
        match state.db.backend().store_media(updated_media.clone()).await {
            Ok(_) => {
                info!("Technical metadata saved for media: {}", id);
            }
            Err(e) => {
                warn!("Failed to save technical metadata: {}", e);
            }
        }
    }

    // Return simple response since metadata fetching is deprecated
    Ok(Json(json!({
        "status": "deprecated",
        "message": "This endpoint is deprecated. Use the new reference-based API.",
        "technical_metadata_extracted": technical_metadata_extracted
    })))
}

async fn fetch_show_metadata_handler(
    State(_state): State<AppState>,
    Path(show_name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!(
        "TV show metadata fetch request for show: {} - DEPRECATED",
        show_name
    );

    // This endpoint is deprecated - return appropriate response
    Ok(Json(json!({
        "status": "deprecated",
        "message": "This endpoint is deprecated. Use the new reference-based API for TV show metadata.",
        "show_name": show_name
    })))
}

// The rest of the original function has been removed as it relied on deprecated external_info fields

async fn poster_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, StatusCode> {
    info!("Poster request for media ID: {}", id);

    // Check if this is a TV show request
    if id.starts_with("tvshow:") {
        let show_name = id.strip_prefix("tvshow:").unwrap_or(&id);
        info!("TV show poster request for: {}", show_name);

        // Get the first episode of the show to find poster URL
        let filters = MediaFilters {
            media_type: Some("tv_show".to_string()),
            show_name: Some(show_name.to_string()),
            limit: Some(5), // Get more episodes to debug
            ..Default::default()
        };

        info!(
            "Querying for TV show '{}' with filters: media_type='tv_show', show_name='{}'",
            show_name, show_name
        );

        match state.db.backend().list_media(filters).await {
            Ok(episodes) => {
                info!("Found {} episodes for show '{}'", episodes.len(), show_name);

                // Debug: Log what show names we have in the database
                if episodes.is_empty() {
                    // Try to find what shows we DO have
                    let all_tv_filters = MediaFilters {
                        media_type: Some("tv_show".to_string()),
                        limit: Some(100),
                        ..Default::default()
                    };

                    if let Ok(all_episodes) = state.db.backend().list_media(all_tv_filters).await {
                        let mut unique_shows = std::collections::HashSet::new();
                        for ep in &all_episodes {
                            if let Some(parsed) = ep
                                .media_file_metadata
                                .as_ref()
                                .and_then(|m| m.parsed_info.as_ref())
                            {
                                if let ParsedMediaInfo::Episode(episode_info) = parsed {
                                    unique_shows.insert(episode_info.show_name.clone());
                                }
                            }
                        }
                        info!("Available TV shows in database: {:?}", unique_shows);
                        info!(
                            "Requested show '{}' not found. Possible mismatch?",
                            show_name
                        );
                    }
                }

                if let Some(_episode) = episodes.first() {
                    // Since we no longer have external_info in MediaFileMetadata,
                    // poster URLs should come from reference types in the database
                    // Poster handling has been moved to reference types
                } else {
                    warn!("No poster URL found for any episode of TV show '{}'. Episodes found but no metadata.", show_name);

                    // If we have episodes but no poster metadata, trigger a metadata fetch
                    if !episodes.is_empty() {
                        if let Some(first_episode) = episodes.first() {
                            let media_id = first_episode.id.clone();
                            info!(
                                "Triggering metadata fetch for episode {} to get show poster",
                                media_id
                            );

                            // Fire and forget metadata fetch
                            let media_id_str = media_id.to_string();
                            tokio::spawn(async move {
                                let clean_id = if media_id_str.starts_with("media:") {
                                    media_id_str.strip_prefix("media:").unwrap_or(&media_id_str)
                                } else {
                                    &media_id_str
                                };
                                if let Ok(response) = reqwest::Client::new()
                                    .post(&format!(
                                        "http://localhost:3000/metadata/fetch/{}",
                                        clean_id
                                    ))
                                    .send()
                                    .await
                                {
                                    info!(
                                        "Metadata fetch triggered for {}: {:?}",
                                        clean_id,
                                        response.status()
                                    );
                                }
                            });
                        }
                    }
                }
                warn!("No poster available for TV show '{}' yet", show_name);
                Err(StatusCode::NOT_FOUND)
            }
            Err(e) => {
                warn!("Failed to fetch TV show episodes: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        // Regular media ID handling
        // Extract just the UUID part (consistent with caching logic)
        let media_id = id.split(':').last().unwrap_or(&id);

        // Check for cached poster
        if let Some(poster_path) = state.metadata_service.get_cached_poster(media_id) {
            // Serve the cached poster file
            match tokio::fs::read(&poster_path).await {
                Ok(bytes) => {
                    let mut response = Response::new(bytes.into());

                    // Determine content type based on file extension
                    let content_type = if poster_path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| ext.eq_ignore_ascii_case("png"))
                        .unwrap_or(false)
                    {
                        "image/png"
                    } else {
                        "image/jpeg"
                    };

                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        header::HeaderValue::from_static(content_type),
                    );
                    Ok(response)
                }
                Err(e) => {
                    warn!("Failed to read poster file: {}", e);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        } else {
            // No cached poster available - try to fetch and cache it
            info!(
                "Poster not cached for {}, attempting to fetch from TMDB",
                media_id
            );

            // Try to get movie reference to find poster path
            use ferrex_core::media::MovieID;

            if let Ok(movie_id) = MovieID::new(media_id.to_string()) {
                match state.db.backend().get_movie_reference(&movie_id).await {
                    Ok(movie_ref) => {
                        // Extract poster path from metadata
                        if let ferrex_core::media::MediaDetailsOption::Details(
                            ferrex_core::media::TmdbDetails::Movie(details),
                        ) = &movie_ref.details
                        {
                            if let Some(poster_path) = &details.poster_path {
                                // Cache the poster
                                match state
                                    .metadata_service
                                    .cache_poster(poster_path, media_id)
                                    .await
                                {
                                    Ok(cached_path) => {
                                        // Read and serve the newly cached poster
                                        match tokio::fs::read(&cached_path).await {
                                            Ok(bytes) => {
                                                let mut response = Response::new(bytes.into());
                                                response.headers_mut().insert(
                                                    header::CONTENT_TYPE,
                                                    header::HeaderValue::from_static("image/png"),
                                                );
                                                return Ok(response);
                                            }
                                            Err(e) => {
                                                warn!("Failed to read newly cached poster: {}", e);
                                                return Err(StatusCode::INTERNAL_SERVER_ERROR);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to cache poster for {}: {}", media_id, e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        info!("Not a movie reference, trying series: {}", e);
                        // Try as series
                        use ferrex_core::media::SeriesID;
                        if let Ok(series_id) = SeriesID::new(media_id.to_string()) {
                            match state.db.backend().get_series_reference(&series_id).await {
                                Ok(series_ref) => {
                                    if let ferrex_core::media::MediaDetailsOption::Details(
                                        ferrex_core::media::TmdbDetails::Series(details),
                                    ) = &series_ref.details
                                    {
                                        if let Some(poster_path) = &details.poster_path {
                                            // Cache the poster
                                            match state
                                                .metadata_service
                                                .cache_poster(poster_path, media_id)
                                                .await
                                            {
                                                Ok(cached_path) => {
                                                    // Read and serve the newly cached poster
                                                    match tokio::fs::read(&cached_path).await {
                                                        Ok(bytes) => {
                                                            let mut response =
                                                                Response::new(bytes.into());
                                                            response.headers_mut().insert(
                                                                header::CONTENT_TYPE,
                                                                header::HeaderValue::from_static(
                                                                    "image/png",
                                                                ),
                                                            );
                                                            return Ok(response);
                                                        }
                                                        Err(e) => {
                                                            warn!("Failed to read newly cached poster: {}", e);
                                                            return Err(
                                                                StatusCode::INTERNAL_SERVER_ERROR,
                                                            );
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    warn!(
                                                        "Failed to cache poster for {}: {}",
                                                        media_id, e
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to get series reference for {}: {}", media_id, e);
                                }
                            }
                        }
                    }
                }
            }

            // If we get here, we couldn't fetch/cache the poster
            Err(StatusCode::NOT_FOUND)
        }
    }
}

async fn season_poster_handler(
    State(state): State<AppState>,
    Path((show_name, season_num)): Path<(String, u32)>,
) -> Result<Response, StatusCode> {
    // Decode + signs back to spaces (form encoding artifact)
    let show_name = show_name.replace("+", " ");
    info!(
        "Season poster request for show: {}, season: {}",
        show_name, season_num
    );

    // Check for cached season poster
    if let Some(poster_path) = state
        .metadata_service
        .get_cached_season_poster(&show_name, season_num)
    {
        // Serve the cached poster file
        match tokio::fs::read(&poster_path).await {
            Ok(bytes) => {
                let mut response = Response::new(bytes.into());
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("image/jpeg"),
                );
                Ok(response)
            }
            Err(e) => {
                warn!("Failed to read season poster file: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        // No cached poster available
        Err(StatusCode::NOT_FOUND)
    }
}

async fn thumbnail_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, StatusCode> {
    info!("Thumbnail request for media ID: {}", id);

    // Try to get or extract thumbnail
    match state.thumbnail_service.get_or_extract_thumbnail(&id).await {
        Ok(thumbnail_path) => {
            // Serve the thumbnail file
            match tokio::fs::read(&thumbnail_path).await {
                Ok(bytes) => {
                    let mut response = Response::new(bytes.into());
                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        header::HeaderValue::from_static("image/jpeg"),
                    );
                    Ok(response)
                }
                Err(e) => {
                    warn!("Failed to read thumbnail file: {}", e);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Err(e) => {
            warn!("Failed to get thumbnail for {}: {}", id, e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

// Temporary maintenance handler to delete media by title
async fn delete_by_title_handler(
    State(state): State<AppState>,
    Path(title): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!(
        "Maintenance: Deleting all media with title containing: {}",
        title
    );

    // Get all media files
    let all_media = match state.db.backend().get_all_media().await {
        Ok(media) => media,
        Err(e) => {
            warn!("Failed to get all media: {}", e);
            return Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })));
        }
    };

    let mut deleted_count = 0;
    let mut errors = Vec::new();
    let title_lower = title.to_lowercase();

    for media in all_media {
        // Check if filename contains the title (case insensitive)
        if media.filename.to_lowercase().contains(&title_lower) {
            info!("Deleting media: {} (ID: {})", media.filename, media.id);

            if let Err(e) = state.db.backend().delete_media(&media.id.to_string()).await {
                errors.push(format!("Failed to delete {}: {}", media.filename, e));
            } else {
                deleted_count += 1;

                // Clean up thumbnail
                let thumbnail_path = state
                    .thumbnail_service
                    .get_thumbnail_path(media.id.as_ref());
                if thumbnail_path.exists() {
                    if let Err(e) = tokio::fs::remove_file(&thumbnail_path).await {
                        warn!("Failed to delete thumbnail: {}", e);
                    }
                }

                // Clean up poster
                let poster_path = state.metadata_service.get_poster_path(media.id.as_ref());
                if poster_path.exists() {
                    if let Err(e) = tokio::fs::remove_file(&poster_path).await {
                        warn!("Failed to delete poster: {}", e);
                    }
                }
            }
        }
    }

    Ok(Json(json!({
        "status": "success",
        "message": format!("Deleted {} media files containing '{}'", deleted_count, title),
        "deleted": deleted_count,
        "errors": errors
    })))
}

// Clear all media from the database (for testing/debugging)
async fn clear_database_handler(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    info!("Maintenance: Clearing entire database");

    // Get all media files
    let all_media = match state.db.backend().get_all_media().await {
        Ok(media) => media,
        Err(e) => {
            warn!("Failed to get all media: {}", e);
            return Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })));
        }
    };

    let total_count = all_media.len();
    let mut deleted_count = 0;
    let mut errors = Vec::new();

    for media in all_media {
        info!("Deleting media: {} (ID: {})", media.filename, media.id);

        if let Err(e) = state.db.backend().delete_media(&media.id.to_string()).await {
            errors.push(format!("Failed to delete {}: {}", media.filename, e));
        } else {
            deleted_count += 1;

            let media_id_str = media.id.to_string();

            // Clean up thumbnail
            let thumbnail_path = state.thumbnail_service.get_thumbnail_path(&media.id);
            if thumbnail_path.exists() {
                if let Err(e) = tokio::fs::remove_file(&thumbnail_path).await {
                    warn!("Failed to delete thumbnail: {}", e);
                }
            }

            // Clean up poster (try both PNG and JPG)
            let png_poster_path = state
                .config
                .cache_dir
                .join("posters")
                .join(format!("{}_poster.png", media_id_str));
            if png_poster_path.exists() {
                if let Err(e) = tokio::fs::remove_file(&png_poster_path).await {
                    warn!("Failed to delete PNG poster: {}", e);
                }
            }

            let jpg_poster_path = state
                .config
                .cache_dir
                .join("posters")
                .join(format!("{}_poster.jpg", media_id_str));
            if jpg_poster_path.exists() {
                if let Err(e) = tokio::fs::remove_file(&jpg_poster_path).await {
                    warn!("Failed to delete JPG poster: {}", e);
                }
            }
        }
    }

    // Clear the entire poster cache directory as a final cleanup
    let poster_cache_dir = state.config.cache_dir.join("posters");
    if poster_cache_dir.exists() {
        match tokio::fs::read_dir(&poster_cache_dir).await {
            Ok(mut entries) => {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if let Err(e) = tokio::fs::remove_file(entry.path()).await {
                        warn!(
                            "Failed to delete poster cache file {:?}: {}",
                            entry.path(),
                            e
                        );
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read poster cache directory: {}", e);
            }
        }
    }

    Ok(Json(json!({
        "status": "success",
        "message": format!("Cleared database: deleted {} out of {} media files", deleted_count, total_count),
        "total": total_count,
        "deleted": deleted_count,
        "errors": errors
    })))
}

#[derive(Deserialize)]
struct BatchMetadataRequest {
    media_ids: Vec<Uuid>,
    priority: Option<String>, // "posters_only" or "full"
}

#[derive(Serialize)]
struct BatchMetadataResponse {
    updated: Vec<String>,
    errors: Vec<String>,
}

async fn fetch_metadata_batch_handler(
    State(state): State<AppState>,
    Json(request): Json<BatchMetadataRequest>,
) -> Result<Json<Value>, StatusCode> {
    info!(
        "Batch metadata request for {} items",
        request.media_ids.len()
    );

    let priority = request.priority.as_deref().unwrap_or("posters_only");
    let mut updated: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // Limit batch size to prevent overload
    let batch_size = std::cmp::min(request.media_ids.len(), 50);
    let media_ids = &request.media_ids[..batch_size];

    // Use semaphore to limit concurrent requests
    let semaphore = Arc::new(Semaphore::new(5)); // Max 5 concurrent metadata fetches

    let futures = media_ids.iter().map(|id| {
        let state = state.clone();
        let semaphore = semaphore.clone();
        let id = id.clone();
        let priority = priority.to_string();

        async move {
            let _permit = semaphore.acquire().await.unwrap();

            // Get media from database
            match state.db.backend().get_media(&id).await {
                Ok(Some(media)) => {
                    // Batch metadata fetching is deprecated
                    // TMDB metadata should come from reference types in the database
                    Err(format!(
                        "Batch metadata fetching is deprecated for media ID: {}",
                        id
                    ))
                }
                Ok(None) => Err(format!("Media not found: {}", id)),
                Err(e) => Err(format!("Database error for {}: {}", id, e)),
            }
        }
    });

    // Process all requests concurrently
    let results = futures::future::join_all(futures).await;

    for result in results {
        match result {
            Ok(id) => updated.push(id),
            Err(e) => errors.push(e),
        }
    }

    Ok(Json(json!({
        "status": "success",
        "updated": updated,
        "errors": errors,
        "total_requested": media_ids.len(),
        "total_updated": updated.len()
    })))
}

#[derive(Deserialize)]
struct BatchPostersRequest {
    media_ids: Vec<String>,
}

#[derive(Serialize)]
struct PosterInfo {
    media_id: String,
    has_poster: bool,
    poster_url: Option<String>,
}

async fn fetch_posters_batch_handler(
    State(state): State<AppState>,
    Json(request): Json<BatchPostersRequest>,
) -> Result<Json<Value>, StatusCode> {
    info!("Batch poster check for {} items", request.media_ids.len());

    let mut poster_info = Vec::new();

    for id in request.media_ids.iter().take(100) {
        // Limit to 100 items
        let media_id = id.split(':').last().unwrap_or(id);
        let has_poster = state.metadata_service.get_cached_poster(media_id).is_some();

        poster_info.push(PosterInfo {
            media_id: id.clone(),
            has_poster,
            poster_url: if has_poster {
                Some(format!("/poster/{}", media_id))
            } else {
                None
            },
        });
    }

    Ok(Json(json!({
        "status": "success",
        "posters": poster_info
    })))
}

#[derive(Deserialize)]
struct QueueMissingMetadataRequest {
    media_ids: Vec<Uuid>,
}

async fn queue_missing_metadata_handler(
    State(state): State<AppState>,
    Json(request): Json<QueueMissingMetadataRequest>,
) -> Result<Json<Value>, StatusCode> {
    info!(
        "Queuing metadata fetch for {} items",
        request.media_ids.len()
    );

    // Spawn a background task to fetch metadata without blocking
    let scan_manager = state.scan_manager.clone();
    let db = state.db.clone();
    let metadata_service = state.metadata_service.clone();
    let media_ids = request.media_ids.clone();

    tokio::spawn(async move {
        // Process in small batches with delays to avoid overloading
        for chunk in media_ids.chunks(10) {
            for id in chunk {
                // Get media from database
                if let Ok(Some(media)) = db.backend().get_media(id).await {
                    // DEPRECATED: External metadata fetching disabled during transition
                    // The new reference-based API handles metadata differently
                }

                // Small delay between items
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            // Longer delay between batches
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });

    Ok(Json(json!({
        "status": "success",
        "message": format!("Queued {} items for metadata fetch", request.media_ids.len())
    })))
}

async fn library_status_handler(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    // Check if media root exists
    let library_status = if let Some(media_root) = &state.config.media_root {
        if media_root.exists() {
            "online"
        } else {
            "offline"
        }
    } else {
        "not_configured"
    };

    Ok(Json(json!({
        "status": library_status,
        "media_root": state.config.media_root.as_ref().map(|p| p.display().to_string()),
        "media_root_exists": state.config.media_root.as_ref().map(|p| p.exists()).unwrap_or(false)
    })))
}

async fn media_availability_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    // Get media file from database
    let media_file = match state.db.backend().get_media(&id).await {
        Ok(Some(media)) => media,
        Ok(None) => {
            return Ok(Json(json!({
                "available": false,
                "reason": "not_found",
                "message": "Media not found in database"
            })));
        }
        Err(_) => {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Check if file exists
    if !media_file.path.exists() {
        // Check if library is offline
        if let Some(media_root) = &state.config.media_root {
            if !media_root.exists() {
                return Ok(Json(json!({
                    "available": false,
                    "reason": "library_offline",
                    "message": "Media library is offline",
                    "path": media_file.path.display().to_string()
                })));
            }
        }

        // File is missing but library is online
        return Ok(Json(json!({
            "available": false,
            "reason": "file_missing",
            "message": "Media file not found on disk",
            "path": media_file.path.display().to_string()
        })));
    }

    // File exists and is available
    Ok(Json(json!({
        "available": true,
        "path": media_file.path.display().to_string(),
        "size": media_file.size
    })))
}
