//! Simplified device authentication handlers that work with existing database traits
//!
//! This is a temporary implementation until the database traits are extended
//! to support the full device authentication schema.

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{extract::State, Extension, Json};
use chrono::{Duration, Utc};
use ferrex_core::{
    api_types::ApiResponse,
    auth::{AuthError, DeviceInfo, SetPinRequest, SetPinResponse},
    user::{AuthToken, User},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    errors::{AppError, AppResult},
    AppState,
};

/// Enhanced login request with device info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceLoginRequest {
    pub username: String,
    pub password: String,
    pub device_info: Option<DeviceInfo>,
}

/// Enhanced login response with device registration info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceLoginResponse {
    pub auth_token: AuthToken,
    pub user: User,
    pub requires_pin_setup: bool,
    pub device_id: Uuid,
}

/// Login with device registration support
pub async fn login_with_device(
    State(state): State<AppState>,
    Json(request): Json<DeviceLoginRequest>,
) -> AppResult<Json<ApiResponse<DeviceLoginResponse>>> {
    // Get user
    let user = state
        .db
        .backend()
        .get_user_by_username(&request.username)
        .await
        .map_err(|_| AppError::internal(AuthError::InternalError.to_string()))?
        .ok_or_else(|| AppError::unauthorized(AuthError::InvalidCredentials.to_string()))?;

    // Get password hash from credentials table
    let password_hash = state
        .db
        .backend()
        .get_user_password_hash(user.id)
        .await
        .map_err(|_| AppError::internal("Failed to get password hash"))?
        .ok_or_else(|| AppError::unauthorized(AuthError::InvalidCredentials.to_string()))?;

    // Verify password
    let parsed_hash = PasswordHash::new(&password_hash)
        .map_err(|_| AppError::internal("Invalid password hash"))?;

    let argon2 = Argon2::default();
    argon2
        .verify_password(request.password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::unauthorized(AuthError::InvalidCredentials.to_string()))?;

    // Generate tokens
    use super::jwt::{generate_access_token, generate_refresh_token};
    let access_token = generate_access_token(user.id)
        .map_err(|_| AppError::internal("Failed to generate access token"))?;

    let refresh_token = generate_refresh_token();

    // Extract device info
    let device_id = request
        .device_info
        .as_ref()
        .map(|d| d.device_id)
        .unwrap_or_else(Uuid::new_v4);

    let device_name = request
        .device_info
        .as_ref()
        .map(|d| d.device_name.clone())
        .or_else(|| Some("Unknown Device".to_string()));

    // Store refresh token with device info
    let expires_at = Utc::now() + Duration::days(30);
    state
        .db
        .backend()
        .store_refresh_token(&refresh_token, user.id, device_name.clone(), expires_at)
        .await
        .map_err(|_| AppError::internal("Failed to store refresh token"))?;

    // Create session
    let session = ferrex_core::user::UserSession {
        id: Uuid::new_v4(),
        user_id: user.id,
        device_name,
        ip_address: None,
        user_agent: None,
        last_active: Utc::now().timestamp(),
        created_at: Utc::now().timestamp(),
    };

    state
        .db
        .backend()
        .create_session(&session)
        .await
        .map_err(|_| AppError::internal("Failed to create session"))?;

    // For now, always indicate PIN setup is available
    // In a full implementation, we'd check if this device has a PIN set
    let requires_pin_setup = true;

    Ok(Json(ApiResponse::success(DeviceLoginResponse {
        auth_token: AuthToken {
            access_token,
            refresh_token,
            expires_in: 900, // 15 minutes
        },
        user,
        requires_pin_setup,
        device_id,
    })))
}

/// Simplified PIN setup endpoint
///
/// This is a placeholder that demonstrates the API structure.
/// The actual PIN storage would require extending the database traits.
pub async fn set_pin_simple(
    State(_state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<SetPinRequest>,
) -> AppResult<Json<ApiResponse<SetPinResponse>>> {
    // Validate PIN
    use ferrex_core::auth::pin::{validate_pin, PinPolicy};
    let policy = PinPolicy {
        min_length: 4,
        max_length: 8,
        max_attempts: 5,
        lockout_duration_minutes: 30,
        requires_device_trust: true,
        allow_simple_pins: false,
    };

    validate_pin(&request.new_pin, &policy)
        .map_err(|e| AppError::bad_request(format!("Invalid PIN: {}", e)))?;

    // In a full implementation, we would:
    // 1. Verify the current password
    // 2. Hash the PIN with device-specific salt
    // 3. Store the PIN hash in the device_user_credentials table
    //
    // For now, we just return success

    tracing::info!(
        "PIN setup requested for user {} on device {}",
        user.id,
        request.device_id
    );

    Ok(Json(ApiResponse::success(SetPinResponse {
        success: true,
        message: "PIN setup is not yet implemented. This is a placeholder endpoint.".to_string(),
    })))
}

/// Device status check endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceStatusRequest {
    pub device_id: Uuid,
    pub user_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceStatus {
    pub is_trusted: bool,
    pub has_pin: bool,
    pub message: String,
}

pub async fn check_device_status(
    State(_state): State<AppState>,
    Json(request): Json<DeviceStatusRequest>,
) -> AppResult<Json<ApiResponse<DeviceStatus>>> {
    // In a full implementation, we would check:
    // 1. If the device is registered in authenticated_devices
    // 2. If the device has a PIN set for this user
    // 3. If the device trust hasn't expired

    tracing::debug!(
        "Device status check for device {} and user {}",
        request.device_id,
        request.user_id
    );

    Ok(Json(ApiResponse::success(DeviceStatus {
        is_trusted: false,
        has_pin: false,
        message: "Device authentication is not yet fully implemented".to_string(),
    })))
}

/// Helper to hash a PIN (for future use)
#[allow(dead_code)]
fn hash_pin_with_device_salt(pin: &str, device_id: &Uuid) -> Result<String, ()> {
    use base64::{engine::general_purpose, Engine as _};

    // Create device-specific salt
    let mut hasher = Sha256::new();
    hasher.update(device_id.to_string().as_bytes());
    hasher.update(b"ferrex-pin-salt-v1");
    let salt_bytes = hasher.finalize();

    // Create SaltString from hash
    let salt_b64 = general_purpose::STANDARD.encode(&salt_bytes[..22]);
    let salt = SaltString::from_b64(&salt_b64).map_err(|_| ())?;

    // Hash PIN
    let argon2 = Argon2::default();
    argon2
        .hash_password(pin.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| ())
}
