use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use ferrex_core::{
    api_types::ApiResponse,
    error::MediaError,
    user::{AuthError, AuthToken, LoginRequest, RegisterRequest, User, UserSession},
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}
use uuid::Uuid;

use super::jwt::{generate_access_token, generate_refresh_token};
use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};

pub async fn register(
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> AppResult<Json<ApiResponse<AuthToken>>> {
    request
        .validate()
        .map_err(|e| AppError::bad_request(format!("Validation error: {}", e)))?;

    // Check if username already exists
    if let Ok(Some(_)) = state
        .db
        .backend()
        .get_user_by_username(&request.username)
        .await
    {
        return Err(AppError::conflict(AuthError::UsernameTaken.to_string()));
    }

    // Hash password
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(request.password.as_bytes(), &salt)
        .map_err(|_| AppError::internal("Failed to hash password"))?
        .to_string();

    // Create user
    let user_id = Uuid::now_v7();
    let user = User {
        id: user_id,
        username: request.username.to_lowercase(),
        display_name: request.display_name.clone(),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: None,
        is_active: true,
        email: None,
        preferences: Default::default(),
    };

    state
        .db
        .backend()
        .create_user(&user)
        .await
        .map_err(|e| match e {
            MediaError::Conflict(msg) => AppError::conflict(msg),
            _ => AppError::internal("Failed to create user"),
        })?;

    // Store password hash in user_credentials table
    state
        .db
        .backend()
        .update_user_password(user_id, &password_hash)
        .await
        .map_err(|_| AppError::internal("Failed to store password"))?;

    // Generate tokens for the new user
    let access_token = generate_access_token(user.id)
        .map_err(|_| AppError::internal("Failed to generate access token"))?;

    let refresh_token = generate_refresh_token();

    // Store refresh token
    let expires_at = chrono::Utc::now() + chrono::Duration::days(30);

    state
        .db
        .backend()
        .store_refresh_token(&refresh_token, user.id, None, expires_at)
        .await
        .map_err(|_| AppError::internal("Failed to store refresh token"))?;

    // Create session
    let session = UserSession {
        id: Uuid::now_v7(),
        user_id: user.id,
        device_name: None,
        ip_address: None,
        user_agent: None,
        last_active: Utc::now().timestamp(),
        created_at: Utc::now().timestamp(),
    };

    state
        .db
        .backend()
        .create_session(&session)
        .await
        .map_err(|_| AppError::internal("Failed to create session"))?;

    Ok(Json(ApiResponse::success(AuthToken {
        access_token,
        refresh_token,
        expires_in: 900, // 15 minutes
    })))
}

pub async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> AppResult<Json<ApiResponse<AuthToken>>> {
    let user = state
        .db
        .backend()
        .get_user_by_username(&request.username)
        .await
        .map_err(|_| AppError::internal(AuthError::InternalError.to_string()))?
        .ok_or_else(|| AppError::unauthorized(AuthError::InvalidCredentials.to_string()))?;

    // Get password hash from credentials table
    let password_hash = state
        .db
        .backend()
        .get_user_password_hash(user.id)
        .await
        .map_err(|_| AppError::internal("Failed to get password hash"))?
        .ok_or_else(|| AppError::unauthorized(AuthError::InvalidCredentials.to_string()))?;

    // Verify password
    let parsed_hash = PasswordHash::new(&password_hash)
        .map_err(|_| AppError::internal("Invalid password hash"))?;

    let argon2 = Argon2::default();
    argon2
        .verify_password(request.password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::unauthorized(AuthError::InvalidCredentials.to_string()))?;

    // Generate tokens
    let access_token = generate_access_token(user.id)
        .map_err(|_| AppError::internal("Failed to generate access token"))?;

    let refresh_token = generate_refresh_token();

    // Store refresh token
    // Calculate expiry time (30 days from now)
    let expires_at = chrono::Utc::now() + chrono::Duration::days(30);

    state
        .db
        .backend()
        .store_refresh_token(
            &refresh_token,
            user.id,
            request.device_name.clone(),
            expires_at,
        )
        .await
        .map_err(|_| AppError::internal("Failed to store refresh token"))?;

    // Create session
    let session = UserSession {
        id: Uuid::now_v7(),
        user_id: user.id,
        device_name: request.device_name,
        ip_address: None, // TODO: Extract from request
        user_agent: None, // TODO: Extract from request
        last_active: Utc::now().timestamp(),
        created_at: Utc::now().timestamp(),
    };

    state
        .db
        .backend()
        .create_session(&session)
        .await
        .map_err(|_| AppError::internal("Failed to create session"))?;

    Ok(Json(ApiResponse::success(AuthToken {
        access_token,
        refresh_token,
        expires_in: 900, // 15 minutes
    })))
}

pub async fn refresh(
    State(state): State<AppState>,
    Json(request): Json<RefreshRequest>,
) -> AppResult<Json<ApiResponse<AuthToken>>> {
    let refresh_token_data = state
        .db
        .backend()
        .get_refresh_token(&request.refresh_token)
        .await
        .map_err(|_| AppError::internal(AuthError::InternalError.to_string()))?;

    let (user_id, _expires_at) = refresh_token_data
        .ok_or_else(|| AppError::unauthorized(AuthError::TokenInvalid.to_string()))?;

    // Invalidate old refresh token
    state
        .db
        .backend()
        .delete_refresh_token(&request.refresh_token)
        .await
        .map_err(|_| AppError::internal("Failed to invalidate old token"))?;

    // Generate new tokens
    let access_token = generate_access_token(user_id)
        .map_err(|_| AppError::internal("Failed to generate access token"))?;

    let refresh_token = generate_refresh_token();

    // Store new refresh token
    // Calculate expiry time (30 days from now)
    let expires_at = chrono::Utc::now() + chrono::Duration::days(30);

    state
        .db
        .backend()
        .store_refresh_token(&refresh_token, user_id, None, expires_at)
        .await
        .map_err(|_| AppError::internal("Failed to store refresh token"))?;

    Ok(Json(ApiResponse::success(AuthToken {
        access_token,
        refresh_token,
        expires_in: 900,
    })))
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> Result<StatusCode, AppError> {
    // Disable auto-login for the user
    let mut updated_user = user.clone();
    updated_user.preferences.auto_login_enabled = false;
    updated_user.updated_at = Utc::now();

    state
        .db
        .backend()
        .update_user(&updated_user)
        .await
        .map_err(|_| AppError::internal("Failed to update user preferences"))?;

    // Delete all refresh tokens for this user (logout from all devices)
    state
        .db
        .backend()
        .delete_user_refresh_tokens(user.id)
        .await
        .map_err(|_| AppError::internal("Failed to invalidate refresh tokens"))?;

    // Get all sessions for this user and delete them
    let sessions = state
        .db
        .backend()
        .get_user_sessions(user.id)
        .await
        .map_err(|_| AppError::internal("Failed to get user sessions"))?;

    for session in sessions {
        let _ = state.db.backend().delete_session(session.id).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_current_user(
    Extension(user): Extension<User>,
) -> AppResult<Json<ApiResponse<User>>> {
    Ok(Json(ApiResponse::success(user)))
}
