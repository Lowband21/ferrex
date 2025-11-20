//! Device trust validation endpoints
//!
//! Provides server-side validation of device trust status,
//! allowing clients to verify their device registration and
//! trust expiration without relying on local storage.

use axum::{
    extract::{Query, State},
    Extension, Json,
};
use chrono::Utc;
use ferrex_core::{api_types::ApiResponse, user::User};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{errors::AppResult, AppState};
use ferrex_core::database::postgres::PostgresDatabase;

/// Device trust validation query parameters
#[derive(Debug, Deserialize)]
pub struct DeviceTrustQuery {
    /// Optional device ID to validate (defaults to current device)
    pub device_id: Option<Uuid>,
    /// Optional device fingerprint for additional validation
    pub fingerprint: Option<String>,
}

/// Helper function to get the database pool
fn get_pool(state: &AppState) -> Result<&sqlx::PgPool, crate::errors::AppError> {
    state
        .database
        .as_any()
        .downcast_ref::<PostgresDatabase>()
        .ok_or_else(|| crate::errors::AppError::internal("Database not available".to_string()))
        .map(|db| db.pool())
}

/// Device trust validation response
#[derive(Debug, Serialize)]
pub struct DeviceTrustStatus {
    /// Whether the device is currently trusted
    pub is_trusted: bool,
    /// When the device trust expires (if trusted)
    pub trusted_until: Option<chrono::DateTime<chrono::Utc>>,
    /// Device name (if registered)
    pub device_name: Option<String>,
    /// Device registration date
    pub registered_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Reason if not trusted
    pub reason: Option<String>,
}

/// Validate device trust status
///
/// This endpoint allows clients to verify their device trust status
/// against server records, ensuring consistency between client and
/// server state.
pub async fn validate_device_trust(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(device_id_ext): Extension<Option<Uuid>>,
    Query(params): Query<DeviceTrustQuery>,
) -> AppResult<Json<ApiResponse<DeviceTrustStatus>>> {
    let user_id = user.id;

    // Determine which device to check
    let device_id = params
        .device_id
        .or(device_id_ext)
        .ok_or_else(|| crate::errors::AppError::bad_request("Device ID required".to_string()))?;

    info!(
        "Validating device trust for user {} device {}",
        user_id, device_id
    );

    // Query device trust from database
    let device_record = sqlx::query!(
        r#"
        SELECT 
            ads.id,
            ads.device_name,
            ads.device_fingerprint as fingerprint,
            ads.created_at,
            ads.trusted_until,
            ads.last_activity as last_seen,
            CASE WHEN ads.status = 'revoked' THEN true ELSE false END as revoked
        FROM auth_device_sessions ads
        WHERE ads.id = $1 
            AND ads.user_id = $2
        LIMIT 1
        "#,
        device_id,
        user_id
    )
    .fetch_optional(get_pool(&state)?)
    .await
    .map_err(|e| crate::errors::AppError::internal(format!("Database error: {}", e)))?;

    let status = match device_record {
        Some(record) => {
            // Check if device is revoked
            if record.revoked.unwrap_or(false) {
                DeviceTrustStatus {
                    is_trusted: false,
                    trusted_until: None,
                    device_name: Some(record.device_name),
                    registered_at: Some(record.created_at),
                    reason: Some("Device has been revoked".to_string()),
                }
            }
            // Check if device trust has expired
            else if let Some(trusted_until) = record.trusted_until {
                let now = Utc::now();
                if trusted_until > now {
                    // Optionally validate fingerprint if provided
                    if let Some(ref client_fingerprint) = params.fingerprint {
                        let stored_fingerprint = &record.fingerprint;
                        if client_fingerprint != stored_fingerprint {
                            warn!(
                                "Device fingerprint mismatch for device {}: client={}, stored={}",
                                device_id, client_fingerprint, stored_fingerprint
                            );
                            return Ok(Json(ApiResponse::success(DeviceTrustStatus {
                                is_trusted: false,
                                trusted_until: None,
                                device_name: Some(record.device_name),
                                registered_at: Some(record.created_at),
                                reason: Some("Device fingerprint mismatch".to_string()),
                            })));
                        }
                    }

                    // Update last seen timestamp
                    let _ = sqlx::query!(
                        "UPDATE auth_device_sessions SET last_activity = $1 WHERE id = $2 AND user_id = $3",
                        Utc::now(),
                        device_id,
                        user_id
                    )
                    .execute(get_pool(&state)?)
                    .await
                    .map_err(|e| crate::errors::AppError::internal(format!("Database error: {}", e)));

                    DeviceTrustStatus {
                        is_trusted: true,
                        trusted_until: Some(trusted_until),
                        device_name: Some(record.device_name),
                        registered_at: Some(record.created_at),
                        reason: None,
                    }
                } else {
                    DeviceTrustStatus {
                        is_trusted: false,
                        trusted_until: Some(trusted_until),
                        device_name: Some(record.device_name),
                        registered_at: Some(record.created_at),
                        reason: Some("Device trust has expired".to_string()),
                    }
                }
            } else {
                // Device exists but has no active trust session
                DeviceTrustStatus {
                    is_trusted: false,
                    trusted_until: None,
                    device_name: Some(record.device_name),
                    registered_at: Some(record.created_at),
                    reason: Some("No active trust session".to_string()),
                }
            }
        }
        None => {
            // Device not found or doesn't belong to user
            DeviceTrustStatus {
                is_trusted: false,
                trusted_until: None,
                device_name: None,
                registered_at: None,
                reason: Some("Device not registered".to_string()),
            }
        }
    };

    info!(
        "Device trust validation result for device {}: trusted={}",
        device_id, status.is_trusted
    );

    Ok(Json(ApiResponse::success(status)))
}

