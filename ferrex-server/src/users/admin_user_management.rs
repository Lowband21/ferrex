//! Admin-only user management endpoints.
//!
//! Thin wrappers over the shared `UserService` so we avoid business logic
//! duplication while providing a clearly separated admin API surface.

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use ferrex_core::{
    api_types::ApiResponse,
    rbac::UserPermissions,
    user::User,
};
use sqlx::types::ipnetwork::IpNetwork;
use serde_json::json;
use uuid::Uuid;

use crate::infra::app_state::AppState;
use crate::infra::errors::{AppError, AppResult};
use crate::users::admin_handlers::AdminUserInfo;
use crate::users::user_management::{CreateUserRequest, UpdateUserRequest, map_auth_facade_error};
use crate::users::{CreateUserParams, UpdateUserParams, UserService};

/// Create a user via the admin API.
pub async fn admin_create_user(
    State(state): State<AppState>,
    Extension(admin): Extension<User>,
    // Admin permissions are enforced by route layer middleware; keep signature to make it explicit.
    Extension(_perms): Extension<UserPermissions>,
    Json(request): Json<CreateUserRequest>,
) -> AppResult<Json<ApiResponse<AdminUserInfo>>> {
    let service = UserService::new(&state);
    let created = service
        .create_user(CreateUserParams {
            username: request.username,
            display_name: request.display_name,
            password: request.password,
            email: request.email,
            avatar_url: request.avatar_url,
            role_ids: request.role_ids,
            is_active: request.is_active,
            created_by: Some(admin.id),
        })
        .await?;

    // Resolve roles and sessions
    let permissions = state
        .unit_of_work
        .rbac
        .get_user_permissions(created.id)
        .await?;
    let role_names: Vec<String> = permissions.roles.into_iter().map(|r| r.name).collect();

    let sessions = state
        .auth_facade
        .list_user_sessions(created.id)
        .await
        .map_err(map_auth_facade_error)?;

    // Record admin action + security event
    record_admin_action(
        &state,
        admin.id,
        "user.create",
        Some("user"),
        Some(created.id),
        Some(&format!("Created user {}", created.username)),
        Some(json!({
            "username": created.username,
            "display_name": created.display_name,
            "role_names": role_names,
        })),
    )
    .await?;
    record_security_event(
        &state,
        Some(created.id),
        None,
        "user_created",
        true,
        None,
        Some(json!({"admin_id": admin.id})),
    )
    .await?;

    let dto = AdminUserInfo {
        id: created.id,
        username: created.username,
        display_name: created.display_name,
        roles: role_names,
        created_at: created.created_at.timestamp(),
        session_count: sessions.len() as i64,
    };

    Ok(Json(ApiResponse::success(dto)))
}

/// Update a user via the admin API.
pub async fn admin_update_user(
    State(state): State<AppState>,
    Extension(admin): Extension<User>,
    Extension(_perms): Extension<UserPermissions>,
    Path(user_id): Path<Uuid>,
    Json(request): Json<UpdateUserRequest>,
) -> AppResult<Json<ApiResponse<AdminUserInfo>>> {
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
                updated_by: admin.id,
            },
        )
        .await?;

    if let Some(new_password) = request.new_password.as_ref() {
        state
            .auth_facade
            .change_password(ferrex_core::auth::domain::services::PasswordChangeRequest {
                user_id,
                new_password: new_password.clone(),
                current_password: None,
                actor: ferrex_core::auth::domain::services::PasswordChangeActor::AdminInitiated {
                    admin_user_id: admin.id,
                },
                context: None,
            })
            .await
            .map_err(map_auth_facade_error)?;

        // Audit password change
        record_security_event(
            &state,
            Some(user_id),
            None,
            "password_changed",
            true,
            None,
            Some(json!({"admin_id": admin.id})),
        )
        .await?;
    }

    let permissions = state
        .unit_of_work
        .rbac
        .get_user_permissions(user.id)
        .await?;
    let role_names: Vec<String> = permissions.roles.into_iter().map(|r| r.name).collect();
    let sessions = state
        .auth_facade
        .list_user_sessions(user.id)
        .await
        .map_err(map_auth_facade_error)?;

    // Record admin action + security event
    record_admin_action(
        &state,
        admin.id,
        "user.update",
        Some("user"),
        Some(user.id),
        Some(&format!("Updated user {}", user.username)),
        Some(json!({
            "display_name": request.display_name,
            "email": request.email,
            "avatar_url": request.avatar_url,
            "is_active": request.is_active,
            "role_ids": request.role_ids,
        })),
    )
    .await?;
    record_security_event(
        &state,
        Some(user.id),
        None,
        "user_updated",
        true,
        None,
        Some(json!({"admin_id": admin.id})),
    )
    .await?;

    let dto = AdminUserInfo {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        roles: role_names,
        created_at: user.created_at.timestamp(),
        session_count: sessions.len() as i64,
    };

    Ok(Json(ApiResponse::success(dto)))
}

/// Delete a user via the admin API.
pub async fn admin_delete_user(
    State(state): State<AppState>,
    Extension(admin): Extension<User>,
    Extension(_perms): Extension<UserPermissions>,
    Path(user_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    if admin.id == user_id {
        return Err(AppError::bad_request("Cannot delete your own account"));
    }

    // Forbid deleting users that currently have the admin role (policy parity with existing handler)
    let user_perms = state
        .unit_of_work
        .rbac
        .get_user_permissions(user_id)
        .await?;
    if user_perms.has_role("admin") {
        return Err(AppError::forbidden("Cannot delete users with admin role"));
    }

    let service = UserService::new(&state);
    service.delete_user(user_id, admin.id).await?;

    // Record admin action + security event
    record_admin_action(
        &state,
        admin.id,
        "user.delete",
        Some("user"),
        Some(user_id),
        Some("Deleted user"),
        None,
    )
    .await?;
    record_security_event(
        &state,
        Some(user_id),
        None,
        "user_deleted",
        true,
        None,
        Some(json!({"admin_id": admin.id})),
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn record_admin_action(
    state: &AppState,
    admin_id: Uuid,
    action_type: &str,
    target_type: Option<&str>,
    target_id: Option<Uuid>,
    description: Option<&str>,
    metadata: Option<serde_json::Value>,
) -> Result<(), AppError> {
    let ip: Option<IpNetwork> = None;

    sqlx::query!(
        r#"INSERT INTO admin_actions
        (admin_id, action_type, target_type, target_id, description, metadata, ip_address)
        VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        admin_id,
        action_type,
        target_type,
        target_id,
        description,
        metadata,
        ip
    )
    .execute(state.postgres.pool())
    .await
    .map_err(AppError::from)?;
    Ok(())
}

async fn record_security_event(
    state: &AppState,
    user_id: Option<Uuid>,
    device_session_id: Option<Uuid>,
    event_type: &str,
    success: bool,
    error_message: Option<&str>,
    event_data: Option<serde_json::Value>,
) -> Result<(), AppError> {
    let ip: Option<IpNetwork> = None;
    let ua: Option<&str> = None;
    let request_id: Option<Uuid> = None;
    let severity = "info";

    sqlx::query!(
        r#"INSERT INTO security_audit_log
        (user_id, device_session_id, event_type, severity, event_data, ip_address, user_agent, request_id, success, error_message)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
        user_id,
        device_session_id,
        event_type,
        severity,
        event_data,
        ip,
        ua,
        request_id,
        success,
        error_message
    )
    .execute(state.postgres.pool())
    .await
    .map_err(AppError::from)?;
    Ok(())
}
