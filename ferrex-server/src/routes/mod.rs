pub mod v1;

use crate::AppState;
use axum::Router;

/// Create the main API router with all versions
pub fn create_api_router(state: AppState) -> Router<AppState> {
    Router::new().nest("/api/v1", v1::create_v1_router(state))
    // Future versions can be added here:
    // .nest("/api/v2", v2::create_v2_router(state))
}
