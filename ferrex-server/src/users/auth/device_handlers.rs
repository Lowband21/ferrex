//! Device-aware authentication handlers

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{Extension, Json, extract::State, http::HeaderMap};
use chrono::{Duration, Utc};
use ferrex_core::{
    api_types::ApiResponse,
    auth::{
        AuthError, AuthEvent, AuthEventType, AuthResult, AuthenticatedDevice, DeviceInfo,
        DeviceRegistration, DeviceUpdateParams, DeviceUserCredential, Platform,
        SessionDeviceSession, generate_trust_token,
    },
    user::User,
};
use ring::constant_time;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{error, info};
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};

const REMEMBER_DEVICE_DAYS: i64 = 30;

/// Device fingerprint from user agent and other factors
fn generate_device_fingerprint(
    user_agent: &str,
    platform: &Platform,
    hardware_id: Option<&str>,
    device_id: &Uuid,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(user_agent.as_bytes());
    hasher.update(platform.as_ref().as_bytes());
    if let Some(hw_id) = hardware_id {
        hasher.update(hw_id.as_bytes());
    }
    hasher.update(device_id.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract device info from request headers
fn extract_device_info(headers: &HeaderMap, body_device_info: Option<DeviceInfo>) -> DeviceInfo {
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("Unknown");

    if let Some(device_info) = body_device_info {
        device_info
    } else {
        // Generate device info from headers
        DeviceInfo {
            device_id: Uuid::new_v4(),
            device_name: format!("{} Device", Platform::from_user_agent(user_agent).as_ref()),
            platform: Platform::from_user_agent(user_agent),
            app_version: "1.0.0".to_string(),
            hardware_id: None,
        }
    }
}

/// Login request with optional device info
#[derive(Debug, Deserialize)]
pub struct DeviceLoginRequest {
    pub username: String,
    pub password: String,
    pub device_info: Option<DeviceInfo>,
    pub remember_device: bool,
}

/// PIN login request
#[derive(Debug, Deserialize)]
pub struct PinLoginRequest {
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub pin: String,
}

/// Device authentication status response
#[derive(Debug, Serialize)]
pub struct DeviceAuthStatus {
    pub device_registered: bool,
    pub has_pin: bool,
    pub remaining_attempts: Option<u8>,
}

/// Device-aware login handler
pub async fn device_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DeviceLoginRequest>,
) -> AppResult<Json<ApiResponse<AuthResult>>> {
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let ip_address = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Get user
    let user = state
        .db
        .backend()
        .get_user_by_username(&request.username)
        .await
        .map_err(|_| AppError::internal("Database error"))?
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
    let password_valid = argon2
        .verify_password(request.password.as_bytes(), &parsed_hash)
        .is_ok();

    // Log auth event
    let event = AuthEvent {
        id: Uuid::new_v4(),
        user_id: Some(user.id),
        device_id: None,
        event_type: if password_valid {
            AuthEventType::PasswordLoginSuccess
        } else {
            AuthEventType::PasswordLoginFailure
        },
        success: password_valid,
        failure_reason: if !password_valid {
            Some("Invalid password".to_string())
        } else {
            None
        },
        ip_address: ip_address.clone(),
        user_agent: user_agent.clone(),
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
    };

    let _ = state.db.backend().log_auth_event(&event).await;

    if !password_valid {
        return Err(AppError::unauthorized(
            AuthError::InvalidCredentials.to_string(),
        ));
    }

    // Extract device info
    let device_info = extract_device_info(&headers, request.device_info);
    let now = Utc::now();
    let remember_duration = Duration::days(REMEMBER_DEVICE_DAYS);

    // Generate device fingerprint
    let fingerprint = generate_device_fingerprint(
        user_agent.as_deref().unwrap_or("Unknown"),
        &device_info.platform,
        device_info.hardware_id.as_deref(),
        &device_info.device_id,
    );

    // Check if device is already registered
    let existing_device = state
        .db
        .backend()
        .get_device_by_fingerprint(&fingerprint)
        .await
        .map_err(|_| AppError::internal("Database error"))?;

    let (device_id, trusted_until) = if let Some(device) = existing_device {
        // Check if this device has existing sessions for other users
        // If so, disable auto-login for those users (user switching scenario)
        if let Ok(device_sessions) = state.db.backend().get_device_sessions(device.id).await {
            for session in device_sessions {
                if session.user_id != user.id {
                    // Get the other user and disable their auto-login
                    if let Ok(Some(mut other_user)) =
                        state.db.backend().get_user_by_id(session.user_id).await
                        && other_user.preferences.auto_login_enabled
                    {
                        other_user.preferences.auto_login_enabled = false;
                        other_user.updated_at = Utc::now();
                        let _ = state.db.backend().update_user(&other_user).await;

                        // Also check and update device credential if it exists
                        if let Ok(Some(mut credential)) = state
                            .db
                            .backend()
                            .get_device_credential(session.user_id, device.id)
                            .await
                        {
                            credential.auto_login_enabled = false;
                            credential.updated_at = Utc::now();
                            let _ = state
                                .db
                                .backend()
                                .upsert_device_credential(&credential)
                                .await;
                        }

                        info!(
                            "Disabled auto-login for user {} due to user switch on device {}",
                            other_user.username, device.id
                        );
                    }
                }
            }
        }

        if device.revoked {
            return Err(AppError::unauthorized(AuthError::DeviceRevoked.to_string()));
        }

        let mut updated_trusted_until = device.trusted_until;
        let mut updates = DeviceUpdateParams {
            name: None,
            app_version: Some(device_info.app_version.clone()),
            last_seen_at: Some(now),
            trusted_until: None,
        };

        if request.remember_device {
            updated_trusted_until = now + remember_duration;
            updates.trusted_until = Some(updated_trusted_until);
        }

        state
            .db
            .backend()
            .update_device(device.id, &updates)
            .await
            .map_err(|_| AppError::internal("Failed to update device"))?;

        (device.id, updated_trusted_until)
    } else {
        // Register new device
        let trust_duration = if request.remember_device {
            remember_duration
        } else {
            Duration::hours(24)
        };

        let device = AuthenticatedDevice {
            id: Uuid::new_v4(),
            fingerprint: fingerprint.clone(),
            name: device_info.device_name.clone(),
            platform: device_info.platform.clone(),
            app_version: Some(device_info.app_version.clone()),
            first_authenticated_by: user.id,
            first_authenticated_at: now,
            trusted_until: now + trust_duration,
            last_seen_at: now,
            revoked: false,
            revoked_by: None,
            revoked_at: None,
            metadata: serde_json::json!({
                "hardware_id": device_info.hardware_id,
            }),
        };

        state
            .db
            .backend()
            .register_device(&device)
            .await
            .map_err(|_| AppError::internal("Failed to register device"))?;

        // Log device registration
        let event = AuthEvent {
            id: Uuid::new_v4(),
            user_id: Some(user.id),
            device_id: Some(device.id),
            event_type: AuthEventType::DeviceRegistered,
            success: true,
            failure_reason: None,
            ip_address: ip_address.clone(),
            user_agent: user_agent.clone(),
            metadata: serde_json::json!({
                "device_name": device.name,
                "platform": device.platform,
            }),
            created_at: Utc::now(),
        };

        let _ = state.db.backend().log_auth_event(&event).await;

        (device.id, device.trusted_until)
    };

    // Create device session
    let session = SessionDeviceSession::new(
        user.id,
        device_id,
        ip_address,
        user_agent,
        if request.remember_device {
            remember_duration
        } else {
            Duration::hours(24)
        },
    );

    // Store session (token will be hashed in the database layer)
    let token_hash = hash_token(&session.session_token);
    let mut session_to_store = session.clone();
    session_to_store.session_token = token_hash;

    state
        .db
        .backend()
        .create_device_session(&session_to_store)
        .await
        .map_err(|_| AppError::internal("Failed to create session"))?;

    // Check if device has PIN set
    let credential = state
        .db
        .backend()
        .get_device_credential(user.id, device_id)
        .await
        .map_err(|_| AppError::internal("Database error"))?;

    let requires_pin_setup =
        credential.is_none() || credential.as_ref().unwrap().pin_hash.is_none();

    Ok(Json(ApiResponse::success(AuthResult {
        user_id: user.id,
        session_token: session.session_token, // Return the actual session token
        device_registration: Some(DeviceRegistration {
            id: device_id,
            user_id: user.id,
            device_id,
            device_name: device_info.device_name,
            platform: device_info.platform,
            app_version: device_info.app_version,
            trust_token: generate_trust_token(),
            pin_hash: credential.and_then(|c| c.pin_hash),
            registered_at: now,
            last_used_at: now,
            expires_at: Some(trusted_until),
            revoked: false,
            revoked_by: None,
            revoked_at: None,
        }),
        requires_pin_setup,
    })))
}

/// PIN login handler
pub async fn pin_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PinLoginRequest>,
) -> AppResult<Json<ApiResponse<AuthResult>>> {
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let ip_address = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Get device credential
    let mut credential = state
        .db
        .backend()
        .get_device_credential(request.user_id, request.device_id)
        .await
        .map_err(|_| AppError::internal("Database error"))?
        .ok_or_else(|| AppError::unauthorized("Device not registered for this user"))?;

    if !credential.auto_login_enabled {
        return Err(AppError::unauthorized(
            AuthError::DeviceNotTrusted.to_string(),
        ));
    }

    let device = state
        .db
        .backend()
        .get_device_by_id(request.device_id)
        .await
        .map_err(|_| AppError::internal("Database error"))?
        .ok_or_else(|| AppError::unauthorized("Device not registered"))?;

    if device.revoked {
        return Err(AppError::unauthorized(AuthError::DeviceRevoked.to_string()));
    }

    // Check if locked
    if let Some(locked_until) = credential.locked_until
        && locked_until > Utc::now()
    {
        return Err(AppError::unauthorized(
            AuthError::TooManyAttempts {
                locked_until: locked_until.timestamp(),
            }
            .to_string(),
        ));
    }

    // Verify PIN
    let now = Utc::now();
    let remember_duration = Duration::days(REMEMBER_DEVICE_DAYS);

    if device.trusted_until < now {
        return Err(AppError::unauthorized(
            AuthError::DeviceNotTrusted.to_string(),
        ));
    }

    let previous_attempts = credential.failed_attempts;
    let pin_valid = if let Some(pin_hash) = &credential.pin_hash {
        verify_pin_with_device_salt(&request.pin, pin_hash, request.user_id, request.device_id)
    } else {
        false
    };

    // Update failed attempts
    if !pin_valid {
        let new_attempts = credential.failed_attempts + 1;
        let locked_until = if new_attempts >= 5 {
            Some(Utc::now() + Duration::minutes(15))
        } else {
            None
        };

        state
            .db
            .backend()
            .update_device_failed_attempts(
                request.user_id,
                request.device_id,
                new_attempts,
                locked_until,
            )
            .await
            .map_err(|_| AppError::internal("Database error"))?;
    } else {
        // Reset failed attempts on success
        credential.failed_attempts = 0;
        credential.locked_until = None;
        credential.pin_last_used_at = Some(now);
        credential.updated_at = now;

        state
            .db
            .backend()
            .upsert_device_credential(&credential)
            .await
            .map_err(|_| AppError::internal("Database error"))?;

        let updates = DeviceUpdateParams {
            name: None,
            app_version: None,
            last_seen_at: Some(now),
            trusted_until: Some(now + remember_duration),
        };

        state
            .db
            .backend()
            .update_device(request.device_id, &updates)
            .await
            .map_err(|_| AppError::internal("Failed to update device trust"))?;
    }

    // Log auth event
    let event = AuthEvent {
        id: Uuid::new_v4(),
        user_id: Some(request.user_id),
        device_id: Some(request.device_id),
        event_type: if pin_valid {
            AuthEventType::PinLoginSuccess
        } else {
            AuthEventType::PinLoginFailure
        },
        success: pin_valid,
        failure_reason: if !pin_valid {
            Some("Invalid PIN".to_string())
        } else {
            None
        },
        ip_address: ip_address.clone(),
        user_agent: user_agent.clone(),
        metadata: serde_json::json!({
            "attempts": previous_attempts + if !pin_valid { 1 } else { 0 },
        }),
        created_at: Utc::now(),
    };

    let _ = state.db.backend().log_auth_event(&event).await;

    if !pin_valid {
        let attempts_remaining = 5 - (credential.failed_attempts + 1);
        return Err(AppError::unauthorized(
            AuthError::InvalidPin {
                attempts_remaining: attempts_remaining as u8,
            }
            .to_string(),
        ));
    }

    // Create session
    let session = SessionDeviceSession::new(
        request.user_id,
        request.device_id,
        ip_address,
        user_agent,
        remember_duration,
    );

    // Store session
    let token_hash = hash_token(&session.session_token);
    let mut session_to_store = session.clone();
    session_to_store.session_token = token_hash;

    state
        .db
        .backend()
        .create_device_session(&session_to_store)
        .await
        .map_err(|_| AppError::internal("Failed to create session"))?;

    Ok(Json(ApiResponse::success(AuthResult {
        user_id: request.user_id,
        session_token: session.session_token, // Return the actual session token
        device_registration: None,
        requires_pin_setup: false,
    })))
}

