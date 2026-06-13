//! Device-aware authentication handlers built on the new auth domain services.

use axum::{Extension, Json, extract::State, http::HeaderMap};
use base64::Engine as _;
use chrono::{Duration, Utc};
use ferrex_core::{
    api::types::ApiResponse,
    domain::users::{
        auth::{
            AuthError,
            device::{
                AuthDeviceStatus, AuthenticatedDevice, DeviceInfo,
                DeviceRegistration, Platform,
            },
            domain::{
                aggregates::{
                    DeviceSession, DeviceSessionClientMetadata, DeviceStatus,
                },
                services::{
                    AuthEventContext, AuthenticationError, DeviceTrustError,
                    PinManagementError, TokenBundle,
                },
                value_objects::{DeviceFingerprint, PinPolicy},
            },
        },
        user::User,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use tracing::info;
use uuid::Uuid;

use crate::handlers::users::map_auth_facade_error;
use crate::{
    application::auth::AuthFacadeError,
    infra::{
        app_state::AppState,
        errors::{AppError, AppResult},
    },
};
use ferrex_core::domain::users::auth::domain::services::AuthenticationError as CoreAuthError;

const MAX_PIN_ATTEMPTS: u8 = 3;

#[derive(Debug, Deserialize)]
pub struct DeviceLoginRequest {
    pub username: String,
    pub password: String,
    pub device_info: Option<DeviceInfo>,
    pub remember_device: bool,
    /// Optional device public key for possession validation (base64-encoded)
    #[serde(default)]
    pub device_public_key: Option<String>,
    /// Optional algorithm for device public key (default: ed25519)
    #[serde(default)]
    pub device_key_alg: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PinLoginRequest {
    pub device_id: Uuid,
    /// Client-derived PIN proof (raw PIN must never be sent)
    pub client_proof: String,
    /// Challenge id obtained from PIN challenge endpoint
    pub challenge_id: Uuid,
    /// Base64-encoded device signature over ("Ferrex-PIN-v1" || challenge_id || nonce || user_uuid)
    pub device_signature: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceAuthToken {
    pub access_token: String,
    /// Backwards-compatible alias for older device clients.
    pub session_token: String,
    pub refresh_token: String,
    pub expires_in: u32,
    pub session_id: Option<Uuid>,
    pub device_session_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub scope:
        ferrex_core::domain::users::auth::domain::value_objects::SessionScope,
    pub device_registration: Option<DeviceRegistration>,
    pub requires_pin_setup: bool,
}

#[derive(Debug, Serialize)]
pub struct KnownDeviceProfilesResponse {
    pub known_device: bool,
    pub users: Vec<KnownDeviceUserCard>,
}

#[derive(Debug, Serialize)]
pub struct KnownDeviceUserCard {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub has_pin: bool,
}

#[derive(Debug, Deserialize)]
pub struct KnownDeviceProfilesRequest {
    pub device_info: Option<DeviceInfo>,
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
    /// Client-derived PIN proof (raw PIN must never be sent)
    pub client_proof: String,
    /// Challenge id obtained from PIN challenge endpoint
    pub challenge_id: Uuid,
    /// Base64-encoded device signature over challenge
    pub device_signature: String,
}

#[derive(Debug, Deserialize)]
pub struct RevokeDeviceRequest {
    pub device_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct ChangePinRequest {
    pub device_id: Uuid,
    /// Current PIN client proof
    pub current_proof: String,
    /// New PIN client proof
    pub new_proof: String,
    /// Challenge id obtained from PIN challenge endpoint
    pub challenge_id: Uuid,
    /// Base64-encoded device signature over challenge
    pub device_signature: String,
}

pub async fn device_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DeviceLoginRequest>,
) -> AppResult<Json<ApiResponse<DeviceAuthToken>>> {
    let device_info = extract_device_info(&headers, request.device_info);
    let fingerprint = generate_device_fingerprint(&device_info, &headers)
        .map_err(AppError::bad_request)?;

    let public_key = validate_device_public_key(
        request.device_public_key.as_deref(),
        request.device_key_alg.as_deref(),
    )?;

    if request.remember_device && public_key.is_none() {
        return Err(AppError::bad_request(
            "remember_device requires a registered device_public_key"
                .to_string(),
        ));
    }

    let mut context = build_event_context(&headers);
    context
        .insert_metadata("device_name", json!(device_info.device_name.clone()));
    context.insert_metadata("remember_device", json!(request.remember_device));

    let (device_key_alg, device_public_key) = public_key
        .map(|(alg, key)| (Some(alg), Some(key)))
        .unwrap_or((None, None));

    let metadata = DeviceSessionClientMetadata {
        platform: Some(device_info.platform.as_ref().to_string()),
        app_version: Some(device_info.app_version.clone()),
        hardware_id: device_info.hardware_id.clone(),
        device_public_key,
        device_key_alg,
        trusted_until: request
            .remember_device
            .then(|| Utc::now() + Duration::days(30)),
        auto_login_enabled: Some(request.remember_device),
        metadata: Some(json!({
            "device_id": device_info.device_id,
            "remember_device": request.remember_device,
        })),
    };

    let facade = state.auth_facade().clone();

    let (bundle, session) = facade
        .device_password_login(
            &request.username,
            &request.password,
            fingerprint,
            device_info.device_name.clone(),
            metadata,
            context,
        )
        .await
        .map_err(map_facade_error)?;

    // remember_device is intentionally per-device. The legacy user preference
    // remains a client default and is not mutated by this device contract.
    info!(
        user_id = %bundle.user_id,
        device_session = %session.id(),
        "device login successful"
    );

    let result = bundle_to_device_auth_token(
        bundle,
        Some(device_session_to_device_registration(&session)),
        !session.has_pin(),
    );

    Ok(Json(ApiResponse::success(result)))
}

pub async fn pin_login(
    State(state): State<AppState>,
    Json(request): Json<PinLoginRequest>,
) -> AppResult<Json<ApiResponse<DeviceAuthToken>>> {
    // Global rate limiting middleware enforces PIN auth limits.
    // Decode device signature
    let sig = base64::engine::general_purpose::STANDARD
        .decode(request.device_signature.as_bytes())
        .map_err(|_| {
            AppError::bad_request(
                "invalid device_signature encoding".to_string(),
            )
        })?;
    let bundle = state
        .auth_service()
        .authenticate_with_pin_session(
            request.device_id,
            &request.client_proof,
            request.challenge_id,
            &sig,
        )
        .await
        .map_err(map_authentication_error)?;

    let session = state
        .auth_facade()
        .get_device_by_id(request.device_id)
        .await
        .map_err(map_facade_error)?;

    let result = bundle_to_device_auth_token(
        bundle,
        Some(device_session_to_device_registration(&session)),
        false,
    );

    Ok(Json(ApiResponse::success(result)))
}

#[derive(Debug, Deserialize)]
pub struct PinChallengeRequest {
    pub device_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct PinChallengeResponse {
    pub challenge_id: Uuid,
    pub nonce: String, // base64
    pub expires_in_secs: i64,
    pub pin_salt: String, // base64
}

/// Issue a device possession challenge for PIN login
pub async fn pin_challenge(
    State(state): State<AppState>,
    Json(request): Json<PinChallengeRequest>,
) -> AppResult<Json<ApiResponse<PinChallengeResponse>>> {
    let facade = state.auth_facade().clone();
    // Global rate limiting middleware enforces challenge issuance limits.
    let session = facade
        .get_device_by_id(request.device_id)
        .await
        .map_err(map_auth_facade_error)?;
    let user_id = session.user_id();
    let pin_salt = facade
        .get_pin_client_salt(user_id)
        .await
        .map_err(map_auth_facade_error)?;
    // 2 minute TTL
    let (id, nonce) = state
        .auth_service()
        .create_device_challenge(request.device_id, 120)
        .await
        .map_err(map_authentication_error)?;
    let nonce_b64 = base64::engine::general_purpose::STANDARD.encode(nonce);
    let pin_salt_b64 =
        base64::engine::general_purpose::STANDARD.encode(pin_salt);
    Ok(Json(ApiResponse::success(PinChallengeResponse {
        challenge_id: id,
        nonce: nonce_b64,
        expires_in_secs: 120,
        pin_salt: pin_salt_b64,
    })))
}

pub async fn known_device_profiles(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<KnownDeviceProfilesRequest>,
) -> AppResult<Json<ApiResponse<KnownDeviceProfilesResponse>>> {
    let device_info = extract_device_info(&headers, request.device_info);
    let fingerprint = generate_device_fingerprint(&device_info, &headers)
        .map_err(AppError::bad_request)?;

    let (known_device, users, pin_map) = state
        .auth_facade()
        .device_user_listing(&fingerprint)
        .await
        .map_err(map_facade_error)?;

    let cards = users
        .into_iter()
        .map(|user| KnownDeviceUserCard {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            has_pin: pin_map.get(&user.id).copied().unwrap_or(false),
        })
        .collect();

    Ok(Json(ApiResponse::success(KnownDeviceProfilesResponse {
        known_device,
        users: cards,
    })))
}

pub async fn set_device_pin(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<SetPinRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    let facade = state.auth_facade().clone();
    let session = facade
        .get_device_by_id(request.device_id)
        .await
        .map_err(map_facade_error)?;

    if session.user_id() != user.id {
        return Err(AppError::forbidden(
            "Device not owned by user".to_string(),
        ));
    }

    // Device must have a registered public key
    if session.device_public_key().is_none() {
        return Err(AppError::bad_request(
            "device key not registered; cannot set PIN".to_string(),
        ));
    }

    // Verify device possession via atomic challenge consumption
    let sig = base64::engine::general_purpose::STANDARD
        .decode(request.device_signature.as_bytes())
        .map_err(|_| {
            AppError::bad_request(
                "invalid device_signature encoding".to_string(),
            )
        })?;
    state
        .auth_service()
        .verify_device_possession(request.device_id, request.challenge_id, &sig)
        .await
        .map_err(map_core_auth_error)?;

    let policy = PinPolicy::default();
    facade
        .set_device_pin(
            user.id,
            session.device_fingerprint(),
            request.client_proof,
            &policy,
            None,
        )
        .await
        .map_err(map_facade_error)?;

    Ok(Json(ApiResponse::success(())))
}

pub async fn check_device_status(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    axum::extract::Query(query): axum::extract::Query<DeviceStatusQuery>,
) -> AppResult<Json<ApiResponse<DeviceAuthStatus>>> {
    let facade = state.auth_facade().clone();
    let status = match facade.get_device_by_id(query.device_id).await {
        Ok(session) if session.user_id() == user.id => {
            if matches!(session.status(), DeviceStatus::Revoked) {
                DeviceAuthStatus {
                    device_registered: false,
                    has_pin: false,
                    remaining_attempts: Some(0),
                }
            } else {
                let remaining =
                    MAX_PIN_ATTEMPTS.saturating_sub(session.failed_attempts());
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
        Err(AuthFacadeError::DeviceTrust(_))
        | Err(AuthFacadeError::UserNotFound) => DeviceAuthStatus {
            device_registered: false,
            has_pin: false,
            remaining_attempts: Some(MAX_PIN_ATTEMPTS),
        },
        Err(err) => return Err(map_facade_error(err)),
    };

    Ok(Json(ApiResponse::success(status)))
}

pub async fn list_user_devices(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> AppResult<Json<ApiResponse<Vec<AuthenticatedDevice>>>> {
    let facade = state.auth_facade().clone();
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
    let facade = state.auth_facade().clone();
    let session = facade
        .get_device_by_id(request.device_id)
        .await
        .map_err(map_facade_error)?;

    if session.user_id() != user.id {
        return Err(AppError::forbidden(
            "Device not owned by user".to_string(),
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

pub async fn change_device_pin(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<ChangePinRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    let facade = state.auth_facade().clone();
    let session = facade
        .get_device_by_id(request.device_id)
        .await
        .map_err(map_facade_error)?;

    if session.user_id() != user.id {
        return Err(AppError::forbidden(
            "Device not owned by user".to_string(),
        ));
    }

    // Device must have a registered public key
    if session.device_public_key().is_none() {
        return Err(AppError::bad_request(
            "device key not registered; cannot change PIN".to_string(),
        ));
    }

    // Verify device possession via atomic challenge consumption
    let sig = base64::engine::general_purpose::STANDARD
        .decode(request.device_signature.as_bytes())
        .map_err(|_| {
            AppError::bad_request(
                "invalid device_signature encoding".to_string(),
            )
        })?;
    state
        .auth_service()
        .verify_device_possession(request.device_id, request.challenge_id, &sig)
        .await
        .map_err(map_core_auth_error)?;

    let policy = PinPolicy::default();
    facade
        .rotate_device_pin(
            user.id,
            session.device_fingerprint(),
            &request.current_proof,
            request.new_proof,
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
    pub device_id: Uuid,
}

fn extract_device_info(
    headers: &HeaderMap,
    body_device_info: Option<DeviceInfo>,
) -> DeviceInfo {
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("Unknown");

    body_device_info.unwrap_or_else(|| DeviceInfo {
        device_id: Uuid::now_v7(),
        device_name: format!(
            "{} Device",
            Platform::from_user_agent(user_agent).as_ref()
        ),
        platform: Platform::from_user_agent(user_agent),
        app_version: "1.0.0".to_string(),
        hardware_id: None,
    })
}

fn map_core_auth_error(err: CoreAuthError) -> AppError {
    match err {
        CoreAuthError::InvalidCredentials | CoreAuthError::InvalidPin => {
            AppError::unauthorized("Invalid authentication".to_string())
        }
        CoreAuthError::TooManyFailedAttempts => {
            AppError::rate_limited("Too many failed attempts".to_string())
        }
        CoreAuthError::DeviceNotFound => {
            AppError::not_found("Device session not found".to_string())
        }
        CoreAuthError::DeviceNotTrusted => {
            AppError::forbidden("Device is not trusted".to_string())
        }
        CoreAuthError::SessionExpired => {
            AppError::unauthorized("Session expired".to_string())
        }
        CoreAuthError::UserNotFound => {
            AppError::not_found("User not found".to_string())
        }
        CoreAuthError::DatabaseError(e) => {
            AppError::internal(format!("Auth error: {e}"))
        }
    }
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
    DeviceFingerprint::from_hash(hash)
        .map_err(|_| "Invalid device fingerprint".to_string())
}

fn validate_device_public_key(
    public_key: Option<&str>,
    key_alg: Option<&str>,
) -> AppResult<Option<(String, String)>> {
    let Some(pk_b64) = public_key else {
        return Ok(None);
    };

    let alg = key_alg.unwrap_or("ed25519");
    if alg != "ed25519" {
        return Err(AppError::bad_request(
            "unsupported device_key_alg".to_string(),
        ));
    }

    let pk_bytes = base64::engine::general_purpose::STANDARD
        .decode(pk_b64.as_bytes())
        .map_err(|_| {
            AppError::bad_request(
                "invalid device_public_key encoding".to_string(),
            )
        })?;
    if pk_bytes.len() != 32 {
        return Err(AppError::bad_request(
            "invalid device_public_key length for ed25519".to_string(),
        ));
    }

    Ok(Some((alg.to_string(), pk_b64.to_string())))
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

    AuthEventContext {
        ip_address,
        user_agent,
        ..Default::default()
    }
}

fn bundle_to_device_auth_token(
    bundle: TokenBundle,
    registration: Option<DeviceRegistration>,
    requires_pin_setup: bool,
) -> DeviceAuthToken {
    let expires_in = bundle
        .session_token
        .expires_at()
        .signed_duration_since(Utc::now())
        .num_seconds()
        .max(0) as u32;
    let access_token = bundle.session_token.as_str().to_string();

    DeviceAuthToken {
        access_token: access_token.clone(),
        session_token: access_token,
        refresh_token: bundle.refresh_token.as_str().to_string(),
        expires_in,
        session_id: Some(bundle.session_record_id),
        device_session_id: bundle.device_session_id,
        user_id: Some(bundle.user_id),
        scope: bundle.scope,
        device_registration: registration,
        requires_pin_setup,
    }
}

fn device_session_to_device_registration(
    session: &DeviceSession,
) -> DeviceRegistration {
    DeviceRegistration {
        id: session.id(),
        user_id: session.user_id(),
        device_id: session_device_id(session),
        device_name: session.device_name().to_string(),
        platform: session_platform(session),
        app_version: session.app_version().unwrap_or("unknown").to_string(),
        pin_configured: session.has_pin(),
        registered_at: session.created_at(),
        last_used_at: session.last_activity(),
        expires_at: session.trusted_until(),
        revoked: matches!(session.status(), DeviceStatus::Revoked),
        revoked_by: session.revoked_by(),
        revoked_at: session.revoked_at(),
    }
}

fn session_device_id(session: &DeviceSession) -> Uuid {
    session
        .metadata()
        .get("device_id")
        .and_then(|value| value.as_str())
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or_else(|| session.id())
}

fn session_platform(session: &DeviceSession) -> Platform {
    match session.platform().unwrap_or("unknown") {
        "macos" => Platform::MacOS,
        "linux" => Platform::Linux,
        "windows" => Platform::Windows,
        "ios" => Platform::IOS,
        "android" => Platform::Android,
        "tvos" => Platform::TvOS,
        "web" => Platform::Web,
        _ => Platform::Unknown,
    }
}

fn device_session_to_authenticated_device(
    session: &DeviceSession,
) -> AuthenticatedDevice {
    AuthenticatedDevice {
        id: session.id(),
        user_id: session.user_id(),
        fingerprint: session.device_fingerprint().as_str().to_string(),
        name: session.device_name().to_string(),
        platform: session_platform(session),
        app_version: session.app_version().map(str::to_string),
        hardware_id: session.hardware_id().map(str::to_string),
        status: map_device_status(session.status()),
        pin_configured: session.has_pin(),
        failed_attempts: i32::from(session.failed_attempts()),
        locked_until: session.locked_until(),
        first_authenticated_by: session.first_authenticated_by(),
        first_authenticated_at: session.first_authenticated_at(),
        trusted_until: session.trusted_until(),
        last_seen_at: session.last_seen_at(),
        last_activity: session.last_activity(),
        auto_login_enabled: session.auto_login_enabled(),
        revoked_by: session.revoked_by(),
        revoked_at: session.revoked_at(),
        revoked_reason: session.revoked_reason().map(str::to_string),
        created_at: session.created_at(),
        updated_at: session.updated_at(),
        metadata: session.metadata().clone(),
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
