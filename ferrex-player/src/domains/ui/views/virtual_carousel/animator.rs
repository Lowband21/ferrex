//! Simple time-based tween animator for snapping to a target offset

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct SnapAnimator {
    active: bool,
    start: f32,
    target: f32,
    started_at: Instant,
    duration: Duration,
    easing_kind: u8, // 0=Linear,1=EaseIn,2=EaseOut,3=EaseInOut
}

impl Default for SnapAnimator {
    fn default() -> Self {
        Self {
            active: false,
            start: 0.0,
            target: 0.0,
            started_at: Instant::now(),
            duration: Duration::from_millis(200),
            easing_kind: 2,
        }
    }
}

impl SnapAnimator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn start(
        &mut self,
        current: f32,
        target: f32,
        duration_ms: u64,
        easing_kind: u8,
    ) {
        self.active = true;
        self.start = current;
        self.target = target;
        self.started_at = Instant::now();
        self.duration = Duration::from_millis(duration_ms);
        self.easing_kind = easing_kind;
    }

    /// Returns Some(next_offset) when animating, or None when finished/inactive
    pub fn tick(&mut self) -> Option<f32> {
        if !self.active {
            return None;
        }
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.started_at);
        if elapsed >= self.duration {
            self.active = false;
            return Some(self.target);
        }
        let t = (elapsed.as_secs_f32() / self.duration.as_secs_f32())
            .clamp(0.0, 1.0);
        let te = apply_easing(t, self.easing_kind);
        let next = self.start + (self.target - self.start) * te;
        Some(next)
    }

    /// Cancel the current animation immediately.
    pub fn cancel(&mut self) {
        self.active = false;
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
