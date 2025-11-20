use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use ferrex_core::{
    api_types::ApiResponse,
    user::{User, UserUpdateRequest},
};
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::{
    errors::AppResult,
    services::{UserService, user_service::UpdateUserParams},
    AppState,
};

/// List all users (public endpoint for user selection screen)
pub async fn list_users_handler(
    State(state): State<AppState>,
) -> AppResult<Json<ApiResponse<Vec<UserListItemDto>>>> {
    // Get all users from database
    let users = state.database.backend().get_all_users().await?;
    
    // Convert to UserListItemDto for the client
    let user_list: Vec<UserListItemDto> = users.into_iter()
        .map(|user| UserListItemDto {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            has_pin: false, // TODO: Check if user has PIN on any device
            last_login: user.last_login,
        })
        .collect();
    
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
    let updated_user = user_service.update_user(
        user_id,
        UpdateUserParams {
            display_name: request.display_name,
            password: request.new_password,
        },
    ).await?;
    
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
        .ok_or_else(|| crate::errors::AppError::bad_request(
            "Cannot change password for user without password"
        ))?;
    
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
    user_service.update_user(
        current_user.id,
        UpdateUserParams {
            display_name: None,
            password: Some(request.new_password),
        },
    ).await?;
    
    info!("User {} changed their password", current_user.username);
    
    Ok(StatusCode::NO_CONTENT)
}