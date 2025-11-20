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
        domain::{
            services::{
                AuthEventContext, AuthenticationError, DeviceTrustError, PinManagementError,
                TokenBundle,
            },
            value_objects::PinPolicy,
        },
        pin::{SetPinRequest, SetPinResponse},
    },
    user::{AuthToken, User},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    application::auth::AuthFacadeError,
    infra::{
        app_state::{AdminSessionInfo, AppState},
        errors::{AppError, AppResult},
    },
};

const PIN_ROTATION_MAX_ATTEMPTS: u8 = 5;

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
        .auth_service()
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
    if !state
        .is_admin_authenticated_on_device(request.device_id)
        .await
    {
        return Err(AppError::forbidden(
            "PIN setup requires an admin to be logged in on this device",
        ));
    }

    let admin_session = state
        .get_admin_session(request.device_id)
        .await
        .ok_or_else(|| AppError::forbidden("Admin session not found or expired"))?;

    let auth_facade = state.auth_facade.clone();
    let device_session = auth_facade
        .get_device_by_id(request.device_id)
        .await
        .map_err(map_facade_error)?;

    let user_id = device_session.user_id();
    let fingerprint = device_session.device_fingerprint().clone();
    let policy = PinPolicy::default();
    let pin_service = state.pin_management_service();
    let mut verify_password =
        !device_session.has_pin() && !request.current_password_or_pin.is_empty();

    tracing::info!(
        user_id = %user_id,
        device_session = %device_session.id(),
        admin_id = %admin_session.user_id,
        "processing admin-initiated PIN update"
    );

    if device_session.has_pin() {
        verify_password = false;
        if request.current_password_or_pin.is_empty() {
            return Ok(Json(ApiResponse::success(SetPinResponse {
                success: false,
                message: "Current PIN must be provided to update an existing PIN".to_string(),
            })));
        }

        let context = admin_event_context(&admin_session);
        match pin_service
            .rotate_pin(
                user_id,
                &fingerprint,
                &request.current_password_or_pin,
                request.new_pin.clone(),
                &policy,
                PIN_ROTATION_MAX_ATTEMPTS,
                Some(context),
            )
            .await
        {
            Ok(_) => {
                return Ok(Json(ApiResponse::success(SetPinResponse {
                    success: true,
                    message: "PIN updated successfully".to_string(),
                })));
            }
            Err(PinManagementError::PinVerificationFailed) => {
                return Ok(Json(ApiResponse::success(SetPinResponse {
                    success: false,
                    message: "Current PIN was rejected".to_string(),
                })));
            }
            Err(PinManagementError::InvalidPinFormat) => {
                return Ok(Json(ApiResponse::success(SetPinResponse {
                    success: false,
                    message: "New PIN does not meet policy requirements".to_string(),
                })));
            }
            Err(PinManagementError::TooManyFailedAttempts) => {
                return Ok(Json(ApiResponse::success(SetPinResponse {
                    success: false,
                    message: "Too many failed attempts – device temporarily locked".to_string(),
                })));
            }
            Err(PinManagementError::PinNotSet) => {
                // Rare race condition: fall back to initial set logic below.
            }
            Err(err) => return Err(map_pin_error(err)),
        }
    }

    if verify_password {
        let user = auth_facade
            .get_user_by_id(user_id)
            .await
            .map_err(map_facade_error)?;

        match state
            .auth_service()
            .authenticate_user(&user.username, &request.current_password_or_pin)
            .await
        {
            Ok(_) => {}
            Err(AuthenticationError::InvalidCredentials)
            | Err(AuthenticationError::UserNotFound) => {
                return Ok(Json(ApiResponse::success(SetPinResponse {
                    success: false,
                    message: "Current password was rejected".to_string(),
                })));
            }
            Err(AuthenticationError::TooManyFailedAttempts) => {
                return Ok(Json(ApiResponse::success(SetPinResponse {
                    success: false,
                    message: "Too many failed authentication attempts".to_string(),
                })));
            }
            Err(err) => return Err(map_auth_error(err)),
        }
    }

    let context = admin_event_context(&admin_session);
    match pin_service
        .set_pin(
            user_id,
            &fingerprint,
            request.new_pin.clone(),
            &policy,
            Some(context),
        )
        .await
    {
        Ok(_) => Ok(Json(ApiResponse::success(SetPinResponse {
            success: true,
            message: "PIN configured successfully".to_string(),
        }))),
        Err(PinManagementError::InvalidPinFormat) => {
            Ok(Json(ApiResponse::success(SetPinResponse {
                success: false,
                message: "New PIN does not meet policy requirements".to_string(),
            })))
        }
        Err(err) => Err(map_pin_error(err)),
    }
}

