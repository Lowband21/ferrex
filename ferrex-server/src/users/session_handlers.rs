use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use ferrex_core::{
    auth::domain::services::AuthenticationError,
    user::{User, UserSession},
};
use uuid::Uuid;

use crate::application::auth::AuthFacadeError;
use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};

/// Get all active sessions for the current user
pub async fn get_user_sessions_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> AppResult<Json<Vec<UserSession>>> {
    let sessions = state
        .auth_facade()
        .list_user_sessions(user.id)
        .await
        .map_err(map_facade_error)?;

    Ok(Json(sessions))
}

/// Delete a specific session (logout from device)
pub async fn delete_session_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(session_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    state
        .auth_facade()
        .revoke_user_session(user.id, session_id)
        .await
        .map_err(map_facade_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Delete all sessions for the current user (logout from all devices)
pub async fn delete_all_sessions_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> AppResult<StatusCode> {
    // Disable auto-login for the user when logging out from all devices
    let mut updated_user = user.clone();
    updated_user.preferences.auto_login_enabled = false;
    updated_user.updated_at = chrono::Utc::now();

    state
        .unit_of_work()
        .users
        .update_user(&updated_user)
        .await
        .map_err(|_| AppError::internal("Failed to update user preferences"))?;

    state
        .auth_facade()
        .revoke_all_user_sessions(user.id)
        .await
        .map_err(map_facade_error)?;

    Ok(StatusCode::NO_CONTENT)
}

fn map_facade_error(err: AuthFacadeError) -> AppError {
    match err {
        AuthFacadeError::Authentication(auth_err) => map_auth_error(auth_err),
        AuthFacadeError::UserNotFound => {
            AppError::not_found("User not found".to_string())
        }
        AuthFacadeError::Storage(e) => {
            AppError::internal(format!("Storage error: {e}"))
        }
        other => AppError::internal(format!("Auth facade error: {other}")),
    }
}

fn map_auth_error(err: AuthenticationError) -> AppError {
    match err {
        AuthenticationError::InvalidCredentials
        | AuthenticationError::InvalidPin => {
            AppError::forbidden("Invalid credentials".to_string())
        }
        AuthenticationError::UserNotFound => {
            AppError::not_found("User not found".to_string())
        }
        AuthenticationError::DeviceNotFound => {
            AppError::not_found("Device session not found".to_string())
        }
        AuthenticationError::DeviceNotTrusted => {
            AppError::forbidden("Device is not trusted".to_string())
        }
        AuthenticationError::TooManyFailedAttempts => {
            AppError::rate_limited("Too many failed attempts".to_string())
        }
        AuthenticationError::SessionExpired => {
            AppError::unauthorized("Session expired".to_string())
        }
        AuthenticationError::DatabaseError(e) => {
            AppError::internal(format!("Authentication storage error: {e}"))
        }
    }
}
