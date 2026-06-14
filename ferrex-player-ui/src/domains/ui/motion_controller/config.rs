//! Configuration for the motion controller
//!
//! This struct configures the motion controller in a unit-agnostic way so the
//! same engine can be used for vertical grid (rows) and horizontal carousel
//! (item stride) scrolling.

#[derive(Debug, Clone, Copy)]
pub struct MotionControllerConfig {
    /// Recommended tick interval (ns). Consumers can drive timers based on this.
    pub tick_ns: u64,
    /// Acceleration time constant (ms). If 0, derive from ramp via ratio.
    pub accel_tau_ms: u64,
    /// When accel_tau_ms == 0, tau = ramp_ms * accel_tau_to_ramp_ratio.
    pub accel_tau_to_ramp_ratio: f32,
    /// Decay time constant after key release (ms).
    pub decay_tau_ms: u64,
    /// Baseline units per second at hold start.
    pub base_units_per_s: f32,
    /// Maximum units per second at sustained hold.
    pub max_units_per_s: f32,
    /// Minimum units per second threshold to stop during decay.
    pub min_units_per_s_stop: f32,
    /// Ramp duration from BASE->MAX using easing (ms).
    pub ramp_ms: u64,
    /// Easing kind: 0=Linear, 1=EaseIn, 2=EaseOut, 3=EaseInOut.
    pub easing_kind: u8,
    /// Boost multiplier while a modifier (e.g., Shift) is held.
    pub boost_multiplier: f32,
}

impl Default for MotionControllerConfig {
    fn default() -> Self {
        // Backward-compatible defaults from performance_config::scrolling (grid)
        use crate::infra::constants::performance_config::scrolling as grid_cfg;
        Self {
            tick_ns: grid_cfg::TICK_NS,
            accel_tau_ms: grid_cfg::ACCEL_TAU_MS,
            accel_tau_to_ramp_ratio: grid_cfg::ACCEL_TAU_TO_RAMP_RATIO,
            decay_tau_ms: grid_cfg::DECAY_TAU_MS,
            base_units_per_s: grid_cfg::BASE_ROWS_PER_S,
            max_units_per_s: grid_cfg::MAX_ROWS_PER_S,
            min_units_per_s_stop: grid_cfg::MIN_ROWS_PER_S_STOP,
            ramp_ms: grid_cfg::RAMP_MS,
            easing_kind: grid_cfg::EASING_KIND,
            boost_multiplier: grid_cfg::BOOST_MULTIPLIER,
        }
    }
}

impl MotionControllerConfig {
    /// Create grid scrolling config from runtime settings.
    pub fn from_runtime_config(
        rc: &crate::infra::runtime_config::RuntimeConfig,
    ) -> Self {
        use crate::infra::constants::performance_config::scrolling as grid_cfg;
        Self {
            // These are kept static (not exposed in settings UI)
            tick_ns: grid_cfg::TICK_NS,
            accel_tau_ms: grid_cfg::ACCEL_TAU_MS,
            accel_tau_to_ramp_ratio: grid_cfg::ACCEL_TAU_TO_RAMP_RATIO,
            min_units_per_s_stop: grid_cfg::MIN_ROWS_PER_S_STOP,
            // These come from runtime config
            decay_tau_ms: rc.scroll_decay_tau_ms(),
            base_units_per_s: rc.scroll_base_velocity(),
            max_units_per_s: rc.scroll_max_velocity(),
            ramp_ms: rc.scroll_ramp_ms(),
            easing_kind: rc.scroll_easing().to_u8(),
            boost_multiplier: rc.scroll_boost_multiplier(),
        }
    }
}