/// Remove PIN for a device - requires admin session
pub async fn remove_pin(
    State(state): State<AppState>,
    Path(device_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    if !state.is_admin_authenticated_on_device(device_id).await {
        return Err(AppError::forbidden(
            "PIN removal requires an admin to be logged in on this device",
        ));
    }

    let admin_session = state
        .get_admin_session(device_id)
        .await
        .ok_or_else(|| AppError::forbidden("Admin session not found or expired"))?;

    let auth_facade = state.auth_facade.clone();
    let device_session = auth_facade
        .get_device_by_id(device_id)
        .await
        .map_err(map_facade_error)?;

    let pin_service = state.pin_management_service();
    pin_service
        .force_clear_pin(
            device_session.user_id(),
            device_session.device_fingerprint(),
            Some(admin_event_context(&admin_session)),
        )
        .await
        .map_err(map_pin_error)?;

    tracing::info!(
        user_id = %device_session.user_id(),
        device_session = %device_session.id(),
        admin_id = %admin_session.user_id,
        "admin removed device PIN"
    );

    Ok(StatusCode::NO_CONTENT)
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
        scope: bundle.scope,
    }
}

#[derive(Debug, Deserialize)]
pub struct RegisterAdminSessionRequest {
    pub device_id: Uuid,
    pub session_token: String,
}

fn admin_event_context(admin: &AdminSessionInfo) -> AuthEventContext {
    let mut context = AuthEventContext::default();
    context.insert_metadata(
        "approved_by",
        json!({
            "admin_user_id": admin.user_id,
            "admin_device_id": admin.device_id,
            "admin_session_expires_at": admin.expires_at,
        }),
    );
    context
}

fn map_pin_error(err: PinManagementError) -> AppError {
    use PinManagementError as E;
    match err {
        E::UserNotFound => AppError::not_found("User not found".to_string()),
        E::UserInactive | E::UserLocked => {
            AppError::forbidden("User is not allowed to manage PINs".to_string())
        }
        E::DeviceNotFound => AppError::not_found("Device session not found".to_string()),
        E::DeviceRevoked => AppError::forbidden("Device has been revoked".to_string()),
        E::PinNotSet => AppError::not_found("PIN is not configured".to_string()),
        E::InvalidPinFormat => AppError::bad_request("Invalid PIN format".to_string()),
        E::PinVerificationFailed => AppError::unauthorized("PIN verification failed".to_string()),
        E::TooManyFailedAttempts => {
            AppError::rate_limited("Too many failed PIN attempts".to_string())
        }
        E::DatabaseError(e) => AppError::internal(format!("PIN management error: {e}")),
    }
}

fn map_facade_error(err: AuthFacadeError) -> AppError {
    match err {
        AuthFacadeError::Authentication(err) => map_auth_error(err),
        AuthFacadeError::DeviceTrust(err) => match err {
            DeviceTrustError::UserNotFound => AppError::not_found("User not found".to_string()),
            DeviceTrustError::UserInactive | DeviceTrustError::UserLocked => {
                AppError::forbidden("User is not allowed to authenticate".to_string())
            }
            DeviceTrustError::DeviceNotFound => {
                AppError::not_found("Device session not found".to_string())
            }
            DeviceTrustError::DeviceAlreadyTrusted => {
                AppError::conflict("Device already trusted".to_string())
            }
            DeviceTrustError::DeviceRevoked => {
                AppError::forbidden("Device has been revoked".to_string())
            }
            DeviceTrustError::TooManyDevices { .. } => {
                AppError::conflict("Too many devices registered".to_string())
            }
            DeviceTrustError::DeviceNotTrusted => {
                AppError::forbidden("Device is not trusted".to_string())
            }
            DeviceTrustError::DatabaseError(e) => {
                AppError::internal(format!("Device trust error: {e}"))
            }
        },
        AuthFacadeError::PinManagement(err) => map_pin_error(err),
        AuthFacadeError::UserNotFound => AppError::not_found("User not found".to_string()),
        AuthFacadeError::Storage(err) => AppError::internal(format!("Storage error: {err}")),
    }
}
