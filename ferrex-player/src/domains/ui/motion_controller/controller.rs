use super::config::MotionControllerConfig;
use std::time::Instant;

#[derive(Debug, Default, Clone)]
pub struct MotionController {
    /// Configuration for the scroller; determines ramp/decay and rates.
    cfg: MotionControllerConfig,
    /// Whether the scroller is engaged (holding or decaying)
    active: bool,
    /// Whether a key is currently held
    holding: bool,
    /// Scroll direction: +1 for down, -1 for up, 0 for none
    dir: i32,
    /// Current velocity in px/s (signed)
    v: f32,
    /// Last tick time
    last_tick: Option<Instant>,
    /// Start of holding (for optional easing)
    hold_started: Option<Instant>,
    /// Boost mode active (e.g., Shift pressed)
    boost_active: bool,
}

impl MotionController {
    /// Create with default config tuned for grid (backward compatible).
    pub fn new() -> Self {
        Self {
            cfg: MotionControllerConfig::default(),
            ..Default::default()
        }
    }

    /// Create with custom kinetic configuration (for carousels or other contexts).
    pub fn new_with_config(cfg: MotionControllerConfig) -> Self {
        Self {
            cfg,
            ..Default::default()
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn start(&mut self, dir: i32) {
        let now = Instant::now();
        let new_dir = dir.signum().clamp(-1, 1);
        // Ignore auto-repeat KeyPressed while already holding the same direction
        if self.active && self.holding && self.dir == new_dir {
            return;
        }

        self.active = true;
        // If direction changed while holding, reset velocity for a clean ramp
        if self.dir != new_dir {
            self.v = 0.0;
        }
        self.holding = true;
        self.dir = new_dir;
        // Initialize timing for ramp and integration
        self.last_tick = Some(now);
        self.hold_started = Some(now);
    }

    pub fn stop_holding(&mut self, _dir: i32) {
        // Only transition to decay; keep active until velocity drops to near-zero
        self.holding = false;
        // keep self.active true; tick() will turn it off when velocity is small
    }

    /// Immediately cancel any kinetic motion and decay.
    ///
    /// Useful for contexts that transition to a different motion mode on
    /// key release (e.g., snapping a carousel to the nearest index), where
    /// continued kinetic decay would conflict with the follow-up animation.
    pub fn abort(&mut self) {
        self.active = false;
        self.holding = false;
        self.v = 0.0;
    }

    pub fn set_boost(&mut self, active: bool) {
        self.boost_active = active;
    }

    /// Advance the scroller by dt and return the new absolute offset if movement occurs.
    /// Returns None if inactive or no meaningful change.
    /// Advance the scroller by dt and return the new absolute offset if movement occurs.
    /// The `unit_size` is the height of a grid row or the width+spacing stride of a carousel item.
    pub fn tick(
        &mut self,
        current_offset: f32,
        unit_size: f32,
        max_scroll: f32,
    ) -> Option<f32> {
        if !self.active {
            return None;
        }

        let now = Instant::now();
        let last = self.last_tick.unwrap_or(now);
        let dt = now.saturating_duration_since(last);
        self.last_tick = Some(now);

        if dt.is_zero() {
            return None;
        }

        // Convert to seconds
        let dt_s = dt.as_secs_f32();

        if self.holding {
            // Target rows/s ramps from BASE -> MAX using easing over a ramp duration.
            let hold_elapsed_ms = self
                .hold_started
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
            let ramp = self.cfg.ramp_ms.max(1) as f32;
            let mut t = (hold_elapsed_ms as f32 / ramp).clamp(0.0, 1.0);
            t = apply_easing(t, self.cfg.easing_kind);
            let max_ups = self
                .boosted_max_units_per_s()
                .max(self.cfg.base_units_per_s);
            let target_ups = self.cfg.base_units_per_s
                + (max_ups - self.cfg.base_units_per_s) * t;
            let target_mag = target_ups * unit_size; // DIP/s from units/s
            let target = (self.dir as f32) * target_mag;

            // Time-constant filter to smoothly approach target
            let tau = self.accel_tau_secs();
            let alpha = 1.0 - (-dt_s / tau).exp();
            self.v += (target - self.v) * alpha;
        } else {
            // Decay exponentially toward zero
            let tau = (self.cfg.decay_tau_ms.max(1) as f32) / 1000.0;
            let decay = (-dt_s / tau).exp();
            self.v *= decay;
        }

        // Clamp velocity against max allowed speed, but only when accelerating.
        // When boost is released, we want velocity to decay smoothly via the
        // time-constant filter rather than snapping down instantly.
        let max_speed = self
            .boosted_max_units_per_s()
            .max(self.cfg.base_units_per_s)
            * unit_size;
        let target_mag = (self.dir as f32) * max_speed;
        let accelerating_past_limit =
            (self.v - target_mag).signum() == self.dir as f32;
        if self.v.abs() > max_speed && accelerating_past_limit {
            self.v = self.v.signum() * max_speed;
        }

        // If very slow, stop completely
        if self.v.abs() <= (self.cfg.min_units_per_s_stop * unit_size)
            && !self.holding
        {
            self.active = false;
            self.v = 0.0;
            return None;
        }

        // Integrate position
        let mut next = current_offset + self.v * dt_s;
        if next < 0.0 {
            next = 0.0;
            if !self.holding {
                self.active = false;
                self.v = 0.0;
            } else {
                // Bounce off bound slightly by resetting velocity
                self.v = 0.0;
            }
        } else if next > max_scroll {
            next = max_scroll;
            if !self.holding {
                self.active = false;
                self.v = 0.0;
            } else {
                self.v = 0.0;
            }
        }

        // Avoid tiny movements
        if (next - current_offset).abs() < 0.5 {
            return None;
        }

        Some(next)
    }

    /// Advance using external frame timestamp and return delta to apply.
    /// Uses frame-synchronized timing from `window::frames()` for smooth scrolling.
    /// Returns None if inactive or no meaningful movement.
    pub fn tick_delta_at(
        &mut self,
        now: Instant,
        unit_size: f32,
    ) -> Option<f32> {
        if !self.active {
            return None;
        }

        let last = self.last_tick.unwrap_or(now);
        let dt = now.saturating_duration_since(last);
        self.last_tick = Some(now);

        if dt.is_zero() {
            return None;
        }

        // Clamp dt to ~33ms (30fps floor) to prevent velocity spikes on frame drops
        let dt_s = dt.as_secs_f32().min(0.033);

        if self.holding {
            // Target rows/s ramps from BASE -> MAX using easing over a ramp duration.
            let hold_elapsed_ms = self
                .hold_started
                .map(|t| now.saturating_duration_since(t).as_millis() as u64)
                .unwrap_or(0);
            let ramp = self.cfg.ramp_ms.max(1) as f32;
            let mut t = (hold_elapsed_ms as f32 / ramp).clamp(0.0, 1.0);
            t = apply_easing(t, self.cfg.easing_kind);
            let max_ups = self
                .boosted_max_units_per_s()
                .max(self.cfg.base_units_per_s);
            let target_ups = self.cfg.base_units_per_s
                + (max_ups - self.cfg.base_units_per_s) * t;
            let target_mag = target_ups * unit_size;
            let target = (self.dir as f32) * target_mag;

            // Time-constant filter to smoothly approach target
            let tau = self.accel_tau_secs();
            let alpha = 1.0 - (-dt_s / tau).exp();
            self.v += (target - self.v) * alpha;
        } else {
            // Decay exponentially toward zero
            let tau = (self.cfg.decay_tau_ms.max(1) as f32) / 1000.0;
            let decay = (-dt_s / tau).exp();
            self.v *= decay;
        }

        // Clamp velocity against max allowed speed, but only when accelerating.
        // When boost is released, we want velocity to decay smoothly via the
        // time-constant filter rather than snapping down instantly.
        let max_speed = self
            .boosted_max_units_per_s()
            .max(self.cfg.base_units_per_s)
            * unit_size;
        let target_mag = (self.dir as f32) * max_speed;
        let accelerating_past_limit =
            (self.v - target_mag).signum() == self.dir as f32;
        if self.v.abs() > max_speed && accelerating_past_limit {
            self.v = self.v.signum() * max_speed;
        }

        // If very slow and not holding, stop completely
        if self.v.abs() <= (self.cfg.min_units_per_s_stop * unit_size)
            && !self.holding
        {
            self.active = false;
            self.v = 0.0;
            return None;
        }

        // Return delta instead of absolute position
        let delta = self.v * dt_s;

        // Avoid tiny movements
        if delta.abs() < 0.5 {
            return None;
        }

        Some(delta)
    }

    /// Current scroll direction: +1 for down, -1 for up, 0 for none.
    pub fn direction(&self) -> i32 {
        self.dir
    }
}

fn apply_easing(t: f32, kind: u8) -> f32 {
    match kind {
        1 => t * t,                       // EaseIn (quad)
        2 => 1.0 - (1.0 - t) * (1.0 - t), // EaseOut (quad)
        3 => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - 2.0 * (1.0 - t) * (1.0 - t)
            }
        } // EaseInOut (quad)
        _ => t,                           // Linear
    }
}

impl MotionController {
    /// Effective acceleration time constant in seconds.
    fn accel_tau_secs(&self) -> f32 {
        if self.cfg.accel_tau_ms > 0 {
            (self.cfg.accel_tau_ms as f32) / 1000.0
        } else {
            let ramp_s = (self.cfg.ramp_ms.max(1) as f32) / 1000.0;
            (ramp_s * self.cfg.accel_tau_to_ramp_ratio).max(0.05)
        }
    }

    fn boosted_max_units_per_s(&self) -> f32 {
        let base_max = self.cfg.max_units_per_s;
        if self.boost_active {
            base_max * self.cfg.boost_multiplier
        } else {
            base_max
        }
    }

    /// Access current config.
    pub fn config(&self) -> MotionControllerConfig {
        self.cfg
    }

    /// Replace configuration at runtime (takes effect on next tick).
    pub fn set_config(&mut self, cfg: MotionControllerConfig) {
        self.cfg = cfg;
    }
}
