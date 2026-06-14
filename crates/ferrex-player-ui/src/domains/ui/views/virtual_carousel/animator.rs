//! Frame-synchronized tween animator for snapping to a target offset
//!
//! Uses iced's Animation API (built on lilt) for smooth, interruptible
//! animations that sync with display refresh rate.

use iced::animation::{Animation, Easing};
use std::time::{Duration, Instant};

/// Maps easing kind (u8) to iced's Easing enum
fn easing_from_kind(kind: u8) -> Easing {
    match kind {
        1 => Easing::EaseInQuad,
        2 => Easing::EaseOutQuad,
        3 => Easing::EaseInOutQuad,
        _ => Easing::Linear,
    }
}

#[derive(Debug, Clone)]
pub struct SnapAnimator {
    /// The underlying animation (false→true = start→target)
    animation: Animation<bool>,
    /// Starting scroll position
    start: f32,
    /// Target scroll position
    target: f32,
    /// Animation duration (cached for cancel/reset)
    duration: Duration,
    /// Current easing (cached for cancel/reset)
    easing: Easing,
}

impl Default for SnapAnimator {
    fn default() -> Self {
        Self {
            animation: Animation::new(false)
                .duration(Duration::from_millis(200))
                .easing(Easing::EaseOutQuad),
            start: 0.0,
            target: 0.0,
            duration: Duration::from_millis(200),
            easing: Easing::EaseOutQuad,
        }
    }
}

impl SnapAnimator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if animation is currently in progress.
    ///
    /// Uses `Instant::now()` - suitable for subscription checks where
    /// precise frame timing is not critical.
    pub fn is_active(&self) -> bool {
        self.animation.is_animating(Instant::now())
    }

    /// Check if animation is currently in progress at a specific time.
    ///
    /// Use with timestamps from `window::frames()` for frame-synchronized
    /// animation updates.
    pub fn is_active_at(&self, now: Instant) -> bool {
        self.animation.is_animating(now)
    }

    /// Start a new snap animation from current position to target.
    ///
    /// # Arguments
    /// * `current` - Current scroll position
    /// * `target` - Target scroll position to animate to
    /// * `duration_ms` - Animation duration in milliseconds
    /// * `easing_kind` - Easing type: 0=Linear, 1=EaseIn, 2=EaseOut, 3=EaseInOut
    /// * `now` - Current timestamp (use Instant::now() when starting)
    pub fn start_at(
        &mut self,
        current: f32,
        target: f32,
        duration_ms: u64,
        easing_kind: u8,
        now: Instant,
    ) {
        self.start = current;
        self.target = target;
        self.duration = Duration::from_millis(duration_ms);
        self.easing = easing_from_kind(easing_kind);

        // Create a fresh animation: false→true maps to start→target
        self.animation = Animation::new(false)
            .duration(self.duration)
            .easing(self.easing)
            .go(true, now);
    }

    /// Get the current interpolated scroll position at a specific time.
    ///
    /// Use with timestamps from `window::frames()` for frame-synchronized
    /// animation rendering.
    pub fn value_at(&self, now: Instant) -> f32 {
        self.animation.interpolate(self.start, self.target, now)
    }

    /// Tick the animation at a specific time and return the current position.
    ///
    /// Returns `Some(position)` if animating or just finished, `None` if inactive.
    /// Use with timestamps from `window::frames()` for frame-synchronized updates.
    pub fn tick_at(&self, now: Instant) -> Option<f32> {
        if self.animation.is_animating(now) {
            Some(self.animation.interpolate(self.start, self.target, now))
        } else {
            // Return final target when animation completes
            if self.animation.value() {
                Some(self.target)
            } else {
                None
            }
        }
    }

    /// Cancel the current animation immediately.
    pub fn cancel(&mut self) {
        // Reset to non-animating state (false, not transitioning)
        self.animation = Animation::new(false)
            .duration(self.duration)
            .easing(self.easing);
    }

    /// Get the target position this animator is moving toward.
    pub fn target(&self) -> f32 {
        self.target
    }
}
