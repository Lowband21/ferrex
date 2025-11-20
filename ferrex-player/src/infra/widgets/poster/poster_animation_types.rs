use std::time::{Duration, Instant};

use iced::Rectangle;

/// Dynamic bounds for animated posters
#[derive(Debug, Clone, Copy)]
pub struct AnimatedPosterBounds {
    pub base_height: f32,
    /// Base size of the poster
    pub base_width: f32,
    /// Extra horizontal padding for animation overflow (e.g., scale and shadows)
    pub horizontal_padding: f32,
    /// Global UI scale factor for DPI independence
    pub ui_scale_factor: f32,
    /// Extra vertical padding for animation overflow
    pub vertical_padding: f32,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl AnimatedPosterBounds {
    /// Create new bounds with default padding
    pub fn new(width: f32, height: f32) -> Self {
        use crate::infra::constants::animation;

        // Calculate padding using centralized constants
        //let horizontal_padding = animation::calculate_horizontal_padding(width);
        let horizontal_padding = animation::calculate_horizontal_padding(width);
        let vertical_padding = animation::calculate_vertical_padding(height);

        Self {
            base_width: width,
            base_height: height,
            horizontal_padding,
            vertical_padding,
            ui_scale_factor: 1.0,
        }
    }

    /// Get the layout bounds - includes padding for effects
    pub fn layout_bounds(&self) -> (f32, f32) {
        // Return size with padding included - this is what the layout system sees
        (
            (self.base_width + self.horizontal_padding * 2.0)
                * self.ui_scale_factor,
            (self.base_height + self.vertical_padding * 2.0)
                * self.ui_scale_factor,
        )
    }

    /// Get the render bounds including animation overflow space
    pub fn render_bounds(&self) -> Rectangle {
        // Center the base bounds within the padded area
        Rectangle {
            x: -self.horizontal_padding,
            y: -self.vertical_padding,
            width: self.base_width + (self.horizontal_padding * 2.0),
            height: self.base_height + (self.vertical_padding * 2.0),
        }
    }
}

// Image loading functions are in the image crate root

/// Animation type for poster loading
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PosterAnimationType {
    None,
    Fade {
        duration: Duration,
    },
    /// The enhanced flip is now the default and only flip variant
    Flip {
        total_duration: Duration,
        rise_end: f32,   // Phase end: 0.0-0.25
        emerge_end: f32, // Phase end: 0.25-0.5
        flip_end: f32,   // Phase end: 0.5-0.75
                         // Settle: 0.75-1.0
    },
    /// Special state for placeholders - shows backface in sunken state
    PlaceholderSunken,
}

impl PosterAnimationType {
    pub fn as_u32(&self) -> u32 {
        match self {
            PosterAnimationType::None => 0,
            PosterAnimationType::Fade { .. } => 1,
            PosterAnimationType::Flip { .. } => 2,
            PosterAnimationType::PlaceholderSunken => 3,
        }
    }

    /// Create default flip animation with standard timings
    pub fn flip() -> Self {
        use crate::infra::constants::animation;

        PosterAnimationType::Flip {
            total_duration: Duration::from_millis(
                animation::DEFAULT_DURATION_MS,
            ),
            rise_end: 0.10,
            emerge_end: 0.20,
            flip_end: 0.80,
        }
    }

    pub fn effective_duration(&self) -> Duration {
        match self {
            PosterAnimationType::None
            | PosterAnimationType::PlaceholderSunken => Duration::ZERO,
            PosterAnimationType::Fade { duration } => *duration,
            PosterAnimationType::Flip { total_duration, .. } => *total_duration,
        }
    }
}

/// Describes how poster animations should behave across the first and subsequent renders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationBehavior {
    first: PosterAnimationType,
    repeat: PosterAnimationType,
    fresh_window: Duration,
}

impl AnimationBehavior {
    /// Always use the same animation for every render.
    pub fn constant(animation: PosterAnimationType) -> Self {
        let window = (animation.effective_duration() * 2)
            .max(Duration::from_millis(50))
            .max(Duration::from_secs(10));
        Self {
            first: animation,
            repeat: animation,
            fresh_window: window,
        }
    }

    /// Use `first` for freshly loaded textures, then fall back to `repeat` after the window.
    pub fn first_then(
        first: PosterAnimationType,
        repeat: PosterAnimationType,
    ) -> Self {
        let window = std::cmp::max(
            first.effective_duration(),
            repeat.effective_duration(),
        )
        .saturating_mul(2)
        .max(Duration::from_millis(50))
        .max(Duration::from_secs(10));
        Self {
            first,
            repeat,
            fresh_window: window,
        }
    }

