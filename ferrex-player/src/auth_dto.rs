//! Data Transfer Objects for authentication
//!
//! These types provide a clean separation between server models and client-side usage,
//! preventing tight coupling and allowing independent evolution of both sides.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Client-side user representation
/// This is what the UI works with, not the server's User model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDto {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub is_admin: bool,
    pub preferences: UserPreferencesDto,
}

/// Client-side user preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferencesDto {
    pub theme: ThemePreference,
    pub language: String,
    pub auto_login_enabled: bool,
    pub auto_play_next: bool,
    pub subtitle_language: Option<String>,
    pub subtitle_size: SubtitleSize,
    pub playback_quality: PlaybackQuality,
}

/// Theme preference
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ThemePreference {
    Light,
    Dark,
    System,
}

/// Subtitle size options
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SubtitleSize {
    Small,
    Medium,
    Large,
}

/// Playback quality preference
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum PlaybackQuality {
    Auto,
    Original,
    UHD4K,
    FHD1080p,
    HD720p,
    SD480p,
}

/// Authentication session info
#[derive(Debug, Clone)]
pub struct SessionDto {
    pub user: UserDto,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub device_name: Option<String>,
}

/// Login request DTO
#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequestDto {
    pub username: String,
    pub password: String,
    pub device_name: Option<String>,
    pub remember_me: bool,
}

/// PIN login request DTO
#[derive(Debug, Serialize, Deserialize)]
pub struct PinLoginRequestDto {
    pub user_id: Uuid,
    pub pin: String,
    pub device_id: Uuid,
}

/// User list item for selection screen
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserListItemDto {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub has_pin: bool,
    pub last_login: Option<chrono::DateTime<chrono::Utc>>,
}

/// Converters from server models to DTOs
impl UserDto {
    /// Convert from server User model
    pub fn from_server_model(user: ferrex_core::user::User, is_admin: bool) -> Self {
        Self {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            is_admin,
            preferences: UserPreferencesDto::from_server_model(user.preferences),
        }
    }
}

impl UserPreferencesDto {
    /// Convert from server preferences
    pub fn from_server_model(prefs: ferrex_core::user::UserPreferences) -> Self {
        Self {
            theme: match prefs.theme {
                ferrex_core::user::ThemePreference::Light => ThemePreference::Light,
                ferrex_core::user::ThemePreference::Dark => ThemePreference::Dark,
                ferrex_core::user::ThemePreference::System => ThemePreference::System,
            },
            language: prefs.language,
            auto_login_enabled: prefs.auto_login_enabled,
            auto_play_next: prefs.playback_preferences.auto_play_next,
            subtitle_language: prefs.subtitle_preferences.preferred_language,
            subtitle_size: if prefs.subtitle_preferences.font_scale <= 0.9 {
                SubtitleSize::Small
            } else if prefs.subtitle_preferences.font_scale >= 1.1 {
                SubtitleSize::Large
            } else {
                SubtitleSize::Medium
            },
            playback_quality: match prefs.playback_preferences.preferred_quality {
                ferrex_core::user::PlaybackQuality::Auto => PlaybackQuality::Auto,
                ferrex_core::user::PlaybackQuality::Original => PlaybackQuality::Original,
                ferrex_core::user::PlaybackQuality::High4K => PlaybackQuality::UHD4K,
                ferrex_core::user::PlaybackQuality::High1080p => PlaybackQuality::FHD1080p,
                ferrex_core::user::PlaybackQuality::Medium720p => PlaybackQuality::HD720p,
                ferrex_core::user::PlaybackQuality::Low480p => PlaybackQuality::SD480p,
            },
        }
    }
}