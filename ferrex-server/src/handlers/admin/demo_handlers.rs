#![cfg(feature = "demo")]

use axum::extract::State;
use axum::response::Json;

use crate::demo::DemoStatus;
use crate::infra::app_state::AppState;
use crate::infra::errors::{AppError, AppResult};

pub async fn status(State(state): State<AppState>) -> AppResult<Json<DemoStatus>> {
    let Some(coordinator) = state.demo.clone() else {
        return Err(AppError::not_found("demo mode is not enabled"));
    };

    Ok(Json(coordinator.describe().await))
}

pub async fn reset(State(state): State<AppState>) -> AppResult<Json<DemoStatus>> {
    let Some(coordinator) = state.demo.clone() else {
        return Err(AppError::not_found("demo mode is not enabled"));
    };

    coordinator
        .reset(state.unit_of_work.clone())
        .await
        .map_err(|err| AppError::internal(format!("failed to reset demo data: {}", err)))?;

    Ok(Json(coordinator.describe().await))
}
