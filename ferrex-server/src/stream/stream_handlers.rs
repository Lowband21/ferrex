use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use ferrex_core::{MediaType, User, watch_status::UpdateProgressRequest};
use serde::Deserialize;
use tokio_util::io::ReaderStream;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::infra::app_state::AppState;

#[derive(Debug, Deserialize)]
pub struct ProgressReport {
    pub position: f32,
    pub duration: f32,
}

/// Stream media with automatic progress tracking.
pub async fn stream_with_progress_handler(
    State(state): State<AppState>,
    Path(media_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    // NOTE: Authentication is temporarily disabled here until the GStreamer
    // souphttpsrc extra-headers hook is wired to forward the Bearer token from
    // the player. Re-enable once the pipeline sends the Authorization header.
    debug!("stream request");
    debug!("Requested media ID: {}", media_id);

    // Fetch media metadata
    let media_file = state
        .db
        .backend()
        .get_media(&media_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error retrieving media {}: {}", media_id, e),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Media not found".to_string()))?;

    debug!(
        "Found media file: {:?} (path: {:?})",
        media_file.filename, media_file.path
    );

    if !media_file.path.exists() {
        warn!("Media file not found on disk: {:?}", media_file.path);

        if let Some(media_root) = &state.config.media_root
            && !media_root.exists()
        {
            warn!("Media library root is offline: {:?}", media_root);
            return Ok(Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header("X-Media-Error", "library-offline")
                .body(axum::body::Body::empty())
                .expect("failed to build SERVICE_UNAVAILABLE response"));
        }

        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("X-Media-Error", "file-missing")
            .body(axum::body::Body::empty())
            .expect("failed to build NOT_FOUND response"));
    }

    let file_size = media_file.size;
    let extension = media_file.path.extension().and_then(|ext| ext.to_str());
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
    debug!("Content-Type: {}", content_type);

    let file = tokio::fs::File::open(&media_file.path).await.map_err(|e| {
        warn!("Failed to open file {:?}: {}", media_file.path, e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Media file not accessible".to_string(),
        )
    })?;

    if let Some(range_header) = headers.get(header::RANGE)
        && let Ok(range_str) = range_header.to_str()
        && let Some(range) = parse_range_header(range_str, file_size)
    {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        debug!("Range request: {}-{}/{}", range.start, range.end, file_size);
        let mut file = file;
        if let Err(e) = file.seek(std::io::SeekFrom::Start(range.start)).await {
            warn!("Failed to seek in file: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to seek in media file".to_string(),
            ));
        }

        let content_length = range.end - range.start + 1;
        info!(
            "Serving range {}-{} ({} bytes) for media {}",
            range.start, range.end, content_length, media_id
        );

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
            .header("Cache-Control", "public, max-age=3600")
            .header("Connection", "keep-alive")
            .body(axum::body::Body::from_stream(stream))
            .expect("failed to build PARTIAL_CONTENT response"));
    }

    info!(
        "Streaming entire file: {} ({} bytes)",
        media_file.filename, file_size
    );

    let stream = ReaderStream::new(file);
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header("Cache-Control", "public, max-age=3600")
        .header("Connection", "keep-alive")
        .body(axum::body::Body::from_stream(stream))
        .expect("failed to build OK response"))
}

/// Report playback progress during streaming
pub async fn report_progress_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path((media_type, media_id)): Path<(MediaType, Uuid)>,
    Json(progress): Json<ProgressReport>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Create update request
    let request = UpdateProgressRequest {
        media_id,
        media_type,
        position: progress.position,
        duration: progress.duration,
    };

    // Update progress
    state
        .db
        .backend()
        .update_watch_progress(user.id, &request)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to update progress: {}", e),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug)]
struct ByteRange {
    start: u64,
    end: u64,
}

fn parse_range_header(range_str: &str, file_size: u64) -> Option<ByteRange> {
    if !range_str.starts_with("bytes=") {
        return None;
    }

    let range_part = &range_str[6..];
    let parts: Vec<&str> = range_part.split('-').collect();
    if parts.len() != 2 {
        return None;
    }

    let start = if parts[0].is_empty() {
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
        file_size.saturating_sub(1)
    } else if let Ok(end) = parts[1].parse::<u64>() {
        std::cmp::min(end, file_size.saturating_sub(1))
    } else {
        return None;
    };

    if start <= end && start < file_size {
        Some(ByteRange { start, end })
    } else {
        None
    }
}
