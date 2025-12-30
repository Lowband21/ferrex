use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};
use axum::{Extension, Json, extract::State};
use ferrex_core::{api::types::ApiResponse, domain::users::user::User};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Request to update user preferences
#[derive(Debug, Deserialize)]
pub struct UpdatePreferencesRequest {
    pub auto_login_enabled: Option<bool>,
    // Add other preference fields as needed
}

/// Response with updated preferences
#[derive(Debug, Serialize)]
pub struct PreferencesResponse {
    pub auto_login_enabled: bool,
    // Add other preference fields as needed
}

/// Update user preferences
pub async fn update_preferences(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(_device_id): Extension<Option<Uuid>>,
    Json(request): Json<UpdatePreferencesRequest>,
) -> AppResult<Json<ApiResponse<PreferencesResponse>>> {
    let mut updated_user = user.clone();
    let mut changed = false;

    // Update auto-login preference if provided
    if let Some(auto_login) = request.auto_login_enabled
        && updated_user.preferences.auto_login_enabled != auto_login
    {
        updated_user.preferences.auto_login_enabled = auto_login;
        changed = true;
    }

    // Only update if something changed
    if changed {
        updated_user.updated_at = chrono::Utc::now();

        state
            .unit_of_work()
            .users
            .update_user(&updated_user)
            .await
            .map_err(|_| {
                AppError::internal("Failed to update user preferences")
            })?;
    }

    Ok(Json(ApiResponse::success(PreferencesResponse {
        auto_login_enabled: updated_user.preferences.auto_login_enabled,
    })))
}

/// Get user preferences
pub async fn get_preferences(
    State(_state): State<AppState>,
    Extension(user): Extension<User>,
) -> AppResult<Json<ApiResponse<PreferencesResponse>>> {
    Ok(Json(ApiResponse::success(PreferencesResponse {
        auto_login_enabled: user.preferences.auto_login_enabled,
    })))
}
