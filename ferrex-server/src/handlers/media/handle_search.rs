use axum::{Extension, Json, extract::State};
use ferrex_core::{
    query::types::{MediaQuery, MediaWithStatus},
    user::User,
};

use crate::infra::{app_state::AppState, errors::AppResult};

/// Execute a media query
pub async fn query_media_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(mut query): Json<MediaQuery>,
) -> AppResult<Json<Vec<MediaWithStatus>>> {
    // Add user context to the query
    query.user_context = Some(user.id);

    // Execute the query
    let results = state.unit_of_work.query.query_media(&query).await?;

    Ok(Json(results))
}

/// Execute a media query without authentication (public)
pub async fn query_media_public_handler(
    State(state): State<AppState>,
    Json(query): Json<MediaQuery>,
) -> AppResult<Json<Vec<MediaWithStatus>>> {
    // Execute the query without user context
    let results = state.unit_of_work.query.query_media(&query).await?;

    Ok(Json(results))
}
