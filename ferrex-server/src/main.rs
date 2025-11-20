pub mod config;
pub mod metadata_service;
pub mod scan_manager;
pub mod thumbnail_service;
pub mod transcoding;
pub mod test_endpoints;

use scan_manager::MediaEvent;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{sse::Sse, Json, Response},
    routing::{get, post},
    Router,
};
use config::Config;
use ferrex_core::{
    database::traits::MediaFilters, EpisodeSummary, Library, LibraryType, MediaDatabase, MediaScanner, MetadataExtractor,
    ScanResult, SeasonDetails, SeasonSummary, TvShowDetails,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::io::ReaderStream;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<MediaDatabase>,
    pub config: Arc<Config>,
    pub metadata_service: Arc<metadata_service::MetadataService>,
    pub thumbnail_service: Arc<thumbnail_service::ThumbnailService>,
    pub scan_manager: Arc<scan_manager::ScanManager>,
    pub transcoding_service: Arc<transcoding::TranscodingService>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration from environment
    let config = Arc::new(Config::from_env()?);

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
                    warn!(
                        "Failed to connect to PostgreSQL: {}. Falling back to SurrealDB",
                        e
                    );
                    Arc::new(MediaDatabase::new_surrealdb().await?)
                }
            }
        } else {
            info!("Using SurrealDB backend");
            Arc::new(MediaDatabase::new_surrealdb().await?)
        }
    } else {
        info!("Using in-memory SurrealDB");
        Arc::new(MediaDatabase::new_surrealdb().await?)
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

    let state = AppState {
        db,
        config: config.clone(),
        metadata_service,
        thumbnail_service,
        scan_manager,
        transcoding_service,
    };

    let app = create_app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.server_port));
    info!(
        "Starting Rusty Media Server on {}:{}",
        config.server_host, config.server_port
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/ping", get(ping_handler))
        .route("/health", get(health_handler))
        .route("/scan", post(scan_handler))
        .route("/scan", get(scan_status_handler))
        .route("/metadata", post(metadata_handler))
        .route(
            "/library",
            get(library_get_handler).post(library_post_handler),
        )
        .route("/library/scan-and-store", post(scan_and_store_handler))
        .route("/scan/start", post(start_scan_handler))
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
        .route("/transcode/:id/adaptive", post(start_adaptive_transcode_handler))
        .route("/transcode/:id/segment/:segment_number", get(get_segment_handler))
        .route("/transcode/:id/master.m3u8", get(get_master_playlist_handler))
        .route("/transcode/:id/variant/:profile/playlist.m3u8", get(get_variant_playlist_handler))
        .route("/transcode/:id/variant/:profile/:segment", get(get_variant_segment_handler))
        .route("/transcode/cancel/:job_id", post(cancel_transcode_handler))
        .route("/transcode/profiles", get(list_transcode_profiles_handler))
        .route("/transcode/cache/stats", get(transcode_cache_stats_handler))
        .route("/transcode/:id/clear-cache", post(clear_transcode_cache_handler))
        .route("/library/status", get(library_status_handler))
        .route("/media/:id/availability", get(media_availability_handler))
        .route("/config", get(config_handler))
        .route("/metadata/fetch/:id", post(fetch_metadata_handler))
        .route("/poster/:id", get(poster_handler))
        .route("/thumbnail/:id", get(thumbnail_handler))
        .route(
            "/season-poster/:show_name/:season_num",
            get(season_poster_handler),
        )
        // TV Show endpoints
        .route("/shows", get(list_shows_handler))
        .route("/shows/:show_name", get(show_details_handler))
        .route(
            "/shows/:show_name/seasons/:season_num",
            get(season_details_handler),
        )
        // Library management endpoints
        .route("/libraries", get(list_libraries_handler).post(create_library_handler))
        .route("/libraries/:id", get(get_library_handler))
        .route("/libraries/:id", axum::routing::put(update_library_handler))
        .route("/libraries/:id", axum::routing::delete(delete_library_handler))
        .route("/libraries/:id/scan", post(scan_library_handler))
        // Temporary maintenance endpoint
        .route(
            "/maintenance/delete-by-title/:title",
            axum::routing::delete(delete_by_title_handler),
        )
        .route("/metadata/fetch-batch", post(fetch_metadata_batch_handler))
        .route("/posters/batch", post(fetch_posters_batch_handler))
        .route(
            "/metadata/queue-missing",
            post(queue_missing_metadata_handler),
        )
        // Database maintenance endpoints (for testing/debugging)
        .route("/maintenance/clear-database", post(clear_database_handler))
        // Test endpoints for metadata extraction and transcoding
        .route("/test/metadata/:path", get(test_endpoints::test_metadata_extraction))
        .route("/test/transcode/:path", post(test_endpoints::test_transcoding))
        .route("/test/transcode/status/:job_id", get(test_endpoints::test_transcode_status))
        .route("/test/hls/:path", post(test_endpoints::test_hls_streaming))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

#[derive(Deserialize)]
struct MetadataRequest {
    path: String,
}

#[derive(Deserialize)]
struct CreateLibraryRequest {
    name: String,
    library_type: String,
    paths: Vec<String>,
    #[serde(default = "default_scan_interval")]
    scan_interval_minutes: u32,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

#[derive(Deserialize)]
struct UpdateLibraryRequest {
    name: Option<String>,
    paths: Option<Vec<String>>,
    scan_interval_minutes: Option<u32>,
    enabled: Option<bool>,
}

fn default_scan_interval() -> u32 {
    60
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct LibraryFilters {
    media_type: Option<String>,
    show_name: Option<String>,
    season: Option<u32>,
    order_by: Option<String>,
    limit: Option<u64>,
    library_id: Option<String>,
}

#[derive(Deserialize)]
struct ScanAndStoreRequest {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    max_depth: Option<usize>,
    #[serde(default)]
    follow_links: bool,
    #[serde(default = "default_extract_metadata")]
    extract_metadata: bool,
}

fn default_extract_metadata() -> bool {
    true
}

#[derive(Deserialize)]
struct ScanRequest {
    path: String,
    #[serde(default)]
    max_depth: Option<usize>,
    #[serde(default)]
    follow_links: bool,
}

#[derive(Serialize)]
struct ScanResponse {
    status: String,
    message: String,
    result: Option<ScanResult>,
    error: Option<String>,
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

async fn scan_handler(Json(request): Json<ScanRequest>) -> Result<Json<ScanResponse>, StatusCode> {
    info!("Scan request for path: {}", request.path);

    let mut scanner = MediaScanner::new();

    if let Some(depth) = request.max_depth {
        scanner = scanner.with_max_depth(depth);
    }

    scanner = scanner.with_follow_links(request.follow_links);

    match scanner.scan_directory(&request.path) {
        Ok(result) => {
            info!(
                "Scan completed: {} video files found in {}",
                result.video_files.len(),
                request.path
            );

            Ok(Json(ScanResponse {
                status: "success".to_string(),
                message: format!(
                    "Found {} video files out of {} total files",
                    result.video_files.len(),
                    result.total_files
                ),
                result: Some(result),
                error: None,
            }))
        }
        Err(e) => {
            warn!("Scan failed for {}: {}", request.path, e);

            Ok(Json(ScanResponse {
                status: "error".to_string(),
                message: "Scan failed".to_string(),
                result: None,
                error: Some(e.to_string()),
            }))
        }
    }
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

async fn metadata_handler(Json(request): Json<MetadataRequest>) -> Result<Json<Value>, StatusCode> {
    info!("Metadata extraction request for: {}", request.path);

    let mut extractor = MetadataExtractor::new();

    match extractor.extract_metadata(&request.path) {
        Ok(metadata) => {
            info!("Metadata extraction successful for: {}", request.path);
            Ok(Json(json!({
                "status": "success",
                "metadata": metadata
            })))
        }
        Err(e) => {
            warn!("Metadata extraction failed for {}: {}", request.path, e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

async fn library_get_handler(
    State(state): State<AppState>,
    Query(filters): Query<LibraryFilters>,
) -> Result<Json<Value>, StatusCode> {
    info!("Library GET request with filters: {:?}", filters);
    library_handler_impl(state.db, filters).await
}

async fn library_post_handler(
    State(state): State<AppState>,
    Json(filters): Json<LibraryFilters>,
) -> Result<Json<Value>, StatusCode> {
    info!("Library POST request with filters: {:?}", filters);
    library_handler_impl(state.db, filters).await
}

async fn library_handler_impl(
    db: Arc<MediaDatabase>,
    filters: LibraryFilters,
) -> Result<Json<Value>, StatusCode> {
    info!("Library request with filters: {:?}", filters);

    let media_filters = MediaFilters {
        media_type: filters.media_type,
        show_name: filters.show_name,
        season: filters.season,
        order_by: filters.order_by,
        limit: filters.limit,
        library_id: filters.library_id.and_then(|id| uuid::Uuid::parse_str(&id).ok()),
    };

    match db.backend().list_media(media_filters).await {
        Ok(media_files) => {
            info!("Retrieved {} media files from library", media_files.len());
            Ok(Json(json!({
                "status": "success",
                "media_files": media_files,
                "count": media_files.len()
            })))
        }
        Err(e) => {
            warn!("Failed to retrieve library: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

async fn scan_and_store_handler(
    State(state): State<AppState>,
    Json(request): Json<ScanAndStoreRequest>,
) -> Result<Json<Value>, StatusCode> {
    // Use provided path or fall back to MEDIA_ROOT
    let scan_path = match request.path {
        Some(path) => {
            info!("Using provided path: {}", path);
            path
        }
        None => {
            info!("No path provided, checking MEDIA_ROOT environment variable");
            match std::env::var("MEDIA_ROOT") {
                Ok(path) => {
                    info!("Using MEDIA_ROOT: {}", path);
                    path
                }
                Err(_) => {
                    warn!("No path provided and MEDIA_ROOT environment variable not set");
                    return Ok(Json(json!({
                        "status": "error",
                        "error": "No path provided and MEDIA_ROOT not configured on server"
                    })));
                }
            }
        }
    };

    info!("Scan and store request for path: {}", scan_path);

    // Scan for media files
    let mut scanner = MediaScanner::new();
    if let Some(depth) = request.max_depth {
        scanner = scanner.with_max_depth(depth);
    }
    scanner = scanner.with_follow_links(request.follow_links);

    let scan_result = match scanner.scan_directory(&scan_path) {
        Ok(result) => result,
        Err(e) => {
            warn!("Scan failed for {}: {}", scan_path, e);
            return Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })));
        }
    };

    let mut stored_count = 0;
    let mut extraction_errors = Vec::new();
    let mut extractor = if request.extract_metadata {
        Some(MetadataExtractor::new())
    } else {
        None
    };

    // Store each media file and fetch external metadata
    let mut metadata_fetch_count = 0;
    let mut metadata_fetch_errors = Vec::new();

    for mut media_file in scan_result.video_files {
        // Extract technical metadata if requested
        if let Some(ref mut metadata_extractor) = extractor {
            match metadata_extractor.extract_metadata(&media_file.path) {
                Ok(metadata) => {
                    // Log the extracted metadata for debugging
                    if let Some(ref parsed_info) = metadata.parsed_info {
                        info!("Extracted metadata for {}: media_type={}, show_name={:?}, season={:?}, episode={:?}", 
                            media_file.filename, 
                            parsed_info.media_type,
                            parsed_info.show_name,
                            parsed_info.season,
                            parsed_info.episode
                        );
                    }
                    media_file.metadata = Some(metadata);
                }
                Err(e) => {
                    warn!(
                        "Metadata extraction failed for {}: {}",
                        media_file.filename, e
                    );
                    extraction_errors.push(format!("{}: {}", media_file.filename, e));
                }
            }
        }

        // Store in database
        let stored_id = match state.db.backend().store_media(media_file.clone()).await {
            Ok(id) => {
                stored_count += 1;
                Some(id)
            }
            Err(e) => {
                warn!("Failed to store media file: {}", e);
                extraction_errors.push(format!("Storage failed: {}", e));
                None
            }
        };

        // Fetch external metadata from TMDB if we stored the file successfully
        if let Some(id) = stored_id {
            // Only fetch if we don't already have external metadata
            let needs_external_metadata = media_file
                .metadata
                .as_ref()
                .map(|m| m.external_info.is_none())
                .unwrap_or(true);

            if needs_external_metadata {
                info!("Fetching TMDB metadata for: {}", media_file.filename);
                match state.metadata_service.fetch_metadata(&media_file).await {
                    Ok(detailed_info) => {
                        // Update the media file with external info
                        let mut updated_media = media_file.clone();
                        if let Some(ref mut metadata) = updated_media.metadata {
                            metadata.external_info = Some(detailed_info.external_info.clone());
                        }

                        // Store updated media file
                        if let Err(e) = state.db.backend().store_media(updated_media).await {
                            warn!("Failed to update media with TMDB metadata: {}", e);
                        } else {
                            metadata_fetch_count += 1;

                            // Try to cache poster
                            if let Some(poster_path) = &detailed_info.external_info.poster_url {
                                let media_id = id.split(':').last().unwrap_or(&id);
                                match state
                                    .metadata_service
                                    .cache_poster(poster_path, media_id)
                                    .await
                                {
                                    Ok(path) => info!("Poster cached at: {:?}", path),
                                    Err(e) => warn!("Failed to cache poster: {}", e),
                                }
                            }

                            // Cache season poster if available for TV episodes
                            if let Some(season_poster) =
                                &detailed_info.external_info.season_poster_url
                            {
                                if let Some(metadata) = &media_file.metadata {
                                    if let Some(parsed) = &metadata.parsed_info {
                                        if let (Some(show_name), Some(season)) =
                                            (&parsed.show_name, parsed.season)
                                        {
                                            match state
                                                .metadata_service
                                                .cache_season_poster(
                                                    season_poster,
                                                    show_name,
                                                    season,
                                                )
                                                .await
                                            {
                                                Ok(path) => {
                                                    info!("Season poster cached at: {:?}", path)
                                                }
                                                Err(e) => {
                                                    warn!("Failed to cache season poster: {}", e)
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to fetch TMDB metadata for {}: {}",
                            media_file.filename, e
                        );
                        metadata_fetch_errors.push(format!("{}: {}", media_file.filename, e));
                    }
                }
            }

            // Extract thumbnail for TV episodes during scan
            if let Some(metadata) = &media_file.metadata {
                if let Some(parsed) = &metadata.parsed_info {
                    if parsed.media_type == ferrex_core::MediaType::TvEpisode {
                        let media_id = id.split(':').last().unwrap_or(&id);
                        match state
                            .thumbnail_service
                            .extract_thumbnail(media_id, &media_file.path.to_string_lossy())
                            .await
                        {
                            Ok(path) => info!(
                                "Thumbnail extracted for {} at {:?}",
                                media_file.filename, path
                            ),
                            Err(e) => warn!(
                                "Failed to extract thumbnail for {}: {}",
                                media_file.filename, e
                            ),
                        }
                    }
                }
            }
        }
    }

    info!(
        "Stored {} media files from scan of {}",
        stored_count, scan_path
    );
    info!("Fetched TMDB metadata for {} files", metadata_fetch_count);

    Ok(Json(json!({
        "status": "success",
        "message": format!("Scanned {} files, stored {}, fetched metadata for {}",
            scan_result.total_files, stored_count, metadata_fetch_count),
        "scanned": scan_result.total_files,
        "stored": stored_count,
        "metadata_fetched": metadata_fetch_count,
        "skipped": scan_result.skipped_files,
        "extraction_errors": extraction_errors,
        "metadata_errors": metadata_fetch_errors,
        "scan_errors": scan_result.errors
    })))
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
    let db_id = decoded_id.into_owned();

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
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!("Metadata fetch request for media ID: {}", id);

    // Format ID for database lookup
    let db_id = if id.starts_with("media:") {
        id.clone()
    } else {
        format!("media:{}", id)
    };

    // Get media file from database
    let media_file = match state.db.backend().get_media(&db_id).await {
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
        info!("Re-extracting technical metadata from: {:?}", media_file.path);
        let mut extractor = ferrex_core::MetadataExtractor::new();
        
        match extractor.extract_metadata(&media_file.path) {
            Ok(new_metadata) => {
                info!("Technical metadata extracted successfully:");
                info!("  Color transfer: {:?}", new_metadata.color_transfer);
                info!("  Color space: {:?}", new_metadata.color_space);
                info!("  Color primaries: {:?}", new_metadata.color_primaries);
                info!("  Bit depth: {:?}", new_metadata.bit_depth);
                
                // Update media with new technical metadata
                if let Some(ref mut existing_metadata) = updated_media.metadata {
                    // Preserve external info but update technical fields
                    let external_info = existing_metadata.external_info.clone();
                    let parsed_info = existing_metadata.parsed_info.clone();
                    *existing_metadata = new_metadata;
                    existing_metadata.external_info = external_info;
                    existing_metadata.parsed_info = parsed_info;
                } else {
                    updated_media.metadata = Some(new_metadata);
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

    // Then fetch external metadata from TMDB
    match state.metadata_service.fetch_metadata(&media_file).await {
        Ok(detailed_info) => {
            info!("External metadata fetched successfully for: {}", id);

            // Update the media file with external info
            if let Some(ref mut metadata) = updated_media.metadata {
                metadata.external_info = Some(detailed_info.external_info.clone());
            }

            // Store updated media file
            match state.db.backend().store_media(updated_media.clone()).await {
                Ok(_) => {
                    info!("Media updated in database with new metadata");
                    
                    // Send MediaUpdated event if technical metadata was extracted
                    if technical_metadata_extracted {
                        let event = MediaEvent::MediaUpdated { media: updated_media };
                        state.scan_manager.send_media_event(event).await;
                        info!("MediaUpdated event sent for media: {}", id);
                    }
                }
                Err(e) => {
                    warn!("Failed to update media with metadata: {}", e);
                }
            }

            // Download poster if available
            if let Some(poster_path) = &detailed_info.external_info.poster_url {
                match state.metadata_service.cache_poster(poster_path, &id).await {
                    Ok(path) => info!("Poster cached at: {:?}", path),
                    Err(e) => warn!("Failed to cache poster: {}", e),
                }
            }

            Ok(Json(json!({
                "status": "success",
                "metadata": detailed_info,
                "technical_metadata_extracted": technical_metadata_extracted
            })))
        }
        Err(e) => {
            warn!("External metadata fetch failed for {}: {}", id, e);
            
            // Even if external metadata fails, save technical metadata if we extracted it
            if technical_metadata_extracted {
                match state.db.backend().store_media(updated_media.clone()).await {
                    Ok(_) => {
                        info!("Technical metadata saved despite external metadata failure");
                        
                        // Send MediaUpdated event
                        let event = MediaEvent::MediaUpdated { media: updated_media };
                        state.scan_manager.send_media_event(event).await;
                        info!("MediaUpdated event sent for media: {}", id);
                    }
                    Err(db_err) => {
                        warn!("Failed to save technical metadata: {}", db_err);
                    }
                }
            }
            
            Ok(Json(json!({
                "status": "partial",
                "error": e.to_string(),
                "technical_metadata_extracted": technical_metadata_extracted
            })))
        }
    }
}

async fn poster_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, StatusCode> {
    info!("Poster request for media ID: {}", id);

    // Extract just the UUID part (consistent with caching logic)
    let media_id = id.split(':').last().unwrap_or(&id);

    // Check for cached poster
    if let Some(poster_path) = state.metadata_service.get_cached_poster(media_id) {
        // Serve the cached poster file
        match tokio::fs::read(&poster_path).await {
            Ok(bytes) => {
                let mut response = Response::new(bytes.into());
                
                // Determine content type based on file extension
                let content_type = if poster_path.extension()
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
        // No cached poster available
        Err(StatusCode::NOT_FOUND)
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
    Path(id): Path<String>,
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

// TV Show handlers
async fn list_shows_handler(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    info!("Listing all TV shows");

    // Get all TV episodes
    let filters = MediaFilters {
        media_type: Some("tv_show".to_string()),
        ..Default::default()
    };

    match state.db.backend().list_media(filters).await {
        Ok(episodes) => {
            // Aggregate episodes into shows
            let mut shows: HashMap<String, TvShowDetails> = HashMap::new();

            for episode in episodes {
                if let Some(parsed_info) = &episode
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                {
                    if let Some(show_name) = &parsed_info.show_name {
                        let show = shows.entry(show_name.clone()).or_insert_with(|| {
                            let external = episode
                                .metadata
                                .as_ref()
                                .and_then(|m| m.external_info.as_ref());
                            TvShowDetails {
                                name: show_name.clone(),
                                tmdb_id: external.and_then(|e| e.tmdb_id),
                                description: external.and_then(|e| {
                                    e.show_description.clone().or(e.description.clone())
                                }),
                                poster_url: external
                                    .and_then(|e| {
                                        e.show_poster_url.clone().or(e.poster_url.clone())
                                    })
                                    .and_then(|path| {
                                        state.metadata_service.get_tmdb_image_url(&path)
                                    }),
                                backdrop_url: external
                                    .and_then(|e| e.backdrop_url.clone())
                                    .and_then(|path| {
                                        state.metadata_service.get_tmdb_image_url(&path)
                                    }),
                                genres: external.map(|e| e.genres.clone()).unwrap_or_default(),
                                rating: external.and_then(|e| e.rating),
                                seasons: Vec::new(),
                                total_episodes: 0,
                            }
                        });

                        show.total_episodes += 1;

                        // Update season info
                        if let Some(season_num) = parsed_info.season {
                            if !show.seasons.iter().any(|s| s.number == season_num) {
                                show.seasons.push(SeasonSummary {
                                    number: season_num,
                                    name: if season_num == 0 {
                                        Some("Specials".to_string())
                                    } else {
                                        None
                                    },
                                    episode_count: 1,
                                    poster_url: if state
                                        .metadata_service
                                        .get_cached_season_poster(&show_name, season_num)
                                        .is_some()
                                    {
                                        Some(format!(
                                            "/season-poster/{}/{}",
                                            show_name.replace(' ', "+"),
                                            season_num
                                        ))
                                    } else {
                                        episode
                                            .metadata
                                            .as_ref()
                                            .and_then(|m| m.external_info.as_ref())
                                            .and_then(|e| e.season_poster_url.clone())
                                            .and_then(|path| {
                                                state.metadata_service.get_tmdb_image_url(&path)
                                            })
                                    },
                                });
                            } else if let Some(season) =
                                show.seasons.iter_mut().find(|s| s.number == season_num)
                            {
                                season.episode_count += 1;
                            }
                        }
                    }
                }
            }

            // Sort seasons
            for show in shows.values_mut() {
                show.seasons.sort_by_key(|s| s.number);
            }

            let show_list: Vec<_> = shows.into_values().collect();
            info!("Found {} TV shows", show_list.len());

            Ok(Json(json!({
                "status": "success",
                "shows": show_list,
                "count": show_list.len()
            })))
        }
        Err(e) => {
            warn!("Failed to retrieve TV shows: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

async fn show_details_handler(
    State(state): State<AppState>,
    Path(show_name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    // Decode + signs back to spaces (form encoding artifact)
    let show_name = show_name.replace("+", " ");
    info!("Getting details for show: {}", show_name);

    // Get all episodes for this show
    let filters = MediaFilters {
        media_type: Some("tv_show".to_string()),
        show_name: Some(show_name.clone()),
        ..Default::default()
    };

    match state.db.backend().list_media(filters).await {
        Ok(episodes) => {
            if episodes.is_empty() {
                return Err(StatusCode::NOT_FOUND);
            }

            // Build show details from episodes
            let first_episode = &episodes[0];
            let external = first_episode
                .metadata
                .as_ref()
                .and_then(|m| m.external_info.as_ref());

            let mut show_details = TvShowDetails {
                name: show_name.clone(),
                tmdb_id: external.and_then(|e| e.tmdb_id),
                description: external
                    .and_then(|e| e.show_description.clone().or(e.description.clone())),
                poster_url: external
                    .and_then(|e| e.show_poster_url.clone().or(e.poster_url.clone()))
                    .and_then(|path| state.metadata_service.get_tmdb_image_url(&path)),
                backdrop_url: external
                    .and_then(|e| e.backdrop_url.clone())
                    .and_then(|path| state.metadata_service.get_tmdb_image_url(&path)),
                genres: external.map(|e| e.genres.clone()).unwrap_or_default(),
                rating: external.and_then(|e| e.rating),
                seasons: Vec::new(),
                total_episodes: episodes.len(),
            };

            // Build season information
            let mut season_map: HashMap<u32, Vec<_>> = HashMap::new();
            for episode in episodes {
                if let Some(parsed_info) = &episode
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                {
                    if let Some(season_num) = parsed_info.season {
                        season_map.entry(season_num).or_default().push(episode);
                    }
                }
            }

            for (season_num, season_episodes) in season_map {
                let season_poster = season_episodes
                    .iter()
                    .find_map(|e| {
                        e.metadata
                            .as_ref()?
                            .external_info
                            .as_ref()?
                            .season_poster_url
                            .clone()
                    })
                    .and_then(|path| state.metadata_service.get_tmdb_image_url(&path));

                show_details.seasons.push(SeasonSummary {
                    number: season_num,
                    name: if season_num == 0 {
                        Some("Specials".to_string())
                    } else {
                        None
                    },
                    episode_count: season_episodes.len(),
                    poster_url: season_poster,
                });
            }

            show_details.seasons.sort_by_key(|s| s.number);

            Ok(Json(json!({
                "status": "success",
                "show": show_details
            })))
        }
        Err(e) => {
            warn!("Failed to retrieve show details: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

async fn season_details_handler(
    State(state): State<AppState>,
    Path((show_name, season_num)): Path<(String, u32)>,
) -> Result<Json<Value>, StatusCode> {
    // Decode + signs back to spaces (form encoding artifact)
    let show_name = show_name.replace("+", " ");
    info!(
        "Getting details for show: {}, season: {}",
        show_name, season_num
    );

    // Get all episodes for this show and season
    let filters = MediaFilters {
        media_type: Some("tv_show".to_string()),
        show_name: Some(show_name.clone()),
        season: Some(season_num),
        ..Default::default()
    };

    match state.db.backend().list_media(filters).await {
        Ok(episodes) => {
            if episodes.is_empty() {
                return Err(StatusCode::NOT_FOUND);
            }

            // Build season details
            let season_poster = if state
                .metadata_service
                .get_cached_season_poster(&show_name, season_num)
                .is_some()
            {
                Some(format!(
                    "/season-poster/{}/{}",
                    show_name.replace(' ', "+"),
                    season_num
                ))
            } else {
                episodes
                    .iter()
                    .find_map(|e| {
                        e.metadata
                            .as_ref()?
                            .external_info
                            .as_ref()?
                            .season_poster_url
                            .clone()
                    })
                    .and_then(|path| state.metadata_service.get_tmdb_image_url(&path))
            };

            let mut episode_summaries = Vec::new();
            for episode in episodes {
                if let Some(parsed_info) = &episode
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                {
                    if let Some(episode_num) = parsed_info.episode {
                        let external = episode
                            .metadata
                            .as_ref()
                            .and_then(|m| m.external_info.as_ref());

                        episode_summaries.push(EpisodeSummary {
                            id: episode.id,
                            number: episode_num,
                            title: parsed_info.episode_title.clone(),
                            description: external.and_then(|e| e.description.clone()),
                            still_url: external.and_then(|e| e.episode_still_url.clone()),
                            duration: episode.metadata.as_ref().and_then(|m| m.duration),
                            air_date: external.and_then(|e| e.release_date),
                        });
                    }
                }
            }

            episode_summaries.sort_by_key(|e| e.number);

            let season_details = SeasonDetails {
                show_name: show_name.clone(),
                number: season_num,
                name: if season_num == 0 {
                    Some("Specials".to_string())
                } else {
                    None
                },
                poster_url: season_poster,
                episodes: episode_summaries,
            };

            Ok(Json(json!({
                "status": "success",
                "season": season_details
            })))
        }
        Err(e) => {
            warn!("Failed to retrieve season details: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

// Library management handlers
async fn list_libraries_handler(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    info!("Listing all libraries");
    
    match state.db.backend().list_libraries().await {
        Ok(libraries) => {
            info!("Found {} libraries", libraries.len());
            Ok(Json(json!({
                "status": "success",
                "libraries": libraries
            })))
        }
        Err(e) => {
            warn!("Failed to list libraries: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

async fn get_library_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!("Getting library: {}", id);
    
    match state.db.backend().get_library(&id).await {
        Ok(Some(library)) => {
            Ok(Json(json!({
                "status": "success",
                "library": library
            })))
        }
        Ok(None) => {
            warn!("Library not found: {}", id);
            Err(StatusCode::NOT_FOUND)
        }
        Err(e) => {
            warn!("Failed to get library: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

async fn create_library_handler(
    State(state): State<AppState>,
    Json(request): Json<CreateLibraryRequest>,
) -> Result<Json<Value>, StatusCode> {
    info!("Creating library: {}", request.name);
    
    // Parse library type
    let library_type = match request.library_type.to_lowercase().as_str() {
        "movies" => LibraryType::Movies,
        "tvshows" | "tv_shows" | "tv" => LibraryType::TvShows,
        _ => {
            return Ok(Json(json!({
                "status": "error",
                "error": "Invalid library type. Use 'movies' or 'tvshows'"
            })));
        }
    };
    
    // Convert string paths to PathBuf
    let paths: Vec<std::path::PathBuf> = request.paths.into_iter()
        .map(std::path::PathBuf::from)
        .collect();
    
    // Validate paths exist
    for path in &paths {
        if !path.exists() {
            return Ok(Json(json!({
                "status": "error",
                "error": format!("Path does not exist: {}", path.display())
            })));
        }
    }
    
    let library = Library {
        id: uuid::Uuid::new_v4(),
        name: request.name,
        library_type,
        paths,
        scan_interval_minutes: request.scan_interval_minutes,
        last_scan: None,
        enabled: request.enabled,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    
    match state.db.backend().create_library(library).await {
        Ok(id) => {
            info!("Library created with ID: {}", id);
            Ok(Json(json!({
                "status": "success",
                "id": id,
                "message": "Library created successfully"
            })))
        }
        Err(e) => {
            warn!("Failed to create library: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

async fn update_library_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateLibraryRequest>,
) -> Result<Json<Value>, StatusCode> {
    info!("Updating library: {}", id);
    
    // Get existing library
    let mut library = match state.db.backend().get_library(&id).await {
        Ok(Some(lib)) => lib,
        Ok(None) => {
            warn!("Library not found: {}", id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            warn!("Failed to get library: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    
    // Update fields
    if let Some(name) = request.name {
        library.name = name;
    }
    
    if let Some(paths) = request.paths {
        let new_paths: Vec<std::path::PathBuf> = paths.into_iter()
            .map(std::path::PathBuf::from)
            .collect();
        
        // Validate paths exist
        for path in &new_paths {
            if !path.exists() {
                return Ok(Json(json!({
                    "status": "error",
                    "error": format!("Path does not exist: {}", path.display())
                })));
            }
        }
        
        library.paths = new_paths;
    }
    
    if let Some(interval) = request.scan_interval_minutes {
        library.scan_interval_minutes = interval;
    }
    
    if let Some(enabled) = request.enabled {
        library.enabled = enabled;
    }
    
    library.updated_at = chrono::Utc::now();
    
    match state.db.backend().update_library(&id, library).await {
        Ok(()) => {
            info!("Library updated: {}", id);
            Ok(Json(json!({
                "status": "success",
                "message": "Library updated successfully"
            })))
        }
        Err(e) => {
            warn!("Failed to update library: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

async fn delete_library_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!("Deleting library: {}", id);
    
    match state.db.backend().delete_library(&id).await {
        Ok(()) => {
            info!("Library deleted: {}", id);
            Ok(Json(json!({
                "status": "success",
                "message": "Library deleted successfully"
            })))
        }
        Err(e) => {
            warn!("Failed to delete library: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

// Library scan handler
async fn scan_library_handler(
    State(state): State<AppState>,
    Path(library_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    info!("Scan request for library: {}", library_id);
    
    // Get the library details
    let library = match state.db.backend().get_library(&library_id).await {
        Ok(Some(lib)) => lib,
        Ok(None) => {
            warn!("Library not found: {}", library_id);
            return Ok(Json(json!({
                "status": "error",
                "error": "Library not found"
            })));
        }
        Err(e) => {
            warn!("Failed to get library: {}", e);
            return Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })));
        }
    };
    
    // Check if library is enabled
    if !library.enabled {
        return Ok(Json(json!({
            "status": "error",
            "error": "Library is disabled"
        })));
    }
    
    // Check for force rescan parameter
    let force_rescan = params.get("force")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    
    // Check if we should use streaming scanner (default to true for libraries)
    let use_streaming = params.get("streaming")
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true);
    
    let library_name = library.name.clone();
    
    // Start the scan
    let scan_result = if use_streaming {
        // Use new streaming scanner for better performance
        state.scan_manager.start_library_scan(Arc::new(library), force_rescan).await
    } else {
        // Fall back to traditional scanner if requested
        let scan_request = scan_manager::ScanRequest {
            paths: Some(library.paths.iter().map(|p| p.to_string_lossy().to_string()).collect()),
            path: None,
            library_id: Some(library.id),
            library_type: Some(library.library_type),
            extract_metadata: true,
            follow_links: false,
            force_rescan,
            max_depth: None,
            use_streaming: false,
        };
        state.scan_manager.start_scan(scan_request).await
    };
    
    match scan_result {
        Ok(scan_id) => {
            // Update library last scan time
            let _ = state.db.backend().update_library_last_scan(&library_id).await;
            
            info!("Library scan started with ID: {}", scan_id);
            Ok(Json(json!({
                "status": "success",
                "scan_id": scan_id,
                "message": format!("Scan started for library: {}", library_name)
            })))
        }
        Err(e) => {
            warn!("Failed to start library scan: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

// New scan management handlers
async fn start_scan_handler(
    State(state): State<AppState>,
    Json(request): Json<scan_manager::ScanRequest>,
) -> Result<Json<Value>, StatusCode> {
    match state.scan_manager.start_scan(request).await {
        Ok(scan_id) => Ok(Json(json!({
            "status": "success",
            "scan_id": scan_id,
            "message": "Scan started successfully"
        }))),
        Err(e) => {
            warn!("Failed to start scan: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

async fn scan_progress_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    match state.scan_manager.get_scan_progress(&id).await {
        Some(progress) => Ok(Json(json!({
            "status": "success",
            "progress": progress
        }))),
        None => Ok(Json(json!({
            "status": "error",
            "error": "Scan not found"
        }))),
    }
}

async fn scan_progress_sse_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<axum::response::sse::Event, anyhow::Error>>>,
    StatusCode,
> {
    info!("SSE connection requested for scan {}", id);
    let receiver = state.scan_manager.subscribe_to_progress(id.clone()).await;
    Ok(scan_manager::scan_progress_sse(id, receiver))
}

async fn media_events_sse_handler(
    State(state): State<AppState>,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<axum::response::sse::Event, anyhow::Error>>>,
    StatusCode,
> {
    info!("SSE connection requested for media events");
    let receiver = state.scan_manager.subscribe_to_media_events().await;
    Ok(scan_manager::media_events_sse(receiver))
}

async fn active_scans_handler(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let active_scans = state.scan_manager.get_active_scans().await;
    Ok(Json(json!({
        "status": "success",
        "scans": active_scans,
        "count": active_scans.len()
    })))
}

async fn scan_history_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    let limit = params
        .get("limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(10);

    let history = state.scan_manager.get_scan_history(limit).await;
    Ok(Json(json!({
        "status": "success",
        "history": history,
        "count": history.len()
    })))
}

async fn cancel_scan_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    match state.scan_manager.cancel_scan(&id).await {
        Ok(_) => Ok(Json(json!({
            "status": "success",
            "message": "Scan cancelled"
        }))),
        Err(e) => Ok(Json(json!({
            "status": "error",
            "error": e.to_string()
        }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use ferrex_core::MediaFile;
    use std::path::PathBuf;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_ping_endpoint() {
        let config = Arc::new(Config::from_env().unwrap());
        let db = Arc::new(MediaDatabase::new_surrealdb().await.unwrap());
        let metadata_service = Arc::new(metadata_service::MetadataService::new(
            None,
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
        let state = AppState {
            db,
            config,
            metadata_service,
            thumbnail_service,
            scan_manager,
        };
        let app = create_app(state);

        let response = app
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_clear_database_endpoint() {
        let config = Arc::new(Config::from_env().unwrap());
        let db = Arc::new(MediaDatabase::new_surrealdb().await.unwrap());
        
        // Initialize database schema
        db.backend().initialize_schema().await.unwrap();
        
        let metadata_service = Arc::new(metadata_service::MetadataService::new(
            None,
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
        let state = AppState {
            db: db.clone(),
            config,
            metadata_service,
            thumbnail_service,
            scan_manager,
        };
        let app = create_app(state);

        // Call clear database endpoint
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/maintenance/clear-database")
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        
        // Check that we get a valid JSON response
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("\"status\":\"success\""));
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

                // Clean up associated files
                let media_id_str = media.id.to_string();
                let media_id = media_id_str.split(':').last().unwrap_or(&media_id_str);

                // Clean up thumbnail
                let thumbnail_path = state.thumbnail_service.get_thumbnail_path(media_id);
                if thumbnail_path.exists() {
                    if let Err(e) = tokio::fs::remove_file(&thumbnail_path).await {
                        warn!("Failed to delete thumbnail: {}", e);
                    }
                }

                // Clean up poster
                let poster_path = state.metadata_service.get_poster_path(media_id);
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

            // Clean up associated files
            let media_id_str = media.id.to_string();
            let media_id = media_id_str.split(':').last().unwrap_or(&media_id_str);

            // Clean up thumbnail
            let thumbnail_path = state.thumbnail_service.get_thumbnail_path(media_id);
            if thumbnail_path.exists() {
                if let Err(e) = tokio::fs::remove_file(&thumbnail_path).await {
                    warn!("Failed to delete thumbnail: {}", e);
                }
            }

            // Clean up poster (try both PNG and JPG)
            let png_poster_path = state.config.cache_dir.join("posters").join(format!("{}_poster.png", media_id));
            if png_poster_path.exists() {
                if let Err(e) = tokio::fs::remove_file(&png_poster_path).await {
                    warn!("Failed to delete PNG poster: {}", e);
                }
            }
            
            let jpg_poster_path = state.config.cache_dir.join("posters").join(format!("{}_poster.jpg", media_id));
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
                        warn!("Failed to delete poster cache file {:?}: {}", entry.path(), e);
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
    media_ids: Vec<String>,
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
    let mut updated = Vec::new();
    let mut errors = Vec::new();

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

            // Strip "media:" prefix if present
            let db_id = if id.starts_with("media:") {
                id.strip_prefix("media:").unwrap_or(&id).to_string()
            } else {
                id.clone()
            };

            // Get media from database
            match state.db.backend().get_media(&db_id).await {
                Ok(Some(media)) => {
                    // Check if we already have the data we need
                    let has_poster = media
                        .metadata
                        .as_ref()
                        .and_then(|m| m.external_info.as_ref())
                        .and_then(|e| e.poster_url.as_ref())
                        .is_some();

                    if priority == "posters_only" && has_poster {
                        return Ok(id); // Already has poster, skip
                    }

                    // Fetch metadata
                    match state.metadata_service.fetch_metadata(&media).await {
                        Ok(detailed_info) => {
                            let mut updated_media = media;
                            if let Some(ref mut metadata) = updated_media.metadata {
                                metadata.external_info = Some(detailed_info.external_info.clone());
                            }

                            // Store updated media
                            if let Err(e) = state.db.backend().store_media(updated_media).await {
                                Err(format!("Failed to update {}: {}", id, e))
                            } else {
                                // Cache poster if available
                                if let Some(poster_url) = &detailed_info.external_info.poster_url {
                                    let media_id = db_id.split(':').last().unwrap_or(&db_id);
                                    let _ = state
                                        .metadata_service
                                        .cache_poster(poster_url, media_id)
                                        .await;
                                }
                                Ok(id)
                            }
                        }
                        Err(e) => Err(format!("Failed to fetch metadata for {}: {}", id, e)),
                    }
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
    media_ids: Vec<String>,
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
                // Strip media: prefix if present
                let db_id = if id.starts_with("media:") {
                    id.strip_prefix("media:").unwrap_or(id).to_string()
                } else {
                    id.clone()
                };

                // Get media from database
                if let Ok(Some(media)) = db.backend().get_media(&db_id).await {
                    // Check if metadata is actually missing
                    let needs_metadata = media
                        .metadata
                        .as_ref()
                        .and_then(|m| m.external_info.as_ref())
                        .and_then(|e| e.poster_url.as_ref())
                        .is_none();

                    if needs_metadata {
                        // Fetch metadata
                        match metadata_service.fetch_metadata(&media).await {
                            Ok(detailed_info) => {
                                let mut updated_media = media;
                                if let Some(ref mut metadata) = updated_media.metadata {
                                    metadata.external_info =
                                        Some(detailed_info.external_info.clone());
                                }

                                // Store updated media
                                if let Ok(_) = db.backend().store_media(updated_media.clone()).await
                                {
                                    info!("Updated metadata for {}", id);

                                    // Send media updated event
                                    scan_manager
                                        .send_media_event(scan_manager::MediaEvent::MediaUpdated {
                                            media: updated_media,
                                        })
                                        .await;

                                    // Cache poster if available
                                    if let Some(poster_url) =
                                        &detailed_info.external_info.poster_url
                                    {
                                        let media_id = db_id.split(':').last().unwrap_or(&db_id);
                                        let _ = metadata_service
                                            .cache_poster(poster_url, media_id)
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to fetch metadata for {}: {}", id, e);
                            }
                        }
                    }
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
    Path(id): Path<String>,
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

// HLS Streaming handlers
async fn hls_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, StatusCode> {
    info!("HLS playlist request for media ID: {}", id);
    
    // Check if we have a transcoded version
    let profile_name = params.get("profile").cloned().unwrap_or_else(|| "hdr_to_sdr_1080p".to_string());
    
    // First check if we have a cached version
    if let Some(playlist_path) = state.transcoding_service.get_playlist_url(&id, &profile_name).await {
        // Serve the cached playlist
        match tokio::fs::read_to_string(&playlist_path).await {
            Ok(content) => {
                let mut response = Response::new(content.into());
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("application/vnd.apple.mpegurl"),
                );
                Ok(response)
            }
            Err(e) => {
                warn!("Failed to read playlist file: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        // Check if media is HDR and needs transcoding
        match state.db.backend().get_media(&id).await {
            Ok(Some(media)) => {
                if transcoding::TranscodingService::is_hdr_content(&media).await {
                    // Start transcoding job if not already running
                    let profile = transcoding::profiles::TranscodingProfile::hdr_to_sdr_1080p();
                    match state.transcoding_service.start_transcoding(&id, profile.clone(), None, None).await {
                        Ok(job_id) => {
                            info!("Started on-the-fly transcoding job {} for media {}", job_id, id);
                            
                            // Wait for first segment to be available (up to 10 seconds)
                            let mut retries = 0;
                            const MAX_RETRIES: u32 = 20;
                            const RETRY_DELAY_MS: u64 = 500;
                            
                            loop {
                                // Check if we have a playlist with segments
                                if let Some(playlist_path) = state.transcoding_service.get_playlist_url(&id, &profile_name).await {
                                    if let Ok(content) = tokio::fs::read_to_string(&playlist_path).await {
                                        // Check if playlist has at least one segment
                                        if content.contains("#EXTINF:") {
                                            let mut response = Response::new(content.into());
                                            response.headers_mut().insert(
                                                header::CONTENT_TYPE,
                                                header::HeaderValue::from_static("application/vnd.apple.mpegurl"),
                                            );
                                            response.headers_mut().insert(
                                                header::CACHE_CONTROL,
                                                header::HeaderValue::from_static("no-cache"),
                                            );
                                            return Ok(response);
                                        }
                                    }
                                }
                                
                                retries += 1;
                                if retries >= MAX_RETRIES {
                                    warn!("Timeout waiting for first segment after {} retries", retries);
                                    break;
                                }
                                
                                tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                            }
                            
                            // If we're here, no segments were generated in time
                            // Return error to trigger fallback
                            Err(StatusCode::SERVICE_UNAVAILABLE)
                        }
                        Err(e) => {
                            warn!("Failed to start transcoding: {}", e);
                            Err(StatusCode::INTERNAL_SERVER_ERROR)
                        }
                    }
                } else {
                    // Not HDR content, redirect to direct stream
                    Ok(Response::builder()
                        .status(StatusCode::TEMPORARY_REDIRECT)
                        .header("Location", format!("/stream/{}", id))
                        .body(axum::body::Body::empty())
                        .unwrap())
                }
            }
            Ok(None) => Err(StatusCode::NOT_FOUND),
            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

// Hardware encoder detection
#[derive(Debug, Clone)]
enum HardwareEncoder {
    AMF,
    VAAPI,
    None,
}

async fn detect_hardware_encoder(ffmpeg_path: &str) -> HardwareEncoder {
    // Check for available encoders
    let output = tokio::process::Command::new(ffmpeg_path)
        .arg("-hide_banner")
        .arg("-encoders")
        .output()
        .await
        .ok();
    
    if let Some(output) = output {
        let encoders = String::from_utf8_lossy(&output.stdout);
        
        // Check for AMF first (preferred for AMD)
        if encoders.contains("h264_amf") {
            // Verify AMF runtime is available by testing encoding
            let test_output = tokio::process::Command::new(ffmpeg_path)
                .arg("-f").arg("lavfi")
                .arg("-i").arg("testsrc2=duration=0.1:size=320x240:rate=30")
                .arg("-c:v").arg("h264_amf")
                .arg("-f").arg("null")
                .arg("-")
                .output()
                .await
                .ok();
            
            if let Some(test) = test_output {
                if test.status.success() {
                    info!("AMF hardware encoder detected and verified");
                    return HardwareEncoder::AMF;
                } else {
                    let stderr = String::from_utf8_lossy(&test.stderr);
                    if stderr.contains("libamfrt64.so.1") {
                        warn!("AMF encoder found but runtime libraries missing");
                    }
                }
            }
        }
        
        // Check for VAAPI
        if encoders.contains("h264_vaapi") && std::path::Path::new("/dev/dri/renderD128").exists() {
            info!("VAAPI hardware encoder detected");
            return HardwareEncoder::VAAPI;
        }
    }
    
    info!("No hardware encoder detected, using software encoding");
    HardwareEncoder::None
}

// Direct transcoding stream handler - pipes FFmpeg output directly to client
async fn stream_transcode_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response<Body>, StatusCode> {
    info!("Direct transcode stream request for media ID: {}", id);
    
    // Decode the percent-encoded ID
    let decoded_id = urlencoding::decode(&id).map_err(|e| {
        error!("Failed to decode media ID: {}", e);
        StatusCode::BAD_REQUEST
    })?;
    
    let profile_name = params.get("profile").cloned().unwrap_or_else(|| "hdr_to_sdr_1080p".to_string());
    
    // Determine target resolution and bitrate based on profile
    let (target_width, video_bitrate, max_bitrate, buffer_size) = match profile_name.as_str() {
        "hdr_to_sdr_4k" => (3840, "25M", "30M", "10M"),      // 4K needs higher bitrate for quality
        "hdr_to_sdr_1440p" => (2560, "15M", "18M", "6M"),    // 1440p medium bitrate
        _ => (1920, "10M", "12M", "4M"),                     // 1080p default
    };
    
    // Get media file
    let media = match state.db.backend().get_media(&decoded_id).await {
        Ok(Some(media)) => media,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    
    // Build FFmpeg command for direct streaming
    let mut cmd = tokio::process::Command::new(&state.config.ffmpeg_path);
    
    // Check if this is HDR content (including Dolby Vision)
    let is_hdr = if let Some(metadata) = &media.metadata {
        metadata.bit_depth.map(|b| b > 8).unwrap_or(false) ||
        metadata.color_transfer.as_ref().map(|t| 
            t.contains("smpte2084") || t.contains("arib-std-b67") || t.contains("smpte2086")
        ).unwrap_or(false) ||
        metadata.color_primaries.as_ref().map(|p| p.contains("bt2020")).unwrap_or(false)
    } else {
        false
    };
    
    // Detect available hardware acceleration options
    let hardware_encoder = detect_hardware_encoder(&state.config.ffmpeg_path).await;
    
    // Fast startup options - balance speed and reliability
    cmd.arg("-probesize").arg("1048576"); // 1MB probe size for fast but reliable startup
    cmd.arg("-analyzeduration").arg("500000"); // 0.5 second analysis
    cmd.arg("-fpsprobesize").arg("0"); // Skip FPS probing
    cmd.arg("-flags").arg("low_delay"); // Low latency mode
    cmd.arg("-movflags").arg("+faststart"); // Low latency mode
    cmd.arg("-tune").arg("zerolatency"); // Low latency mode
    cmd.arg("-strict").arg("experimental"); // Allow experimental features
    
    // Apply hardware decoding options BEFORE input file
    if matches!(hardware_encoder, HardwareEncoder::VAAPI) {
        cmd.arg("-hwaccel").arg("vaapi");
        cmd.arg("-hwaccel_device").arg("/dev/dri/renderD128");
        cmd.arg("-hwaccel_output_format").arg("vaapi");
    }
    
    // Input file
    cmd.arg("-i").arg(&media.path);
    
    // Build filter chain and encoder selection based on detected hardware
    match (is_hdr, &hardware_encoder) {
        (true, HardwareEncoder::AMF) => {
            // HDR with AMF: Optimized for performance
            // Simplified filter chain for faster processing
            cmd.arg("-vf").arg(format!("setpts=PTS-STARTPTS,scale={}:-2:flags=fast_bilinear,format=p010le,tonemap=hable:desat=0,format=nv12", target_width));
            
            // Use AMF H.264 encoder
            cmd.arg("-c:v").arg("h264_amf");
            cmd.arg("-usage").arg("lowlatency");
            cmd.arg("-quality").arg("speed");
            cmd.arg("-b:v").arg(video_bitrate);
            cmd.arg("-maxrate").arg(max_bitrate);
            cmd.arg("-bufsize").arg(buffer_size);
            cmd.arg("-rc").arg("cbr");
        },
        (true, HardwareEncoder::VAAPI) => {
            // HDR with VAAPI: Simplified for performance
            cmd.arg("-vf").arg(format!("hwdownload,format=p010le,setpts=PTS-STARTPTS,scale={}:-2:flags=fast_bilinear,tonemap=hable:desat=0,format=nv12,hwupload", target_width));
            
            // Use VAAPI H.264 encoder
            cmd.arg("-c:v").arg("h264_vaapi");
            cmd.arg("-b:v").arg(video_bitrate);
            cmd.arg("-maxrate").arg(max_bitrate);
            cmd.arg("-bufsize").arg(buffer_size);
        },
        (true, HardwareEncoder::None) => {
            // HDR software only - simplified for real-time performance
            cmd.arg("-vf").arg(format!("setpts=PTS-STARTPTS,scale={}:-2:flags=fast_bilinear,format=p010le,tonemap=hable:desat=0,format=yuv420p", target_width));
            
            cmd.arg("-c:v").arg("libx264");
            cmd.arg("-preset").arg("ultrafast");
            cmd.arg("-tune").arg("zerolatency");
            cmd.arg("-b:v").arg(video_bitrate);
            cmd.arg("-maxrate").arg(max_bitrate);
            cmd.arg("-bufsize").arg(buffer_size);
            cmd.arg("-x264opts").arg("bframes=0:threads=16:rc-lookahead=0:weightp=0:aq-mode=0:ref=1:me=dia:subme=0:trellis=0:no-deblock:no-cabac:scenecut=0:sync-lookahead=0:aud=1");
        },
        (false, HardwareEncoder::AMF) => {
            // SDR with AMF - keep original resolution for SDR
            cmd.arg("-vf").arg(format!("scale={}:-2,format=nv12", target_width));
            cmd.arg("-c:v").arg("h264_amf");
            cmd.arg("-usage").arg("lowlatency");
            cmd.arg("-quality").arg("balanced");
            cmd.arg("-b:v").arg("6M");
            cmd.arg("-rc").arg("vbr");
        },
        (false, HardwareEncoder::VAAPI) => {
            // SDR with VAAPI - filters only, hwaccel already set
            cmd.arg("-vf").arg(format!("scale_vaapi=w={}:h=-2:format=nv12", target_width));
            cmd.arg("-c:v").arg("h264_vaapi");
            cmd.arg("-quality").arg("0");
            cmd.arg("-b:v").arg("6M");
        },
        (false, HardwareEncoder::None) => {
            // SDR software
            cmd.arg("-vf").arg(format!("scale={}:-2,format=yuv420p", target_width));
            cmd.arg("-c:v").arg("libx264");
            cmd.arg("-preset").arg("veryfast");
            cmd.arg("-crf").arg("22");
            cmd.arg("-x264opts").arg("threads=16");
        }
    }
    
    // Stream mapping - select first video and best audio stream
    cmd.arg("-map").arg("0:v:0"); // First video stream
    cmd.arg("-map").arg("0:a:0"); // First audio stream (FFmpeg will handle TrueHD)
    
    /*
    // Audio transcoding settings with better buffering
    cmd.arg("-c:a").arg("aac");
    cmd.arg("-profile:a").arg("aac_low");
    cmd.arg("-b:a").arg("192k");
    cmd.arg("-ac").arg("2"); // Stereo
    cmd.arg("-ar").arg("48000"); // Standard sample rate
    // Simpler audio filter for better stability
    cmd.arg("-af").arg("aresample=async=1:first_pts=0");
    */
    
    // Threading optimization for AMD 7950X (16 cores, 32 threads)
    cmd.arg("-threads").arg("16"); // Use half the threads for good balance
    
    // Use MPEGTS format optimized for streaming
    //cmd.arg("-f").arg("mpegts");
    //cmd.arg("-mpegts_copyts").arg("0"); // Don't copy timestamps to ensure they start at 0
    //cmd.arg("-pes_payload_size").arg("0"); // Let FFmpeg decide PES payload size
    
    // Better timestamp handling (fflags already set above with nobuffer)
    cmd.arg("-fflags").arg("+genpts+discardcorrupt+nobuffer");
    cmd.arg("-avoid_negative_ts").arg("make_zero");
    cmd.arg("-fps_mode").arg("cfr"); // Constant frame rate for consistent timing
    cmd.arg("-start_at_zero").arg("1"); // Force timestamps to start at 0
    
    // GOP settings for better seeking
    cmd.arg("-g").arg("48"); // Keyframe every 2 seconds
    cmd.arg("-keyint_min").arg("24");
    cmd.arg("-sc_threshold").arg("0");
    
    // Remove H.264 SEI data that can cause timing issues
    // Use dump_extra to remove SEI NAL units
    cmd.arg("-bsf:v").arg("dump_extra");
    
    // Buffer settings for preventing audio cutouts
    cmd.arg("-max_delay").arg("500000");
    cmd.arg("-muxdelay").arg("0.1");
    cmd.arg("-muxpreload").arg("0.5");
    cmd.arg("-max_muxing_queue_size").arg("1024");
    
    // Output to stdout
    cmd.arg("pipe:1");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped()); // Capture stderr for debugging
    
    // Log the FFmpeg command for debugging
    info!("FFmpeg command: {:?}", cmd);
    info!("HDR content detected: {}", is_hdr);
    info!("Hardware encoder: {:?}", hardware_encoder);
    
    // Spawn FFmpeg
    let mut child = cmd.spawn().map_err(|e| {
        error!("Failed to spawn FFmpeg: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    // Spawn a task to log FFmpeg stderr
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stderr);
            let mut lines = tokio::io::AsyncBufReadExt::lines(reader);
            while let Ok(Some(line)) = lines.next_line().await {
                if line.contains("error") || line.contains("Error") {
                    error!("FFmpeg: {}", line);
                } else {
                    debug!("FFmpeg: {}", line);
                }
            }
        });
    }
    
    // Get stdout
    let stdout = child.stdout.take().ok_or_else(|| {
        error!("Failed to get FFmpeg stdout");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    // Create async stream from stdout
    let stream = tokio_util::io::ReaderStream::new(stdout);
    let body = Body::from_stream(stream);
    
    // Build response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "video/mp2t")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .unwrap())
}

async fn hls_segment_handler(
    State(state): State<AppState>,
    Path((id, segment)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, StatusCode> {
    info!("HLS segment request for media ID: {}, segment: {}", id, segment);
    
    let profile_name = params.get("profile").cloned().unwrap_or_else(|| "hdr_to_sdr_1080p".to_string());
    
    // Build segment path
    let segment_path = state.config.transcode_cache_dir
        .join(&id)
        .join(&profile_name)
        .join(&segment);
    
    // Serve the segment file
    match tokio::fs::read(&segment_path).await {
        Ok(bytes) => {
            let mut response = Response::new(bytes.into());
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("video/mp2t"),
            );
            Ok(response)
        }
        Err(e) => {
            warn!("Failed to read segment file: {}", e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

#[derive(Deserialize)]
struct TranscodeRequest {
    profile: Option<String>,
    tone_mapping: Option<transcoding::config::ToneMappingConfig>,
}

async fn start_transcode_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<TranscodeRequest>,
) -> Result<Json<Value>, StatusCode> {
    info!("Transcode request for media ID: {}", id);
    
    // Get media to check if it exists and is HDR
    let media = match state.db.backend().get_media(&id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return Ok(Json(json!({
                "status": "error",
                "error": "Media not found"
            })));
        }
        Err(e) => {
            return Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })));
        }
    };
    
    // Determine profile
    let profile = if let Some(profile_name) = request.profile {
        match profile_name.as_str() {
            "hdr_to_sdr_1080p" => transcoding::profiles::TranscodingProfile::hdr_to_sdr_1080p(),
            "hdr_to_sdr_4k" => transcoding::profiles::TranscodingProfile::hdr_to_sdr_4k(),
            _ => {
                return Ok(Json(json!({
                    "status": "error",
                    "error": "Unknown profile"
                })));
            }
        }
    } else if transcoding::TranscodingService::is_hdr_content(&media).await {
        transcoding::profiles::TranscodingProfile::hdr_to_sdr_1080p()
    } else {
        return Ok(Json(json!({
            "status": "error",
            "error": "Media is not HDR content"
        })));
    };
    
    // Start transcoding
    match state.transcoding_service.start_transcoding(&id, profile, request.tone_mapping, None).await {
        Ok(job_id) => {
            Ok(Json(json!({
                "status": "success",
                "job_id": job_id,
                "message": "Transcoding started"
            })))
        }
        Err(e) => {
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

async fn transcode_status_handler(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<ferrex_core::TranscodingJobResponse>, StatusCode> {
    if let Some(job) = state.transcoding_service.get_job_status(&job_id).await {
        // Use stored source metadata duration instead of re-extracting
        let duration = job.source_metadata.as_ref().map(|m| m.duration);
        
        if let Some(dur) = duration {
            debug!("Job {} has stored duration: {} seconds", job_id, dur);
        } else {
            debug!("Job {} has no stored duration metadata", job_id);
        }
        
        // Get latest progress information if available
        // For master jobs, we need to aggregate progress from variant jobs
        let progress_details = if matches!(&job.job_type, transcoding::job::JobType::Master { .. }) {
            state.transcoding_service.get_master_job_progress(&job_id).await
        } else {
            state.transcoding_service.get_job_progress(&job_id).await
        };
        
        // Log job type and status for debugging
        info!("Job {} - Type: {:?}, Status: {:?}", job_id, job.job_type, job.status);
        
        // Convert server TranscodingStatus to shared TranscodingStatus
        let status = match &job.status {
            transcoding::job::TranscodingStatus::Pending => ferrex_core::TranscodingStatus::Pending,
            transcoding::job::TranscodingStatus::Queued => ferrex_core::TranscodingStatus::Queued,
            transcoding::job::TranscodingStatus::Processing { progress } => {
                ferrex_core::TranscodingStatus::Processing { progress: *progress }
            },
            transcoding::job::TranscodingStatus::Completed => ferrex_core::TranscodingStatus::Completed,
            transcoding::job::TranscodingStatus::Failed { error } => {
                ferrex_core::TranscodingStatus::Failed { error: error.clone() }
            },
            transcoding::job::TranscodingStatus::Cancelled => ferrex_core::TranscodingStatus::Cancelled,
        };
        
        // Log progress details for debugging
        if let Some(ref p) = progress_details {
            info!("Progress details for job {}: status={:?}, frames={:?}/{:?}, fps={:?}", 
                job_id, p.status, p.current_frame, p.total_frames, p.fps);
        } else {
            info!("No progress details available for job {}", job_id);
        }
        
        // Convert progress details if available
        let shared_progress_details = progress_details.map(|p| {
            ferrex_core::TranscodingProgressDetails {
                percentage: match &status { // Use the aggregated status, not p.status
                    ferrex_core::TranscodingStatus::Processing { progress } => *progress * 100.0,
                    _ => 0.0
                },
                time_elapsed: None, // Could calculate from job.started_at if available
                estimated_time_remaining: p.eta.map(|d| d.as_secs_f64()),
                frames_processed: p.current_frame,
                current_fps: p.fps.map(|f| f as f64),
                current_bitrate: p.bitrate.as_ref()
                    .and_then(|b| b.parse::<u64>().ok()),
            }
        });
        
        Ok(Json(ferrex_core::TranscodingJobResponse {
            id: job.id.clone(),
            media_id: job.media_id.clone(),
            media_path: job.media_id.clone(), // The actual file path
            profile: job.profile.name.clone(),
            status,
            created_at: job.created_at.elapsed().as_secs(),
            output_path: Some(job.output_dir.to_string_lossy().to_string()),
            playlist_path: Some(job.playlist_path.to_string_lossy().to_string()),
            error: job.error.clone(),
            progress_details: shared_progress_details,
            duration,
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// Start adaptive bitrate transcoding for a media file
/// This creates multiple quality variants for adaptive streaming
async fn start_adaptive_transcode_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!("Adaptive transcode request for media ID: {}", id);
    
    // Decode the percent-encoded ID
    let decoded_id = urlencoding::decode(&id).map_err(|e| {
        error!("Failed to decode media ID: {}", e);
        StatusCode::BAD_REQUEST
    })?;
    
    match state.transcoding_service.start_adaptive_transcoding(&decoded_id, None).await {
        Ok(job_id) => {
            Ok(Json(json!({
                "status": "success",
                "master_job_id": job_id,
                "message": "Adaptive bitrate transcoding started",
                "info": "Use /transcode/{id}/master.m3u8 to get the master playlist once ready"
            })))
        }
        Err(e) => {
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

/// Get a specific segment on-the-fly
/// This endpoint generates segments on demand for true streaming
async fn get_segment_handler(
    State(state): State<AppState>,
    Path((job_id, segment_number)): Path<(String, u32)>,
) -> Result<Response<Body>, StatusCode> {
    match state.transcoding_service.get_segment(&job_id, segment_number).await {
        Ok(segment_path) => {
            // Stream the segment file
            match tokio::fs::File::open(&segment_path).await {
                Ok(file) => {
                    let stream = ReaderStream::new(file);
                    let body = Body::from_stream(stream);
                    
                    Ok(Response::builder()
                        .header(header::CONTENT_TYPE, "video/MP2T")
                        .header(header::CACHE_CONTROL, "public, max-age=3600")
                        .body(body)
                        .unwrap())
                }
                Err(_) => Err(StatusCode::NOT_FOUND),
            }
        }
        Err(e) => {
            error!("Failed to get segment: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get master playlist for adaptive bitrate streaming
async fn get_master_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    info!("Master playlist request for media ID: {}", id);
    
    // Decode the percent-encoded ID
    let decoded_id = urlencoding::decode(&id).map_err(|e| {
        error!("Failed to decode media ID: {}", e);
        StatusCode::BAD_REQUEST
    })?;
    
    match state.transcoding_service.get_master_playlist(&decoded_id).await {
        Some(playlist_path) => {
            info!("Found master playlist at: {:?}", playlist_path);
            match tokio::fs::read_to_string(&playlist_path).await {
                Ok(content) => {
                    info!("Successfully read master playlist, size: {} bytes", content.len());
                    Ok(Response::builder()
                        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                        .header(header::CACHE_CONTROL, "no-cache")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .body(Body::from(content))
                        .unwrap())
                }
                Err(e) => {
                    warn!("Failed to read master playlist: {}", e);
                    Err(StatusCode::NOT_FOUND)
                }
            }
        }
        None => {
            warn!("Master playlist not found for media ID: {}", id);
            // Check if variant directories exist and generate master playlist on the fly
            let cache_dir = state.config.transcode_cache_dir.join(&id);
            match tokio::fs::read_dir(&cache_dir).await {
                Ok(mut entries) => {
                    let mut variants = Vec::new();
                    
                    // Scan for variant directories
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        if let Ok(file_type) = entry.file_type().await {
                            if file_type.is_dir() {
                                let name = entry.file_name();
                                if let Some(name_str) = name.to_str() {
                                    if name_str.starts_with("adaptive_") {
                                        variants.push(name_str.to_string());
                                    }
                                }
                            }
                        }
                    }
                    
                    if !variants.is_empty() {
                        info!("Found {} variants, generating master playlist on the fly", variants.len());
                        
                        // Generate master playlist content
                        let mut master_content = "#EXTM3U\n#EXT-X-VERSION:3\n\n".to_string();
                        
                        // Sort variants by quality
                        variants.sort();
                        
                        for variant_name in variants {
                            // Extract resolution from variant name (e.g., "adaptive_720p" -> "720p")
                            let quality = variant_name.trim_start_matches("adaptive_");
                            
                            let (width, height, bandwidth) = match quality {
                                "360p" => (640, 360, 1000000),
                                "480p" => (854, 480, 2000000),
                                "720p" => (1280, 720, 3000000),
                                "1080p" => (1920, 1080, 5000000),
                                "4k" => (3840, 2160, 20000000),
                                "original" => {
                                    // For original quality, we should use actual source dimensions
                                    // For now, use high values that will be sorted last
                                    (7680, 4320, 50000000) // 8K placeholder with very high bitrate
                                },
                                _ => continue,
                            };
                            
                            // Check if playlist exists in this variant
                            let playlist_path = cache_dir.join(&variant_name).join("playlist.m3u8");
                            if tokio::fs::metadata(&playlist_path).await.is_ok() {
                                master_content.push_str(&format!(
                                    "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}\nvariant/{}/playlist.m3u8\n\n",
                                    bandwidth, width, height, variant_name
                                ));
                            }
                        }
                        
                        // Save the generated master playlist for future use
                        let master_path = cache_dir.join("master.m3u8");
                        if let Err(e) = tokio::fs::write(&master_path, &master_content).await {
                            warn!("Failed to save generated master playlist: {}", e);
                        }
                        
                        Ok(Response::builder()
                            .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                            .header(header::CACHE_CONTROL, "no-cache")
                            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                            .body(Body::from(master_content))
                            .unwrap())
                    } else {
                        warn!("No variant directories found for media ID: {}", id);
                        Err(StatusCode::NOT_FOUND)
                    }
                }
                Err(e) => {
                    warn!("Failed to read cache directory: {}", e);
                    Err(StatusCode::NOT_FOUND)
                }
            }
        }
    }
}

/// Get variant playlist for a specific quality profile
async fn get_variant_playlist_handler(
    State(state): State<AppState>,
    Path((id, profile)): Path<(String, String)>,
) -> Result<Response<Body>, StatusCode> {
    match state.transcoding_service.get_playlist_url(&id, &profile).await {
        Some(playlist_path) => {
            match tokio::fs::read_to_string(&playlist_path).await {
                Ok(mut content) => {
                    // FFmpeg generates segment files with relative paths like "segment_000.ts"
                    // We need to update these to include the full path for our server
                    // Replace segment references to use our segment endpoint
                    content = content.replace("segment_", &format!("/transcode/{}/variant/{}/segment_", id, profile));
                    
                    Ok(Response::builder()
                        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                        .header(header::CACHE_CONTROL, "no-cache")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .body(Body::from(content))
                        .unwrap())
                }
                Err(_) => Err(StatusCode::NOT_FOUND),
            }
        }
        None => {
            // Variant doesn't exist - check if we should start on-demand transcoding
            info!("Variant {} not found for media {}, checking for on-demand transcoding", profile, id);
            
            // Extract quality from profile (e.g., "adaptive_720p" -> "720p")
            if profile.starts_with("adaptive_") {
                let quality = profile.trim_start_matches("adaptive_");
                
                // Check if this is a valid quality we support
                let valid_qualities = ["360p", "480p", "720p", "1080p", "4k", "original"];
                if valid_qualities.contains(&quality) {
                    // Get media info to create proper transcoding profile
                    match state.db.backend().get_media(&id).await {
                        Ok(Some(media)) => {
                            // Create transcoding profile for this specific variant
                            let transcode_profile = match quality {
                                "360p" => transcoding::profiles::TranscodingProfile {
                                    name: profile.clone(),
                                    video_codec: "libx264".to_string(),
                                    audio_codec: "copy".to_string(), // Pass through original audio
                                    video_bitrate: "800k".to_string(),
                                    audio_bitrate: "0".to_string(), // Not used with copy codec
                                    resolution: Some("640x360".to_string()),
                                    preset: "fast".to_string(),
                                    apply_tone_mapping: false,
                                },
                                "480p" => transcoding::profiles::TranscodingProfile {
                                    name: profile.clone(),
                                    video_codec: "libx264".to_string(),
                                    audio_codec: "copy".to_string(), // Pass through original audio
                                    video_bitrate: "2M".to_string(),
                                    audio_bitrate: "0".to_string(), // Not used with copy codec
                                    resolution: Some("854x480".to_string()),
                                    preset: "fast".to_string(),
                                    apply_tone_mapping: false,
                                },
                                "1080p" => transcoding::profiles::TranscodingProfile {
                                    name: profile.clone(),
                                    video_codec: "libx264".to_string(),
                                    audio_codec: "copy".to_string(), // Pass through original audio
                                    video_bitrate: "8M".to_string(),
                                    audio_bitrate: "0".to_string(), // Not used with copy codec
                                    resolution: Some("1920x1080".to_string()),
                                    preset: "fast".to_string(),
                                    apply_tone_mapping: false,
                                },
                                _ => {
                                    warn!("Unsupported on-demand quality: {}", quality);
                                    return Err(StatusCode::NOT_FOUND);
                                }
                            };
                            
                            // Start on-demand transcoding
                            info!("Starting on-demand transcoding for {} variant of media {}", quality, id);
                            match state.transcoding_service
                                .start_transcoding(&id, transcode_profile, None, Some(transcoding::job::JobPriority::High))
                                .await
                            {
                                Ok(job_id) => {
                                    info!("Started on-demand transcoding job {} for variant {}", job_id, profile);
                                    // Return 202 Accepted to indicate transcoding has started
                                    Ok(Response::builder()
                                        .status(StatusCode::ACCEPTED)
                                        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                                        .header(header::CACHE_CONTROL, "no-cache")
                                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                                        .header("X-Transcode-Status", "started")
                                        .header("X-Transcode-Job-Id", job_id)
                                        .body(Body::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-PLAYLIST-TYPE:EVENT\n# Transcoding in progress..."))
                                        .unwrap())
                                }
                                Err(e) => {
                                    warn!("Failed to start on-demand transcoding: {}", e);
                                    Err(StatusCode::SERVICE_UNAVAILABLE)
                                }
                            }
                        }
                        _ => {
                            warn!("Media {} not found for on-demand transcoding", id);
                            Err(StatusCode::NOT_FOUND)
                        }
                    }
                } else {
                    Err(StatusCode::NOT_FOUND)
                }
            } else {
                Err(StatusCode::NOT_FOUND)
            }
        },
    }
}

/// Serve variant segment files
async fn get_variant_segment_handler(
    State(state): State<AppState>,
    Path((id, profile, segment)): Path<(String, String, String)>,
) -> Result<Response<Body>, StatusCode> {
    let segment_path = state.config.transcode_cache_dir
        .join(&id)
        .join(&profile)
        .join(&segment);
    
    match tokio::fs::File::open(&segment_path).await {
        Ok(file) => {
            let stream = ReaderStream::new(file);
            let body = Body::from_stream(stream);
            
            Ok(Response::builder()
                .header(header::CONTENT_TYPE, "video/MP2T")
                .header(header::CACHE_CONTROL, "public, max-age=3600")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(body)
                .unwrap())
        }
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

/// Cancel a transcoding job
async fn cancel_transcode_handler(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    match state.transcoding_service.cancel_job(&job_id).await {
        Ok(()) => {
            Ok(Json(json!({
                "status": "success",
                "message": "Job cancelled"
            })))
        }
        Err(e) => {
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

/// List available transcoding profiles
async fn list_transcode_profiles_handler(
    State(_state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    // Return available profiles
    Ok(Json(json!({
        "status": "success",
        "profiles": [
            {
                "name": "hdr_to_sdr_1080p",
                "description": "HDR to SDR conversion at 1080p",
                "video_codec": "libx264",
                "audio_codec": "aac",
                "resolution": "1920x1080"
            },
            {
                "name": "hdr_to_sdr_4k",
                "description": "HDR to SDR conversion at 4K",
                "video_codec": "libx265",
                "audio_codec": "aac",
                "resolution": "3840x2160"
            },
            {
                "name": "adaptive",
                "description": "Adaptive bitrate with multiple quality variants",
                "variants": ["360p", "480p", "720p", "1080p", "4k"]
            }
        ]
    })))
}

/// Get transcoding cache statistics
async fn transcode_cache_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    match state.transcoding_service.get_cache_stats().await {
        Ok(stats) => {
            Ok(Json(json!({
                "status": "success",
                "cache_stats": {
                    "total_size_mb": stats.total_size_mb,
                    "file_count": stats.file_count,
                    "media_count": stats.media_count,
                    "oldest_file_age_days": stats.oldest_file_age_days
                }
            })))
        }
        Err(e) => {
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

/// Clear transcoding cache for a specific media file
async fn clear_transcode_cache_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!("Clear cache request for media ID: {}", id);
    
    // Clear all cached variants for this media
    let cache_dir = state.config.transcode_cache_dir.join(&id);
    if cache_dir.exists() {
        match tokio::fs::remove_dir_all(&cache_dir).await {
            Ok(()) => {
                info!("Cleared transcode cache for media: {}", id);
                Ok(Json(json!({
                    "status": "success",
                    "message": "Cache cleared successfully"
                })))
            }
            Err(e) => {
                error!("Failed to clear cache: {}", e);
                Ok(Json(json!({
                    "status": "error",
                    "error": e.to_string()
                })))
            }
        }
    } else {
        Ok(Json(json!({
            "status": "success",
            "message": "No cache to clear"
        })))
    }
}
