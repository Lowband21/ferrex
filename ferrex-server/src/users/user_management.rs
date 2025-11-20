//! Administrative user management endpoints.

use axum::http::StatusCode;
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use ferrex_core::{
    api_types::ApiResponse,
    auth::domain::services::{AuthenticationError, PasswordChangeActor, PasswordChangeRequest},
    rbac::{self, UserPermissions},
    user::User,
    user_management::{ListUsersOptions, UserAdminRecord},
};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::users::{CreateUserParams, UpdateUserParams, UserService};
use crate::{
    application::auth::AuthFacadeError,
    infra::{
        app_state::AppState,
        errors::{AppError, AppResult},
    },
};

#[derive(Debug, Deserialize)]
pub struct ListUsersQuery {
    pub role: Option<String>,
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    #[serde(default)]
    pub include_inactive: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub display_name: String,
    pub password: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub role_ids: Vec<Uuid>,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub is_active: Option<bool>,
    pub role_ids: Option<Vec<Uuid>>,
    pub new_password: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_login: Option<i64>,
    pub is_active: bool,
    pub roles: Vec<String>,
    pub session_count: usize,
}

#[derive(Debug, Serialize)]
pub struct UserListResponse {
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub users: Vec<UserResponse>,
}

fn default_true() -> bool {
    true
}

