use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use ferrex_core::types::watch::{
    EpisodeKey, NextEpisode, SeasonWatchStatus, SeriesWatchStatus,
};
use ferrex_core::{
    api::types::ApiResponse,
    domain::users::user::User,
    domain::watch::{
        ContinueWatchingItem, UpdateProgressRequest, UserWatchState,
    },
};
use ferrex_model::VideoMediaType;
use serde::{Deserialize, Serialize};
use sqlx::Row;
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

/// Backward-compatible JSON payload for watch progress writes.
#[derive(Debug, Deserialize)]
pub struct UpdateProgressBody {
    pub media_id: Uuid,
    #[serde(default)]
    pub media_type: Option<VideoMediaType>,
    pub position: f32,
    pub duration: f32,
    #[serde(default)]
    pub episode: Option<EpisodeKey>,
    #[serde(default)]
    pub last_media_uuid: Option<Uuid>,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedWatchTarget {
    logical_media_id: Uuid,
    media_type: VideoMediaType,
    last_media_uuid: Option<Uuid>,
}

fn parse_media_type_label(label: &str) -> Option<VideoMediaType> {
    match label.to_ascii_lowercase().as_str() {
        "movie" => Some(VideoMediaType::Movie),
        "series" => Some(VideoMediaType::Series),
        "season" => Some(VideoMediaType::Season),
        "episode" => Some(VideoMediaType::Episode),
        _ => None,
    }
}

async fn resolve_watch_target(
    state: &AppState,
    media_id: Uuid,
    media_type: Option<VideoMediaType>,
    last_media_uuid: Option<Uuid>,
) -> Result<ResolvedWatchTarget, (StatusCode, String)> {
    let postgres = state.postgres();
    let pool = postgres.pool();

    if let Some(row) = sqlx::query(
        r#"
        SELECT media_id, media_type::text AS media_type
        FROM media_files
        WHERE id = $1
          AND is_available = TRUE
        "#,
    )
    .bind(media_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to resolve playable media id: {e}"),
        )
    })? {
        let logical_media_id =
            row.try_get::<Uuid, _>("media_id").map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to decode resolved media id: {e}"),
                )
            })?;
        let resolved_media_type = row
            .try_get::<String, _>("media_type")
            .ok()
            .and_then(|value| parse_media_type_label(&value))
            .ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Resolved media file had an unsupported media_type"
                        .to_string(),
                )
            })?;

        if let Some(requested) = media_type
            && requested != resolved_media_type
        {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "media_type {:?} did not match resolved playback target {:?}",
                    requested, resolved_media_type
                ),
            ));
        }

        return Ok(ResolvedWatchTarget {
            logical_media_id,
            media_type: resolved_media_type,
            last_media_uuid: last_media_uuid.or(Some(media_id)),
        });
    }

    if let Some(media_type) = media_type {
        return Ok(ResolvedWatchTarget {
            logical_media_id: media_id,
            media_type,
            last_media_uuid,
        });
    }

    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM movie_references WHERE id = $1)",
    )
    .bind(media_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to resolve movie watch target: {e}"),
        )
    })? {
        return Ok(ResolvedWatchTarget {
            logical_media_id: media_id,
            media_type: VideoMediaType::Movie,
            last_media_uuid,
        });
    }

    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM episode_references WHERE id = $1)",
    )
    .bind(media_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to resolve episode watch target: {e}"),
        )
    })? {
        return Ok(ResolvedWatchTarget {
            logical_media_id: media_id,
            media_type: VideoMediaType::Episode,
            last_media_uuid,
        });
    }

    Err((
        StatusCode::BAD_REQUEST,
        format!(
            "Unable to resolve media {} to a logical movie/episode target",
            media_id
        ),
    ))
}

async fn resolve_update_progress_request(
    state: &AppState,
    body: UpdateProgressBody,
) -> Result<UpdateProgressRequest, (StatusCode, String)> {
    let target = resolve_watch_target(
        state,
        body.media_id,
        body.media_type,
        body.last_media_uuid,
    )
    .await?;

    Ok(UpdateProgressRequest {
        media_id: target.logical_media_id,
        media_type: target.media_type,
        position: body.position,
        duration: body.duration,
        episode: body.episode,
        last_media_uuid: target.last_media_uuid,
    })
}

fn require_media_type(
    target: ResolvedWatchTarget,
    expected: VideoMediaType,
    route_label: &str,
) -> Result<ResolvedWatchTarget, (StatusCode, String)> {
    if target.media_type == expected {
        Ok(target)
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            format!(
                "{route_label} route only supports {expected:?} targets, got {:?}",
                target.media_type
            ),
        ))
    }
}

