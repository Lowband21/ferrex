use crate::handlers::users::map_auth_facade_error;
use axum::{
    Extension, Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use chrono::{Duration, Utc};
use ferrex_core::{
    api::types::ApiResponse,
    domain::users::{
        auth::{
            domain::{
                services::{AuthenticationError, TokenBundle},
                value_objects::SessionScope as CoreSessionScope,
            },
            policy::PasswordPolicyRule,
        },
        user::{
            AuthError, AuthToken, LoginRequest, RegisterRequest, User,
            ValidationError,
        },
    },
    error::MediaError,
};
use ferrex_flatbuffers::conversions::auth as fb_auth;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    content_negotiation::{AcceptedFormat, json_or_flatbuffers},
    errors::{AppError, AppResult},
    fb_request_parsing::parse_json_or_flatbuffers,
};

pub async fn register(
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> AppResult<Json<ApiResponse<AuthToken>>> {
    request.validate().map_err(|e: ValidationError| {
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
    AcceptedFormat(response_format): AcceptedFormat,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Response> {
    let request: LoginRequest =
        parse_json_or_flatbuffers(&headers, body, |bytes| {
            fb_auth::parse_login_request(bytes).map(|request| LoginRequest {
                username: request.username,
                password: request.password,
                device_name: request.device_name,
            })
        })?;

    let token_bundle = state
        .auth_service()
        .authenticate_with_password(&request.username, &request.password)
        .await
        .map_err(map_auth_error)?;
    let auth_token = bundle_to_auth_token(token_bundle);

    Ok(json_or_flatbuffers(
        response_format,
        ApiResponse::success(auth_token.clone()),
        || fb_auth::serialize_auth_token(&auth_token_to_fb(&auth_token)),
    ))
}

pub async fn refresh(
    State(state): State<AppState>,
    AcceptedFormat(response_format): AcceptedFormat,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Response> {
    let request: RefreshRequest =
        parse_json_or_flatbuffers(&headers, body, |bytes| {
            fb_auth::parse_refresh_request(bytes).map(|request| {
                RefreshRequest {
                    refresh_token: request.refresh_token,
                }
            })
        })?;

    let security_settings = state
        .unit_of_work()
        .security_settings
        .get_settings()
        .await
        .map_err(|e| {
            AppError::internal(format!("Failed to load security settings: {e}"))
        })?;
    let trust_policy = &security_settings.device_trust_policy;

    let token_bundle = state
        .auth_service()
        .refresh_session_with_policy(
            &request.refresh_token,
            trust_policy.pin_max_attempts,
            Duration::minutes(i64::from(trust_policy.pin_lockout_minutes)),
            Duration::days(i64::from(trust_policy.trust_duration_days)),
        )
        .await
        .map_err(map_auth_error)?;
    let auth_token = bundle_to_auth_token(token_bundle);

    Ok(json_or_flatbuffers(
        response_format,
        ApiResponse::success(auth_token.clone()),
        || fb_auth::serialize_auth_token(&auth_token_to_fb(&auth_token)),
    ))
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
    AcceptedFormat(response_format): AcceptedFormat,
    Extension(user): Extension<User>,
) -> AppResult<Response> {
    Ok(json_or_flatbuffers(
        response_format,
        ApiResponse::success(user.clone()),
        || fb_auth::serialize_user_profile(&user_to_fb_profile(&user)),
    ))
}

fn auth_token_to_fb(token: &AuthToken) -> fb_auth::AuthToken<'_> {
    fb_auth::AuthToken {
        access_token: &token.access_token,
        refresh_token: &token.refresh_token,
        expires_in: token.expires_in,
        session_id: token.session_id,
        device_session_id: token.device_session_id,
        user_id: token.user_id,
        scope: session_scope_to_fb(token.scope),
    }
}

fn user_to_fb_profile(user: &User) -> fb_auth::UserProfile<'_> {
    fb_auth::UserProfile {
        id: user.id,
        username: &user.username,
        display_name: &user.display_name,
        avatar_url: user.avatar_url.as_deref(),
        email: user.email.as_deref(),
        created_at: user.created_at,
        updated_at: user.updated_at,
        last_login: user.last_login,
        is_active: user.is_active,
    }
}

fn session_scope_to_fb(scope: CoreSessionScope) -> fb_auth::SessionScope {
    match scope {
        CoreSessionScope::Full => fb_auth::SessionScope::Full,
        CoreSessionScope::Playback => fb_auth::SessionScope::Playback,
    }
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
