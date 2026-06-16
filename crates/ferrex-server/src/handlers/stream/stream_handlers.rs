use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use ferrex_core::api::types::ApiResponse;
use ferrex_core::database::repository_ports::media_files::PlaybackMediaSource;
use ferrex_core::domain::users::auth::domain::value_objects::SessionScope;
use ferrex_core::domain::{users::user::User, watch::UpdateProgressRequest};
use ferrex_model::VideoMediaType;
use serde::Deserialize;
use serde::Serialize;
use std::io::ErrorKind;

use tokio_util::io::ReaderStream;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::infra::app_state::AppState;

const MEDIA_ERROR_HEADER: HeaderName = HeaderName::from_static("x-media-error");
const CACHE_CONTROL_PRIVATE_NO_STORE: HeaderValue =
    HeaderValue::from_static("private, no-store");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackHttpError {
    status: StatusCode,
    media_error: Option<&'static str>,
    message: &'static str,
}

impl PlaybackHttpError {
    const fn typed(
        status: StatusCode,
        media_error: &'static str,
        message: &'static str,
    ) -> Self {
        Self {
            status,
            media_error: Some(media_error),
            message,
        }
    }

    const fn plain(status: StatusCode, message: &'static str) -> Self {
        Self {
            status,
            media_error: None,
            message,
        }
    }

    const fn missing_token() -> Self {
        Self::plain(StatusCode::UNAUTHORIZED, "Missing token")
    }

    const fn invalid_token() -> Self {
        Self::plain(StatusCode::UNAUTHORIZED, "Invalid token")
    }

    const fn media_not_found() -> Self {
        Self::typed(
            StatusCode::NOT_FOUND,
            "media-not-found",
            "Media not found.",
        )
    }

    const fn media_unavailable() -> Self {
        Self::typed(
            StatusCode::GONE,
            "media-unavailable",
            "This media file is no longer available in the library.",
        )
    }

    const fn library_offline() -> Self {
        Self::typed(
            StatusCode::SERVICE_UNAVAILABLE,
            "library-offline",
            "The media library root is offline.",
        )
    }

    const fn file_missing() -> Self {
        Self::typed(
            StatusCode::NOT_FOUND,
            "file-missing",
            "The media file is missing on disk.",
        )
    }

    const fn file_inaccessible() -> Self {
        Self::typed(
            StatusCode::FORBIDDEN,
            "file-inaccessible",
            "The media file exists but is not accessible.",
        )
    }

    const fn internal() -> Self {
        Self::typed(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Ferrex could not prepare playback for this media.",
        )
    }

    fn from_open_error(error: &std::io::Error) -> Self {
        match error.kind() {
            ErrorKind::NotFound => Self::file_missing(),
            ErrorKind::PermissionDenied => Self::file_inaccessible(),
            _ => Self::typed(
                StatusCode::SERVICE_UNAVAILABLE,
                "file-inaccessible",
                "The media file could not be opened for streaming.",
            ),
        }
    }
}

impl IntoResponse for PlaybackHttpError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ApiResponse::<()>::error(self.message.to_string())),
        )
            .into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, CACHE_CONTROL_PRIVATE_NO_STORE);
        if let Some(media_error) = self.media_error {
            response.headers_mut().insert(
                MEDIA_ERROR_HEADER,
                HeaderValue::from_static(media_error),
            );
        }
        response
    }
}

async fn load_playback_source(
    state: &AppState,
    media_id: Uuid,
) -> Result<PlaybackMediaSource, PlaybackHttpError> {
    state
        .unit_of_work()
        .media_files_read
        .get_playback_source_by_id(&media_id)
        .await
        .map_err(|err| {
            error!(?err, %media_id, "failed to load playback media source");
            PlaybackHttpError::internal()
        })?
        .ok_or_else(PlaybackHttpError::media_not_found)
}

fn ensure_playback_source_available(
    state: &AppState,
    source: &PlaybackMediaSource,
) -> Result<(), PlaybackHttpError> {
    if !source.is_available {
        warn!(media_id = %source.id, "playback requested unavailable media file");
        return Err(PlaybackHttpError::media_unavailable());
    }

    if !source.path.exists() {
        warn!(media_id = %source.id, path = ?source.path, "playback file not found on disk");
        if let Some(media_root) = state.config().media.root.as_ref()
            && !media_root.exists()
        {
            warn!(media_root = ?media_root, "media library root is offline");
            return Err(PlaybackHttpError::library_offline());
        }

        return Err(PlaybackHttpError::file_missing());
    }

    Ok(())
}

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
) -> Result<Response, PlaybackHttpError> {
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
            Err(_) => {
                warn!("stream token validation failed");
                return Err(PlaybackHttpError::invalid_token());
            }
        }
    } else {
        return Err(PlaybackHttpError::missing_token());
    }

    let media_file = load_playback_source(&state, media_id).await?;
    debug!(
        "Found playback source: {:?} (path: {:?})",
        media_file.filename, media_file.path
    );
    ensure_playback_source_available(&state, &media_file)?;

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
        PlaybackHttpError::from_open_error(&e)
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
            return Err(PlaybackHttpError::internal());
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
///
/// Tickets intentionally reuse persisted `SessionScope::Playback` sessions
/// rather than a media-bound token table so expiry, revocation, and device
/// binding stay on the existing auth path. The general auth middleware rejects
/// playback-scoped sessions for account/admin APIs; only this stream endpoint
/// accepts them for media delivery.
pub async fn playback_ticket_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(device_session_id): Extension<Option<Uuid>>,
    Path(media_id): Path<Uuid>,
) -> Result<axum::Json<ApiResponse<PlaybackTicketResponse>>, PlaybackHttpError>
{
    let media_file = load_playback_source(&state, media_id).await?;
    ensure_playback_source_available(&state, &media_file)?;

    // Lifetime: 6 hours — long enough for extended playback/seeks
    let lifetime = chrono::Duration::hours(6);
    let token = state
        .auth_service()
        .issue_playback_session(user.id, device_session_id, lifetime)
        .await
        .map_err(|err| {
            error!(?err, %media_id, user_id = %user.id, "failed to issue playback ticket");
            PlaybackHttpError::internal()
        })?;

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
    Path((media_type, media_id)): Path<(VideoMediaType, Uuid)>,
    Json(progress): Json<ProgressReport>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Create update request
    let request = UpdateProgressRequest {
        media_id,
        media_type,
        position: progress.position,
        duration: progress.duration,
        episode: None,
        last_media_uuid: Some(media_id),
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
