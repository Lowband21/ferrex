pub mod v1;

use crate::infra::app_state::AppState;
use axum::Router;

/// Create the main API router with all versions
pub fn create_api_router(state: AppState) -> Router<AppState> {
    v1::create_v1_router(state)
}
