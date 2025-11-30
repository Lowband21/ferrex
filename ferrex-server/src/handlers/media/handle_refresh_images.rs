//! Handler for refreshing (invalidating) cached images for a media item.
//!
//! This endpoint allows users to trigger a cache refresh when TMDB has
//! updated posters or backdrops for a media item.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::Serialize;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::infra::app_state::AppState;

/// Response body for the refresh images endpoint
#[derive(Debug, Serialize)]
pub struct RefreshImagesResponse {
    /// Number of variants that were invalidated
    pub invalidated_count: u32,
    /// Message describing the result
    pub message: String,
}

/// POST /api/v1/media/{type}/{id}/refresh-images
///
/// Invalidates all cached image variants for a media item, allowing them
/// to be re-downloaded on the next request. This is useful when TMDB
/// has updated posters or backdrops.
///
/// Path parameters:
/// - `type`: Media type (movie, series, season, episode, person)
/// - `id`: Media UUID
///
/// Returns:
/// - 200 OK with count of invalidated variants
/// - 400 BAD_REQUEST if media type is invalid
/// - 404 NOT_FOUND if media ID is invalid or not found
/// - 500 INTERNAL_SERVER_ERROR on database errors
pub async fn refresh_images_handler(
    State(state): State<AppState>,
    Path((media_type, media_id)): Path<(String, String)>,
) -> impl IntoResponse {
    info!(
        "Refresh images request: type={}, id={}",
        media_type, media_id
    );

    // Validate media type
    if !["movie", "series", "season", "episode", "person"]
        .contains(&media_type.as_str())
    {
        warn!("Invalid media type for refresh: {}", media_type);
        return Err(StatusCode::BAD_REQUEST);
    }

    // Parse media ID
    let media_uuid = match Uuid::parse_str(&media_id) {
        Ok(uuid) => uuid,
        Err(e) => {
            warn!("Invalid media ID '{}': {}", media_id, e);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Invalidate all cached variants for this media item
    match state
        .image_service()
        .invalidate_all_variants(&media_type, media_uuid)
        .await
    {
        Ok(count) => {
            info!(
                "Invalidated {} image variants for {}/{}",
                count, media_type, media_id
            );
            Ok(Json(RefreshImagesResponse {
                invalidated_count: count,
                message: format!(
                    "Successfully invalidated {} cached image variants",
                    count
                ),
            }))
        }
        Err(e) => {
            error!(
                "Failed to invalidate images for {}/{}: {}",
                media_type, media_id, e
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