/// Set or update PIN for a device
#[derive(Debug, Deserialize)]
pub struct SetPinRequest {
    pub device_id: Uuid,
    pub pin: String,
}

pub async fn set_device_pin(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<SetPinRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    // Validate PIN
    use ferrex_core::auth::{PinPolicy, validate_pin};
    let mut policy = PinPolicy::default();
    policy.min_length = 4; // Allow 4-digit PINs

    if let Err(e) = validate_pin(&request.pin, &policy) {
        return Err(AppError::bad_request(format!("Invalid PIN: {}", e)));
    }

    // Hash PIN with device-specific salt
    let pin_hash = hash_pin_with_device_salt(&request.pin, user.id, request.device_id);

    // Update or create device credential
    // Enable auto-login if the user has it enabled in their preferences
    let credential = DeviceUserCredential {
        user_id: user.id,
        device_id: request.device_id,
        pin_hash: Some(pin_hash),
        pin_set_at: Some(Utc::now()),
        pin_last_used_at: None,
        failed_attempts: 0,
        locked_until: None,
        auto_login_enabled: user.preferences.auto_login_enabled,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    state
        .db
        .backend()
        .upsert_device_credential(&credential)
        .await
        .map_err(|_| AppError::internal("Failed to update PIN"))?;

    // Log event
    let event = AuthEvent {
        id: Uuid::new_v4(),
        user_id: Some(user.id),
        device_id: Some(request.device_id),
        event_type: AuthEventType::PinSet,
        success: true,
        failure_reason: None,
        ip_address: None,
        user_agent: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
    };

    let _ = state.db.backend().log_auth_event(&event).await;

    Ok(Json(ApiResponse::success(())))
}

/// Check device authentication status for a user (public endpoint)
#[derive(Debug, Deserialize)]
pub struct DeviceStatusQuery {
    pub user_id: Uuid,
    pub device_id: Uuid,
}

pub async fn check_device_status(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<DeviceStatusQuery>,
) -> AppResult<Json<ApiResponse<DeviceAuthStatus>>> {
    info!(
        "[DeviceStatus] Checking device status for user_id: {}, device_id: {}",
        query.user_id, query.device_id
    );

    // For now, since we don't have device tracking fully implemented,
    // we'll just check if the user has a PIN set in the credentials table
    // This is a simplified implementation until full device management is in place

    // Check if user has any device credentials
    let credential = state
        .db
        .backend()
        .get_device_credential(query.user_id, query.device_id)
        .await
        .map_err(|e| {
            error!(
                "[DeviceStatus] Database error getting device credential: {}",
                e
            );
            AppError::internal("Database error")
        })?;

    let (device_registered, has_pin, remaining_attempts) = if let Some(cred) = credential {
        let has_pin = cred.pin_hash.is_some();
        let remaining = if cred.failed_attempts >= 5 {
            Some(0)
        } else {
            Some((5 - cred.failed_attempts) as u8)
        };
        (true, has_pin, remaining)
    } else {
        // No credential record, so device not registered for this user
        (false, false, Some(5))
    };

    info!(
        "[DeviceStatus] Result - registered: {}, has_pin: {}, remaining_attempts: {:?}",
        device_registered, has_pin, remaining_attempts
    );

    Ok(Json(ApiResponse::success(DeviceAuthStatus {
        device_registered,
        has_pin,
        remaining_attempts,
    })))
}

/// Hash a token for storage
fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn derive_device_salt(user_id: Uuid, device_id: Uuid) -> String {
    // Create device-specific salt
    let salt_input = format!("{}-{}", user_id, device_id);
    let mut hasher = Sha256::new();
    hasher.update(salt_input.as_bytes());
    let salt_bytes = hasher.finalize();
    base64::encode(&salt_bytes[..16])
}

/// Hash PIN with device-specific salt
fn hash_pin_with_device_salt(pin: &str, user_id: Uuid, device_id: Uuid) -> String {
    let salt_b64 = derive_device_salt(user_id, device_id);

    // Hash PIN
    let argon2 = Argon2::default();
    let salt = SaltString::from_b64(&salt_b64).unwrap();
    argon2
        .hash_password(pin.as_bytes(), &salt)
        .unwrap()
        .to_string()
}

/// Verify PIN with device-specific salt using constant-time comparison
///
/// This function prevents timing attacks by ensuring that PIN verification
/// takes the same amount of time regardless of whether the PIN is correct
/// or how many characters match.
fn verify_pin_with_device_salt(pin: &str, hash: &str, user_id: Uuid, device_id: Uuid) -> bool {
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };

    // Validate the hash salt matches the expected device-specific salt
    if let Some(stored_salt) = parsed_hash.salt {
        let expected_salt = derive_device_salt(user_id, device_id);
        if constant_time::verify_slices_are_equal(
            stored_salt.as_str().as_bytes(),
            expected_salt.as_bytes(),
        )
        .is_err()
        {
            return false;
        }
    } else {
        return false;
    }

    let argon2 = Argon2::default();

    // Perform Argon2 verification (already constant-time internally)
    let argon2_result = argon2.verify_password(pin.as_bytes(), &parsed_hash);

    // Convert result to bytes for constant-time comparison
    // This ensures we don't leak timing information through early returns
    let verification_passed = if argon2_result.is_ok() { 1u8 } else { 0u8 };
    let expected_success = 1u8;

    // Use constant-time comparison to prevent timing attacks
    let is_equal =
        constant_time::verify_slices_are_equal(&[verification_passed], &[expected_success]);

    is_equal.is_ok()
}

