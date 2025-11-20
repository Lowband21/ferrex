use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use ferrex_core::{api_types::ApiResponse, user::User};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{errors::AppResult, users::role_handlers, AppState};

/// Request to assign roles to a user
#[derive(Debug, Deserialize)]
pub struct AssignRolesRequest {
    pub role_ids: Vec<Uuid>,
}

/// Request to delete a user
#[derive(Debug, Deserialize)]
pub struct DeleteUserRequest {
    pub user_id: Uuid,
}

/// Response for user list with role info
#[derive(Debug, Serialize)]
pub struct AdminUserInfo {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub roles: Vec<String>, // Role names
    pub created_at: i64,
    pub session_count: i64,
}

/// Query parameters for filtering users
#[derive(Debug, Deserialize)]
pub struct UserFilters {
    pub role: Option<String>, // Filter by role name
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// List all users (admin only)
pub async fn list_all_users(
    State(state): State<AppState>,
    Extension(admin): Extension<User>, // Already validated by admin_middleware
    Query(filters): Query<UserFilters>,
) -> AppResult<Json<ApiResponse<Vec<AdminUserInfo>>>> {
    // Get all users from database
    let mut users = state.database.backend().get_all_users().await?;

    // We'll filter by role after fetching role info

    if let Some(search) = &filters.search {
        let search_lower = search.to_lowercase();
        users.retain(|u| {
            u.username.to_lowercase().contains(&search_lower)
                || u.display_name.to_lowercase().contains(&search_lower)
        });
    }

    // Convert to AdminUserInfo with session counts and roles
    let mut admin_users = Vec::new();
    for user in users {
        let sessions = state.database.backend().get_user_sessions(user.id).await?;
        let permissions = state
            .database
            .backend()
            .get_user_permissions(user.id)
            .await?;
        let role_names: Vec<String> = permissions.roles.into_iter().map(|r| r.name).collect();

        // Apply role filter if specified
        if let Some(ref role_filter) = filters.role {
            if !role_names.contains(role_filter) {
                continue;
            }
        }

        admin_users.push(AdminUserInfo {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            roles: role_names,
            created_at: user.created_at.timestamp(),
            session_count: sessions.len() as i64,
        });
    }

    // Apply pagination
    let offset = filters.offset.unwrap_or(0) as usize;
    let limit = filters.limit.unwrap_or(100).min(1000) as usize;

    admin_users.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let paginated: Vec<_> = admin_users.into_iter().skip(offset).take(limit).collect();

    Ok(Json(ApiResponse::success(paginated)))
}

/// Assign roles to a user (admin only)
pub async fn assign_user_roles(
    State(state): State<AppState>,
    Extension(admin): Extension<User>,
    Path(user_id): Path<Uuid>,
    Json(request): Json<AssignRolesRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    // Get admin role ID
    let admin_role_id =
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("Invalid admin role UUID");

    // Prevent admin from removing their own admin role
    if admin.id == user_id && !request.role_ids.contains(&admin_role_id) {
        let current_perms = state
            .database
            .backend()
            .get_user_permissions(user_id)
            .await?;
        if current_perms.has_role("admin") {
            return Err(crate::errors::AppError::bad_request(
                "Cannot remove your own admin role",
            ));
        }
    }

    // Verify user exists
    let user = state
        .database
        .backend()
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| crate::errors::AppError::not_found("User not found"))?;

    // Use the role assignment endpoint from role_handlers

    let handler_state = state.clone();
    let result = role_handlers::assign_user_roles_handler(
        State(handler_state),
        Path(user_id),
        Extension(admin.clone()),
        Json(request.role_ids),
    )
    .await?;

    // Log admin action
    tracing::info!(
        "Admin {} ({}) updated roles for user {} ({})",
        admin.username,
        admin.id,
        user.username,
        user.id
    );

    Ok(Json(ApiResponse::success(())))
}

/// Delete a user (admin only)
pub async fn delete_user_admin(
    State(state): State<AppState>,
    Extension(admin): Extension<User>,
    Path(user_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    // Prevent admin from deleting themselves
    if admin.id == user_id {
        return Err(crate::errors::AppError::bad_request(
            "Cannot delete your own account",
        ));
    }

    // Check if user exists
    let user = state
        .database
        .backend()
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| crate::errors::AppError::not_found("User not found"))?;

    // Check if user has admin role
    let user_perms = state
        .database
        .backend()
        .get_user_permissions(user_id)
        .await?;
    if user_perms.has_role("admin") {
        return Err(crate::errors::AppError::forbidden(
            "Cannot delete users with admin role",
        ));
    }

    // Delete the user
    state.database.backend().delete_user(user_id).await?;

    // Log admin action
    tracing::info!(
        "Admin {} ({}) deleted user {} ({})",
        admin.username,
        admin.id,
        user.username,
        user.id
    );

    Ok(StatusCode::NO_CONTENT)
}

/// Get user sessions (admin only)
pub async fn get_user_sessions_admin(
    State(state): State<AppState>,
    Extension(admin): Extension<User>,
    Path(user_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<Vec<ferrex_core::user::UserSession>>>> {
    // Verify user exists
    let user = state
        .database
        .backend()
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| crate::errors::AppError::not_found("User not found"))?;

    // Get user sessions
    let sessions = state.database.backend().get_user_sessions(user_id).await?;

    Ok(Json(ApiResponse::success(sessions)))
}

/// Revoke a user session (admin only)
pub async fn revoke_user_session_admin(
    State(state): State<AppState>,
    Extension(admin): Extension<User>,
    Path((user_id, session_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    // Delete the session
    state.database.backend().delete_session(session_id).await?;

    // Log admin action
    tracing::info!(
        "Admin {} ({}) revoked session {} for user {}",
        admin.username,
        admin.id,
        session_id,
        user_id
    );

    Ok(StatusCode::NO_CONTENT)
}

/// Get admin dashboard statistics
#[derive(Debug, Serialize)]
pub struct AdminStats {
    pub total_users: i64,
    pub admin_users: i64,
    pub active_sessions: i64,
    pub total_libraries: i64,
    pub total_movies: i64,
    pub total_tv_shows: i64,
    pub total_episodes: i64,
}

pub async fn get_admin_stats(
    State(state): State<AppState>,
    Extension(admin): Extension<User>,
) -> AppResult<Json<ApiResponse<AdminStats>>> {
    // Get user stats
    let users = state.database.backend().get_all_users().await?;
    let total_users = users.len() as i64;

    // Count users with admin role
    let mut admin_users = 0i64;
    for user in &users {
        let perms = state
            .database
            .backend()
            .get_user_permissions(user.id)
            .await?;
        if perms.has_role("admin") {
            admin_users += 1;
        }
    }

    // Get session count (simplified - in production you'd want a dedicated query)
    let mut active_sessions = 0;
    for user in &users {
        let sessions = state.database.backend().get_user_sessions(user.id).await?;
        active_sessions += sessions.len() as i64;
    }

    // Get library stats
    let libraries = state.database.backend().list_libraries().await?;
    let total_libraries = libraries.len() as i64;

    // Get media stats (simplified - in production you'd want dedicated queries)
    let stats = AdminStats {
        total_users,
        admin_users,
        active_sessions,
        total_libraries,
        total_movies: 0, // TODO: Implement actual counts
        total_tv_shows: 0,
        total_episodes: 0,
    };

    Ok(Json(ApiResponse::success(stats)))
}
