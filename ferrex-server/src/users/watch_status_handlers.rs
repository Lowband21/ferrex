use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use ferrex_core::{
    api_types::ApiResponse,
    types::MediaType,
    user::User,
    watch_status::{InProgressItem, UpdateProgressRequest, UserWatchState},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infra::app_state::AppState;

#[derive(Debug, Deserialize)]
pub struct ContinueWatchingQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    20
}

#[derive(Debug, Serialize)]
pub struct ProgressResponse {
    pub media_id: Uuid,
    pub position: f32,
    pub duration: f32,
    pub percentage: f32,
    pub is_completed: bool,
}

/// Update watch progress for a media item
///
/// Updates the user's viewing progress for a specific media item.
/// Progress updates are typically sent every 10-30 seconds during playback.
///
/// # Request
///
/// ```json
/// {
///   "media_id": "movie:550e8400-e29b-41d4-a716-446655440000",
///   "position": 1800.0,
///   "duration": 7200.0
/// }
/// ```
///
/// # Response
///
/// - `204 No Content` on success
/// - `400 Bad Request` if validation fails
///
/// # Behavior
///
/// - Progress > 95% automatically marks the item as completed
/// - Position of 0 does not create a progress entry
/// - Limited to 50 in-progress items per user (oldest are removed)
pub async fn update_progress_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<UpdateProgressRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Validate the request
    if request.position < 0.0 || request.duration <= 0.0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid position or duration".to_string(),
        ));
    }

    if request.position > request.duration {
        return Err((
            StatusCode::BAD_REQUEST,
            "Position cannot exceed duration".to_string(),
        ));
    }

    // Update progress in database
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

/// Get the complete watch state for the current user
///
/// Retrieves the user's complete watch state including all in-progress
/// items and the count of completed items.
///
/// # Response
///
/// ```json
/// {
///   "in_progress": [
///     {
///       "media_id": "movie:550e8400-e29b-41d4-a716-446655440000",
///       "position": 3600.0,
///       "duration": 7200.0,
///       "last_watched": 1704067200
///     }
///   ],
///   "completed": ["movie:123e4567-e89b-12d3-a456-426614174000"]
/// }
/// ```
///
/// # Notes
///
/// - In-progress items are sorted by last_watched (most recent first)
/// - Completed items are returned as a set for efficient lookup
pub async fn get_watch_state_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> Result<Json<ApiResponse<UserWatchState>>, (StatusCode, String)> {
    let watch_state = state
        .unit_of_work()
        .watch_status
        .get_user_watch_state(user.id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get watch state: {}", e),
            )
        })?;

    Ok(Json(ApiResponse::success(watch_state)))
}

/// Get continue watching list for the current user
pub async fn get_continue_watching_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(params): Query<ContinueWatchingQuery>,
) -> Result<Json<ApiResponse<Vec<InProgressItem>>>, (StatusCode, String)> {
    let items = state
        .unit_of_work()
        .watch_status
        .get_continue_watching(user.id, params.limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get continue watching: {}", e),
            )
        })?;

    Ok(Json(ApiResponse::success(items)))
}

/// Clear watch progress for a specific media item
pub async fn clear_progress_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .unit_of_work()
        .watch_status
        .clear_watch_progress(user.id, &media_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to clear progress: {}", e),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Get progress for a specific media item
pub async fn get_media_progress_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Option<ProgressResponse>>>, (StatusCode, String)> {
    // Get user's watch state
    let watch_state = state
        .unit_of_work()
        .watch_status
        .get_user_watch_state(user.id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get watch state: {}", e),
            )
        })?;

    // Check if media is completed
    let is_completed = watch_state.completed.contains(&media_id);

    // Check if media is in progress
    let progress = watch_state
        .in_progress
        .iter()
        .find(|(id, _)| *id == &media_id)
        .map(|(_, item)| ProgressResponse {
            media_id,
            position: item.position,
            duration: item.duration,
            percentage: (item.position / item.duration) * 100.0,
            is_completed,
        });

    // If not in progress but is completed, return full progress
    let progress = progress.or({
        if is_completed {
            Some(ProgressResponse {
                media_id,
                position: 0.0, // We don't store position for completed items
                duration: 0.0, // We don't store duration for completed items
                percentage: 100.0,
                is_completed: true,
            })
        } else {
            None
        }
    });

    Ok(Json(ApiResponse::success(progress)))
}

/// Mark a media item as completed
pub async fn mark_completed_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<Uuid>,
    Path(media_type): Path<MediaType>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Create a progress update request with 100% completion
    let request = UpdateProgressRequest {
        media_id,
        media_type,
        position: 1.0, // Dummy position
        duration: 1.0, // Dummy duration to ensure 100% completion
    };

    // Update progress to mark as completed
    state
        .unit_of_work()
        .watch_status
        .update_watch_progress(user.id, &request)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to mark as completed: {}", e),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Check if a media item is completed
pub async fn is_completed_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<Uuid>,
) -> Result<Json<bool>, (StatusCode, String)> {
    let is_completed = state
        .unit_of_work()
        .watch_status
        .is_media_completed(user.id, &media_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to check completion status: {}", e),
            )
        })?;

    Ok(Json(is_completed))
}
