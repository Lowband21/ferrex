use axum::{
    extract::{Path, State},
    http::HeaderMap,
    http::StatusCode,
    Extension, Json,
};
use ferrex_core::{
    api_types::ApiResponse,
    database::postgres::PostgresDatabase,
    user::{User, UserUpdateRequest},
};
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::{
    AppState,
    errors::{AppError, AppResult},
    users::{UserService, user_service::UpdateUserParams},
};

/// Helper function to get the database pool
fn get_pool(state: &AppState) -> Result<&sqlx::PgPool, AppError> {
    state
        .database
        .as_any()
        .downcast_ref::<PostgresDatabase>()
        .ok_or_else(|| AppError::internal("Database not available".to_string()))
        .map(|db| db.pool())
}

/// List users for selection screen (rate-limited public endpoint)
///
/// This endpoint is intentionally limited to prevent user enumeration attacks:
/// - Returns only minimal user information
/// - Requires device fingerprint for tracking
/// - Rate limited to prevent scraping
/// - May require CAPTCHA in future versions
pub async fn list_users_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<ApiResponse<Vec<UserListItemDto>>>> {
    // Extract device fingerprint from headers (required for user selection)
    let device_fingerprint = headers
        .get("X-Device-Fingerprint")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            AppError::bad_request("Device fingerprint required for user list".to_string())
        })?;

    // Validate device fingerprint format (basic validation)
    if device_fingerprint.len() < 32 || device_fingerprint.len() > 256 {
        return Err(AppError::bad_request(
            "Invalid device fingerprint".to_string(),
        ));
    }

    // Check if this is a known/trusted device (optional enhancement)
    let is_known_device = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM auth_device_sessions WHERE device_fingerprint = $1)",
        device_fingerprint
    )
    .fetch_one(get_pool(&state)?)
    .await
    .ok()
    .and_then(|opt| opt)
    .unwrap_or(false);

    // For unknown devices, return limited information
    if !is_known_device {
        // Return only usernames without UUIDs or other sensitive info
        let users = sqlx::query!(
            r#"
            SELECT username, display_name, avatar_url
            FROM users
            -- No soft delete check needed
            ORDER BY username
            LIMIT 50
            "#
        )
        .fetch_all(get_pool(&state)?)
        .await
        .map_err(|e| AppError::internal(format!("Database error: {}", e)))?;

        // Create anonymized user list (no UUIDs, no activity info)
        let user_list: Vec<UserListItemDto> = users
            .into_iter()
            .map(|user| UserListItemDto {
                // Use a deterministic but non-reversible hash of username as ID
                // This allows consistent selection without exposing real UUIDs
                id: Uuid::new_v5(&Uuid::NAMESPACE_DNS, user.username.as_bytes()),
                username: user.username,
                display_name: user.display_name,
                avatar_url: user.avatar_url,
                has_pin: false,   // Never reveal PIN status to unknown devices
                last_login: None, // Never reveal activity patterns
            })
            .collect();

        return Ok(Json(ApiResponse::success(user_list)));
    }

    // For known devices, return slightly more information (but still limited)
    let users = sqlx::query!(
        r#"
        SELECT
            u.id,
            u.username,
            u.display_name,
            u.avatar_url,
            EXISTS(
                SELECT 1 FROM auth_device_sessions ads
                WHERE ads.user_id = u.id
                AND ads.device_fingerprint = $1
                AND ads.pin_hash IS NOT NULL
            ) as has_pin_on_device
        FROM users u
        -- No soft delete check needed
        ORDER BY u.username
        "#,
        device_fingerprint
    )
    .fetch_all(get_pool(&state)?)
    .await
    .map_err(|e| AppError::internal(format!("Database error: {}", e)))?;

    let user_list: Vec<UserListItemDto> = users
        .into_iter()
        .map(|user| UserListItemDto {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            has_pin: user.has_pin_on_device.unwrap_or(false),
            last_login: None, // Still don't reveal activity patterns
        })
        .collect();

    Ok(Json(ApiResponse::success(user_list)))
}

/// List all users with full information (authenticated endpoint)
///
/// This endpoint requires authentication and returns complete user information
/// for administrative purposes or authenticated device management.
pub async fn list_users_authenticated_handler(
    State(state): State<AppState>,
    Extension(user): Extension<ferrex_core::user::User>,
    Extension(device_id): Extension<Option<Uuid>>,
) -> AppResult<Json<ApiResponse<Vec<UserListItemDto>>>> {
    // Check if user has permission to list all users
    // TODO: Implement role checking when User has role field
    // For now, only allow users to see themselves (no admin check)
    let is_admin = false; // Will be: user.role == UserRole::Admin;

    let users = if is_admin {
        // Admin gets full user list
        state.database.backend().get_all_users().await?
    } else {
        // Regular users only see themselves
        vec![state
            .database
            .backend()
            .get_user_by_id(user.id)
            .await?
            .ok_or_else(|| AppError::not_found("User not found".to_string()))?]
    };

    // Get device information for PIN status - already extracted as Extension

    let mut user_list = Vec::new();
    for user in users {
        // Check if user has PIN on current device
        let has_pin = if let Some(device_id) = device_id {
            sqlx::query_scalar!(
                "SELECT EXISTS(SELECT 1 FROM auth_device_sessions WHERE user_id = $1 AND id = $2 AND pin_hash IS NOT NULL)",
                user.id,
                device_id
            )
            .fetch_one(get_pool(&state)?)
            .await
            .ok()
            .and_then(|opt| opt)
            .unwrap_or(false)
        } else {
            false
        };

        user_list.push(UserListItemDto {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            has_pin,
            last_login: if is_admin { user.last_login } else { None },
        });
    }

    Ok(Json(ApiResponse::success(user_list)))
}

