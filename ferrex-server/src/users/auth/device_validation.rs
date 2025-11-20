//! Device trust validation endpoints backed by the auth domain services.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use chrono::{DateTime, Utc};
use ferrex_core::{
    api_types::ApiResponse,
    auth::{
        AuthError,
        domain::{
            aggregates::{DeviceSession, DeviceStatus},
            services::{
                AuthenticationError, DeviceTrustError, PinManagementError,
            },
        },
    },
    user::User,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    application::auth::AuthFacadeError,
    infra::{
        app_state::AppState,
        errors::{AppError, AppResult},
    },
};

#[derive(Debug, Deserialize)]
pub struct DeviceTrustQuery {
    pub device_id: Option<Uuid>,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeviceTrustStatus {
    pub is_trusted: bool,
    pub trusted_until: Option<DateTime<Utc>>,
    pub device_name: Option<String>,
    pub registered_at: Option<DateTime<Utc>>,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TrustedDevice {
    pub device_id: Uuid,
    pub device_name: String,
    pub platform: String,
    pub trusted_until: Option<DateTime<Utc>>,
    pub last_seen: DateTime<Utc>,
    pub is_current: bool,
}

#[derive(Debug, Deserialize)]
pub struct RevokeDeviceRequest {
    pub device_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct ExtendTrustRequest {
    pub device_id: Option<Uuid>,
    pub days: Option<i64>,
}

pub async fn validate_device_trust(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(device_id_ext): Extension<Option<Uuid>>,
    Query(params): Query<DeviceTrustQuery>,
) -> AppResult<Json<ApiResponse<DeviceTrustStatus>>> {
    let device_id = params.device_id.or(device_id_ext).ok_or_else(|| {
        AppError::bad_request("Device ID required".to_string())
    })?;

    let facade = state.auth_facade().clone();
    let status = match facade.get_device_by_id(device_id).await {
        Ok(session) if session.user_id() == user.id => {
            validate_session(session, params.fingerprint.as_deref())?
        }
        Ok(_) => DeviceTrustStatus {
            is_trusted: false,
            trusted_until: None,
            device_name: None,
            registered_at: None,
            reason: Some("Device not registered for this user".to_string()),
        },
        Err(AuthFacadeError::DeviceTrust(DeviceTrustError::DeviceNotFound))
        | Err(AuthFacadeError::UserNotFound) => DeviceTrustStatus {
            is_trusted: false,
            trusted_until: None,
            device_name: None,
            registered_at: None,
            reason: Some("Device not registered".to_string()),
        },
        Err(err) => return Err(map_facade_error(err)),
    };

    info!(user_id = %user.id, device_id = %device_id, trusted = status.is_trusted);
    Ok(Json(ApiResponse::success(status)))
}

pub async fn revoke_device_trust(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<RevokeDeviceRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    let facade = state.auth_facade().clone();
    let session = facade
        .get_device_by_id(request.device_id)
        .await
        .map_err(map_facade_error)?;

    if session.user_id() != user.id {
        return Err(AppError::forbidden(
            "Device not registered for this user".to_string(),
        ));
    }

    facade
        .revoke_device(
            user.id,
            session.device_fingerprint(),
            Some("user_initiated".to_string()),
            None,
        )
        .await
        .map_err(map_facade_error)?;

    Ok(Json(ApiResponse::success(())))
}

pub async fn list_trusted_devices(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(current_device): Extension<Option<Uuid>>,
) -> AppResult<Json<ApiResponse<Vec<TrustedDevice>>>> {
    let facade = state.auth_facade().clone();
    let devices = facade
        .list_user_devices(user.id)
        .await
        .map_err(map_facade_error)?
        .into_iter()
        .filter(|session| matches!(session.status(), DeviceStatus::Trusted))
        .map(|session| TrustedDevice {
            device_id: session.id(),
            device_name: session.device_name().to_string(),
            platform: "unknown".to_string(),
            trusted_until: None,
            last_seen: session.last_activity(),
            is_current: current_device.is_some_and(|id| id == session.id()),
        })
        .collect();

    Ok(Json(ApiResponse::success(devices)))
}

pub async fn extend_device_trust(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(device_id_ext): Extension<Option<Uuid>>,
    Json(request): Json<ExtendTrustRequest>,
) -> AppResult<Json<ApiResponse<DeviceTrustStatus>>> {
    let device_id = request.device_id.or(device_id_ext).ok_or_else(|| {
        AppError::bad_request("Device ID required".to_string())
    })?;

    let facade = state.auth_facade().clone();
    let session = facade
        .get_device_by_id(device_id)
        .await
        .map_err(map_facade_error)?;

    if session.user_id() != user.id {
        return Err(AppError::forbidden(
            "Device not registered for this user".to_string(),
        ));
    }

    let status = DeviceTrustStatus {
        is_trusted: matches!(session.status(), DeviceStatus::Trusted),
        trusted_until: None,
        device_name: Some(session.device_name().to_string()),
        registered_at: Some(session.created_at()),
        reason: Some("Device trust does not expire".to_string()),
    };

    Ok(Json(ApiResponse::success(status)))
}

fn validate_session(
    session: DeviceSession,
    expected_fingerprint: Option<&str>,
) -> Result<DeviceTrustStatus, AppError> {
    if let Some(expected) = expected_fingerprint {
        let stored = session.device_fingerprint().as_str();
        if stored != expected {
            warn!(
                "Device fingerprint mismatch: expected {}, got {}",
                expected, stored
            );
            return Ok(DeviceTrustStatus {
                is_trusted: false,
                trusted_until: None,
                device_name: Some(session.device_name().to_string()),
                registered_at: Some(session.created_at()),
                reason: Some("Device fingerprint mismatch".to_string()),
            });
        }
    }

    let status = match session.status() {
        DeviceStatus::Trusted => DeviceTrustStatus {
            is_trusted: true,
            trusted_until: None,
            device_name: Some(session.device_name().to_string()),
            registered_at: Some(session.created_at()),
            reason: None,
        },
        DeviceStatus::Pending => DeviceTrustStatus {
            is_trusted: false,
            trusted_until: None,
            device_name: Some(session.device_name().to_string()),
            registered_at: Some(session.created_at()),
            reason: Some("Device is pending trust".to_string()),
        },
        DeviceStatus::Revoked => DeviceTrustStatus {
            is_trusted: false,
            trusted_until: None,
            device_name: Some(session.device_name().to_string()),
            registered_at: Some(session.created_at()),
            reason: Some("Device has been revoked".to_string()),
        },
    };

    Ok(status)
}

fn map_facade_error(err: AuthFacadeError) -> AppError {
    match err {
        AuthFacadeError::Authentication(err) => map_authentication_error(err),
        AuthFacadeError::DeviceTrust(err) => map_device_trust_error(err),
        AuthFacadeError::PinManagement(err) => map_pin_error(err),
        AuthFacadeError::UserNotFound => {
            AppError::not_found("User not found".to_string())
        }
        AuthFacadeError::Storage(err) => {
            AppError::internal(format!("Storage error: {err}"))
        }
    }
}

fn map_authentication_error(err: AuthenticationError) -> AppError {
    match err {
        AuthenticationError::InvalidCredentials
        | AuthenticationError::InvalidPin => {
            AppError::unauthorized(AuthError::InvalidCredentials.to_string())
        }
        AuthenticationError::TooManyFailedAttempts => AppError::rate_limited(
            "Too many failed authentication attempts".to_string(),
        ),
        AuthenticationError::SessionExpired => {
            AppError::unauthorized(AuthError::SessionExpired.to_string())
        }
        AuthenticationError::DeviceNotFound
        | AuthenticationError::DeviceNotTrusted => AppError::forbidden(
            "Device not eligible for authentication".to_string(),
        ),
        AuthenticationError::UserNotFound => {
            AppError::not_found("User not found".to_string())
        }
        AuthenticationError::DatabaseError(e) => {
            AppError::internal(format!("Authentication failed: {e}"))
        }
    }
}

fn map_device_trust_error(err: DeviceTrustError) -> AppError {
    use DeviceTrustError as E;
    match err {
        E::UserNotFound => AppError::not_found("User not found".to_string()),
        E::UserInactive | E::UserLocked => AppError::forbidden(
            "User is not allowed to authenticate".to_string(),
        ),
        E::DeviceNotFound => {
            AppError::not_found("Device not found".to_string())
        }
        E::DeviceAlreadyTrusted => {
            AppError::conflict("Device already trusted".to_string())
        }
        E::DeviceRevoked => {
            AppError::forbidden("Device has been revoked".to_string())
        }
        E::TooManyDevices { .. } => {
            AppError::conflict("Too many devices registered".to_string())
        }
        E::DeviceNotTrusted => {
            AppError::forbidden("Device is not trusted".to_string())
        }
        E::DatabaseError(e) => {
            AppError::internal(format!("Device trust error: {e}"))
        }
    }
}

fn map_pin_error(err: PinManagementError) -> AppError {
    use PinManagementError as E;
    match err {
        E::UserNotFound => AppError::not_found("User not found".to_string()),
        E::UserInactive | E::UserLocked => {
            AppError::forbidden("User is not allowed to update PIN".to_string())
        }
        E::DeviceNotFound => {
            AppError::not_found("Device not found".to_string())
        }
        E::DeviceRevoked => {
            AppError::forbidden("Device has been revoked".to_string())
        }
        E::PinNotSet => {
            AppError::not_found("PIN is not configured".to_string())
        }
        E::InvalidPinFormat => {
            AppError::bad_request("Invalid PIN format".to_string())
        }
        E::PinVerificationFailed => {
            AppError::unauthorized("PIN verification failed".to_string())
        }
        E::TooManyFailedAttempts => {
            AppError::rate_limited("Too many failed PIN attempts".to_string())
        }
        E::DatabaseError(e) => {
            AppError::internal(format!("PIN management error: {e}"))
        }
    }
}
