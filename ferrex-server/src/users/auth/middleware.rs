use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use ferrex_core::api_types::ApiResponse;
use ferrex_core::rbac::UserPermissions;
use ferrex_core::user::User;
use uuid::Uuid;

use super::jwt::validate_token;
use crate::infra::app_state::AppState;
use ferrex_core::database::postgres::PostgresDatabase;

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = extract_bearer_token(&request)?;
    let (user, device_id) = validate_and_get_user(&state, &token).await?;

    // Load user permissions
    let permissions = state
        .db
        .backend()
        .get_user_permissions(user.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    request.extensions_mut().insert(user);
    request.extensions_mut().insert(permissions);
    request.extensions_mut().insert(device_id); // Add device_id as Option<Uuid>
    Ok(next.run(request).await)
}

pub async fn optional_auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Ok(token) = extract_bearer_token(&request)
        && let Ok((user, device_id)) = validate_and_get_user(&state, &token).await
    {
        // Also load permissions when user is authenticated
        if let Ok(permissions) = state.db.backend().get_user_permissions(user.id).await {
            request.extensions_mut().insert(permissions);
        }
        request.extensions_mut().insert(user);
        request.extensions_mut().insert(device_id); // Add device_id as Option<Uuid>
    }

    next.run(request).await
}

/// Middleware that ensures the user is authenticated and has admin privileges
/// This middleware must be run AFTER auth_middleware in the layer stack
/// DEPRECATED: Use require_permission from permission_middleware instead
pub async fn admin_middleware(request: Request, next: Next) -> Response {
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

    // Extract permissions from extensions (set by auth_middleware)
    let permissions = match request.extensions().get::<UserPermissions>() {
        Some(perms) => perms,
        None => {
            return (
                StatusCode::FORBIDDEN,
                axum::Json(ApiResponse::<()>::error(
                    "Permission system not initialized".to_string(),
                )),
            )
                .into_response();
        }
    };

    // Check if user has admin role or all user management permissions
    if !permissions.has_role("admin")
        && !permissions.has_all_permissions(&[
            "users:read",
            "users:create",
            "users:update",
            "users:delete",
            "users:manage_roles",
        ])
    {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<()>::error(
                "Admin access required".to_string(),
            )),
        )
            .into_response();
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
) -> Result<(User, Option<Uuid>), StatusCode> {
    // First try to validate as a session token
    if let Ok((user, device_id)) = validate_session_token(state, token).await {
        return Ok((user, device_id));
    }

    // Fall back to JWT validation with revocation check (no device_id for JWT)
    let pool = if let Some(pg_db) = state.db.as_any().downcast_ref::<PostgresDatabase>() {
        pg_db.pool()
    } else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    let claims = validate_token(token, pool)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let user = state
        .db
        .backend()
        .get_user_by_id(claims.sub)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    Ok((user, None)) // JWT tokens don't have device_id
}

async fn validate_session_token(
    state: &AppState,
    token: &str,
) -> Result<(User, Option<Uuid>), StatusCode> {
    use chrono::Utc;
    use sha2::{Digest, Sha256};

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

    // Query the sessions table including device_id
    let session_row = sqlx::query!(
        r#"
        SELECT user_id, device_id, expires_at, revoked
        FROM sessions
        WHERE token_hash = $1
        "#,
        token_hash
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!("Database error validating session token: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check if session is revoked
    if session_row.revoked {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Check if session is expired
    if session_row.expires_at < Utc::now() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Get the user
    let user = state
        .db
        .backend()
        .get_user_by_id(session_row.user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Update last activity (fire-and-forget)
    let pool_clone = pool.clone();
    let token_hash_clone = token_hash.clone();
    tokio::spawn(async move {
        let _ = sqlx::query!(
            "UPDATE sessions SET last_activity = NOW() WHERE token_hash = $1",
            token_hash_clone
        )
        .execute(&pool_clone)
        .await;
    });

    Ok((user, Some(session_row.device_id)))
}
