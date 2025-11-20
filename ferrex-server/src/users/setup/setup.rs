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

use axum::{extract::State, Json};
use chrono::{DateTime, Duration, Utc};
use ferrex_core::{api_types::ApiResponse, user::AuthToken};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use axum::extract::ConnectInfo;
use std::net::SocketAddr;

use crate::{
    AppState,
    errors::{AppError, AppResult},
    users::{
        UserService,
        user_service::{CreateUserParams, PasswordRequirements},
    },
};

/// Simple in-memory rate limiter for setup endpoints
/// Tracks setup attempts by IP address to prevent brute force attacks
#[derive(Clone)]
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
        let ip_attempts = attempts.entry(ip.to_string()).or_insert_with(Vec::new);

        // Remove old attempts outside the window
        ip_attempts.retain(|&attempt| attempt > window_start);

        // Check if rate limit exceeded
        if ip_attempts.len() >= self.max_attempts {
            let oldest_attempt = ip_attempts.first().copied().unwrap_or(now);
            let wait_time = (oldest_attempt + Duration::minutes(self.window_minutes)) - now;
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
}

// Global rate limiter instance
// Using a function to get the rate limiter instead of lazy_static
fn get_rate_limiter() -> &'static SetupRateLimiter {
    static INSTANCE: std::sync::OnceLock<SetupRateLimiter> = std::sync::OnceLock::new();
    INSTANCE.get_or_init(SetupRateLimiter::default)
}

/// Response for setup status check
#[derive(Debug, Serialize)]
pub struct SetupStatus {
    /// Whether the server needs initial setup
    pub needs_setup: bool,
    /// Whether an admin user exists
    pub has_admin: bool,
    /// Total number of users
    pub user_count: usize,
    /// Total number of libraries
    pub library_count: usize,
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

        // Use admin password requirements
        UserService::validate_password(&self.password, &PasswordRequirements::admin())?;

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
    let users = state
        .database
        .backend()
        .get_all_users()
        .await
        .map_err(|e| AppError::internal(format!("Failed to get users: {}", e)))?;

    let libraries = state
        .database
        .backend()
        .list_libraries()
        .await
        .map_err(|e| AppError::internal(format!("Failed to get libraries: {}", e)))?;

    let status = SetupStatus {
        needs_setup,
        has_admin: !needs_setup,
        user_count: users.len(),
        library_count: libraries.len(),
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

    // Check setup token if configured
    if let Ok(expected_token) = std::env::var("FERREX_SETUP_TOKEN") {
        if !expected_token.is_empty() {
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

    // Create user using UserService
    let user_service = UserService::new(&state);
    let user = user_service
        .create_user(CreateUserParams {
            username: request.username,
            display_name: request.display_name,
            password: request.password,
            created_by: None, // First admin creates themselves
        })
        .await?;

    // Assign admin role
    let admin_role_id =
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("Invalid admin role UUID");

    user_service
        .assign_role(user.id, admin_role_id, user.id)
        .await?;

    // Generate tokens
    let auth_token = user_service
        .generate_auth_tokens(user.id, Some("Ferrex Setup".to_string()))
        .await?;

    info!(
        "Initial admin user created: {} ({}) from IP: {}",
        user.username, user.id, client_ip
    );

    // Clear rate limit for this IP after successful setup
    get_rate_limiter().clear_ip(&client_ip).await;

    Ok(Json(ApiResponse::success(auth_token)))
}

/// Internal helper to check setup status
async fn check_setup_status_internal(state: &AppState) -> AppResult<SetupStatus> {
    let user_service = UserService::new(state);
    let needs_setup = user_service.needs_setup().await?;

    let users = state
        .database
        .backend()
        .get_all_users()
        .await
        .map_err(|e| AppError::internal(format!("Failed to get users: {}", e)))?;

    let libraries = state
        .database
        .backend()
        .list_libraries()
        .await
        .map_err(|e| AppError::internal(format!("Failed to get libraries: {}", e)))?;

    Ok(SetupStatus {
        needs_setup,
        has_admin: !needs_setup,
        user_count: users.len(),
        library_count: libraries.len(),
    })
}