    /// Convenience for highlighting newly added media: flip once, then fade as normal.
    pub fn flip_then_fade() -> Self {
        Self::first_then(
        PosterAnimationType::flip(),
        PosterAnimationType::Fade {
            duration: Duration::from_millis(
                crate::infra::constants::layout::animation::TEXTURE_FADE_DURATION_MS,
            ),
        },
    )
    }

    /// Derive a behavior from a single animation intent.
    ///
    /// Flip animations degrade to flip-then-fade, other animations stay constant.
    pub fn from_primary(animation: PosterAnimationType) -> Self {
        match animation {
            PosterAnimationType::Flip { .. }
            | PosterAnimationType::Fade { .. } => Self::flip_then_fade(),
            _ => Self::constant(animation),
        }
    }

    /// Select which animation should run given when the texture finished loading.
    pub fn select(&self, loaded_at: Option<Instant>) -> PosterAnimationType {
        if let Some(loaded_at) = loaded_at
            && loaded_at.elapsed() <= self.fresh_window
        {
            return self.first;
        }
        self.repeat
    }
}

pub fn calculate_animation_state(
    animation: PosterAnimationType,
    elapsed: Duration,
    opacity: f32,
) -> (f32, f32, f32, f32, f32, f32, f32) {
    match animation {
        PosterAnimationType::None => (opacity, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0),
        PosterAnimationType::PlaceholderSunken => {
            (0.7, std::f32::consts::PI, 0.0, -10.0, 1.0, 0.0, 0.0)
        }
        PosterAnimationType::Fade { duration } => {
            let progress =
                (elapsed.as_secs_f32() / duration.as_secs_f32()).min(1.0);
            (opacity * progress, 0.0, progress, 0.0, 1.0, 0.0, 0.0)
        }
        PosterAnimationType::Flip {
            total_duration,
            rise_end,
            emerge_end,
            flip_end,
        } => {
            let overall_progress =
                (elapsed.as_secs_f32() / total_duration.as_secs_f32()).min(1.0);

            // Simplified easing functions
            let ease_out_cubic = |t: f32| -> f32 {
                let t = t - 1.0;
                t * t * t + 1.0
            };
            let ease_in_out_sine = |t: f32| -> f32 {
                let t = t.clamp(0.0, 1.0);
                -(t * std::f32::consts::PI).cos() / 2.0 + 0.5
            };

            let (
                z_depth,
                scale,
                shadow_intensity,
                border_glow,
                rotation_y,
                final_opacity,
            ) = if overall_progress < rise_end {
                let phase_progress = overall_progress / rise_end;
                let eased = ease_out_cubic(phase_progress);
                let z = -10.0 * (1.0 - eased);
                let shadow = 0.5 * eased;
                let opacity = opacity * (0.7 + 0.2 * eased);
                (z, 1.0, shadow, 0.0, std::f32::consts::PI, opacity)
            } else if overall_progress < emerge_end {
                let phase_progress =
                    (overall_progress - rise_end) / (emerge_end - rise_end);
                let eased = ease_out_cubic(phase_progress);
                let z = 10.0 * eased;
                let scale = 1.0 + 0.05 * eased;
                let shadow = 0.5 + 0.5 * eased;
                let glow = 0.5 * eased;
                (z, scale, shadow, glow, std::f32::consts::PI, opacity * 0.9)
            } else if overall_progress < flip_end {
                let phase_progress =
                    (overall_progress - emerge_end) / (flip_end - emerge_end);
                let rotation_eased = ease_in_out_sine(phase_progress);
                let rotation = std::f32::consts::PI * (1.0 - rotation_eased);
                let glow = 0.5 * (1.0 - phase_progress);
                (10.0, 1.05, 1.0, glow, rotation, opacity)
            } else {
                let phase_progress =
                    (overall_progress - flip_end) / (1.0 - flip_end);
                let eased = ease_out_cubic(phase_progress);
                let z = 10.0 * (1.0 - eased);
                let scale = 1.0 + 0.05 * (1.0 - eased);
                let shadow = 1.0 * (1.0 - eased) + 0.3;
                (z, scale, shadow, 0.0, 0.0, opacity)
            };

            (
                final_opacity,
                rotation_y,
                overall_progress,
                z_depth,
                scale,
                shadow_intensity,
                border_glow,
            )
        }
    }
}
