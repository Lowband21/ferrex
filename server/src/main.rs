use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Json, Response},
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use rusty_media_core::{MediaScanner, ScanResult, MetadataExtractor, MediaDatabase, MediaFilters};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tokio_util::io::ReaderStream;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present
    dotenv::dotenv().ok();
    
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rusty_media_server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Create shared database instance
    let db = Arc::new(MediaDatabase::new_memory().await?);
    if let Err(e) = db.initialize_schema().await {
        warn!("Failed to initialize database schema: {}", e);
    }
    info!("Database initialized successfully");

    let app = create_app(db);

    let host = std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("SERVER_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .unwrap_or(3000);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Starting Rusty Media Server on {}:{}", host, port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn create_app(db: Arc<MediaDatabase>) -> Router {
    Router::new()
        .route("/ping", get(ping_handler))
        .route("/scan", post(scan_handler))
        .route("/scan", get(scan_status_handler))
        .route("/metadata", post(metadata_handler))
        .route("/library", get(library_get_handler).post(library_post_handler))
        .route("/library/scan-and-store", post(scan_and_store_handler))
        .route("/stream/:id", get(stream_handler))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(db)
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
    State(db): State<Arc<MediaDatabase>>,
    Query(filters): Query<LibraryFilters>
) -> Result<Json<Value>, StatusCode> {
    info!("Library GET request with filters: {:?}", filters);
    library_handler_impl(db, filters).await
}

async fn library_post_handler(
    State(db): State<Arc<MediaDatabase>>,
    Json(filters): Json<LibraryFilters>
) -> Result<Json<Value>, StatusCode> {
    info!("Library POST request with filters: {:?}", filters);
    library_handler_impl(db, filters).await
}

async fn library_handler_impl(db: Arc<MediaDatabase>, filters: LibraryFilters) -> Result<Json<Value>, StatusCode> {
    info!("Library request with filters: {:?}", filters);
    
    let media_filters = MediaFilters {
        media_type: filters.media_type,
        show_name: filters.show_name,
        season: filters.season,
        order_by: filters.order_by,
        limit: filters.limit,
    };
    
    match db.list_media(media_filters).await {
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
    State(db): State<Arc<MediaDatabase>>,
    Json(request): Json<ScanAndStoreRequest>
) -> Result<Json<Value>, StatusCode> {
    // Use provided path or fall back to MEDIA_ROOT
    let scan_path = match request.path {
        Some(path) => {
            info!("Using provided path: {}", path);
            path
        },
        None => {
            info!("No path provided, checking MEDIA_ROOT environment variable");
            match std::env::var("MEDIA_ROOT") {
                Ok(path) => {
                    info!("Using MEDIA_ROOT: {}", path);
                    path
                },
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
    
    // Store each media file
    for mut media_file in scan_result.video_files {
        // Extract metadata if requested
        if let Some(ref mut metadata_extractor) = extractor {
            match metadata_extractor.extract_metadata(&media_file.path) {
                Ok(metadata) => {
                    media_file.metadata = Some(metadata);
                }
                Err(e) => {
                    warn!("Metadata extraction failed for {}: {}", media_file.filename, e);
                    extraction_errors.push(format!("{}: {}", media_file.filename, e));
                }
            }
        }
        
        // Store in database
        match db.store_media(media_file).await {
            Ok(_) => {
                stored_count += 1;
            }
            Err(e) => {
                warn!("Failed to store media file: {}", e);
                extraction_errors.push(format!("Storage failed: {}", e));
            }
        }
    }
    
    info!("Stored {} media files from scan of {}", stored_count, scan_path);
    
    Ok(Json(json!({
        "status": "success",
        "message": format!("Scanned and stored {} media files", stored_count),
        "scanned": scan_result.total_files,
        "stored": stored_count,
        "skipped": scan_result.skipped_files,
        "extraction_errors": extraction_errors,
        "scan_errors": scan_result.errors
    })))
}

async fn stream_handler(
    State(db): State<Arc<MediaDatabase>>,
    Path(id): Path<String>, 
    headers: HeaderMap
) -> Result<Response, StatusCode> {
    info!("Stream request for media ID: {}", id);
    
    // Format ID for database lookup (add "media:" prefix if not present)
    let db_id = if id.starts_with("media:") {
        id.clone()
    } else {
        format!("media:{}", id)
    };
    
    // Get media file from database
    let media_file = match db.get_media(&db_id).await {
        Ok(Some(media)) => media,
        Ok(None) => {
            warn!("Media file not found: {}", id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            warn!("Database error retrieving media {}: {}", id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    
    // Check if file exists on disk
    if !media_file.path.exists() {
        warn!("Media file not found on disk: {:?}", media_file.path);
        return Err(StatusCode::NOT_FOUND);
    }
    
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
    let content_type = match media_file.path.extension().and_then(|ext| ext.to_str()) {
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
                
                // Read the requested range
                let content_length = range.end - range.start + 1;
                let mut buffer = vec![0; content_length as usize];
                match file.read_exact(&mut buffer).await {
                    Ok(_) => {
                        info!("Serving range {}-{} ({} bytes)", range.start, range.end, content_length);
                        
                        return Ok(Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header(header::CONTENT_TYPE, content_type)
                            .header(header::CONTENT_LENGTH, content_length.to_string())
                            .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", range.start, range.end, file_size))
                            .header(header::ACCEPT_RANGES, "bytes")
                            .body(axum::body::Body::from(buffer))
                            .unwrap());
                    }
                    Err(e) => {
                        warn!("Failed to read file range: {}", e);
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                }
            }
        }
    }
    
    // Stream entire file
    info!("Streaming entire file: {} ({} bytes)", media_file.filename, file_size);
    
    let stream = ReaderStream::new(file);
    let body = axum::body::Body::from_stream(stream);
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_ping_endpoint() {
        let db = Arc::new(MediaDatabase::new_memory().await.unwrap());
        let app = create_app(db);

        let response = app
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}