async fn resolve_episode_key_for_logical_media(
    state: &AppState,
    logical_media_id: Uuid,
) -> Result<EpisodeKey, (StatusCode, String)> {
    let postgres = state.postgres();
    let row = sqlx::query(
        r#"
        SELECT tmdb_series_id, season_number, episode_number
        FROM episode_references
        WHERE id = $1
        "#,
    )
    .bind(logical_media_id)
    .fetch_optional(postgres.pool())
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to resolve episode identity: {e}"),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!(
                "Unable to resolve logical episode {} to an episode identity",
                logical_media_id
            ),
        )
    })?;

    Ok(EpisodeKey {
        tmdb_series_id: row.try_get::<i64, _>("tmdb_series_id").map_err(
            |e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to decode tmdb_series_id: {e}"),
                )
            },
        )? as u64,
        season_number: row.try_get::<i16, _>("season_number").map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to decode season_number: {e}"),
            )
        })? as u16,
        episode_number: row.try_get::<i16, _>("episode_number").map_err(
            |e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to decode episode_number: {e}"),
                )
            },
        )? as u16,
    })
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
/// - Progress >= 95% automatically marks the item as completed
/// - Position of 0 does not create a progress entry
/// - Limited to 50 in-progress items per user (oldest are removed)
pub async fn update_progress_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(body): Json<UpdateProgressBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let request = resolve_update_progress_request(&state, body).await?;

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
) -> Result<Json<ApiResponse<Vec<ContinueWatchingItem>>>, (StatusCode, String)>
{
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
    let target = resolve_watch_target(&state, media_id, None, None).await?;

    state
        .unit_of_work()
        .watch_status
        .clear_watch_progress(user.id, &target.logical_media_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to clear progress: {}", e),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Explicitly mark a movie as watched.
pub async fn mark_movie_watched_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let target = require_media_type(
        resolve_watch_target(&state, media_id, None, None).await?,
        VideoMediaType::Movie,
        "movie watched",
    )?;

    state
        .unit_of_work()
        .watch_status
        .mark_media_watched(
            user.id,
            target.logical_media_id,
            target.media_type,
            target.last_media_uuid,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to mark movie as watched: {}", e),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Explicitly mark a movie as unwatched.
pub async fn mark_movie_unwatched_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let target = require_media_type(
        resolve_watch_target(&state, media_id, None, None).await?,
        VideoMediaType::Movie,
        "movie unwatched",
    )?;

    state
        .unit_of_work()
        .watch_status
        .mark_media_unwatched(
            user.id,
            target.logical_media_id,
            target.media_type,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to mark movie as unwatched: {}", e),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Explicitly mark an episode as watched.
pub async fn mark_episode_watched_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let target = require_media_type(
        resolve_watch_target(&state, media_id, None, None).await?,
        VideoMediaType::Episode,
        "episode watched",
    )?;
    let _episode_key =
        resolve_episode_key_for_logical_media(&state, target.logical_media_id)
            .await?;

    state
        .unit_of_work()
        .watch_status
        .mark_media_watched(
            user.id,
            target.logical_media_id,
            target.media_type,
            target.last_media_uuid,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to mark episode as watched: {}", e),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Explicitly mark an episode as unwatched.
pub async fn mark_episode_unwatched_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let target = require_media_type(
        resolve_watch_target(&state, media_id, None, None).await?,
        VideoMediaType::Episode,
        "episode unwatched",
    )?;
    let _episode_key =
        resolve_episode_key_for_logical_media(&state, target.logical_media_id)
            .await?;

    state
        .unit_of_work()
        .watch_status
        .mark_media_unwatched(
            user.id,
            target.logical_media_id,
            target.media_type,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to mark episode as unwatched: {}", e),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Explicitly mark all known episodes in a series as watched.
pub async fn mark_series_watched_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(tmdb_series_id): Path<u64>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .unit_of_work()
        .watch_status
        .mark_series_watched(user.id, tmdb_series_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to mark series as watched: {}", e),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Explicitly clear all known watch state for a series.
pub async fn mark_series_unwatched_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(tmdb_series_id): Path<u64>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .unit_of_work()
        .watch_status
        .mark_series_unwatched(user.id, tmdb_series_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to mark series as unwatched: {}", e),
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
    let target = resolve_watch_target(&state, media_id, None, None).await?;
    let logical_media_id = target.logical_media_id;

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

    let is_completed = watch_state.completed.contains(&logical_media_id);

    let progress = watch_state.in_progress.get(&logical_media_id).map(|item| {
        ProgressResponse {
            media_id: logical_media_id,
            position: item.position,
            duration: item.duration,
            percentage: (item.position / item.duration) * 100.0,
            is_completed,
        }
    });

    // If not in progress but is completed, return full progress
    let progress = progress.or({
        if is_completed {
            Some(ProgressResponse {
                media_id: logical_media_id,
                position: 0.0,
                duration: 0.0,
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
) -> Result<StatusCode, (StatusCode, String)> {
    let target = resolve_watch_target(&state, media_id, None, None).await?;

    state
        .unit_of_work()
        .watch_status
        .mark_media_watched(
            user.id,
            target.logical_media_id,
            target.media_type,
            target.last_media_uuid,
        )
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
    let target = resolve_watch_target(&state, media_id, None, None).await?;

    let is_completed = state
        .unit_of_work()
        .watch_status
        .is_media_completed(user.id, &target.logical_media_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to check completion status: {}", e),
            )
        })?;

    Ok(Json(is_completed))
}

/// Get series watch state (identity-based aggregation)
pub async fn get_series_watch_state_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(tmdb_series_id): Path<u64>,
) -> Result<Json<ApiResponse<SeriesWatchStatus>>, (StatusCode, String)> {
    let status = state
        .unit_of_work()
        .watch_status
        .get_series_watch_status(user.id, tmdb_series_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get series watch state: {}", e),
            )
        })?;
    Ok(Json(ApiResponse::success(status)))
}

/// Get season watch state (identity-based aggregation)
pub async fn get_season_watch_state_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path((tmdb_series_id, season_number)): Path<(u64, u16)>,
) -> Result<Json<ApiResponse<SeasonWatchStatus>>, (StatusCode, String)> {
    let status = state
        .unit_of_work()
        .watch_status
        .get_season_watch_status(user.id, tmdb_series_id, season_number)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get season watch state: {}", e),
            )
        })?;
    Ok(Json(ApiResponse::success(status)))
}

/// Get next episode for a series (identity-based)
pub async fn get_series_next_episode_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(tmdb_series_id): Path<u64>,
) -> Result<Json<ApiResponse<Option<NextEpisode>>>, (StatusCode, String)> {
    let next = state
        .unit_of_work()
        .watch_status
        .get_next_episode(user.id, tmdb_series_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get next episode: {}", e),
            )
        })?;
    Ok(Json(ApiResponse::success(next)))
}
