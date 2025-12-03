//! Runtime configuration for user-adjustable constants
//!
//! This module provides a RuntimeConfig struct with Option<T> fields that override
//! the default constants. Accessor methods fall back to constants when None.

use crate::infra::constants::{
    layout::animation, layout::virtual_grid, performance_config, player,
    virtual_carousel,
};

/// Easing function type for animations
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EasingKind {
    Linear = 0,
    EaseIn = 1,
    #[default]
    EaseOut = 2,
    EaseInOut = 3,
}

impl EasingKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Linear,
            1 => Self::EaseIn,
            2 => Self::EaseOut,
            3 => Self::EaseInOut,
            _ => Self::EaseOut,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub const ALL: [Self; 4] =
        [Self::Linear, Self::EaseIn, Self::EaseOut, Self::EaseInOut];
}

impl std::fmt::Display for EasingKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Linear => write!(f, "Linear"),
            Self::EaseIn => write!(f, "Ease In"),
            Self::EaseOut => write!(f, "Ease Out"),
            Self::EaseInOut => write!(f, "Ease In/Out"),
        }
    }
}

/// Runtime configuration with optional overrides for constants.
/// Fields are None by default, falling back to compiled constants.
#[derive(Debug, Default)]
pub struct RuntimeConfig {
    /// Tracks if any actively-consumed setting was modified since last clear
    pub dirty: bool,

    // ========== GRID SCROLLING ==========
    /// Debounce before considering scroll stopped (ms)
    pub scroll_debounce_ms: Option<u64>,
    /// Base rows per second at start of hold
    pub scroll_base_velocity: Option<f32>,
    /// Maximum rows per second
    pub scroll_max_velocity: Option<f32>,
    /// Decay time constant (ms)
    pub scroll_decay_tau_ms: Option<u64>,
    /// Ramp duration from base to max (ms)
    pub scroll_ramp_ms: Option<u64>,
    /// Boost multiplier when shift held
    pub scroll_boost_multiplier: Option<f32>,
    /// Easing function for scroll ramp
    pub scroll_easing: Option<EasingKind>,

    // ========== CAROUSEL MOTION ==========
    /// Base items per second for carousel
    pub carousel_base_velocity: Option<f32>,
    /// Maximum items per second for carousel
    pub carousel_max_velocity: Option<f32>,
    /// Carousel decay time constant (ms)
    pub carousel_decay_tau_ms: Option<u64>,
    /// Carousel ramp duration (ms)
    pub carousel_ramp_ms: Option<u64>,
    /// Carousel boost multiplier
    pub carousel_boost_multiplier: Option<f32>,
    /// Carousel easing function
    pub carousel_easing: Option<EasingKind>,

    // ========== SNAP ANIMATIONS ==========
    /// Item snap duration (ms)
    pub snap_item_duration_ms: Option<u64>,
    /// Page snap duration (ms)
    pub snap_page_duration_ms: Option<u64>,
    /// Hold/tap threshold (ms)
    pub snap_hold_threshold_ms: Option<u64>,
    /// Snap epsilon fraction
    pub snap_epsilon_fraction: Option<f32>,
    /// Snap easing function
    pub snap_easing: Option<EasingKind>,

    // ========== ANIMATION EFFECTS ==========
    /// Hover scale factor
    pub animation_hover_scale: Option<f32>,
    /// Hover scale transition duration (ms)
    pub animation_hover_transition_ms: Option<u64>,
    /// Default animation duration (ms)
    pub animation_default_duration_ms: Option<u64>,
    /// Initial texture fade duration (ms)
    pub animation_texture_fade_initial_ms: Option<u64>,
    /// Texture fade duration (ms)
    pub animation_texture_fade_ms: Option<u64>,

    // ========== GPU/MEMORY ==========
    /// Max texture uploads per frame
    pub texture_max_uploads: Option<u32>,
    /// Prefetch rows above viewport
    pub prefetch_rows_above: Option<usize>,
    /// Prefetch rows below viewport
    pub prefetch_rows_below: Option<usize>,
    /// Carousel prefetch items
    pub carousel_prefetch_items: Option<usize>,
    /// Carousel background items
    pub carousel_background_items: Option<usize>,
    /// Keep-alive duration (ms)
    pub keep_alive_ms: Option<u64>,

