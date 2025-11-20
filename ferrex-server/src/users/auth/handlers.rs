use crate::users::map_auth_facade_error;
use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use ferrex_core::{
    api_types::ApiResponse,
    auth::{
        domain::services::{AuthenticationError, TokenBundle},
        policy::PasswordPolicyRule,
    },
    error::MediaError,
    user::{AuthError, AuthToken, LoginRequest, RegisterRequest, User},
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};

pub async fn register(
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> AppResult<Json<ApiResponse<AuthToken>>> {
    request.validate().map_err(|e| {
        AppError::bad_request(format!("Validation error: {}", e))
    })?;

    if request.password.is_empty() {
        return Err(AppError::bad_request("Password cannot be empty"));
    }
    if request.password.len() > 128 {
        return Err(AppError::bad_request(
            "Password cannot exceed 128 characters",
        ));
    }

    let security_repo = state.unit_of_work().security_settings.clone();
    let security_settings =
        security_repo.get_settings().await.map_err(|e| {
            AppError::internal(format!(
                "Failed to load security settings: {}",
                e
            ))
        })?;

    let user_policy = security_settings.user_password_policy.clone();
    let policy_check = user_policy.check(&request.password);

    if user_policy.enforce && !policy_check.is_satisfied() {
        return Err(AppError::bad_request(format!(
            "Password does not meet the required policy: {}",
            describe_policy_failures(&policy_check.failures)
        )));
    }

    if !user_policy.enforce && !policy_check.is_satisfied() {
        tracing::info!(
            "User registration proceeding with relaxed password policy (failures: {})",
            describe_policy_failures(&policy_check.failures)
        );
    }

    // Check if username already exists
    if let Ok(Some(_)) = state
        .unit_of_work()
        .users
        .get_user_by_username(&request.username)
        .await
    {
        return Err(AppError::conflict(AuthError::UsernameTaken.to_string()));
    }

    // Hash password using centralized crypto helper
    let password_hash = state
        .auth_crypto()
        .hash_password(&request.password)
        .map_err(|e| {
            AppError::internal(format!("Failed to hash password: {e}"))
        })?;

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
        .unit_of_work()
        .users
        .create_user_with_password(&user, &password_hash)
        .await
        .map_err(|e| match e {
            MediaError::Conflict(msg) => AppError::conflict(msg),
            _ => AppError::internal("Failed to create user"),
        })?;

    let token_bundle = state
        .auth_service()
        .authenticate_with_password(&user.username, &request.password)
        .await
        .map_err(map_auth_error)?;

    Ok(Json(ApiResponse::success(bundle_to_auth_token(
        token_bundle,
    ))))
}

pub async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> AppResult<Json<ApiResponse<AuthToken>>> {
    let token_bundle = state
        .auth_service()
        .authenticate_with_password(&request.username, &request.password)
        .await
        .map_err(map_auth_error)?;

    Ok(Json(ApiResponse::success(bundle_to_auth_token(
        token_bundle,
    ))))
}

pub async fn refresh(
    State(state): State<AppState>,
    Json(request): Json<RefreshRequest>,
) -> AppResult<Json<ApiResponse<AuthToken>>> {
    let token_bundle = state
        .auth_service()
        .refresh_session(&request.refresh_token)
        .await
        .map_err(map_auth_error)?;

    Ok(Json(ApiResponse::success(bundle_to_auth_token(
        token_bundle,
    ))))
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
        .unit_of_work()
        .users
        .update_user(&updated_user)
        .await
        .map_err(|_| AppError::internal("Failed to update user preferences"))?;

    state
        .auth_facade()
        .revoke_all_user_sessions(user.id)
        .await
        .map_err(map_auth_facade_error)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_current_user(
    Extension(user): Extension<User>,
) -> AppResult<Json<ApiResponse<User>>> {
    Ok(Json(ApiResponse::success(user)))
}

fn bundle_to_auth_token(bundle: TokenBundle) -> AuthToken {
    let expires_in = bundle
        .session_token
        .expires_at()
        .signed_duration_since(Utc::now())
        .num_seconds()
        .max(0) as u32;

    AuthToken {
        access_token: bundle.session_token.as_str().to_string(),
        refresh_token: bundle.refresh_token.as_str().to_string(),
        expires_in,
        session_id: Some(bundle.session_record_id),
        device_session_id: bundle.device_session_id,
        user_id: Some(bundle.user_id),
        scope: bundle.scope,
    }
}

fn map_auth_error(err: AuthenticationError) -> AppError {
    match err {
        AuthenticationError::InvalidCredentials
        | AuthenticationError::InvalidPin => {
            AppError::unauthorized(AuthError::InvalidCredentials.to_string())
        }
        AuthenticationError::TooManyFailedAttempts => AppError::rate_limited(
            "Too many failed authentication attempts".to_string(),
        ),
        AuthenticationError::SessionExpired => {
            AppError::unauthorized(AuthError::TokenInvalid.to_string())
        }
        AuthenticationError::DeviceNotFound
        | AuthenticationError::DeviceNotTrusted => AppError::forbidden(
            "Device not eligible for authentication".to_string(),
        ),
        AuthenticationError::UserNotFound => {
            AppError::not_found("User not found".to_string())
        }
        AuthenticationError::DatabaseError(e) => {
            AppError::internal(format!("Authentication failed: {e}"))
        }
    }
}

fn describe_policy_failures(failures: &[PasswordPolicyRule]) -> String {
    if failures.is_empty() {
        return "no failures".to_string();
    }

    failures
        .iter()
        .map(|rule| rule.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
