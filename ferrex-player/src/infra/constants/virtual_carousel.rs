//! Virtual Carousel constants
//!
//! Shared constants for virtual carousel behavior, including window sizes
//! for prefetch/background and kinetic scrolling defaults. Tuning should
//! happen here so all carousels update consistently.

/// Defaults for preload/background window sizes relative to visible range.
pub mod windows {
    /// Number of items to prefetch ahead of the currently visible window.
    pub const PREFETCH_ITEMS: usize = 8;
    /// Additional background window items beyond prefetch.
    pub const BACKGROUND_ITEMS: usize = 16;
}

/// Animation and scrolling defaults for kinetic horizontal motion.
/// These can be aligned with the grid's vertical kinetic settings or
/// diverge if carousels require different feel.
pub mod motion {
    /// Tick interval in nanoseconds (aim ~120 FPS like grid kinetic).
    pub const TICK_NS: u64 = 8_333_333;
    /// Decay time constant (ms) after key release.
    pub const DECAY_TAU_MS: u64 = 240;
    /// Base items per second at start of a hold.
    pub const BASE_ITEMS_PER_S: f32 = 0.75;
    /// Maximum items per second under sustained hold.
    pub const MAX_ITEMS_PER_S: f32 = 6.0;
    /// Multiplier when boost modifier (e.g., Shift) is held.
    pub const BOOST_MULTIPLIER: f32 = 4.0;
    /// Ramp shaping duration from BASE -> MAX (ms).
    pub const RAMP_MS: u64 = 1000;
    /// Easing kind: 0=Linear, 1=EaseIn, 2=EaseOut, 3=EaseInOut.
    pub const EASING_KIND: u8 = 2;
}

/// Snap/tween animation defaults and planner cadence.
pub mod snap {
    /// Default duration (ms) for per-item snap.
    pub const ITEM_DURATION_MS: u64 = 180;
    /// Default duration (ms) for page snap.
    pub const PAGE_DURATION_MS: u64 = 240;
    /// Easing kind for snaps: 0=Linear, 1=EaseIn, 2=EaseOut, 3=EaseInOut.
    pub const EASING_KIND: u8 = 2; // EaseOut
    /// After releasing a key, if held for less than this threshold (ms), treat as a tap.
    /// Increased to allow rapid tap scrolling to be classified as taps rather than holds.
    pub const HOLD_TAP_THRESHOLD_MS: u64 = 260;
    /// If within this fraction of a stride from the nearest boundary after kinetic, skip tween.
    pub const SNAP_EPSILON_FRACTION: f32 = 0.06; // ~6% of stride
    /// Debounce planner snapshot interval during motion (ms).
    pub const SNAPSHOT_DEBOUNCE_MS: u64 = 33; // ~30Hz
    /// Time the viewport must remain near an aligned boundary before
    /// committing the `reference_index` for mouse/trackpad scrolls (ms).
    pub const ANCHOR_SETTLE_MS: u64 = 10;
}

/// Focus and hover-related behavior constants for virtual carousels.
pub mod focus {
    /// Time window (ms) within which mouse movement authorizes hover-driven
    /// focus switches. If the last focus source was mouse, hover remains
    /// preferred while the cursor stays over a carousel.
    pub const HOVER_SWITCH_WINDOW_MS: u64 = 150;
}

/// Layout constants for the virtual carousel view composition.
pub mod layout {
    /// Horizontal padding applied on each side of the carousel container in the view.
    /// The view uses `container(...).padding([0, 20])`, so this is 20 px per side.
    pub const HORIZONTAL_PADDING_SIDE: f32 = 20.0;
    /// Total horizontal padding applied (left + right).
    pub const HORIZONTAL_PADDING_TOTAL: f32 = HORIZONTAL_PADDING_SIDE * 2.0; // 40.0

    /// Approximate height of the horizontal scrollable area (cards strip)
    pub const SCROLL_HEIGHT: f32 = 370.0;
    /// Estimated header height (title + padding)
    pub const HEADER_HEIGHT_EST: f32 = 40.0;
    /// Spacing between header and scroll strip in the view
    pub const HEADER_SCROLL_SPACING: f32 = 10.0;
    /// Gap between sections (column spacing)
    pub const SECTION_GAP: f32 = 30.0;
}
