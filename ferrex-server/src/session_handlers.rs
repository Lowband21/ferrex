use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::Utc;
use ferrex_core::user::{User, UserSession};
use uuid::Uuid;

use crate::{errors::AppResult, AppState};

/// Get all active sessions for the current user
pub async fn get_user_sessions_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> AppResult<Json<Vec<UserSession>>> {
    let sessions = state
        .db
        .backend()
        .get_user_sessions(user.id)
        .await?;
    
    Ok(Json(sessions))
}

/// Delete a specific session (logout from device)
pub async fn delete_session_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(session_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    // Get the session to verify ownership
    let sessions = state
        .db
        .backend()
        .get_user_sessions(user.id)
        .await?;
    
    // Check if the session belongs to the user
    if !sessions.iter().any(|s| s.id == session_id) {
        return Err(crate::errors::AppError::forbidden(
            "You can only delete your own sessions",
        ));
    }
    
    // Delete the session
    state.db.backend().delete_session(session_id).await
        .map_err(|_| crate::errors::AppError::internal("Failed to delete session"))?;
    
    // Also delete any refresh tokens associated with this session
    // Note: This is handled by the database implementation
    
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
    
    state.db.backend().update_user(&updated_user).await
        .map_err(|_| crate::errors::AppError::internal("Failed to update user preferences"))?;
    
    // Get all sessions
    let sessions = state
        .db
        .backend()
        .get_user_sessions(user.id)
        .await
        .map_err(|_| crate::errors::AppError::internal("Failed to get user sessions"))?;
    
    // Delete each session
    for session in sessions {
        let _ = state.db.backend().delete_session(session.id).await;
    }
    
    // Delete all refresh tokens
    state
        .db
        .backend()
        .delete_user_refresh_tokens(user.id)
        .await
        .map_err(|_| crate::errors::AppError::internal("Failed to delete refresh tokens"))?;
    
    Ok(StatusCode::NO_CONTENT)
}