    // ========== PLAYER SEEKING ==========
    /// Coarse seek forward (seconds)
    pub seek_forward_coarse: Option<f64>,
    /// Coarse seek backward (seconds, positive value)
    pub seek_backward_coarse: Option<f64>,
    /// Fine seek forward (seconds)
    pub seek_forward_fine: Option<f64>,
    /// Fine seek backward (seconds, positive value)
    pub seek_backward_fine: Option<f64>,
}

impl RuntimeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark config as dirty (an in-use setting was changed)
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Clear dirty flag and return whether it was dirty
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    // ========== GRID SCROLLING ACCESSORS ==========

    pub fn scroll_debounce_ms(&self) -> u64 {
        self.scroll_debounce_ms
            .unwrap_or(performance_config::scrolling::SCROLL_STOP_DEBOUNCE_MS)
    }

    pub fn scroll_base_velocity(&self) -> f32 {
        self.scroll_base_velocity
            .unwrap_or(performance_config::scrolling::BASE_ROWS_PER_S)
    }

    pub fn scroll_max_velocity(&self) -> f32 {
        self.scroll_max_velocity
            .unwrap_or(performance_config::scrolling::MAX_ROWS_PER_S)
    }

    pub fn scroll_decay_tau_ms(&self) -> u64 {
        self.scroll_decay_tau_ms
            .unwrap_or(performance_config::scrolling::DECAY_TAU_MS)
    }

    pub fn scroll_ramp_ms(&self) -> u64 {
        self.scroll_ramp_ms
            .unwrap_or(performance_config::scrolling::RAMP_MS)
    }

    pub fn scroll_boost_multiplier(&self) -> f32 {
        self.scroll_boost_multiplier
            .unwrap_or(performance_config::scrolling::BOOST_MULTIPLIER)
    }

    pub fn scroll_easing(&self) -> EasingKind {
        self.scroll_easing.unwrap_or_else(|| {
            EasingKind::from_u8(performance_config::scrolling::EASING_KIND)
        })
    }

    // ========== CAROUSEL MOTION ACCESSORS ==========

    pub fn carousel_base_velocity(&self) -> f32 {
        self.carousel_base_velocity
            .unwrap_or(virtual_carousel::motion::BASE_ITEMS_PER_S)
    }

    pub fn carousel_max_velocity(&self) -> f32 {
        self.carousel_max_velocity
            .unwrap_or(virtual_carousel::motion::MAX_ITEMS_PER_S)
    }

    pub fn carousel_decay_tau_ms(&self) -> u64 {
        self.carousel_decay_tau_ms
            .unwrap_or(virtual_carousel::motion::DECAY_TAU_MS)
    }

    pub fn carousel_ramp_ms(&self) -> u64 {
        self.carousel_ramp_ms
            .unwrap_or(virtual_carousel::motion::RAMP_MS)
    }

    pub fn carousel_boost_multiplier(&self) -> f32 {
        self.carousel_boost_multiplier
            .unwrap_or(virtual_carousel::motion::BOOST_MULTIPLIER)
    }

    pub fn carousel_easing(&self) -> EasingKind {
        self.carousel_easing.unwrap_or_else(|| {
            EasingKind::from_u8(virtual_carousel::motion::EASING_KIND)
        })
    }

    // ========== SNAP ANIMATION ACCESSORS ==========

    pub fn snap_item_duration_ms(&self) -> u64 {
        self.snap_item_duration_ms
            .unwrap_or(virtual_carousel::snap::ITEM_DURATION_MS)
    }

    pub fn snap_page_duration_ms(&self) -> u64 {
        self.snap_page_duration_ms
            .unwrap_or(virtual_carousel::snap::PAGE_DURATION_MS)
    }

    pub fn snap_hold_threshold_ms(&self) -> u64 {
        self.snap_hold_threshold_ms
            .unwrap_or(virtual_carousel::snap::HOLD_TAP_THRESHOLD_MS)
    }

    pub fn snap_epsilon_fraction(&self) -> f32 {
        self.snap_epsilon_fraction
            .unwrap_or(virtual_carousel::snap::SNAP_EPSILON_FRACTION)
    }

    pub fn snap_easing(&self) -> EasingKind {
        self.snap_easing.unwrap_or_else(|| {
            EasingKind::from_u8(virtual_carousel::snap::EASING_KIND)
        })
    }

    // ========== ANIMATION EFFECT ACCESSORS ==========

    pub fn animation_hover_scale(&self) -> f32 {
        self.animation_hover_scale.unwrap_or(animation::HOVER_SCALE)
    }

    pub fn animation_hover_transition_ms(&self) -> u64 {
        self.animation_hover_transition_ms
            .unwrap_or(animation::HOVER_TRANSITION_MS)
    }

    pub fn animation_default_duration_ms(&self) -> u64 {
        self.animation_default_duration_ms
            .unwrap_or(animation::DEFAULT_DURATION_MS)
    }

    pub fn animation_texture_fade_initial_ms(&self) -> u64 {
        self.animation_texture_fade_initial_ms
            .unwrap_or(animation::TEXTURE_FADE_INITIAL_DURATION_MS)
    }

    pub fn animation_texture_fade_ms(&self) -> u64 {
        self.animation_texture_fade_ms
            .unwrap_or(animation::TEXTURE_FADE_DURATION_MS)
    }

    /// Bundle animation settings into a config struct for shader widgets.
    /// This avoids passing RuntimeConfig into low-level rendering code.
    pub fn animation_config(
        &self,
    ) -> crate::infra::shader_widgets::poster::animation::AnimationConfig {
        crate::infra::shader_widgets::poster::animation::AnimationConfig {
            default_duration_ms: self.animation_default_duration_ms(),
            texture_fade_initial_ms: self.animation_texture_fade_initial_ms(),
            texture_fade_ms: self.animation_texture_fade_ms(),
            hover_scale: self.animation_hover_scale(),
            hover_transition_ms: self.animation_hover_transition_ms(),
        }
    }

    // ========== GPU/MEMORY ACCESSORS ==========

    pub fn texture_max_uploads(&self) -> u32 {
        self.texture_max_uploads.unwrap_or(
            performance_config::texture_upload::MAX_UPLOADS_PER_FRAME,
        )
    }

    pub fn prefetch_rows_above(&self) -> usize {
        self.prefetch_rows_above
            .unwrap_or(virtual_grid::PREFETCH_ROWS_ABOVE)
    }

    pub fn prefetch_rows_below(&self) -> usize {
        self.prefetch_rows_below
            .unwrap_or(virtual_grid::PREFETCH_ROWS_BELOW)
    }

    pub fn carousel_prefetch_items(&self) -> usize {
        self.carousel_prefetch_items
            .unwrap_or(virtual_carousel::windows::PREFETCH_ITEMS)
    }

    pub fn carousel_background_items(&self) -> usize {
        self.carousel_background_items
            .unwrap_or(virtual_carousel::windows::BACKGROUND_ITEMS)
    }

    pub fn keep_alive_ms(&self) -> u64 {
        self.keep_alive_ms.unwrap_or(virtual_grid::KEEP_ALIVE_MS)
    }

    // ========== PLAYER SEEKING ACCESSORS ==========

    pub fn seek_forward_coarse(&self) -> f64 {
        self.seek_forward_coarse
            .unwrap_or(player::seeking::SEEK_FORWARD_COURSE)
    }

    pub fn seek_backward_coarse(&self) -> f64 {
        // Return positive value, constant stores negative
        self.seek_backward_coarse
            .unwrap_or(player::seeking::SEEK_BACKWARD_COURSE.abs())
    }

    pub fn seek_forward_fine(&self) -> f64 {
        self.seek_forward_fine
            .unwrap_or(player::seeking::SEEK_FORWARD_FINE)
    }

    pub fn seek_backward_fine(&self) -> f64 {
        // Return positive value, constant stores negative
        self.seek_backward_fine
            .unwrap_or(player::seeking::SEEK_BACKWARD_FINE.abs())
    }
}
