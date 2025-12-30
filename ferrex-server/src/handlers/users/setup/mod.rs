//! First-run setup handlers for the Ferrex media server
//!
//! This module provides endpoints for initial server configuration,
//! particularly the creation of the first admin user.
//!
//! ## Security Features
//!
//! - **Rate Limiting**: Prevents brute force attacks by limiting setup attempts to 5 per 15 minutes per IP
//! - **Setup Token**: Optional environment variable `FERREX_SETUP_TOKEN` can require a secret token
//! - **Admin Check**: Prevents setup endpoint abuse after initial admin creation
//! - **Strong Password Requirements**: Enforces secure passwords for admin accounts
//! - **Security Logging**: All failed attempts are logged with IP addresses for monitoring
//!
//! ## Usage
//!
//! 1. Check setup status: `GET /api/setup/status`
//! 2. Create admin: `POST /api/setup/admin` (only works if no admin exists)
//!
//! For additional security, set `FERREX_SETUP_TOKEN` environment variable before starting the server.
//!
//! These endpoints handle the initial setup flow when no admin user exists.

pub mod claim;

use axum::{Json, extract::State};
use chrono::{DateTime, Duration, Utc};
use ferrex_core::{
    api::types::ApiResponse,
    domain::users::{
        auth::{
            domain::services::{AuthenticationError, TokenBundle},
            policy::{PasswordPolicy, PasswordPolicyRule},
        },
        rbac::roles,
        user::AuthToken,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::handlers::users::{UserService, user_service::CreateUserParams};
use crate::infra::{
    app_state::AppState,
    demo_mode,
    errors::{AppError, AppResult},
};
use axum::extract::ConnectInfo;
use ferrex_core::domain::setup::SetupClaimError;
use std::net::SocketAddr;

/// Simple in-memory rate limiter for setup endpoints
/// Tracks setup attempts by IP address to prevent brute force attacks
#[derive(Debug, Clone)]
pub struct SetupRateLimiter {
    attempts: Arc<RwLock<HashMap<String, Vec<DateTime<Utc>>>>>,
    max_attempts: usize,
    window_minutes: i64,
}

impl Default for SetupRateLimiter {
    fn default() -> Self {
        Self {
            attempts: Arc::new(RwLock::new(HashMap::new())),
            max_attempts: 5,    // Allow 5 attempts
            window_minutes: 15, // Within 15 minutes
        }
    }
}

impl SetupRateLimiter {
    /// Check if an IP address has exceeded the rate limit
    pub async fn check_rate_limit(&self, ip: &str) -> Result<(), AppError> {
        let mut attempts = self.attempts.write().await;
        let now = Utc::now();
        let window_start = now - Duration::minutes(self.window_minutes);

        // Get or create attempt list for this IP
        let ip_attempts =
            attempts.entry(ip.to_string()).or_insert_with(Vec::new);

        // Remove old attempts outside the window
        ip_attempts.retain(|&attempt| attempt > window_start);

        // Check if rate limit exceeded
        if ip_attempts.len() >= self.max_attempts {
            let oldest_attempt = ip_attempts.first().copied().unwrap_or(now);
            let wait_time =
                (oldest_attempt + Duration::minutes(self.window_minutes)) - now;
            let wait_minutes = wait_time.num_minutes().max(1);

            warn!(
                "Rate limit exceeded for IP {}: {} attempts in {} minutes",
                ip,
                ip_attempts.len(),
                self.window_minutes
            );

            return Err(AppError::rate_limited(format!(
                "Too many setup attempts. Please wait {} minutes before trying again.",
                wait_minutes
            )));
        }

        // Record this attempt
        ip_attempts.push(now);

        Ok(())
    }

    /// Clear rate limit history for an IP (called after successful setup)
    pub async fn clear_ip(&self, ip: &str) {
        let mut attempts = self.attempts.write().await;
        attempts.remove(ip);
    }

    #[doc(hidden)]
    pub async fn reset(&self) {
        let mut attempts = self.attempts.write().await;
        attempts.clear();
    }
}

// Global rate limiter instance
// Using a function to get the rate limiter instead of lazy_static
fn get_rate_limiter() -> &'static SetupRateLimiter {
    static INSTANCE: std::sync::OnceLock<SetupRateLimiter> =
        std::sync::OnceLock::new();
    INSTANCE.get_or_init(SetupRateLimiter::default)
}

/// Response for setup status check
#[derive(Debug, Serialize)]
pub struct SetupStatus {
    /// Whether the server needs initial setup
    pub needs_setup: bool,
    /// Whether an admin user exists
    pub has_admin: bool,
    /// Whether clients must supply the configured setup token
    pub requires_setup_token: bool,
    /// Total number of users
    pub user_count: usize,
    /// Total number of libraries
    pub library_count: usize,
    /// Current password policy for admin flows
    pub admin_password_policy: PasswordPolicyResponse,
    /// Current password policy for regular users
    pub user_password_policy: PasswordPolicyResponse,
}

#[derive(Debug, Serialize, Clone)]
pub struct PasswordPolicyResponse {
    pub enforce: bool,
    pub min_length: u16,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_number: bool,
    pub require_special: bool,
}

impl From<&PasswordPolicy> for PasswordPolicyResponse {
    fn from(value: &PasswordPolicy) -> Self {
        Self {
            enforce: value.enforce,
            min_length: value.min_length,
            require_uppercase: value.require_uppercase,
            require_lowercase: value.require_lowercase,
            require_number: value.require_number,
            require_special: value.require_special,
        }
    }
}

/// Request to create the initial admin user
#[derive(Debug, Deserialize)]
pub struct CreateAdminRequest {
    /// Admin username (must be unique)
    pub username: String,
    /// Display name for the admin
    pub display_name: String,
    /// Strong password (not a PIN)
    pub password: String,
    /// Optional setup token (can be set via FERREX_SETUP_TOKEN env var)
    #[serde(default)]
    pub setup_token: Option<String>,
    /// Claim token returned from the secure claim confirmation flow
    #[serde(default)]
    pub claim_token: Option<String>,
}

impl CreateAdminRequest {
    /// Validate the admin creation request
    fn validate(&self) -> Result<(), String> {
        // Use centralized validation from UserService
        UserService::validate_username(&self.username)?;

        // Display name validation
        if self.display_name.trim().is_empty() {
            return Err("Display name cannot be empty".to_string());
        }
        if self.display_name.len() > 64 {
            return Err("Display name cannot exceed 64 characters".to_string());
        }

        if self.password.is_empty() {
            return Err("Password cannot be empty".to_string());
        }
        if self.password.len() > 128 {
            return Err("Password cannot exceed 128 characters".to_string());
        }

        Ok(())
    }
}

/// Check if the server needs initial setup
///
/// This endpoint is public and can be called without authentication.
/// It returns information about whether the server has been set up.
pub async fn check_setup_status(
    State(state): State<AppState>,
) -> AppResult<Json<ApiResponse<SetupStatus>>> {
    let user_service = UserService::new(&state);
    let needs_setup = user_service.needs_setup().await?;

    // Get user and library counts
    let users =
        state
            .unit_of_work()
            .users
            .get_all_users()
            .await
            .map_err(|e| {
                AppError::internal(format!("Failed to get users: {}", e))
            })?;

    let libraries = state
        .unit_of_work()
        .libraries
        .list_libraries()
        .await
        .map_err(|e| {
            AppError::internal(format!("Failed to get libraries: {}", e))
        })?;
    let libraries = demo_mode::filter_libraries(&state, libraries);

    let security_repo = state.unit_of_work().security_settings.clone();
    let security_settings =
        security_repo.get_settings().await.map_err(|e| {
            AppError::internal(format!(
                "Failed to load security settings: {}",
                e
            ))
        })?;

    let requires_setup_token = state
        .config()
        .auth
        .setup_token
        .clone()
        .or_else(|| std::env::var("FERREX_SETUP_TOKEN").ok())
        .is_some_and(|value| !value.trim().is_empty());

    let status = SetupStatus {
        needs_setup,
        has_admin: !needs_setup,
        requires_setup_token,
        user_count: users.len(),
        library_count: libraries.len(),
        admin_password_policy: PasswordPolicyResponse::from(
            &security_settings.admin_password_policy,
        ),
        user_password_policy: PasswordPolicyResponse::from(
            &security_settings.user_password_policy,
        ),
    };

    Ok(Json(ApiResponse::success(status)))
}

/// Create the initial admin user
///
/// This endpoint only works when no admin user exists in the system.
/// It creates a user with full admin privileges.
pub async fn create_initial_admin(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<CreateAdminRequest>,
) -> AppResult<Json<ApiResponse<AuthToken>>> {
    // Extract client IP for rate limiting (fallback to localhost if not available)
    let client_ip = addr.ip().to_string();

    // Check rate limit
    get_rate_limiter().check_rate_limit(&client_ip).await?;

    // Check setup token if configured, preferring config over direct env lookup
    if let Some(expected_token) = state
        .config()
        .auth
        .setup_token
        .clone()
        .or_else(|| std::env::var("FERREX_SETUP_TOKEN").ok())
        .filter(|token| !token.is_empty())
    {
        // Token is required
        let provided_token = request
            .setup_token
            .as_ref()
            .ok_or_else(|| AppError::unauthorized("Setup token required"))?;

        // Constant-time comparison to prevent timing attacks
        if provided_token.len() != expected_token.len() {
            return Err(AppError::unauthorized("Invalid setup token"));
        }

        let mut matches = true;
        for (a, b) in provided_token.bytes().zip(expected_token.bytes()) {
            matches &= a == b;
        }

        if !matches {
            warn!(
                "Failed setup attempt with invalid token from IP: {}",
                client_ip
            );
            return Err(AppError::unauthorized("Invalid setup token"));
        }
    }
    // Validate request
    request.validate().map_err(|e| {
        warn!("Invalid setup request from IP {}: {}", client_ip, e);
        AppError::bad_request(format!("Validation error: {}", e))
    })?;

    // Check if admin already exists
    let setup_status = check_setup_status_internal(&state).await?;
    if setup_status.has_admin {
        warn!(
            "Setup attempt when admin already exists from IP: {}",
            client_ip
        );
        return Err(AppError::forbidden(
            "Admin user already exists. Use normal registration process.",
        ));
    }

    let claim_service = state.setup_claim_service();
    let claim_token = request
        .claim_token
        .as_ref()
        .map(|token| token.trim())
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            AppError::forbidden(
                "Secure claim token required before admin setup",
            )
        })?
        .to_string();

    claim_service
        .validate_claim_token(&claim_token)
        .await
        .map_err(map_claim_error)?;

    let security_repo = state.unit_of_work().security_settings.clone();
    let security_settings =
        security_repo.get_settings().await.map_err(|e| {
            AppError::internal(format!(
                "Failed to load security settings: {}",
                e
            ))
        })?;

    let admin_policy = security_settings.admin_password_policy.clone();
    let policy_check = admin_policy.check(&request.password);

    if admin_policy.enforce && !policy_check.is_satisfied() {
        let failures = describe_policy_failures(&policy_check.failures);
        warn!(
            "Admin setup rejected due to password policy from IP {}",
            client_ip
        );
        return Err(AppError::bad_request(format!(
            "Password does not meet the required policy: {}",
            failures
        )));
    }

    if !admin_policy.enforce && !policy_check.is_satisfied() {
        tracing::info!(
            "Admin setup proceeding with relaxed password (failures: {})",
            describe_policy_failures(&policy_check.failures)
        );
    }

    // Create user using UserService
    let user_service = UserService::new(&state);
    // Ensure the built-in 'admin' role exists before assignment
    user_service.ensure_admin_role_exists().await?;
    let password_clone = request.password.clone();

    let user = user_service
        .create_user(CreateUserParams {
            username: request.username,
            display_name: request.display_name,
            password: request.password,
            email: None,
            avatar_url: None,
            role_ids: Vec::new(),
            is_active: true,
            created_by: None, // First admin creates themselves
        })
        .await?;

    // Assign admin role
    let admin_role = state
        .unit_of_work()
        .rbac
        .get_all_roles()
        .await
        .map_err(|e| {
            AppError::internal(format!("Failed to load roles: {}", e))
        })?
        .into_iter()
        .find(|role| role.name == roles::ADMIN)
        .ok_or_else(|| {
            AppError::internal("Admin role missing after initialization")
        })?;

    user_service
        .assign_role(user.id, admin_role.id, user.id)
        .await?;

    // Generate tokens via authentication service
    let token_bundle = state
        .auth_service()
        .authenticate_with_password(&user.username, &password_clone)
        .await
        .map_err(auth_error_to_app)?;
    let auth_token = bundle_to_auth_token(token_bundle);

    info!(
        "Initial admin user created: {} ({}) from IP: {}",
        user.username, user.id, client_ip
    );

    if let Err(err) = claim_service.consume_claim_token(&claim_token).await {
        warn!(
            error = %err,
            "Failed to mark claim token as consumed after admin setup"
        );
    }

    // Clear rate limit for this IP after successful setup
    get_rate_limiter().clear_ip(&client_ip).await;

    Ok(Json(ApiResponse::success(auth_token)))
}

