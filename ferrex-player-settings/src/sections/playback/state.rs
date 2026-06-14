//! Playback section state
//!
//! Contains all state related to playback settings.
//! Many of these correspond to constants in infra::constants::player

use serde::{Deserialize, Serialize};

/// Playback settings state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackState {
    // General subsection
    /// Auto-play next episode in series
    pub auto_play_next: bool,
    /// How to handle resume points
    pub resume_behavior: ResumeBehavior,
    /// Preferred streaming quality
    pub preferred_quality: PlaybackQuality,

    // Seeking subsection (from constants::player::seeking)
    /// Coarse seek forward amount in seconds (default: 30.0)
    pub seek_forward_coarse: f64,
    /// Coarse seek backward amount in seconds (default: 15.0)
    pub seek_backward_coarse: f64,
    /// Fine seek forward amount in seconds (default: 15.0)
    pub seek_forward_fine: f64,
    /// Fine seek backward amount in seconds (default: 10.0)
    pub seek_backward_fine: f64,

    // Skip subsection
    /// Duration to skip for intro in seconds
    pub skip_intro_duration: u32,
    /// Duration to skip for credits in seconds
    pub skip_credits_duration: u32,

    // Subtitles subsection
    /// Show subtitles by default
    pub subtitles_enabled: bool,
    /// Preferred subtitle language (ISO 639-1)
    pub subtitle_language: Option<String>,
    /// Subtitle font scale (0.5 - 2.0)
    pub subtitle_font_scale: f32,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            // General
            auto_play_next: true,
            resume_behavior: ResumeBehavior::default(),
            preferred_quality: PlaybackQuality::default(),

            // Seeking (matches constants::player::seeking defaults)
            seek_forward_coarse: 30.0,
            seek_backward_coarse: 15.0,
            seek_forward_fine: 15.0,
            seek_backward_fine: 10.0,

            // Skip
            skip_intro_duration: 0,
            skip_credits_duration: 0,

            // Subtitles
            subtitles_enabled: false,
            subtitle_language: None,
            subtitle_font_scale: 1.0,
        }
    }
}

/// Resume behavior options
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum ResumeBehavior {
    /// Always resume from last position
    #[default]
    Always,
    /// Ask before resuming
    Ask,
    /// Never resume, start from beginning
    Never,
}

impl ResumeBehavior {
    pub const ALL: [ResumeBehavior; 3] = [Self::Always, Self::Ask, Self::Never];
}

impl std::fmt::Display for ResumeBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Always => write!(f, "Always"),
            Self::Ask => write!(f, "Ask"),
            Self::Never => write!(f, "Never"),
        }
    }
}

/// Playback quality options
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum PlaybackQuality {
    /// Automatically select based on bandwidth
    #[default]
    Auto,
    /// Original quality (no transcoding)
    Original,
    /// 4K UHD
    UHD4K,
    /// 1080p Full HD
    FHD1080,
    /// 720p HD
    HD720,
    /// 480p SD
    SD480,
}

impl PlaybackQuality {
    pub const ALL: [PlaybackQuality; 6] = [
        Self::Auto,
        Self::Original,
        Self::UHD4K,
        Self::FHD1080,
        Self::HD720,
        Self::SD480,
    ];
}

impl std::fmt::Display for PlaybackQuality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "Auto"),
            Self::Original => write!(f, "Original"),
            Self::UHD4K => write!(f, "4K"),
            Self::FHD1080 => write!(f, "1080p"),
            Self::HD720 => write!(f, "720p"),
            Self::SD480 => write!(f, "480p"),
        }
    }
}