pub async fn list_users(
    State(state): State<AppState>,
    Extension(_current_user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Query(query): Query<ListUsersQuery>,
) -> AppResult<Json<ApiResponse<UserListResponse>>> {
    require_permission(&permissions, &[rbac::permissions::USERS_READ])?;

    let options = ListUsersOptions {
        role: query.role.clone(),
        search: query.search.clone(),
        include_inactive: query.include_inactive,
        limit: query.limit,
        offset: query.offset,
    };

    let service = UserService::new(&state);
    let paged = service.list_users(options).await?;

    let users = paged
        .users
        .into_iter()
        .map(user_record_to_response)
        .collect();

    let payload = UserListResponse {
        total: paged.total,
        limit: paged.limit,
        offset: paged.offset,
        users,
    };

    Ok(Json(ApiResponse::success(payload)))
}

pub async fn create_user(
    State(state): State<AppState>,
    Extension(current_user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Json(request): Json<CreateUserRequest>,
) -> AppResult<Json<ApiResponse<UserResponse>>> {
    require_permission(&permissions, &[rbac::permissions::USERS_CREATE])?;

    let service = UserService::new(&state);
    let user = service
        .create_user(CreateUserParams {
            username: request.username,
            display_name: request.display_name,
            password: request.password,
            email: request.email,
            avatar_url: request.avatar_url,
            role_ids: request.role_ids,
            is_active: request.is_active,
            created_by: Some(current_user.id),
        })
        .await?;

    let roles = state
        .unit_of_work
        .rbac
        .get_user_permissions(user.id)
        .await?
        .roles
        .into_iter()
        .map(|role| role.name)
        .collect::<Vec<_>>();

    let response = UserResponse {
        id: user.id,
        username: user.username.clone(),
        display_name: user.display_name.clone(),
        email: user.email.clone(),
        avatar_url: user.avatar_url.clone(),
        created_at: user.created_at.timestamp(),
        updated_at: user.updated_at.timestamp(),
        last_login: user.last_login.map(|ts| ts.timestamp()),
        is_active: user.is_active,
        roles,
        session_count: 0,
    };

    info!(
        target: "user.admin",
        user_id = %user.id,
        username = %user.username,
        action = "create"
    );

    Ok(Json(ApiResponse::success(response)))
}

pub async fn update_user(
    State(state): State<AppState>,
    Extension(current_user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(user_id): Path<Uuid>,
    Json(request): Json<UpdateUserRequest>,
) -> AppResult<Json<ApiResponse<UserResponse>>> {
    require_permission(&permissions, &[rbac::permissions::USERS_UPDATE])?;

    let service = UserService::new(&state);
    let user = service
        .update_user(
            user_id,
            UpdateUserParams {
                display_name: request.display_name.clone(),
                email: request.email.clone(),
                avatar_url: request.avatar_url.clone(),
                is_active: request.is_active,
                role_ids: request.role_ids.clone(),
                updated_by: current_user.id,
            },
        )
        .await?;

    if let Some(new_password) = request.new_password.as_ref() {
        state
            .auth_facade
            .change_password(PasswordChangeRequest {
                user_id,
                new_password: new_password.clone(),
                current_password: None,
                actor: PasswordChangeActor::AdminInitiated {
                    admin_user_id: current_user.id,
                },
                context: None,
            })
            .await
            .map_err(map_auth_facade_error)?;

        info!(
            target: "user.admin",
            user_id = %user_id,
            actor = %current_user.id,
            action = "reset-password"
        );
    }

    let permissions = state
        .unit_of_work
        .rbac
        .get_user_permissions(user.id)
        .await?;

    let sessions = state
        .auth_facade
        .list_user_sessions(user.id)
        .await
        .map_err(map_auth_facade_error)?;

    let response = UserResponse {
        id: user.id,
        username: user.username.clone(),
        display_name: user.display_name.clone(),
        email: user.email.clone(),
        avatar_url: user.avatar_url.clone(),
        created_at: user.created_at.timestamp(),
        updated_at: user.updated_at.timestamp(),
        last_login: user.last_login.map(|ts| ts.timestamp()),
        is_active: user.is_active,
        roles: permissions
            .roles
            .into_iter()
            .map(|role| role.name)
            .collect(),
        session_count: sessions.len(),
    };

    info!(
        target: "user.admin",
        user_id = %user.id,
        username = %user.username,
        action = "update"
    );

    Ok(Json(ApiResponse::success(response)))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Extension(current_user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(user_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    require_permission(&permissions, &[rbac::permissions::USERS_DELETE])?;

    if current_user.id == user_id {
        return Err(AppError::bad_request(
            "Administrators cannot delete their own account",
        ));
    }

    let service = UserService::new(&state);
    service.delete_user(user_id, current_user.id).await?;

    info!(
        target: "user.admin",
        user_id = %user_id,
        actor = %current_user.id,
        action = "delete"
    );

    Ok(StatusCode::NO_CONTENT)
}

pub fn map_auth_facade_error(err: AuthFacadeError) -> AppError {
    match err {
        AuthFacadeError::UserNotFound => AppError::not_found("User not found"),
        AuthFacadeError::Storage(inner) => AppError::from(inner),
        AuthFacadeError::Authentication(inner) => match inner {
            AuthenticationError::InvalidCredentials => AppError::bad_request("Invalid credentials"),
            AuthenticationError::UserNotFound => AppError::not_found("User not found"),
            AuthenticationError::TooManyFailedAttempts => {
                AppError::forbidden("Too many failed attempts")
            }
            other => AppError::internal(other.to_string()),
        },
        AuthFacadeError::DeviceTrust(inner) => AppError::internal(inner.to_string()),
        AuthFacadeError::PinManagement(inner) => AppError::internal(inner.to_string()),
    }
}

fn user_record_to_response(record: UserAdminRecord) -> UserResponse {
    UserResponse {
        id: record.user.id,
        username: record.user.username.clone(),
        display_name: record.user.display_name.clone(),
        email: record.user.email.clone(),
        avatar_url: record.user.avatar_url.clone(),
        created_at: record.user.created_at.timestamp(),
        updated_at: record.user.updated_at.timestamp(),
        last_login: record.user.last_login.map(|dt| dt.timestamp()),
        is_active: record.user.is_active,
        roles: record.roles.into_iter().map(|role| role.name).collect(),
        session_count: record.session_count,
    }
}

fn require_permission(perms: &UserPermissions, required: &[&str]) -> AppResult<()> {
    if perms.has_role("admin") || perms.has_all_permissions(required) {
        Ok(())
    } else {
        Err(AppError::forbidden("Insufficient permissions"))
    }
}
