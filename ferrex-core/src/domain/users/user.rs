//! User authentication and session management
//!
//! This module provides the core types and functionality for user management in Ferrex,
//! including authentication, session tracking, and profile management.
//!
//! ## Authentication Flow
//!
//! 1. **Registration**: Users create an account with username and password
//! 2. **Login**: Credentials are verified, returning an opaque session token and refresh token
//! 3. **Session**: Each login creates a session entry for device tracking
//! 4. **Token Refresh**: Session tokens (15min default) are rotated via refresh tokens (30 days)
//!
//! ## Security
//!
//! - Passwords are hashed using Argon2id
//! - Session tokens are high-entropy secrets hashed with HMAC before persistence
//! - Sessions track active devices and can be revoked
//!
//! ## Example
//!
//! ```no_run
//! use ferrex_core::domain::users::user::{RegisterRequest, LoginRequest};
//!
//! // Register a new user
//! let register = RegisterRequest {
//!     username: "alice".to_string(),
//!     password: "secure_password".to_string(),
//!     display_name: "Alice".to_string(),
//! };
//!
//! // Login
//! let login = LoginRequest {
//!     username: "alice".to_string(),
//!     password: "secure_password".to_string(),
//!     device_name: Some("Alice's Phone".to_string()),
//! };
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::users::auth::domain::value_objects::SessionScope;
use uuid::Uuid;

/// Core user type for authentication and profile management
///
/// Represents a registered user in the Ferrex system. The password hash
/// is never serialized to prevent accidental exposure.
///
/// # Fields
///
/// * `id` - Unique identifier for the user
/// * `username` - Unique username (lowercase, alphanumeric with underscores)
/// * `display_name` - User's display name (can contain spaces and special characters)
/// * `password_hash` - Argon2id password hash (never serialized)
/// * `created_at` - Timestamp of account creation
/// * `updated_at` - Timestamp of last profile update
/// * `last_login` - Timestamp of most recent login
/// * `is_active` - Whether the user account is active
/// * `email` - Optional email address
/// * `avatar_url` - Optional URL to user's avatar image
/// * `preferences` - User preferences and settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Unique user identifier
    pub id: Uuid,
    /// Unique username (lowercase, 3-30 chars, alphanumeric + underscore)
    pub username: String,
    /// Display name shown in UI
    pub display_name: String,
    /// Optional URL to user's avatar image
    pub avatar_url: Option<String>,
    /// Timestamp of account creation
    pub created_at: DateTime<Utc>,
    /// Timestamp of last profile update
    pub updated_at: DateTime<Utc>,
    /// Timestamp of most recent login
    pub last_login: Option<DateTime<Utc>>,
    /// Whether the user account is active
    pub is_active: bool,
    /// Optional email address
    pub email: Option<String>,
    /// User preferences and settings
    pub preferences: UserPreferences,
}

/// User preferences and settings
///
/// Stores user-specific configuration options including UI preferences,
/// playback settings, and feature toggles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    /// Whether auto-login is enabled for this user
    pub auto_login_enabled: bool,
    /// UI theme preference
    pub theme: ThemePreference,
    /// Language preference (ISO 639-1 code, e.g., "en", "es")
    pub language: String,
    /// Subtitle display preferences
    pub subtitle_preferences: SubtitlePreferences,
    /// Playback behavior preferences
    pub playback_preferences: PlaybackPreferences,
    /// UI customization preferences
    pub ui_preferences: UiPreferences,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            auto_login_enabled: false,
            theme: ThemePreference::default(),
            language: "en".to_string(),
            subtitle_preferences: SubtitlePreferences::default(),
            playback_preferences: PlaybackPreferences::default(),
            ui_preferences: UiPreferences::default(),
        }
    }
}

/// Theme preference options
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ThemePreference {
    Light,
    Dark,
    #[default]
    System, // Follow system theme
}

/// Subtitle display preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitlePreferences {
    /// Whether to show subtitles by default
    pub enabled_by_default: bool,
    /// Preferred subtitle language (ISO 639-1 code)
    pub preferred_language: Option<String>,
    /// Font size multiplier (1.0 = normal)
    pub font_scale: f32,
}

impl Default for SubtitlePreferences {
    fn default() -> Self {
        Self {
            enabled_by_default: false,
            preferred_language: None,
            font_scale: 1.0,
        }
    }
}

/// Playback behavior preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackPreferences {
    /// Whether to auto-play next episode
    pub auto_play_next: bool,
    /// Skip intro duration in seconds (0 = disabled)
    pub skip_intro_duration: u32,
    /// Skip credits duration in seconds (0 = disabled)
    pub skip_credits_duration: u32,
    /// Preferred playback quality
    pub preferred_quality: PlaybackQuality,
    /// Resume playback behavior
    pub resume_behavior: ResumeBehavior,
}

impl Default for PlaybackPreferences {
    fn default() -> Self {
        Self {
            auto_play_next: true,
            skip_intro_duration: 0,
            skip_credits_duration: 0,
            preferred_quality: PlaybackQuality::Auto,
            resume_behavior: ResumeBehavior::Ask,
        }
    }
}

/// Playback quality options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlaybackQuality {
    Auto,
    Original,
    High4K,     // 4K
    High1080p,  // 1080p
    Medium720p, // 720p
    Low480p,    // 480p
}

impl std::fmt::Display for PlaybackQuality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "Auto"),
            Self::Original => write!(f, "Original"),
            Self::High4K => write!(f, "4K"),
            Self::High1080p => write!(f, "1080p"),
            Self::Medium720p => write!(f, "720p"),
            Self::Low480p => write!(f, "480p"),
        }
    }
}

/// Resume playback behavior
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResumeBehavior {
    Always, // Always resume from last position
    Ask,    // Ask user whether to resume
    Never,  // Always start from beginning
}