/// Revoke device trust
///
/// Allows users to revoke trust for a specific device,
/// forcing re-authentication on that device.
#[derive(Debug, Deserialize)]
pub struct RevokeDeviceRequest {
    pub device_id: Uuid,
}

pub async fn revoke_device_trust(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<RevokeDeviceRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    let user_id = user.id;

    info!(
        "Revoking device trust for user {} device {}",
        user_id, request.device_id
    );

    // Verify device belongs to user
    let device_exists = sqlx::query!(
        "SELECT id FROM auth_device_sessions WHERE id = $1 AND user_id = $2",
        request.device_id,
        user_id
    )
    .fetch_optional(get_pool(&state)?)
    .await
    .map_err(|e| crate::errors::AppError::internal(format!("Database error: {}", e)))?
    .is_some();

    if !device_exists {
        return Err(crate::errors::AppError::not_found(
            "Device not found".to_string(),
        ));
    }

    // Revoke all sessions for this device
    sqlx::query!(
        "UPDATE auth_device_sessions SET status = 'revoked', revoked_at = NOW() WHERE id = $1 AND user_id = $2",
        request.device_id,
        user_id
    )
    .execute(get_pool(&state)?)
    .await
    .map_err(|e| crate::errors::AppError::internal(format!("Database error: {}", e)))?;

    // Optionally, also invalidate all active sessions for this device
    sqlx::query!(
        "UPDATE sessions SET expires_at = $1 WHERE device_id = $2 AND user_id = $3",
        Utc::now(),
        request.device_id,
        user_id
    )
    .execute(get_pool(&state)?)
    .await
    .map_err(|e| crate::errors::AppError::internal(format!("Database error: {}", e)))?;

    info!(
        "Successfully revoked device trust for device {}",
        request.device_id
    );

    Ok(Json(ApiResponse::success(())))
}

/// List trusted devices for the current user
#[derive(Debug, Serialize)]
pub struct TrustedDevice {
    pub device_id: Uuid,
    pub device_name: String,
    pub platform: String,
    pub trusted_until: chrono::DateTime<chrono::Utc>,
    pub last_seen: chrono::DateTime<chrono::Utc>,
    pub is_current: bool,
}

