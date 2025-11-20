use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use ferrex_core::{api_types::MediaId, watch_status::UpdateProgressRequest, User};
use serde::Deserialize;
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ProgressReport {
    pub position: f32,
    pub duration: f32,
}

/// Stream media with automatic progress tracking
pub async fn stream_with_progress_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    // Parse media_id
    let media_id = parse_media_id(&media_id)?;

    // Get the media file path from database
    let media_path = get_media_file_path(&state, &media_id).await?;

    // Open the file
    let file = File::open(&media_path)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "Media file not found".to_string()))?;

    // Get file metadata for content length
    let metadata = file.metadata().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to read file metadata".to_string(),
        )
    })?;

    let file_size = metadata.len();

    // Create stream
    let stream = ReaderStream::new(file);

    // Prepare headers
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "video/mp4".parse().unwrap());
    headers.insert(
        header::CONTENT_LENGTH,
        file_size.to_string().parse().unwrap(),
    );
    headers.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());

    // Return streaming response
    Ok((
        StatusCode::OK,
        headers,
        axum::body::Body::from_stream(stream),
    )
        .into_response())
}

/// Report playback progress during streaming
pub async fn report_progress_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<String>,
    Json(progress): Json<ProgressReport>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Parse media_id
    let media_id = parse_media_id(&media_id)?;

    // Create update request
    let request = UpdateProgressRequest {
        media_id,
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

/// Helper function to get media file path from database
async fn get_media_file_path(
    state: &AppState,
    media_id: &MediaId,
) -> Result<String, (StatusCode, String)> {
    // Extract the ID string from the MediaId
    let id_str = match media_id {
        MediaId::Movie(movie_id) => movie_id.as_ref(),
        MediaId::Episode(episode_id) => episode_id.as_ref(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported media type".to_string(),
            ))
        }
    };

    // Get media file from database
    let media_file = state
        .db
        .backend()
        .get_media(id_str)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get media: {}", e),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Media not found".to_string()))?;

    Ok(media_file.path.to_string_lossy().to_string())
}

/// Helper function to parse media ID from string
fn parse_media_id(media_id_str: &str) -> Result<MediaId, (StatusCode, String)> {
    // Try to parse as UUID first
    if let Ok(uuid) = Uuid::parse_str(media_id_str) {
        // For now, assume it's a movie ID
        return Ok(MediaId::Movie(
            ferrex_core::media::MovieID::new(uuid.to_string()).unwrap(),
        ));
    }

    // Try to parse as "type:id" format
    let parts: Vec<&str> = media_id_str.split(':').collect();
    if parts.len() != 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid media ID format. Expected UUID or 'type:id'".to_string(),
        ));
    }

    let id = Uuid::parse_str(parts[1]).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid UUID in media ID".to_string(),
        )
    })?;

    match parts[0] {
        "movie" => Ok(MediaId::Movie(
            ferrex_core::media::MovieID::new(id.to_string()).unwrap(),
        )),
        "episode" => Ok(MediaId::Episode(
            ferrex_core::media::EpisodeID::new(id.to_string()).unwrap(),
        )),
        _ => Err((
            StatusCode::BAD_REQUEST,
            format!("Unknown media type: {}", parts[0]),
        )),
    }
}
