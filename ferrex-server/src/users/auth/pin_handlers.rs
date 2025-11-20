//! PIN authentication handlers with admin session verification

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use ferrex_core::{
    api_types::ApiResponse,
    auth::{
        domain::services::{AuthenticationError, TokenBundle},
        pin::{PinPolicy, SetPinRequest, SetPinResponse},
    },
    user::{AuthToken, User},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};

/// Request to authenticate using PIN
#[derive(Debug, Deserialize)]
pub struct PinAuthRequest {
    pub device_id: Uuid,
    pub pin: String,
}

/// Authenticate using PIN - requires admin session on device
pub async fn authenticate_with_pin(
    State(state): State<AppState>,
    Json(request): Json<PinAuthRequest>,
) -> AppResult<Json<ApiResponse<AuthToken>>> {
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

    tracing::info!(
        "PIN authentication attempt for device {} by admin {}",
        request.device_id,
        admin_session.user_id
    );

    let bundle = state
        .auth_service
        .authenticate_with_pin_session(request.device_id, &request.pin)
        .await
        .map_err(map_auth_error)?;

    Ok(Json(ApiResponse::success(bundle_to_auth_token(bundle))))
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

fn map_auth_error(err: AuthenticationError) -> AppError {
    match err {
        AuthenticationError::InvalidCredentials | AuthenticationError::InvalidPin => {
            AppError::unauthorized("Invalid PIN".to_string())
        }
        AuthenticationError::TooManyFailedAttempts => AppError::rate_limited(
            "Too many failed PIN attempts; device locked temporarily".to_string(),
        ),
        AuthenticationError::DeviceNotFound => {
            AppError::not_found("Device session not found".to_string())
        }
        AuthenticationError::DeviceNotTrusted => {
            AppError::forbidden("Device is not trusted".to_string())
        }
        AuthenticationError::SessionExpired => {
            AppError::unauthorized("Session expired".to_string())
        }
        AuthenticationError::DatabaseError(e) => {
            AppError::internal(format!("Database error during PIN authentication: {e}"))
        }
        AuthenticationError::UserNotFound => AppError::not_found("User not found".to_string()),
    }
}

fn bundle_to_auth_token(bundle: TokenBundle) -> AuthToken {
    let expires_in = bundle
        .session_token
        .expires_at()
        .signed_duration_since(Utc::now())
        .num_seconds()
        .max(0) as u32;

    AuthToken {
        access_token: bundle.session_token.as_str().to_string(),
        refresh_token: bundle.refresh_token.as_str().to_string(),
        expires_in,
        session_id: Some(bundle.session_record_id),
        device_session_id: bundle.device_session_id,
        user_id: Some(bundle.user_id),
    }
}

#[derive(Debug, Deserialize)]
pub struct RegisterAdminSessionRequest {
    pub device_id: Uuid,
    pub session_token: String,
}