/// Internal helper to check setup status
async fn check_setup_status_internal(
    state: &AppState,
) -> AppResult<SetupStatus> {
    let user_service = UserService::new(state);
    let needs_setup = user_service.needs_setup().await?;
    let requires_setup_token = state
        .config()
        .auth
        .setup_token
        .clone()
        .or_else(|| std::env::var("FERREX_SETUP_TOKEN").ok())
        .is_some_and(|value| !value.trim().is_empty());

    let users =
        state
            .unit_of_work()
            .users
            .get_all_users()
            .await
            .map_err(|e| {
                AppError::internal(format!("Failed to get users: {}", e))
            })?;

    let libraries = state
        .unit_of_work()
        .libraries
        .list_libraries()
        .await
        .map_err(|e| {
            AppError::internal(format!("Failed to get libraries: {}", e))
        })?;
    let libraries = demo_mode::filter_libraries(state, libraries);

    let security_repo = state.unit_of_work().security_settings.clone();
    let security_settings =
        security_repo.get_settings().await.map_err(|e| {
            AppError::internal(format!(
                "Failed to load security settings: {}",
                e
            ))
        })?;

    Ok(SetupStatus {
        needs_setup,
        has_admin: !needs_setup,
        requires_setup_token,
        user_count: users.len(),
        library_count: libraries.len(),
        admin_password_policy: PasswordPolicyResponse::from(
            &security_settings.admin_password_policy,
        ),
        user_password_policy: PasswordPolicyResponse::from(
            &security_settings.user_password_policy,
        ),
    })
}

