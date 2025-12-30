//! Performance section state
//!
//! Contains all state related to performance tuning.
//! These correspond to constants in infra::constants::performance_config
//! and infra::constants::virtual_carousel

use serde::{Deserialize, Serialize};

fn default_animation_hover_scale_down_delay_ms() -> u64 {
    crate::infra::constants::layout::animation::HOVER_SCALE_DOWN_DELAY_MS
}

/// Performance settings state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceState {
    // Scrolling subsection (from constants::performance_config::scrolling)
    /// Scroll stop debounce in milliseconds (default: 20)
    pub scroll_debounce_ms: u64,
    /// Tick interval in nanoseconds (default: 8_333_333 = ~120Hz)
    pub scroll_tick_ns: u64,
    /// Decay tau in milliseconds - glide duration (default: 240)
    pub scroll_decay_tau_ms: u64,
    /// Base scroll velocity in rows/second (default: 0.5)
    pub scroll_base_velocity: f32,
    /// Maximum scroll velocity in rows/second (default: 5.0)
    pub scroll_max_velocity: f32,
    /// Minimum velocity to stop scrolling (default: 0.08)
    pub scroll_min_stop_velocity: f32,
    /// Velocity ramp duration in milliseconds (default: 1200)
    pub scroll_ramp_ms: u64,
    /// Boost multiplier when shift is held (default: 4.0)
    pub scroll_boost_multiplier: f32,
    /// Easing kind: 0=Linear, 1=EaseIn, 2=EaseOut, 3=EaseInOut (default: 2)
    pub scroll_easing: EasingKind,

    // Texture Upload subsection (from constants::performance_config::texture_upload)
    /// Maximum texture uploads per frame (default: 6)
    pub texture_max_uploads_per_frame: u32,

    // Prefetch subsection (from constants::layout::virtual_grid)
    /// Rows to prefetch above viewport (default: 1)
    pub prefetch_rows_above: usize,
    /// Rows to prefetch below viewport (default: 1)
    pub prefetch_rows_below: usize,
    /// Keep-alive duration for cached images in ms (default: 50000)
    pub prefetch_keep_alive_ms: u64,

    // Carousel subsection (from constants::virtual_carousel)
    /// Carousel prefetch items ahead (default: 8)
    pub carousel_prefetch_items: usize,
    /// Carousel background fetch items (default: 16)
    pub carousel_background_items: usize,
    /// Carousel base velocity in items/second (default: 0.75)
    pub carousel_base_velocity: f32,
    /// Carousel max velocity in items/second (default: 6.0)
    pub carousel_max_velocity: f32,
    /// Carousel boost multiplier (default: 4.0)
    pub carousel_boost_multiplier: f32,
    /// Carousel ramp duration in ms (default: 1000)
    pub carousel_ramp_ms: u64,
    /// Carousel decay tau in ms (default: 240)
    pub carousel_decay_tau_ms: u64,
    /// Single item snap duration in ms (default: 180)
    pub carousel_item_snap_ms: u64,
    /// Page snap duration in ms (default: 240)
    pub carousel_page_snap_ms: u64,
    /// Hold tap threshold in ms (default: 260)
    pub carousel_hold_tap_threshold_ms: u64,
    /// Snap epsilon fraction (default: 0.06)
    pub carousel_snap_epsilon: f32,
    /// Anchor settle time in ms (default: 10)
    pub carousel_anchor_settle_ms: u64,

    // Animation Effects subsection (from constants::layout::animation)
    /// Hover scale factor (default: 1.05)
    pub animation_hover_scale: f32,
    /// Hover scale transition duration in ms (default: 150)
    pub animation_hover_transition_ms: u64,
    /// Delay before scaling down after hover ends in ms (default: 75)
    #[serde(default = "default_animation_hover_scale_down_delay_ms")]
    pub animation_hover_scale_down_delay_ms: u64,
}

impl Default for PerformanceState {
    fn default() -> Self {
        Self {
            // Scrolling (matches constants::performance_config::scrolling)
            scroll_debounce_ms: 20,
            scroll_tick_ns: 8_333_333,
            scroll_decay_tau_ms: 240,
            scroll_base_velocity: 0.5,
            scroll_max_velocity: 5.0,
            scroll_min_stop_velocity: 0.08,
            scroll_ramp_ms: 1200,
            scroll_boost_multiplier: 4.0,
            scroll_easing: EasingKind::default(),

            // Texture Upload
            texture_max_uploads_per_frame: 6,

            // Prefetch
            prefetch_rows_above: 1,
            prefetch_rows_below: 1,
            prefetch_keep_alive_ms: 50000,

            // Carousel (matches constants::virtual_carousel)
            carousel_prefetch_items: 8,
            carousel_background_items: 16,
            carousel_base_velocity: 0.75,
            carousel_max_velocity: 6.0,
            carousel_boost_multiplier: 4.0,
            carousel_ramp_ms: 1000,
            carousel_decay_tau_ms: 240,
            carousel_item_snap_ms: 180,
            carousel_page_snap_ms: 240,
            carousel_hold_tap_threshold_ms: 260,
            carousel_snap_epsilon: 0.06,
            carousel_anchor_settle_ms: 10,

            // Animation Effects (matches constants::layout::animation)
            animation_hover_scale: 1.05,
            animation_hover_transition_ms: 150,
            animation_hover_scale_down_delay_ms:
                default_animation_hover_scale_down_delay_ms(),
        }
    }
}

/// Easing function kind
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum EasingKind {
    Linear = 0,
    EaseIn = 1,
    #[default]
    EaseOut = 2,
    EaseInOut = 3,
}

impl EasingKind {
    pub const ALL: [EasingKind; 4] =
        [Self::Linear, Self::EaseIn, Self::EaseOut, Self::EaseInOut];

    /// Convert to u8 for constants compatibility
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }

    /// Create from u8
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Linear,
            1 => Self::EaseIn,
            2 => Self::EaseOut,
            3 => Self::EaseInOut,
            _ => Self::EaseOut,
        }
    }
}

impl std::fmt::Display for EasingKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Linear => write!(f, "Linear"),
            Self::EaseIn => write!(f, "Ease In"),
            Self::EaseOut => write!(f, "Ease Out"),
            Self::EaseInOut => write!(f, "Ease In-Out"),
        }
    }
}
