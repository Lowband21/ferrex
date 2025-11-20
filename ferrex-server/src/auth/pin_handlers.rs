//! PIN authentication handlers with admin session verification

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use ferrex_core::{
    api_types::ApiResponse,
    auth::pin::{PinPolicy, SetPinRequest, SetPinResponse},
    user::User,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    errors::{AppError, AppResult},
    AppState,
};

/// Request to authenticate using PIN
#[derive(Debug, Deserialize)]
pub struct PinAuthRequest {
    pub device_id: Uuid,
    pub pin: String,
}

/// Response for successful PIN authentication
#[derive(Debug, Serialize)]
pub struct PinAuthResponse {
    pub success: bool,
    pub message: String,
    pub session_token: Option<String>,
}

/// Authenticate using PIN - requires admin session on device
pub async fn authenticate_with_pin(
    State(state): State<AppState>,
    Json(request): Json<PinAuthRequest>,
) -> AppResult<Json<ApiResponse<PinAuthResponse>>> {
    // CRITICAL: Check if admin is authenticated on this device
    if !state
        .is_admin_authenticated_on_device(request.device_id)
        .await
    {
        return Err(AppError::forbidden(
            "PIN authentication requires an admin to be logged in on this device",
        ));
    }

    // Get admin session info to identify which admin authorized this
    let admin_session = state
        .get_admin_session(request.device_id)
        .await
        .ok_or_else(|| AppError::forbidden("Admin session not found or expired"))?;

    // TODO: Implement actual PIN verification logic
    // This would involve:
    // 1. Looking up the PIN hash for the user/device
    // 2. Verifying the provided PIN against the hash
    // 3. Checking rate limiting/attempt tracking
    // 4. Creating a new session token if successful

    tracing::info!(
        "PIN authentication attempt for device {} by admin {}",
        request.device_id,
        admin_session.user_id
    );

    // Placeholder response - implement actual PIN verification
    Ok(Json(ApiResponse::success(PinAuthResponse {
        success: false,
        message: "PIN authentication not yet fully implemented".to_string(),
        session_token: None,
    })))
}

/// Set or update PIN for a device - requires admin session
pub async fn set_pin(
    State(state): State<AppState>,
    Json(request): Json<SetPinRequest>,
) -> AppResult<Json<ApiResponse<SetPinResponse>>> {
    // CRITICAL: Check if admin is authenticated on this device
    if !state
        .is_admin_authenticated_on_device(request.device_id)
        .await
    {
        return Err(AppError::forbidden(
            "PIN setup requires an admin to be logged in on this device",
        ));
    }

    // Get admin session info
    let admin_session = state
        .get_admin_session(request.device_id)
        .await
        .ok_or_else(|| AppError::forbidden("Admin session not found or expired"))?;

    // Validate PIN according to policy
    let policy = PinPolicy::default();
    if let Err(pin_error) = ferrex_core::auth::pin::validate_pin(&request.new_pin, &policy) {
        return Ok(Json(ApiResponse::success(SetPinResponse {
            success: false,
            message: format!("PIN validation failed: {}", pin_error),
        })));
    }

    // TODO: Implement actual PIN storage logic
    // This would involve:
    // 1. Hashing the PIN securely
    // 2. Storing the hash associated with the device/user
    // 3. Invalidating any existing PIN for the device

    tracing::info!(
        "PIN setup request for device {} by admin {}",
        request.device_id,
        admin_session.user_id
    );

    // Placeholder response - implement actual PIN storage
    Ok(Json(ApiResponse::success(SetPinResponse {
        success: false,
        message: "PIN setup not yet fully implemented".to_string(),
    })))
}

/// Remove PIN for a device - requires admin session
pub async fn remove_pin(
    State(state): State<AppState>,
    Path(device_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    // CRITICAL: Check if admin is authenticated on this device
    if !state.is_admin_authenticated_on_device(device_id).await {
        return Err(AppError::forbidden(
            "PIN removal requires an admin to be logged in on this device",
        ));
    }

    // Get admin session info
    let admin_session = state
        .get_admin_session(device_id)
        .await
        .ok_or_else(|| AppError::forbidden("Admin session not found or expired"))?;

    // TODO: Implement actual PIN removal logic
    // This would involve:
    // 1. Removing PIN hash from storage
    // 2. Invalidating any active PIN sessions

    tracing::info!(
        "PIN removal request for device {} by admin {}",
        device_id,
        admin_session.user_id
    );

    // Placeholder response - implement actual PIN removal
    Ok(StatusCode::NOT_IMPLEMENTED)
}

/// Check if PIN is available for a device (requires admin to be logged in)
pub async fn check_pin_availability(
    State(state): State<AppState>,
    Path(device_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<bool>>> {
    // Check if admin is authenticated on this device
    let admin_authenticated = state.is_admin_authenticated_on_device(device_id).await;

    // Only return true if admin is authenticated (PIN can only be used when admin is logged in)
    Ok(Json(ApiResponse::success(admin_authenticated)))
}

/// Admin endpoint to register admin session for PIN eligibility
pub async fn register_admin_session(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<RegisterAdminSessionRequest>,
) -> AppResult<StatusCode> {
    // TODO: Add proper admin role verification
    // For now, assume the middleware has already verified admin status

    match state
        .register_admin_session(user.id, request.device_id, request.session_token)
        .await
    {
        Ok(_) => Ok(StatusCode::CREATED),
        Err(e) => {
            tracing::error!("Failed to register admin session: {}", e);
            Err(AppError::internal("Failed to register admin session"))
        }
    }
}

/// Admin endpoint to remove admin session
pub async fn remove_admin_session(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(device_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    // TODO: Add proper admin role verification and ownership check

    state.remove_admin_session(device_id).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct RegisterAdminSessionRequest {
    pub device_id: Uuid,
    pub session_token: String,
}
