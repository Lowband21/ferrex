use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use chrono::Utc;
use ferrex_core::api::types::ApiResponse;
use ferrex_core::domain::users::auth::domain::value_objects::SessionScope;
use ferrex_core::{
    domain::{users::user::User, watch::UpdateProgressRequest},
    types::MediaType,
};
use serde::Deserialize;
use serde::Serialize;
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
#[derive(Debug, Deserialize)]
pub struct StreamAuthQuery {
    #[serde(default)]
    access_token: Option<String>,
}

pub async fn stream_with_progress_handler(
    State(state): State<AppState>,
    Path(media_id): Path<Uuid>,
    headers: HeaderMap,
    Query(query): Query<StreamAuthQuery>,
) -> Result<Response, (StatusCode, String)> {
    debug!("stream request");
    debug!("Requested media ID: {}", media_id);

    // Accept either Authorization: Bearer <token> header or an
    // access_token query parameter for clients that cannot set headers.
    let token_opt = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| query.access_token.clone());

    if let Some(token) = token_opt {
        // Validate token; reject unauthorized/expired sessions and enforce scope
        match state.auth_service().validate_session_token(&token).await {
            Ok(validated) => match validated.scope {
                SessionScope::Full | SessionScope::Playback => {}
            },
            Err(err) => {
                warn!("Stream token validation failed: {:?}", err);
                return Err((StatusCode::UNAUTHORIZED, "Invalid token".into()));
            }
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "Missing token".into()));
    }

    // Fetch media metadata
    let media_file = state
        .unit_of_work()
        .media_files_read
        .get_by_id(&media_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error retrieving media {}: {}", media_id, e),
            )
        })?
        .ok_or_else(|| {
            (StatusCode::NOT_FOUND, "Media not found".to_string())
        })?;

    debug!(
        "Found media file: {:?} (path: {:?})",
        media_file.filename, media_file.path
    );

    if !media_file.path.exists() {
        warn!("Media file not found on disk: {:?}", media_file.path);

        if let Some(media_root) = state.config().media.root.as_ref()
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
            .header("Cache-Control", "private, no-store")
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
        .header("Cache-Control", "private, no-store")
        .header("Connection", "keep-alive")
        .body(axum::body::Body::from_stream(stream))
        .expect("failed to build OK response"))
}

#[derive(Debug, Serialize)]
pub struct PlaybackTicketResponse {
    pub access_token: String,
    pub expires_in: i64,
}

/// Issue a short-lived playback token suitable for query-string embedding.
pub async fn playback_ticket_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(device_session_id): Extension<Option<Uuid>>,
    Path(media_id): Path<Uuid>,
) -> Result<axum::Json<ApiResponse<PlaybackTicketResponse>>, (StatusCode, String)>
{
    // Optionally ensure the requested media exists to avoid issuing tokens for unknown items
    if state
        .unit_of_work()
        .media_files_read
        .get_by_id(&media_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .is_none()
    {
        return Err((StatusCode::NOT_FOUND, "Media not found".into()));
    }
    // Lifetime: 6 hours — long enough for extended playback/seeks
    let lifetime = chrono::Duration::hours(6);
    let token = state
        .auth_service()
        .issue_playback_session(user.id, device_session_id, lifetime)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let expires_in = (token.expires_at() - Utc::now()).num_seconds().max(0);
    let body = PlaybackTicketResponse {
        access_token: token.as_str().to_string(),
        expires_in,
    };

    Ok(axum::Json(ApiResponse::success(body)))
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
        .unit_of_work()
        .watch_status
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
