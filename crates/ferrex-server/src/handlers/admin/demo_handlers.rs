use axum::extract::State;
use axum::response::Json;

use crate::demo::DemoSizeOverrides;
use crate::infra::app_state::AppState;
use crate::infra::errors::{AppError, AppResult};
use ferrex_core::api::types::{ApiResponse, DemoResetRequest, DemoStatus};

pub async fn status(
    State(state): State<AppState>,
) -> AppResult<Json<ApiResponse<DemoStatus>>> {
    let Some(coordinator) = state.demo().clone() else {
        return Err(AppError::not_found("demo mode is not enabled"));
    };

    let status = coordinator.describe().await;
    Ok(Json(ApiResponse::success(status)))
}

pub async fn reset(
    State(state): State<AppState>,
    maybe_body: Option<Json<DemoResetRequest>>,
) -> AppResult<Json<ApiResponse<DemoStatus>>> {
    let Some(coordinator) = state.demo().clone() else {
        return Err(AppError::not_found("demo mode is not enabled"));
    };

    let overrides = maybe_body.map(|Json(body)| DemoSizeOverrides::from(body));

    coordinator
        .reset(state.unit_of_work().clone(), overrides)
        .await
        .map_err(|err| {
            AppError::internal(format!("failed to reset demo data: {}", err))
        })?;

    let status = coordinator.describe().await;
    Ok(Json(ApiResponse::success(status)))
}

pub async fn resize(
    State(state): State<AppState>,
    Json(body): Json<DemoResetRequest>,
) -> AppResult<Json<ApiResponse<DemoStatus>>> {
    let Some(coordinator) = state.demo().clone() else {
        return Err(AppError::not_found("demo mode is not enabled"));
    };

    if body.is_empty() {
        return Err(AppError::bad_request(
            "Specify a size for movies, series, or both",
        ));
    }

    let overrides = DemoSizeOverrides::from(body);
    let scan_control = state.scan_control();

    coordinator
        .resize(
            state.unit_of_work().clone(),
            scan_control.as_ref(),
            overrides,
        )
        .await
        .map_err(|err| {
            AppError::internal(format!("failed to resize demo data: {}", err))
        })?;

    let status = coordinator.describe().await;
    Ok(Json(ApiResponse::success(status)))
}
