use crate::infra::constants::performance_config::scrolling as cfg;
use std::time::{Duration, Instant};

#[derive(Debug, Default, Clone)]
pub struct KineticScroller {
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

impl KineticScroller {
    pub fn new() -> Self {
        Self::default()
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

    pub fn set_boost(&mut self, active: bool) {
        self.boost_active = active;
    }

    /// Advance the scroller by dt and return the new absolute offset if movement occurs.
    /// Returns None if inactive or no meaningful change.
    pub fn tick(
        &mut self,
        current_offset: f32,
        row_height: f32,
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
            let ramp = cfg::KINETIC_RAMP_MS.max(1) as f32;
            let mut t = (hold_elapsed_ms as f32 / ramp).clamp(0.0, 1.0);
            t = apply_easing(t, cfg::KINETIC_EASING_KIND);
            let max_rps = self
                .boosted_max_rows_per_s()
                .max(cfg::KINETIC_BASE_ROWS_PER_S);
            let target_rps = cfg::KINETIC_BASE_ROWS_PER_S
                + (max_rps - cfg::KINETIC_BASE_ROWS_PER_S) * t;
            let target_mag = target_rps * row_height; // DIP/s from rows/s
            let target = (self.dir as f32) * target_mag;

            // Time-constant filter to smoothly approach target
            let tau = accel_tau_secs();
            let alpha = 1.0 - (-dt_s / tau).exp();
            self.v += (target - self.v) * alpha;
        } else {
            // Decay exponentially toward zero
            let tau = (cfg::KINETIC_DECAY_TAU_MS.max(1) as f32) / 1000.0;
            let decay = (-dt_s / tau).exp();
            self.v *= decay;
        }

        // Clamp velocity against max allowed DIP/s from rows/s cap
        let max_speed = self
            .boosted_max_rows_per_s()
            .max(cfg::KINETIC_BASE_ROWS_PER_S)
            * row_height;
        if self.v.abs() > max_speed {
            self.v = self.v.signum() * max_speed;
        }

        // If very slow, stop completely
        if self.v.abs() <= (cfg::KINETIC_MIN_ROWS_PER_S_STOP * row_height)
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

    // No estimated_max_speed helper needed in rows/sec mode
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

fn accel_tau_secs() -> f32 {
    if cfg::KINETIC_ACCEL_TAU_MS > 0 {
        (cfg::KINETIC_ACCEL_TAU_MS as f32) / 1000.0
    } else {
        let ramp_s = (cfg::KINETIC_RAMP_MS.max(1) as f32) / 1000.0;
        (ramp_s * cfg::KINETIC_ACCEL_TAU_TO_RAMP_RATIO).max(0.05)
    }
}

impl KineticScroller {
    fn boosted_max_rows_per_s(&self) -> f32 {
        let base_max = cfg::KINETIC_MAX_ROWS_PER_S;
        if self.boost_active {
            base_max * cfg::KINETIC_BOOST_MULTIPLIER
        } else {
            base_max
        }
    }
}
