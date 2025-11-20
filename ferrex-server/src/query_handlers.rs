use axum::{extract::State, Extension, Json};
use ferrex_core::{query::MediaQuery, user::User, MediaWithStatus};

use crate::{errors::AppResult, AppState};

/// Execute a media query
pub async fn query_media_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(mut query): Json<MediaQuery>,
) -> AppResult<Json<Vec<MediaWithStatus>>> {
    // Add user context to the query
    query.user_context = Some(user.id);

    // Execute the query
    let results = state.database.backend().query_media(&query).await?;

    Ok(Json(results))
}

/// Execute a media query without authentication (public)
pub async fn query_media_public_handler(
    State(state): State<AppState>,
    Json(query): Json<MediaQuery>,
) -> AppResult<Json<Vec<MediaWithStatus>>> {
    // Execute the query without user context
    let results = state.database.backend().query_media(&query).await?;

    Ok(Json(results))
}