impl std::fmt::Display for ResumeBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Always => write!(f, "Always Resume"),
            Self::Ask => write!(f, "Ask Me"),
            Self::Never => write!(f, "Start from Beginning"),
        }
    }
}

/// UI customization preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiPreferences {
    /// Show poster titles on hover only
    pub poster_titles_on_hover: bool,
    /// Grid size preference for library view
    pub library_grid_size: GridSize,
    /// Show recently watched section
    pub show_recently_watched: bool,
    /// Show continue watching section
    pub show_continue_watching: bool,
    /// Sidebar collapsed state
    pub sidebar_collapsed: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            poster_titles_on_hover: false,
            library_grid_size: GridSize::Medium,
            show_recently_watched: true,
            show_continue_watching: true,
            sidebar_collapsed: false,
        }
    }
}

/// Grid size options for library view
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GridSize {
    Small,  // More items per row
    Medium, // Default
    Large,  // Fewer items per row
}

impl std::fmt::Display for GridSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Small => write!(f, "Small"),
            Self::Medium => write!(f, "Medium"),
            Self::Large => write!(f, "Large"),
        }
    }
}

/// Authentication token response containing the opaque session token and
/// refresh token returned after successful login. The `access_token` field is
/// now an opaque session secret (no longer a JWT). Clients present it as a
/// bearer token until `expires_in` elapses, then rotate via the refresh token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    /// Opaque access token (session secret) for API authentication
    pub access_token: String,
    /// Opaque refresh token for obtaining new access tokens (30 day expiry)
    pub refresh_token: String,
    /// Seconds until the access token expires (typically 900)
    pub expires_in: u32,
    /// Optional identifier of the persisted session record
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    /// Optional trusted device session identifier tied to the login
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_session_id: Option<Uuid>,
    /// Optional user id for convenience in single-request flows
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,
    /// Scope describing the effective permissions granted to the session
    #[serde(default)]
    pub scope: SessionScope,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_token_scope_defaults_to_full() {
        let raw = r#"{
            "access_token": "<REDACTED>",
            "refresh_token": "refresh",
            "expires_in": 3600
        }"#;

        let token: AuthToken =
            serde_json::from_str(raw).expect("token deserializes");
        assert_eq!(token.scope, SessionScope::Full);
    }
}

/// User session for tracking active devices
///
/// Each login creates a session entry that tracks the device, location,
/// and activity. Sessions can be managed and revoked by users.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSession {
    /// Unique session identifier
    pub id: Uuid,
    /// User this session belongs to
    pub user_id: Uuid,
    /// Optional device name provided during login
    pub device_name: Option<String>,
    /// IP address of the login request
    pub ip_address: Option<String>,
    /// User agent string from the login request
    pub user_agent: Option<String>,
    /// Last activity timestamp (Unix epoch seconds)
    pub last_active: i64,
    /// Session creation timestamp (Unix epoch seconds)
    pub created_at: i64,
}

/// Login request payload
///
/// Used to authenticate a user with username and password.
/// Optionally includes device information for session tracking.
///
/// # Example
///
/// ```json
/// {
///   "username": "alice",
///   "password": "secure_password",
///   "device_name": "Alice's iPhone"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    /// Username (case-insensitive)
    pub username: String,
    /// Plain text password (will be verified against hash)
    pub password: String,
    /// Optional device name for session tracking
    pub device_name: Option<String>,
}

/// Registration request payload
///
/// Used to create a new user account. Username must be unique
/// and will be normalized to lowercase.
///
/// # Example
///
/// ```json
/// {
///   "username": "alice",
///   "password": "secure_password",
///   "display_name": "Alice Smith"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    /// Desired username (3-20 chars, alphanumeric + underscore)
    pub username: String,
    /// Plain text password (PIN for authentication)
    pub password: String,
    /// Display name for the user
    pub display_name: String,
}

/// JWT Claims for access tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,   // User ID
    pub exp: i64,    // Expiration time
    pub iat: i64,    // Issued at
    pub jti: String, // JWT ID for revocation
}

/// Authentication errors
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum AuthError {
    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Username already taken")]
    UsernameTaken,

    #[error("Token expired")]
    TokenExpired,

    #[error("Invalid token")]
    TokenInvalid,

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Internal error")]
    InternalError,
}

/// Validation errors for user input
#[derive(Debug, Clone, thiserror::Error)]
pub enum ValidationError {
    #[error(
        "Invalid username: must be 3-30 characters, alphanumeric or underscore"
    )]
    InvalidUsername,

    #[error("Password too short: minimum 8 characters required")]
    PasswordTooShort,

    #[error("Invalid display name: must be 1-100 characters")]
    InvalidDisplayName,
}

impl RegisterRequest {
    /// Validate registration request
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Username: 3-30 chars, alphanumeric + underscore
        if self.username.len() < 3 || self.username.len() > 30 {
            return Err(ValidationError::InvalidUsername);
        }

        if !self
            .username
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
        {
            return Err(ValidationError::InvalidUsername);
        }

        // Display name: 1-100 chars
        if self.display_name.is_empty() || self.display_name.len() > 100 {
            return Err(ValidationError::InvalidDisplayName);
        }

        Ok(())
    }
}

/// Request to update user profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUpdateRequest {
    pub display_name: Option<String>,
    pub current_password: Option<String>,
    pub new_password: Option<String>,
}

impl UserUpdateRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Validate display name if provided
        if let Some(ref name) = self.display_name {
            if name.trim().is_empty() {
                return Err(ValidationError::InvalidDisplayName);
            }
            if name.len() > 100 {
                return Err(ValidationError::InvalidDisplayName);
            }
        }

        Ok(())
    }
}
