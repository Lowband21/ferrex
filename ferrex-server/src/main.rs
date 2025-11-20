//! # Ferrex Server
//!
//! High-performance media server.
//!
//! ## Overview
//!
//! Ferrex Server is a comprehensive media streaming solution that provides:
//!
//! - **Media Streaming**: Simple direct streaming with transcoding on the way
//! - **User Management**: JWT-based authentication with session tracking
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
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{Json, Response},
    routing::{get, post},
};
use clap::Parser;
use ferrex_core::{
    LibraryActorConfig, LibraryReference, MediaDatabase, ScanRequest,
    auth::domain::services::{AuthenticationService, create_authentication_service},
    database::PostgresDatabase,
};
use ferrex_server::{
    infra::{
        app_state::AppState,
        config::Config,
        middleware::rate_limit_setup::{self, RateLimitConfig},
        orchestration::ScanOrchestrator,
        scan::scan_manager::ScanControlPlane,
        websocket,
    },
    media::prep::thumbnail_service::ThumbnailService,
    routes,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

use ferrex_server::users::auth::tls::{TlsCertConfig, create_tls_acceptor};

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
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                // Quieter defaults with focused scan summaries. Override via RUST_LOG.
                "info,scan::summary=info,scan::queue=info,scan::seed=info,tower_http=warn".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Log configuration
    info!("Server configuration loaded");
    let queue_cfg = &config.scanner.orchestrator.queue;
    let budget_cfg = &config.scanner.orchestrator.budget;
    info!(
        scanner.max_parallel_scans = queue_cfg.max_parallel_scans,
        scanner.max_parallel_scans_per_device = queue_cfg.max_parallel_scans_per_device,
        scanner.budget_library_scan_limit = budget_cfg.library_scan_limit,
        scanner.actor_outstanding_cap = config.scanner.library_actor_max_outstanding_jobs,
        scanner.quiescence_ms = config.scanner.quiescence_window_ms,
        "scanner configuration in effect"
    );
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
            info!("Connecting to PostgreSQL database at {}", db_url);

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

    // TMDB integration handled via ferrex_core providers and orchestrator
    let tmdb_api_key = std::env::var("TMDB_API_KEY").ok();
    match &tmdb_api_key {
        Some(key) => info!("TMDB API key configured (length: {})", key.len()),
        None => warn!("TMDB_API_KEY not set - metadata fetching will be limited"),
    }

    let thumbnail_service = Arc::new(
        ThumbnailService::new(config.cache_dir.clone(), db.clone())
            .expect("Failed to initialize thumbnail service"),
    );

    let image_service = Arc::new(ferrex_core::ImageService::new(
        db.clone(),
        config.cache_dir.clone(),
    ));

    let tmdb_provider = Arc::new(ferrex_core::providers::TmdbApiProvider::new());
    let orchestrator = Arc::new(
        ScanOrchestrator::postgres(
            config.scanner.orchestrator.clone(),
            db.clone(),
            tmdb_provider.clone(),
            image_service.clone(),
        )
        .await?,
    );

    let libraries = db
        .backend()
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
            max_outstanding_jobs: config.scanner.library_actor_max_outstanding_jobs,
        };

        orchestrator
            .register_library(actor_config)
            .await
            .with_context(|| format!("failed to register library {}", library.name))?;
    }

    info!(
        registered = libraries.len(),
        watchers_enabled = watch_enabled,
        watchers_disabled = libraries.len().saturating_sub(watch_enabled),
        "libraries registered with orchestrator"
    );

    orchestrator.start().await?;

    let quiescence = Duration::from_millis(config.scanner.quiescence_window_ms.max(1));
    let scan_control = Arc::new(ScanControlPlane::with_quiescence_window(
        db.clone(),
        orchestrator.clone(),
        quiescence,
    ));

    /*
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
    );*/

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

    let state = AppState {
        db: db.clone(),
        config: config.clone(),
        thumbnail_service,
        scan_control: scan_control.clone(),
        //transcoding_service,
        image_service,
        websocket_manager,
        auth_service,
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
            warn!(
                "TLS is not configured. For production use, set TLS_CERT_PATH and TLS_KEY_PATH environment variables."
            );

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
    let rate_limit_config = RateLimitConfig::default();
    versioned_api = rate_limit_setup::apply_auth_rate_limits(versioned_api, &rate_limit_config);
    versioned_api = rate_limit_setup::apply_public_rate_limits(versioned_api, &rate_limit_config);
    versioned_api = rate_limit_setup::apply_api_rate_limits(versioned_api, &rate_limit_config);

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

/// Create backward compatibility routes that redirect to v1 endpoints
fn create_compatibility_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Auth redirects: /api/auth/* -> /api/v1/auth/*
        .route("/api/library/events/sse", get(redirect_to_v1))
        //
        .route("/api/auth/register", post(redirect_to_v1))
        .route("/api/auth/login", post(redirect_to_v1))
        .route("/api/auth/refresh", post(redirect_to_v1))
        .route("/api/auth/logout", post(redirect_to_v1))
        .route("/api/auth/device/login", post(redirect_to_v1))
        .route("/api/auth/device/pin", post(redirect_to_v1))
        .route("/api/auth/device/status", get(redirect_to_v1))
        // User redirects: /api/users/* -> /api/v1/users/*
        .route("/api/users/me", get(redirect_to_v1))
        .route("/api/users", get(redirect_to_v1))
        .route("/api/users/{id}", get(redirect_to_v1))
        // Media redirects: /api/media/* -> /api/v1/media/*
        .route("/api/media/query", post(redirect_to_v1))
        // Watch status redirects: /api/watch/* -> /api/v1/watch/*
        .route("/api/watch/progress", post(redirect_to_v1))
        .route("/api/watch/state", get(redirect_to_v1))
        .route("/api/watch/continue", get(redirect_to_v1))
        // Setup redirects: /api/setup/* -> /api/v1/setup/*
        .route("/api/setup/status", get(redirect_to_v1))
        .route("/api/setup/admin", post(redirect_to_v1))
        .with_state(state)
}

// Add this redirect handler function to main.rs
async fn redirect_to_v1(uri: axum::http::Uri) -> Response {
    let new_path = uri.path().replace("/api/", "/api/v1/");
    let new_uri = if let Some(query) = uri.query() {
        format!("{}?{}", new_path, query)
    } else {
        new_path
    };

    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header("Location", new_uri)
        .header("X-API-Migration", "Redirected to v1")
        .body(Body::empty())
        .unwrap()
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

// Legacy metadata fetch endpoints removed; metadata is managed by the orchestration pipeline

// Legacy season poster handler removed; use /api/v1/images routes instead

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

            match state.db.backend().delete_media(&media.id.to_string()).await {
                Err(e) => {
                    errors.push(format!("Failed to delete {}: {}", media.filename, e));
                }
                _ => {
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

                    // Posters/images handled by ImageService; no direct FS poster cleanup
                }
            }
        }
    }

    // Cleanup orphaned images after deletions
    match state.image_service.cleanup_orphaned().await {
        Ok(count) => info!("Cleaned up {} orphaned images", count),
        Err(e) => warn!("Failed to cleanup orphaned images: {}", e),
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

        match state.db.backend().delete_media(&media.id.to_string()).await {
            Err(e) => {
                errors.push(format!("Failed to delete {}: {}", media.filename, e));
            }
            _ => {
                deleted_count += 1;

                let media_id_str = media.id.to_string();

                // Clean up thumbnail
                let thumbnail_path = state.thumbnail_service.get_thumbnail_path(&media.id);
                if thumbnail_path.exists() {
                    if let Err(e) = tokio::fs::remove_file(&thumbnail_path).await {
                        warn!("Failed to delete thumbnail: {}", e);
                    }
                }

                // Posters/images handled by ImageService; no direct FS poster cleanup
            }
        }
    }

    // Cleanup orphaned images after mass deletions
    match state.image_service.cleanup_orphaned().await {
        Ok(count) => info!("Cleaned up {} orphaned images after clearing DB", count),
        Err(e) => warn!("Failed to cleanup orphaned images: {}", e),
    }

    Ok(Json(json!({
        "status": "success",
        "message": format!("Cleared database: deleted {} out of {} media files", deleted_count, total_count),
        "total": total_count,
        "deleted": deleted_count,
        "errors": errors
    })))
}

// Legacy batch metadata and poster endpoints removed

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
