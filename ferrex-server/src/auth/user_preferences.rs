use axum::{Extension, Json, extract::State};
use serde::{Deserialize, Serialize};
use ferrex_core::{user::User, ApiResponse};
use uuid::Uuid;
use crate::{
    errors::{AppError, AppResult},
    AppState,
};

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
    Extension(device_id): Extension<Option<Uuid>>,
    Json(request): Json<UpdatePreferencesRequest>,
) -> AppResult<Json<ApiResponse<PreferencesResponse>>> {
    let mut updated_user = user.clone();
    let mut changed = false;
    
    // Update auto-login preference if provided
    if let Some(auto_login) = request.auto_login_enabled {
        if updated_user.preferences.auto_login_enabled != auto_login {
            updated_user.preferences.auto_login_enabled = auto_login;
            changed = true;
            
            // Also update the device credential if we have a device_id
            if let Some(device_id) = device_id {
                if let Ok(Some(mut credential)) = state.db.backend()
                    .get_device_credential(user.id, device_id)
                    .await
                {
                    credential.auto_login_enabled = auto_login;
                    credential.updated_at = chrono::Utc::now();
                    let _ = state.db.backend().upsert_device_credential(&credential).await;
                    
                    tracing::info!("Updated device credential auto-login for user {} on device {}", 
                        user.id, device_id);
                }
            }
        }
    }
    
    // Only update if something changed
    if changed {
        updated_user.updated_at = chrono::Utc::now();
        
        state.db.backend().update_user(&updated_user).await
            .map_err(|_| AppError::internal("Failed to update user preferences"))?;
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