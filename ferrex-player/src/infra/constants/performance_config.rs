//! Performance configuration constants for the Ferrex Player
//!
//! This module centralizes all performance-related settings to make tuning easier.
//! Adjust these values based on your target hardware and performance requirements.

/// Scrolling performance configuration
pub mod scrolling {
    /// Time to wait before considering scroll stopped (milliseconds)
    /// Lower values = quicker poster loading after scroll
    pub const SCROLL_STOP_DEBOUNCE_MS: u64 = 20;

    // Kinetic arrow-key scrolling parameters (grid-only)
    /// Tick interval for kinetic scrolling (nanoseconds). ~120 FPS.
    /// Lower values = smoother but more CPU; higher values = choppier.
    /// Reasonable: 8_333_333 (120Hz) or 16_666_666 (60Hz).
    pub const TICK_NS: u64 = 8_333_333;
    /// Acceleration time constant (ms) for ramping actual velocity toward target while holding.
    /// Smaller values = snappier response. Larger values = heavier feel.
    /// Reasonable: 150–400. Valid: 1–5000. Very large values (>1500) rise very slowly.
    /// Set to 0 to auto-derive: tau = max(50ms, KINETIC_RAMP_MS × KINETIC_ACCEL_TAU_TO_RAMP_RATIO).
    /// Rule of thumb: time to ~90% of target ≈ 2.3 × tau.
    pub const ACCEL_TAU_MS: u64 = 0;
    /// When KINETIC_ACCEL_TAU_MS == 0, tau = KINETIC_RAMP_MS × this ratio.
    /// Reasonable: 0.3–0.6. Lower = snappier than the ramp; higher = heavier than the ramp.
    pub const ACCEL_TAU_TO_RAMP_RATIO: f32 = 0.4; // Default: 0.4
    /// Decay time constant (ms) for gliding after release (exponential decay toward zero).
    /// Reasonable: 180–320. Valid: 1–3000. Larger = longer glide.
    pub const DECAY_TAU_MS: u64 = 240;
    /// Baseline rows per second at the start of a hold (gentle start).
    /// The target starts at this value and ramps to MAX over KINETIC_RAMP_MS with KINETIC_EASING_KIND.
    /// Reasonable: 0.25–0.75 rows/s. Valid: 0.01–20. Higher values reduce perceived ramp.
    pub const BASE_ROWS_PER_S: f32 = 0.5;
    /// Maximum rows per second achievable with a long hold.
    /// Defines the upper bound for the target before filtering.
    /// Reasonable: 1.0–10.0 rows/s. Valid: 0.1–50.
    pub const MAX_ROWS_PER_S: f32 = 5.0;
    /// Minimum rows per second threshold to stop during decay.
    /// If |v| falls below this (converted to DIP/s via row height), kinetic scrolling stops.
    /// Reasonable: 0.05–0.15 rows/s. Valid: 0.0–2.0.
    pub const MIN_ROWS_PER_S_STOP: f32 = 0.08;
    /// Duration for ramping target from BASE->MAX using easing (ms). Independent of ACCEL_TAU.
    /// Smaller values = faster perceived ramp; larger = slower ramp.
    /// Reasonable: 300–1200. Valid: 0–5000. Set to 0 to disable ramp shaping (target jumps to MAX; ACCEL_TAU controls approach).
    pub const RAMP_MS: u64 = 1200;
    /// Easing kind for target ramp: 0=Linear, 1=EaseIn, 2=EaseOut, 3=EaseInOut.
    /// EaseOut is punchier early; EaseInOut is more symmetric.
    pub const EASING_KIND: u8 = 2;

    /// Boost multiplier applied to MAX_ROWS_PER_S while a boost modifier is held (e.g., Shift).
    /// Reasonable: 2.0–6.0. Valid: 1.0–20. Higher values can be disorienting.
    pub const BOOST_MULTIPLIER: f32 = 4.0;
}

pub mod memory_usage {
    /// One GiB in Bytes
    const GIB: u64 = 1_073_741_824;

    /// 2GiB default max ram usage
    pub const MAX_RAM_BYTES: u64 = GIB;
    /// 5GiB default max ram usage
    pub const MAX_IMAGE_CACHE_BYTES: u64 = 5 * GIB;
}
