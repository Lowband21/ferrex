use axum::{extract::State, Extension, Json};
use ferrex_core::{
    api_types::ApiResponse,
    query::MediaQuery,
    query::MediaWithStatus,
    user::User,
};

use crate::infra::{app_state::AppState, errors::AppResult};

const DEFAULT_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_LIMIT: usize = 100;

/// Execute a media query
pub async fn query_media_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(mut query): Json<MediaQuery>,
) -> AppResult<Json<ApiResponse<Vec<MediaWithStatus>>>> {
    // Add user context to the query
    query.user_context = Some(user.id);

    clamp_query_limit(&mut query);

    // Execute the query
    let results = state.unit_of_work().query.query_media(&query).await?;

    Ok(Json(ApiResponse::success(results)))
}

/// Execute a media query without authentication (public)
pub async fn query_media_public_handler(
    State(state): State<AppState>,
    Json(mut query): Json<MediaQuery>,
) -> AppResult<Json<ApiResponse<Vec<MediaWithStatus>>>> {
    // Execute the query without user context
    clamp_query_limit(&mut query);
    clamp_query_limit(&mut query);
    let results = state.unit_of_work().query.query_media(&query).await?;

    Ok(Json(ApiResponse::success(results)))
}

fn clamp_query_limit(query: &mut MediaQuery) {
    if query.pagination.limit == 0 {
        query.pagination.limit = DEFAULT_SEARCH_LIMIT;
    } else if query.pagination.limit > MAX_SEARCH_LIMIT {
        query.pagination.limit = MAX_SEARCH_LIMIT;
    }
}
