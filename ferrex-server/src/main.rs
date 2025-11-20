//! # Ferrex Server
//!
//! High-performance media server with real-time transcoding, streaming, and synchronized playback support.
//!
//! ## Overview
//!
//! Ferrex Server is a comprehensive media streaming solution that provides or plans to provide:
//!
//! - **Media Streaming**: HLS adaptive bitrate streaming with on-the-fly transcoding
//! - **User Management**: JWT-based authentication with session tracking
//! - **Watch Progress**: Automatic progress tracking and "continue watching" features
//! - **Synchronized Playback**: Real-time synchronized viewing sessions via WebSocket
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

/// Versioned route organization
pub mod routes;

/// Media scanning, preprocessing, and serving
pub mod media;

/// User management, authentication, and user-specific data
pub mod users;

/// Media streaming and transcoding
pub mod stream;

/// WebSocket connection management
pub mod websocket;

/// Middleware implementations
pub mod middleware;

/// Server config
pub mod config;

/// Error types and handling
pub mod errors;

/// Dev handlers
pub mod dev_handlers;

use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{Json, Response},
    routing::{get, post},
    Router,
};
use clap::Parser;
use config::Config as ExtConfig;
use ferrex_core::{
    auth::domain::services::{create_authentication_service, AuthenticationService},
    database::PostgresDatabase,
    scanner::FolderMonitor,
    MediaDatabase, ScanRequest,
};
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
use uuid::Uuid;

use crate::{media::{metadata_service::MetadataService, prep::thumbnail_service::ThumbnailService, scan::scan_manager::ScanManager}, stream::transcoding::{config::TranscodingConfig, TranscodingService}, users::auth::tls::{create_tls_acceptor, TlsCertConfig}};

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

// Server application state
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<MediaDatabase>,
    pub database: Arc<MediaDatabase>, // Alias for compatibility
    pub config: Arc<ExtConfig>,
    pub metadata_service: Arc<MetadataService>,
    pub thumbnail_service: Arc<ThumbnailService>,
    pub scan_manager: Arc<ScanManager>,
    pub transcoding_service: Arc<TranscodingService>,
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
    let mut config = ExtConfig::from_env()?;

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
                .unwrap_or_else(|_| "ferrex_server=debug,ferrex_core=debug,tower_http=debug".into()),
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
    let metadata_service = Arc::new(MetadataService::new(
        tmdb_api_key,
        config.cache_dir.clone(),
    ));

    let thumbnail_service = Arc::new(
        ThumbnailService::new(config.cache_dir.clone(), db.clone())
            .expect("Failed to initialize thumbnail service"),
    );

    let scan_manager = Arc::new(ScanManager::new(
        db.clone(),
        metadata_service.clone(),
        thumbnail_service.clone(),
    ));

    // Initialize transcoding service
    let transcoding_config = TranscodingConfig {
        ffmpeg_path: config.ffmpeg_path.clone(),
        ffprobe_path: config.ffprobe_path.clone(),
        transcode_cache_dir: config.transcode_cache_dir.clone(),
        ..Default::default()
    };

    let transcoding_service = Arc::new(
        TranscodingService::new(transcoding_config, db.clone())
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
        match monitor.clone().start().await { Err(e) => {
            warn!("Failed to start FolderMonitor background task: {}", e);
        } _ => {
            info!("FolderMonitor background task started with 60-second scan interval");
        }}

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

            let tls_config = TlsCertConfig {
                cert_path,
                key_path,
                ..Default::default()
            };

            // Create TLS acceptor
            let rustls_config = create_tls_acceptor(tls_config).await?;

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

    // Public routes
    Router::new()
        .route("/ping", get(ping_handler))
        .route("/health", get(health_handler))
        // Add versioned API routes
        .merge(versioned_api)
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
        .with_state(state)
}

// These types are now in ferrex_core::api_types
// #[derive(Deserialize)]
#[derive(Deserialize)]
struct MetadataRequest {
    path: String,
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
            file_size.saturating_sub(suffix_len)
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

            match state.db.backend().delete_media(&media.id.to_string()).await { Err(e) => {
                errors.push(format!("Failed to delete {}: {}", media.filename, e));
            } _ => {
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
            }}
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

        match state.db.backend().delete_media(&media.id.to_string()).await { Err(e) => {
            errors.push(format!("Failed to delete {}: {}", media.filename, e));
        } _ => {
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
        }}
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
        let id = *id;
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
        let media_id = id.split(':').next_back().unwrap_or(id);
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
