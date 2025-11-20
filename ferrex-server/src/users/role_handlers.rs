//! Role and permission management endpoints
//!
//! These endpoints allow administrators to manage roles and permissions
//! in the RBAC system.

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use ferrex_core::{
    api_types::ApiResponse,
    rbac::{OverridePermissionRequest, Permission, Role},
    user::User,
};
use serde::Serialize;
use uuid::Uuid;

use crate::infra::{app_state::AppState, errors::AppResult};

/// Response containing all roles in the system
#[derive(Debug, Serialize)]
pub struct RolesResponse {
    pub roles: Vec<RoleWithPermissions>,
}

/// Role with its associated permissions
#[derive(Debug, Serialize)]
pub struct RoleWithPermissions {
    #[serde(flatten)]
    pub role: Role,
    pub permissions: Vec<String>,
}

/// Get all roles in the system
///
/// Requires: users:manage_roles permission
pub async fn list_roles_handler(
    State(state): State<AppState>,
    Extension(_user): Extension<User>,
) -> AppResult<Json<ApiResponse<RolesResponse>>> {
    // Permission check is handled by middleware

    let roles = state.db.backend().get_all_roles().await?;

    // For each role, get its permissions
    let mut roles_with_perms = Vec::new();
    for role in roles {
        // This is a simplified version - in production you'd want a more efficient query
        let permissions = state
            .db
            .backend()
            .get_all_permissions()
            .await?
            .into_iter()
            .filter(|_| {
                // TODO: Add a method to get permissions for a specific role
                // For now, we'll just return the role without detailed permissions
                false
            })
            .map(|p| p.name)
            .collect();

        roles_with_perms.push(RoleWithPermissions { role, permissions });
    }

    Ok(Json(ApiResponse::success(RolesResponse {
        roles: roles_with_perms,
    })))
}

/// Get all available permissions
///
/// Requires: users:manage_roles permission
pub async fn list_permissions_handler(
    State(state): State<AppState>,
    Extension(_user): Extension<User>,
) -> AppResult<Json<ApiResponse<Vec<Permission>>>> {
    let permissions = state.db.backend().get_all_permissions().await?;

    Ok(Json(ApiResponse::success(permissions)))
}

/// Get a user's effective permissions
///
/// Requires: users:read permission (or requesting own permissions)
pub async fn get_user_permissions_handler(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Extension(current_user): Extension<User>,
) -> AppResult<Json<ApiResponse<ferrex_core::rbac::UserPermissions>>> {
    // Users can always view their own permissions
    // Otherwise requires users:read permission (checked by middleware)
    if current_user.id != user_id {
        // Permission check is handled by middleware
    }

    let permissions = state.db.backend().get_user_permissions(user_id).await?;

    Ok(Json(ApiResponse::success(permissions)))
}

/// Assign roles to a user
///
/// Requires: users:manage_roles permission
pub async fn assign_user_roles_handler(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Extension(admin): Extension<User>,
    Json(request): Json<Vec<Uuid>>,
) -> AppResult<Json<ApiResponse<()>>> {
    // First, remove all existing roles
    let current_roles = state
        .db
        .backend()
        .get_user_permissions(user_id)
        .await?
        .roles;

    for role in current_roles {
        state
            .db
            .backend()
            .remove_user_role(user_id, role.id)
            .await?;
    }

    // Then assign the new roles
    for role_id in request {
        state
            .db
            .backend()
            .assign_user_role(user_id, role_id, admin.id)
            .await?;
    }

    Ok(Json(ApiResponse::success(())))
}

/// Override a specific permission for a user
///
/// Requires: users:manage_roles permission
pub async fn override_user_permission_handler(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Extension(admin): Extension<User>,
    Json(request): Json<OverridePermissionRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    state
        .db
        .backend()
        .override_user_permission(
            user_id,
            &request.permission,
            request.granted,
            admin.id,
            request.reason,
        )
        .await?;

    Ok(Json(ApiResponse::success(())))
}

/// Get current user's permissions (convenience endpoint)
///
/// No additional permissions required - users can always check their own
pub async fn get_my_permissions_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> AppResult<Json<ApiResponse<ferrex_core::rbac::UserPermissions>>> {
    let permissions = state.db.backend().get_user_permissions(user.id).await?;

    Ok(Json(ApiResponse::success(permissions)))
}
