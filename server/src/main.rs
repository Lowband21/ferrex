mod config;
mod metadata_service;
mod scan_manager;
mod thumbnail_service;

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{sse::Sse, Json, Response},
    routing::{get, post},
    Router,
};
use config::Config;
use rusty_media_core::{
    database::traits::MediaFilters, EpisodeSummary, MediaDatabase, MediaScanner, MetadataExtractor,
    ScanResult, SeasonDetails, SeasonSummary, TvShowDetails,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::io::ReaderStream;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
struct AppState {
    db: Arc<MediaDatabase>,
    config: Arc<Config>,
    metadata_service: Arc<metadata_service::MetadataService>,
    thumbnail_service: Arc<thumbnail_service::ThumbnailService>,
    scan_manager: Arc<scan_manager::ScanManager>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration from environment
    let config = Arc::new(Config::from_env()?);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rusty_media_server=debug,tower_http=debug".into()),
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

    let state = AppState {
        db,
        config: config.clone(),
        metadata_service,
        thumbnail_service,
        scan_manager,
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

fn create_app(state: AppState) -> Router {
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
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

#[derive(Deserialize)]
struct MetadataRequest {
    path: String,
}

#[derive(Debug, Deserialize)]
struct LibraryFilters {
    media_type: Option<String>,
    show_name: Option<String>,
    season: Option<u32>,
    order_by: Option<String>,
    limit: Option<u64>,
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
                    if parsed.media_type == rusty_media_core::MediaType::TvEpisode {
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

    // Use the ID directly - the database layer will handle any format conversion
    let db_id = id.clone();

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

    // Fetch metadata
    match state.metadata_service.fetch_metadata(&media_file).await {
        Ok(detailed_info) => {
            info!("Metadata fetched successfully for: {}", id);

            // Update the media file with external info
            let mut updated_media = media_file.clone();
            if let Some(ref mut metadata) = updated_media.metadata {
                metadata.external_info = Some(detailed_info.external_info.clone());
            }

            // Store updated media file
            if let Err(e) = state.db.backend().store_media(updated_media).await {
                warn!("Failed to update media with metadata: {}", e);
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
                "metadata": detailed_info
            })))
        }
        Err(e) => {
            warn!("Metadata fetch failed for {}: {}", id, e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
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
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("image/jpeg"),
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
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_ping_endpoint() {
        let config = Arc::new(Config::from_env().unwrap());
        let db = Arc::new(MediaDatabase::new_memory().await.unwrap());
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
