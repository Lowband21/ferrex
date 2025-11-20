//! Device-aware authentication handlers built on the new auth domain services.

use axum::{Extension, Json, extract::State, http::HeaderMap};
use chrono::Utc;
use ferrex_core::{
    api_types::ApiResponse,
    auth::{
        AuthError, AuthResult,
        device::{AuthDeviceStatus, AuthenticatedDevice, DeviceInfo, DeviceRegistration, Platform},
        domain::{
            aggregates::{DeviceSession, DeviceStatus},
            services::{
                AuthEventContext, AuthenticationError, DeviceTrustError, PinManagementError,
                TokenBundle,
            },
        },
        generate_trust_token,
    },
    user::User,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    application::auth::AuthFacadeError,
    infra::{
        app_state::AppState,
        errors::{AppError, AppResult},
    },
};
use ferrex_core::auth::domain::value_objects::{DeviceFingerprint, PinPolicy};

const MAX_PIN_ATTEMPTS: u8 = 3;

#[derive(Debug, Deserialize)]
pub struct DeviceLoginRequest {
    pub username: String,
    pub password: String,
    pub device_info: Option<DeviceInfo>,
    pub remember_device: bool,
}

#[derive(Debug, Deserialize)]
pub struct PinLoginRequest {
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub pin: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceAuthStatus {
    pub device_registered: bool,
    pub has_pin: bool,
    pub remaining_attempts: Option<u8>,
}

#[derive(Debug, Deserialize)]
pub struct SetPinRequest {
    pub device_id: Uuid,
    pub pin: String,
}

#[derive(Debug, Deserialize)]
pub struct RevokeDeviceRequest {
    pub device_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct ChangePinRequest {
    pub device_id: Uuid,
    pub current_pin: String,
    pub new_pin: String,
}

pub async fn device_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DeviceLoginRequest>,
) -> AppResult<Json<ApiResponse<AuthResult>>> {
    let device_info = extract_device_info(&headers, request.device_info);
    let fingerprint = generate_device_fingerprint(&device_info, &headers)
        .map_err(|e| AppError::bad_request(e))?;

    let mut context = build_event_context(&headers);
    context.insert_metadata("device_name", json!(device_info.device_name.clone()));
    context.insert_metadata("remember_device", json!(request.remember_device));

    let facade = state.auth_facade.clone();

    let (bundle, session) = facade
        .device_password_login(
            &request.username,
            &request.password,
            fingerprint,
            device_info.device_name.clone(),
            context,
        )
        .await
        .map_err(map_facade_error)?;

    // Update user preferences when remember_device is requested
    if request.remember_device {
        match state
            .unit_of_work
            .users
            .get_user_by_id(bundle.user_id)
            .await
        {
            Ok(Some(mut user)) => {
                if !user.preferences.auto_login_enabled {
                    user.preferences.auto_login_enabled = true;
                    user.updated_at = Utc::now();
                    if let Err(err) = state.unit_of_work.users.update_user(&user).await {
                        warn!("failed to persist auto-login preference: {err}");
                    }
                }
            }
            Ok(None) => warn!(
                "user {} authenticated but record missing during device login",
                bundle.user_id
            ),
            Err(err) => warn!(
                "failed to load user {} during device login: {err}",
                bundle.user_id
            ),
        }
    }

    info!(
        user_id = %bundle.user_id,
        device_session = %session.id(),
        "device login successful"
    );

    let registration = device_session_to_device_registration(&bundle, &session, &device_info);

    let result = AuthResult {
        user_id: bundle.user_id,
        session_token: bundle.session_token.as_str().to_string(),
        device_registration: Some(registration),
        requires_pin_setup: !session.has_pin(),
    };

    Ok(Json(ApiResponse::success(result)))
}

pub async fn pin_login(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<PinLoginRequest>,
) -> AppResult<Json<ApiResponse<AuthResult>>> {
    let bundle = state
        .auth_service()
        .authenticate_with_pin_session(request.device_id, &request.pin)
        .await
        .map_err(map_authentication_error)?;

    if bundle.user_id != request.user_id {
        return Err(AppError::forbidden(
            "Device session does not belong to requested user".to_string(),
        ));
    }

    let result = AuthResult {
        user_id: bundle.user_id,
        session_token: bundle.session_token.as_str().to_string(),
        device_registration: None,
        requires_pin_setup: false,
    };

    Ok(Json(ApiResponse::success(result)))
}

pub async fn set_device_pin(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<SetPinRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    let facade = state.auth_facade.clone();
    let session = facade
        .get_device_by_id(request.device_id)
        .await
        .map_err(map_facade_error)?;

    if session.user_id() != user.id {
        return Err(AppError::forbidden("Device not owned by user".to_string()));
    }

    let policy = PinPolicy::default();
    facade
        .set_device_pin(
            user.id,
            session.device_fingerprint(),
            request.pin,
            &policy,
            None,
        )
        .await
        .map_err(map_facade_error)?;

    Ok(Json(ApiResponse::success(())))
}

pub async fn check_device_status(
    State(state): State<AppState>,
    Extension(_user): Extension<User>,
    axum::extract::Query(query): axum::extract::Query<DeviceStatusQuery>,
) -> AppResult<Json<ApiResponse<DeviceAuthStatus>>> {
    let facade = state.auth_facade.clone();
    let status = match facade.get_device_by_id(query.device_id).await {
        Ok(session) if session.user_id() == query.user_id => {
            if matches!(session.status(), DeviceStatus::Revoked) {
                DeviceAuthStatus {
                    device_registered: false,
                    has_pin: false,
                    remaining_attempts: Some(0),
                }
            } else {
                let remaining = MAX_PIN_ATTEMPTS.saturating_sub(session.failed_attempts());
                DeviceAuthStatus {
                    device_registered: true,
                    has_pin: session.has_pin(),
                    remaining_attempts: Some(remaining),
                }
            }
        }
        Ok(_) => DeviceAuthStatus {
            device_registered: false,
            has_pin: false,
            remaining_attempts: Some(MAX_PIN_ATTEMPTS),
        },
        Err(AuthFacadeError::DeviceTrust(_)) | Err(AuthFacadeError::UserNotFound) => {
            DeviceAuthStatus {
                device_registered: false,
                has_pin: false,
                remaining_attempts: Some(MAX_PIN_ATTEMPTS),
            }
        }
        Err(err) => return Err(map_facade_error(err)),
    };

    Ok(Json(ApiResponse::success(status)))
}

pub async fn list_user_devices(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> AppResult<Json<ApiResponse<Vec<AuthenticatedDevice>>>> {
    let facade = state.auth_facade.clone();
    let sessions = facade
        .list_user_devices(user.id)
        .await
        .map_err(map_facade_error)?;

    let devices = sessions
        .iter()
        .map(device_session_to_authenticated_device)
        .collect();

    Ok(Json(ApiResponse::success(devices)))
}

pub async fn revoke_device(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<RevokeDeviceRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    let facade = state.auth_facade.clone();
    let session = facade
        .get_device_by_id(request.device_id)
        .await
        .map_err(map_facade_error)?;

    if session.user_id() != user.id {
        return Err(AppError::forbidden("Device not owned by user".to_string()));
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

pub async fn change_device_pin(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<ChangePinRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    let facade = state.auth_facade.clone();
    let session = facade
        .get_device_by_id(request.device_id)
        .await
        .map_err(map_facade_error)?;

    if session.user_id() != user.id {
        return Err(AppError::forbidden("Device not owned by user".to_string()));
    }

    let policy = PinPolicy::default();
    facade
        .rotate_device_pin(
            user.id,
            session.device_fingerprint(),
            &request.current_pin,
            request.new_pin,
            &policy,
            MAX_PIN_ATTEMPTS,
            None,
        )
        .await
        .map_err(map_facade_error)?;

    Ok(Json(ApiResponse::success(())))
}

#[derive(Debug, Deserialize)]
pub struct DeviceStatusQuery {
    pub user_id: Uuid,
    pub device_id: Uuid,
}

fn extract_device_info(headers: &HeaderMap, body_device_info: Option<DeviceInfo>) -> DeviceInfo {
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("Unknown");

    body_device_info.unwrap_or_else(|| DeviceInfo {
        device_id: Uuid::now_v7(),
        device_name: format!("{} Device", Platform::from_user_agent(user_agent).as_ref()),
        platform: Platform::from_user_agent(user_agent),
        app_version: "1.0.0".to_string(),
        hardware_id: None,
    })
}

fn generate_device_fingerprint(
    device_info: &DeviceInfo,
    headers: &HeaderMap,
) -> Result<DeviceFingerprint, String> {
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("Unknown");

    let mut hasher = Sha256::new();
    hasher.update(user_agent.as_bytes());
    hasher.update(device_info.platform.as_ref().as_bytes());
    if let Some(hw_id) = &device_info.hardware_id {
        hasher.update(hw_id.as_bytes());
    }
    hasher.update(device_info.device_id.as_bytes());

    let hash = format!("{:x}", hasher.finalize());
    DeviceFingerprint::from_hash(hash).map_err(|_| "Invalid device fingerprint".to_string())
}

fn build_event_context(headers: &HeaderMap) -> AuthEventContext {
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let ip_address = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let mut context = AuthEventContext::default();
    context.ip_address = ip_address;
    context.user_agent = user_agent;
    context
}

fn device_session_to_device_registration(
    bundle: &TokenBundle,
    session: &DeviceSession,
    info: &DeviceInfo,
) -> DeviceRegistration {
    DeviceRegistration {
        id: session.id(),
        user_id: bundle.user_id,
        device_id: info.device_id,
        device_name: info.device_name.clone(),
        platform: info.platform.clone(),
        app_version: info.app_version.clone(),
        trust_token: generate_trust_token(),
        pin_hash: session.pin_hash().map(|s| s.to_string()),
        registered_at: session.created_at(),
        last_used_at: session.last_activity(),
        expires_at: None,
        revoked: matches!(session.status(), DeviceStatus::Revoked),
        revoked_by: None,
        revoked_at: None,
    }
}

fn device_session_to_authenticated_device(session: &DeviceSession) -> AuthenticatedDevice {
    AuthenticatedDevice {
        id: session.id(),
        user_id: session.user_id(),
        fingerprint: session.device_fingerprint().as_str().to_string(),
        name: session.device_name().to_string(),
        platform: Platform::Unknown,
        app_version: None,
        hardware_id: None,
        status: map_device_status(session.status()),
        pin_hash: session.pin_hash().map(|s| s.to_string()),
        pin_set_at: None,
        pin_last_used_at: None,
        failed_attempts: i32::from(session.failed_attempts()),
        locked_until: None,
        first_authenticated_by: session.user_id(),
        first_authenticated_at: session.created_at(),
        trusted_until: None,
        last_seen_at: session.last_activity(),
        last_activity: session.last_activity(),
        auto_login_enabled: session.has_pin(),
        revoked_by: None,
        revoked_at: None,
        revoked_reason: None,
        created_at: session.created_at(),
        updated_at: session.last_activity(),
        metadata: json!({}),
    }
}

fn map_device_status(status: DeviceStatus) -> AuthDeviceStatus {
    match status {
        DeviceStatus::Pending => AuthDeviceStatus::Pending,
        DeviceStatus::Trusted => AuthDeviceStatus::Trusted,
        DeviceStatus::Revoked => AuthDeviceStatus::Revoked,
    }
}

fn map_authentication_error(err: AuthenticationError) -> AppError {
    match err {
        AuthenticationError::InvalidCredentials | AuthenticationError::InvalidPin => {
            AppError::unauthorized(AuthError::InvalidCredentials.to_string())
        }
        AuthenticationError::TooManyFailedAttempts => {
            AppError::rate_limited("Too many failed authentication attempts".to_string())
        }
        AuthenticationError::SessionExpired => {
            AppError::unauthorized(AuthError::SessionExpired.to_string())
        }
        AuthenticationError::DeviceNotFound | AuthenticationError::DeviceNotTrusted => {
            AppError::forbidden("Device not eligible for authentication".to_string())
        }
        AuthenticationError::UserNotFound => AppError::not_found("User not found".to_string()),
        AuthenticationError::DatabaseError(e) => {
            AppError::internal(format!("Authentication failed: {e}"))
        }
    }
}

fn map_device_trust_error(err: DeviceTrustError) -> AppError {
    use DeviceTrustError as E;
    match err {
        E::UserNotFound => AppError::not_found("User not found".to_string()),
        E::UserInactive | E::UserLocked => {
            AppError::forbidden("User is not allowed to authenticate".to_string())
        }
        E::DeviceNotFound => AppError::not_found("Device not found".to_string()),
        E::DeviceAlreadyTrusted => AppError::conflict("Device already trusted".to_string()),
        E::DeviceRevoked => AppError::forbidden("Device has been revoked".to_string()),
        E::TooManyDevices { .. } => AppError::conflict("Too many devices registered".to_string()),
        E::DeviceNotTrusted => AppError::forbidden("Device is not trusted".to_string()),
        E::DatabaseError(e) => AppError::internal(format!("Device trust error: {e}")),
    }
}

fn map_pin_error(err: PinManagementError) -> AppError {
    use PinManagementError as E;
    match err {
        E::UserNotFound => AppError::not_found("User not found".to_string()),
        E::UserInactive | E::UserLocked => {
            AppError::forbidden("User is not allowed to update PIN".to_string())
        }
        E::DeviceNotFound => AppError::not_found("Device not found".to_string()),
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
        AuthFacadeError::Authentication(err) => map_authentication_error(err),
        AuthFacadeError::DeviceTrust(err) => map_device_trust_error(err),
        AuthFacadeError::PinManagement(err) => map_pin_error(err),
        AuthFacadeError::UserNotFound => AppError::not_found("User not found".to_string()),
        AuthFacadeError::Storage(err) => AppError::internal(format!("Storage error: {err}")),
    }
}
