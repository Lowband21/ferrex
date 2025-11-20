use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use ferrex_core::{
    api::types::ApiResponse,
    domain::users::{
        auth::domain::services::{
            AuthenticationError, PasswordChangeActor, PasswordChangeRequest,
        },
        user::{User, UserUpdateRequest},
    },
};
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::{
    application::auth::AuthFacadeError,
    infra::{
        app_state::AppState,
        errors::{AppError, AppResult},
    },
    users::{UserService, user_service::UpdateUserParams},
};

/// List all users with full information (authenticated endpoint)
///
/// This endpoint requires authentication and returns complete user information
/// for administrative purposes or authenticated device management.
pub async fn list_users_authenticated_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(device_id): Extension<Option<Uuid>>,
) -> AppResult<Json<ApiResponse<Vec<UserListItemDto>>>> {
    // Check if user has permission to list all users
    // TODO: Implement role checking when User has role field
    // For now, only allow users to see themselves (no admin check)
    let is_admin = false; // Will be: user.role == UserRole::Admin;

    let users = if is_admin {
        // Admin gets full user list
        state.unit_of_work().users.get_all_users().await?
    } else {
        // Regular users only see themselves
        vec![
            state
                .unit_of_work()
                .users
                .get_user_by_id(user.id)
                .await?
                .ok_or_else(|| {
                    AppError::not_found("User not found".to_string())
                })?,
        ]
    };

    // Resolve the current device session once for per-user PIN checks.
    let device_session = if let Some(device_id) = device_id {
        Some(
            state
                .auth_facade()
                .get_device_by_id(device_id)
                .await
                .map_err(map_facade_error)?,
        )
    } else {
        None
    };

    let mut user_list = Vec::new();
    for user in users {
        // Check if user has PIN on current device
        let has_pin = device_session
            .as_ref()
            .filter(|session| session.user_id() == user.id)
            .map(|session| session.has_pin())
            .unwrap_or(false);

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

fn map_facade_error(err: AuthFacadeError) -> AppError {
    match err {
        AuthFacadeError::UserNotFound => {
            AppError::not_found("User not found".to_string())
        }
        AuthFacadeError::Storage(inner) => AppError::from(inner),
        AuthFacadeError::Authentication(inner) => match inner {
            AuthenticationError::InvalidCredentials => {
                AppError::bad_request("Invalid credentials".to_string())
            }
            AuthenticationError::UserNotFound => {
                AppError::not_found("User not found".to_string())
            }
            AuthenticationError::TooManyFailedAttempts => {
                AppError::forbidden("Too many failed attempts".to_string())
            }
            other => AppError::internal(other.to_string()),
        },
        AuthFacadeError::DeviceTrust(inner) => {
            AppError::internal(inner.to_string())
        }
        AuthFacadeError::PinManagement(inner) => {
            AppError::internal(inner.to_string())
        }
    }
}

/// Get user profile by ID
pub async fn get_user_handler(
    State(state): State<AppState>,
    Extension(current_user): Extension<User>,
    Path(user_id): Path<Uuid>,
) -> AppResult<Json<User>> {
    // Users can only view their own profile for now
    if current_user.id != user_id {
        return Err(AppError::forbidden("You can only view your own profile"));
    }

    let user = state
        .unit_of_work()
        .users
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError::not_found("User not found"))?;

    Ok(Json(user))
}

/// Update user profile
pub async fn update_user_handler(
    State(state): State<AppState>,
    Extension(current_user): Extension<User>,
    Path(user_id): Path<Uuid>,
    Json(request): Json<UserUpdateRequest>,
) -> AppResult<Json<User>> {
    request.validate()?;

    let is_self = current_user.id == user_id;
    let actor = if is_self {
        PasswordChangeActor::UserInitiated
    } else {
        let is_admin = state
            .unit_of_work()
            .rbac
            .user_has_role(current_user.id, "admin")
            .await
            .map_err(|e| {
                AppError::internal(format!("Failed to verify admin role: {e}"))
            })?;
        if !is_admin {
            return Err(AppError::forbidden(
                "You do not have permission to modify this user",
            ));
        }
        PasswordChangeActor::AdminInitiated {
            admin_user_id: current_user.id,
        }
    };

    let mut user = state
        .unit_of_work()
        .users
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError::not_found("User not found"))?;

    if let Some(ref display_name) = request.display_name {
        user.display_name = display_name.clone();
    }

    let UserUpdateRequest {
        display_name,
        current_password,
        new_password,
    } = request;

    if let Some(new_password) = new_password {
        let supplied_current = match (&actor, current_password) {
            (PasswordChangeActor::UserInitiated, Some(value)) => Some(value),
            (PasswordChangeActor::UserInitiated, None) => {
                return Err(AppError::bad_request(
                    "Current password is required to change password",
                ));
            }
            (PasswordChangeActor::AdminInitiated { .. }, _) => None,
        };

        state
            .auth_facade()
            .change_password(PasswordChangeRequest {
                user_id,
                new_password,
                current_password: supplied_current,
                actor,
                context: None,
            })
            .await
            .map_err(map_facade_error)?;
    }

    let user_service = UserService::new(&state);
    let updated_user = user_service
        .update_user(
            user_id,
            UpdateUserParams {
                display_name,
                email: None,
                avatar_url: None,
                is_active: None,
                role_ids: None,
                updated_by: current_user.id,
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
        return Err(AppError::forbidden(
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
    state
        .auth_facade()
        .change_password(PasswordChangeRequest {
            user_id: current_user.id,
            new_password: request.new_password,
            current_password: Some(request.current_password),
            actor: PasswordChangeActor::UserInitiated,
            context: None,
        })
        .await
        .map_err(map_facade_error)?;

    info!("User {} changed their password", current_user.username);

    Ok(StatusCode::NO_CONTENT)
}