/// DTO for user list items (used by client)
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct UserListItemDto {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub has_pin: bool,
    pub last_login: Option<chrono::DateTime<chrono::Utc>>,
}

/// Get user profile by ID
pub async fn get_user_handler(
    State(state): State<AppState>,
    Extension(current_user): Extension<User>,
    Path(user_id): Path<Uuid>,
) -> AppResult<Json<User>> {
    // Users can only view their own profile for now
    if current_user.id != user_id {
        return Err(crate::errors::AppError::forbidden(
            "You can only view your own profile",
        ));
    }

    let user = state
        .database
        .backend()
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| crate::errors::AppError::not_found("User not found"))?;

    Ok(Json(user))
}

/// Update user profile
pub async fn update_user_handler(
    State(state): State<AppState>,
    Extension(current_user): Extension<User>,
    Path(user_id): Path<Uuid>,
    Json(request): Json<UserUpdateRequest>,
) -> AppResult<Json<User>> {
    // Users can only update their own profile
    if current_user.id != user_id {
        return Err(crate::errors::AppError::forbidden(
            "You can only update your own profile",
        ));
    }

    // Validate the update request
    request.validate()?;

    // Get current user data
    let mut user = state
        .database
        .backend()
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| crate::errors::AppError::not_found("User not found"))?;

    // Update fields that are provided
    if let Some(ref display_name) = request.display_name {
        user.display_name = display_name.clone();
    }

    // Update password if provided
    if let Some(ref new_password) = request.new_password {
        // Verify current password first
        if let Some(current_password) = request.current_password {
            use argon2::{
                password_hash::{PasswordHash, PasswordVerifier},
                Argon2,
            };

            // Get password hash from credentials table
            let password_hash = state
                .database
                .backend()
                .get_user_password_hash(user.id)
                .await
                .map_err(|_| crate::errors::AppError::internal("Failed to get password hash"))?
                .ok_or_else(|| crate::errors::AppError::bad_request("No password set"))?;

            let parsed_hash = PasswordHash::new(&password_hash)
                .map_err(|_| crate::errors::AppError::internal("Invalid password hash"))?;

            let argon2 = Argon2::default();
            if argon2
                .verify_password(current_password.as_bytes(), &parsed_hash)
                .is_err()
            {
                return Err(crate::errors::AppError::bad_request(
                    "Current password is incorrect",
                ));
            }
        } else {
            return Err(crate::errors::AppError::bad_request(
                "Current password is required to change password",
            ));
        }
    }

    // Use UserService to update the user
    let user_service = UserService::new(&state);
    let updated_user = user_service
        .update_user(
            user_id,
            UpdateUserParams {
                display_name: request.display_name,
                password: request.new_password,
            },
        )
        .await?;

    Ok(Json(updated_user))
}

/// Delete user account
pub async fn delete_user_handler(
    State(state): State<AppState>,
    Extension(current_user): Extension<User>,
    Path(user_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    // Users can only delete their own account
    if current_user.id != user_id {
        return Err(crate::errors::AppError::forbidden(
            "You can only delete your own account",
        ));
    }

    // Use UserService to delete user
    let user_service = UserService::new(&state);
    user_service.delete_user(user_id, current_user.id).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Change password request
#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// Change user password
pub async fn change_password_handler(
    State(state): State<AppState>,
    Extension(current_user): Extension<User>,
    Json(request): Json<ChangePasswordRequest>,
) -> AppResult<StatusCode> {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};

    // Get the user's password hash from the database
    let password_hash = state
        .db
        .backend()
        .get_user_password_hash(current_user.id)
        .await
        .map_err(|_| crate::errors::AppError::internal("Failed to get password hash"))?
        .ok_or_else(|| {
            crate::errors::AppError::bad_request("Cannot change password for user without password")
        })?;

    // Verify current password
    let parsed_hash = PasswordHash::new(&password_hash)
        .map_err(|_| crate::errors::AppError::internal("Invalid password hash"))?;

    let argon2 = Argon2::default();
    if argon2
        .verify_password(request.current_password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return Err(crate::errors::AppError::bad_request(
            "Current password is incorrect",
        ));
    }

    // Update password
    let user_service = UserService::new(&state);
    user_service
        .update_user(
            current_user.id,
            UpdateUserParams {
                display_name: None,
                password: Some(request.new_password),
            },
        )
        .await?;

    info!("User {} changed their password", current_user.username);

    Ok(StatusCode::NO_CONTENT)
}