/// List user's devices
pub async fn list_user_devices(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> AppResult<Json<ApiResponse<Vec<AuthenticatedDevice>>>> {
    let devices = state
        .db
        .backend()
        .get_user_devices(user.id)
        .await
        .map_err(|_| AppError::internal("Failed to get devices"))?;

    Ok(Json(ApiResponse::success(devices)))
}

/// Revoke a device
#[derive(Debug, Deserialize)]
pub struct RevokeDeviceRequest {
    pub device_id: Uuid,
}

pub async fn revoke_device(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<RevokeDeviceRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    state
        .db
        .backend()
        .revoke_device(request.device_id, user.id)
        .await
        .map_err(|_| AppError::internal("Failed to revoke device"))?;

    // Revoke all sessions for this device
    state
        .db
        .backend()
        .revoke_device_sessions(request.device_id)
        .await
        .map_err(|_| AppError::internal("Failed to revoke sessions"))?;

    // Log event
    let event = AuthEvent {
        id: Uuid::new_v4(),
        user_id: Some(user.id),
        device_id: Some(request.device_id),
        event_type: AuthEventType::DeviceRevoked,
        success: true,
        failure_reason: None,
        ip_address: None,
        user_agent: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
    };

    let _ = state.db.backend().log_auth_event(&event).await;

    Ok(Json(ApiResponse::success(())))
}
/// Change PIN request
#[derive(Debug, Deserialize)]
pub struct ChangePinRequest {
    pub device_id: String,
    pub current_pin: String,
    pub new_pin: String,
}

/// Change device PIN
pub async fn change_device_pin(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<ChangePinRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // Parse device ID
    let device_id = Uuid::parse_str(&request.device_id)
        .map_err(|_| AppError::bad_request("Invalid device ID"))?;

    // Validate PIN format
    if request.new_pin.len() != 4 || !request.new_pin.chars().all(|c| c.is_numeric()) {
        return Err(AppError::bad_request("PIN must be 4 digits"));
    }

    // Get current device credential
    let credential = state
        .db
        .backend()
        .get_device_credential(user.id, device_id)
        .await
        .map_err(|_| AppError::internal("Failed to retrieve device credentials"))?
        .ok_or_else(|| AppError::not_found("Device PIN not set"))?;

    // Verify current PIN using constant-time comparison
    let pin_hash = credential
        .pin_hash
        .ok_or_else(|| AppError::not_found("Device PIN not set"))?;

    let current_pin_valid =
        verify_pin_with_device_salt(&request.current_pin, &pin_hash, user.id, device_id);

    if !current_pin_valid {
        return Err(AppError::unauthorized("Current PIN is incorrect"));
    }

    // Hash new PIN
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let new_pin_hash = argon2
        .hash_password(request.new_pin.as_bytes(), &salt)
        .map_err(|_| AppError::internal("Failed to hash PIN"))?
        .to_string();

    // Update PIN
    state
        .db
        .backend()
        .update_device_pin(user.id, device_id, &new_pin_hash)
        .await
        .map_err(|_| AppError::internal("Failed to update PIN"))?;

    // Log event
    let event = AuthEvent {
        id: Uuid::new_v4(),
        user_id: Some(user.id),
        device_id: Some(device_id),
        event_type: AuthEventType::PinSet,
        success: true,
        failure_reason: None,
        ip_address: None,
        user_agent: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
    };

    let _ = state.db.backend().log_auth_event(&event).await;

    Ok(Json(ApiResponse::success(())))
}