pub async fn list_trusted_devices(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(device_id_ext): Extension<Option<Uuid>>,
) -> AppResult<Json<ApiResponse<Vec<TrustedDevice>>>> {
    let user_id = user.id;
    let current_device = device_id_ext.unwrap_or_default();

    info!("Listing trusted devices for user {}", user_id);

    let devices = sqlx::query!(
        r#"
        SELECT 
            ads.id as device_id,
            ads.device_name,
            NULL::text as platform,
            ads.trusted_until,
            ads.last_activity as last_seen
        FROM auth_device_sessions ads
        WHERE ads.user_id = $1 
            AND ads.trusted_until > $2
            AND ads.status = 'trusted'
        ORDER BY ads.last_activity DESC
        "#,
        user_id,
        Utc::now()
    )
    .fetch_all(get_pool(&state)?)
    .await
    .map_err(|e| crate::errors::AppError::internal(format!("Database error: {}", e)))?
    .into_iter()
    .map(|row| TrustedDevice {
        device_id: row.device_id,
        device_name: row.device_name,
        platform: row.platform.unwrap_or_else(|| "unknown".to_string()),
        trusted_until: row.trusted_until.unwrap_or_else(Utc::now),
        last_seen: row.last_seen,
        is_current: row.device_id == current_device,
    })
    .collect();

    Ok(Json(ApiResponse::success(devices)))
}

/// Extend device trust period
///
/// Allows extending the trust period for a device that's about to expire
#[derive(Debug, Deserialize)]
pub struct ExtendTrustRequest {
    pub device_id: Option<Uuid>,
    pub days: Option<i64>,
}

pub async fn extend_device_trust(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(device_id_ext): Extension<Option<Uuid>>,
    Json(request): Json<ExtendTrustRequest>,
) -> AppResult<Json<ApiResponse<DeviceTrustStatus>>> {
    let user_id = user.id;
    let device_id = request
        .device_id
        .or(device_id_ext)
        .ok_or_else(|| crate::errors::AppError::bad_request("Device ID required".to_string()))?;
    let extension_days = request.days.unwrap_or(30).min(90); // Max 90 days

    info!(
        "Extending device trust for user {} device {} by {} days",
        user_id, device_id, extension_days
    );

    // Verify device belongs to user and has active trust
    let current_trust = sqlx::query!(
        r#"
        SELECT 
            ads.trusted_until,
            ads.device_name,
            ads.created_at
        FROM auth_device_sessions ads
        WHERE ads.id = $1 
            AND ads.user_id = $2
            AND ads.status = 'trusted'
        ORDER BY ads.created_at DESC
        LIMIT 1
        "#,
        device_id,
        user_id
    )
    .fetch_optional(get_pool(&state)?)
    .await
    .map_err(|e| crate::errors::AppError::internal(format!("Database error: {}", e)))?;

    match current_trust {
        Some(record) if record.trusted_until.is_some_and(|t| t > Utc::now()) => {
            // Extend from current expiry or from now if less than 7 days remaining
            let current_expiry = record.trusted_until.unwrap();
            let days_remaining = (current_expiry - Utc::now()).num_days();

            let new_expiry = if days_remaining < 7 {
                Utc::now() + chrono::Duration::days(extension_days)
            } else {
                current_expiry + chrono::Duration::days(extension_days)
            };

            // Update trust expiry
            sqlx::query!(
                "UPDATE auth_device_sessions SET trusted_until = $1 WHERE id = $2 AND user_id = $3",
                new_expiry,
                device_id,
                user_id
            )
            .execute(get_pool(&state)?)
            .await
            .map_err(|e| crate::errors::AppError::internal(format!("Database error: {}", e)))?;

            Ok(Json(ApiResponse::success(DeviceTrustStatus {
                is_trusted: true,
                trusted_until: Some(new_expiry),
                device_name: Some(record.device_name),
                registered_at: Some(record.created_at),
                reason: None,
            })))
        }
        _ => Err(crate::errors::AppError::bad_request(
            "Device not found or trust already expired".to_string(),
        )),
    }
}