fn auth_error_to_app(err: AuthenticationError) -> AppError {
    match err {
        AuthenticationError::InvalidCredentials
        | AuthenticationError::InvalidPin => {
            AppError::unauthorized("Invalid credentials".to_string())
        }
        AuthenticationError::TooManyFailedAttempts => AppError::rate_limited(
            "Too many failed authentication attempts".to_string(),
        ),
        AuthenticationError::SessionExpired => {
            AppError::unauthorized("Session expired".to_string())
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

fn describe_policy_failures(failures: &[PasswordPolicyRule]) -> String {
    if failures.is_empty() {
        return "no failures".to_string();
    }

    failures
        .iter()
        .map(|rule: &PasswordPolicyRule| rule.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn map_claim_error(error: SetupClaimError) -> AppError {
    match error {
        SetupClaimError::InvalidCode => {
            AppError::bad_request("Invalid claim code")
        }
        SetupClaimError::InvalidToken => {
            AppError::forbidden("Invalid claim token")
        }
        SetupClaimError::Expired { .. } => {
            AppError::gone("Claim token has expired")
        }
        SetupClaimError::ActiveClaimPending { .. } => {
            AppError::conflict("A claim is still pending; restart the flow")
        }
        SetupClaimError::Storage(err) => {
            AppError::internal(format!("Claim persistence failure: {err}"))
        }
    }
}
