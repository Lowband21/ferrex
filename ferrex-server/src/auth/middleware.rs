use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension,
};
use ferrex_core::user::User;
use ferrex_core::rbac::UserPermissions;
use ferrex_core::api_types::ApiResponse;

use super::jwt::validate_token;
use ferrex_core::database::postgres::PostgresDatabase;
use crate::AppState;

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = extract_bearer_token(&request)?;
    let user = validate_and_get_user(&state, &token).await?;
    
    // Load user permissions
    let permissions = state.database.backend()
        .get_user_permissions(user.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    request.extensions_mut().insert(user);
    request.extensions_mut().insert(permissions);
    Ok(next.run(request).await)
}

pub async fn optional_auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Ok(token) = extract_bearer_token(&request) {
        if let Ok(user) = validate_and_get_user(&state, &token).await {
            // Also load permissions when user is authenticated
            if let Ok(permissions) = state.database.backend()
                .get_user_permissions(user.id)
                .await {
                request.extensions_mut().insert(permissions);
            }
            request.extensions_mut().insert(user);
        }
    }
    
    next.run(request).await
}

/// Middleware that ensures the user is authenticated and has admin privileges
/// This middleware must be run AFTER auth_middleware in the layer stack
/// DEPRECATED: Use require_permission from permission_middleware instead
pub async fn admin_middleware(
    request: Request,
    next: Next,
) -> Response {
    // Extract the user from extensions (set by auth_middleware)
    let user = match request.extensions().get::<User>() {
        Some(user) => user,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(ApiResponse::<()>::error("Authentication required".to_string())),
            ).into_response();
        }
    };
    
    // Extract permissions from extensions (set by auth_middleware)
    let permissions = match request.extensions().get::<UserPermissions>() {
        Some(perms) => perms,
        None => {
            return (
                StatusCode::FORBIDDEN,
                axum::Json(ApiResponse::<()>::error("Permission system not initialized".to_string())),
            ).into_response();
        }
    };
    
    // Check if user has admin role or all user management permissions
    if !permissions.has_role("admin") && 
       !permissions.has_all_permissions(&[
           "users:read",
           "users:create", 
           "users:update",
           "users:delete",
           "users:manage_roles"
       ]) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<()>::error("Admin access required".to_string())),
        ).into_response();
    }
    
    next.run(request).await
}

fn extract_bearer_token(request: &Request) -> Result<String, StatusCode> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !auth_header.starts_with("Bearer ") {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(auth_header[7..].to_string())
}

async fn validate_and_get_user(
    state: &AppState,
    token: &str,
) -> Result<User, StatusCode> {
    // First try to validate as a session token
    if let Ok(user) = validate_session_token(state, token).await {
        return Ok(user);
    }
    
    // Fall back to JWT validation with revocation check
    let pool = if let Some(pg_db) = state.db.as_any().downcast_ref::<PostgresDatabase>() {
        pg_db.pool()
    } else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    
    let claims = validate_token(token, pool)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    
    state
        .db
        .backend()
        .get_user_by_id(claims.sub)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .ok_or(StatusCode::UNAUTHORIZED)
}

async fn validate_session_token(
    state: &AppState,
    token: &str,
) -> Result<User, StatusCode> {
    use sha2::{Sha256, Digest};
    use chrono::Utc;
    
    // Hash the token
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let token_hash = format!("{:x}", hasher.finalize());
    
    // Access the PostgresDatabase pool directly 
    use ferrex_core::database::postgres::PostgresDatabase;
    let pool = if let Some(pg_db) = state.db.as_any().downcast_ref::<PostgresDatabase>() {
        pg_db.pool()
    } else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    
    // Query the sessions table
    let session_row = sqlx::query!(
        r#"
        SELECT user_id, expires_at, revoked
        FROM sessions
        WHERE token_hash = $1
        "#,
        token_hash
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Check if session exists
    let session_row = match session_row {
        Some(row) => row,
        None => return Err(StatusCode::UNAUTHORIZED),
    };
    
    // Check if session is valid
    if session_row.revoked {
        return Err(StatusCode::UNAUTHORIZED);
    }
    
    if session_row.expires_at < Utc::now() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    
    // Get the user
    state
        .db
        .backend()
        .get_user_by_id(session_row.user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)
}