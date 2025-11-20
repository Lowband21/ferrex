use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use ferrex_core::{MediaType, User, watch_status::UpdateProgressRequest};
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
    Path(media_id): Path<Uuid>,
) -> Result<Response, (StatusCode, String)> {
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

/// Helper function to get media file path from database
async fn get_media_file_path(
    state: &AppState,
    media_id: &Uuid,
) -> Result<String, (StatusCode, String)> {
    // Get media file from database
    let media_file = state
        .db
        .backend()
        .get_media(media_id)
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
