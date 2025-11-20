use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use ferrex_core::{api_types::ApiResponse, rbac::UserPermissions, user::User};
use std::future::Future;
use std::pin::Pin;

use crate::infra::app_state::AppState;

/// Middleware that checks if the user has a specific permission
/// This should be run AFTER auth_middleware which sets the User extension
pub fn require_permission(
    permission: &'static str,
) -> impl Fn(Request, Next) -> Pin<Box<dyn Future<Output = Response> + Send>>
+ Clone
+ Send
+ Sync
+ 'static {
    move |request: Request, next: Next| Box::pin(check_permission_async(request, next, permission))
}

async fn check_permission_async(request: Request, next: Next, permission: &str) -> Response {
    // Extract the user from extensions (set by auth_middleware)
    let user = match request.extensions().get::<User>() {
        Some(user) => user,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(ApiResponse::<()>::error(
                    "Authentication required".to_string(),
                )),
            )
                .into_response();
        }
    };

    // Extract permissions from extensions (should be set by auth_middleware)
    let permissions = match request.extensions().get::<UserPermissions>() {
        Some(perms) => perms,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(ApiResponse::<()>::error(
                    "Permissions not loaded".to_string(),
                )),
            )
                .into_response();
        }
    };

    // Check if user has the required permission
    if !permissions.has_permission(permission) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<()>::error(format!(
                "Permission '{}' required",
                permission
            ))),
        )
            .into_response();
    }

    // User has permission, continue
    next.run(request).await
}

/// Middleware that checks if the user has any of the specified permissions
pub fn require_any_permission(
    permissions: &'static [&'static str],
) -> impl Fn(Request, Next) -> Pin<Box<dyn Future<Output = Response> + Send>>
+ Clone
+ Send
+ Sync
+ 'static {
    move |request: Request, next: Next| {
        Box::pin(check_any_permission_async(request, next, permissions))
    }
}

async fn check_any_permission_async(
    request: Request,
    next: Next,
    permissions: &[&str],
) -> Response {
    // Extract the user from extensions (set by auth_middleware)
    let user = match request.extensions().get::<User>() {
        Some(user) => user,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(ApiResponse::<()>::error(
                    "Authentication required".to_string(),
                )),
            )
                .into_response();
        }
    };

    // Extract permissions from extensions (should be set by auth_middleware)
    let user_permissions = match request.extensions().get::<UserPermissions>() {
        Some(perms) => perms,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(ApiResponse::<()>::error(
                    "Permissions not loaded".to_string(),
                )),
            )
                .into_response();
        }
    };

    // Check if user has any of the required permissions
    if !user_permissions.has_any_permission(permissions) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<()>::error(format!(
                "One of these permissions required: {}",
                permissions.join(", ")
            ))),
        )
            .into_response();
    }

    // User has permission, continue
    next.run(request).await
}

/// Async version of permission middleware for use with from_fn_with_state
pub async fn require_permission_async(
    permission: &'static str,
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // Extract the user from extensions (set by auth_middleware)
    let user = match request.extensions().get::<User>() {
        Some(user) => user.clone(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(ApiResponse::<()>::error(
                    "Authentication required".to_string(),
                )),
            )
                .into_response();
        }
    };

    // Load permissions if not already loaded
    let permissions = if let Some(perms) = request.extensions().get::<UserPermissions>() {
        perms.clone()
    } else {
        // Load permissions from database
        match state.db.backend().get_user_permissions(user.id).await {
            Ok(perms) => {
                request.extensions_mut().insert(perms.clone());
                perms
            }
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(ApiResponse::<()>::error(
                        "Failed to load permissions".to_string(),
                    )),
                )
                    .into_response();
            }
        }
    };

    // Check if user has the required permission
    if !permissions.has_permission(permission) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<()>::error(format!(
                "Permission '{}' required",
                permission
            ))),
        )
            .into_response();
    }

    next.run(request).await
}

/// Create a middleware function that requires a specific permission
pub fn permission_layer(
    permission: &'static str,
) -> impl Fn(Request, Next) -> Pin<Box<dyn Future<Output = Response> + Send>>
+ Clone
+ Send
+ Sync
+ 'static {
    require_permission(permission)
}
