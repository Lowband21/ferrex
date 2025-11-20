//! Transition system for smooth animations in the background shader
//!
//! This module provides a scalable framework for animating between different
//! states in the background shader, including color transitions, backdrop animations,
//! and other visual effects.

use std::time::{Duration, Instant};

// TODO: Move to constants
/// Duration for color transitions
pub const COLOR_TRANSITION_DURATION: Duration = Duration::from_millis(5000);

/// Duration for backdrop fade/slide transitions
pub const BACKDROP_TRANSITION_DURATION: Duration = Duration::from_millis(5000);

/// Duration for gradient center movement transitions
pub const GRADIENT_TRANSITION_DURATION: Duration = Duration::from_millis(5000);

/// Easing function types for transitions
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EasingFunction {
    Linear,
    EaseOutCubic,
    EaseInOutCubic,
    EaseOutQuart,
    EaseOutExpo,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl EasingFunction {
    /// Apply the easing function to a progress value (0.0 to 1.0)
    pub fn apply(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            EasingFunction::Linear => t,
            EasingFunction::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            EasingFunction::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            EasingFunction::EaseOutQuart => 1.0 - (1.0 - t).powi(4),
            EasingFunction::EaseOutExpo => {
                if t >= 1.0 {
                    1.0
                } else {
                    1.0 - 2.0_f32.powf(-10.0 * t)
                }
            }
        }
    }
}

/// Generic transition state for animating between values
#[derive(Debug, Clone)]
pub struct Transition<T: Clone> {
    pub from: T,
    pub to: T,
    pub start_time: Option<Instant>,
    pub duration: Duration,
    pub easing: EasingFunction,
    pub progress: f32,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl<T: Clone> Transition<T> {
    /// Create a new transition
    pub fn new(initial_value: T, duration: Duration, easing: EasingFunction) -> Self {
        Self {
            from: initial_value.clone(),
            to: initial_value,
            start_time: None,
            duration,
            easing,
            progress: 1.0, // Start fully transitioned
        }
    }

    /// Start a transition to a new value
    pub fn transition_to(&mut self, new_value: T) {
        if self.is_transitioning() {
            // If already transitioning, update from value to current interpolated state
            self.from = self.current_value();
        } else {
            self.from = self.to.clone();
        }
        self.to = new_value;
        self.start_time = Some(Instant::now());
        self.progress = 0.0;
    }

    /// Update the transition progress
    pub fn update(&mut self) {
        if let Some(start) = self.start_time {
            let elapsed = Instant::now().duration_since(start);
            let raw_progress = elapsed.as_secs_f32() / self.duration.as_secs_f32();

            if raw_progress >= 1.0 {
                self.progress = 1.0;
                self.start_time = None; // Transition complete
            } else {
                self.progress = self.easing.apply(raw_progress);
            }
        }
    }

    /// Check if currently transitioning
    pub fn is_transitioning(&self) -> bool {
        self.start_time.is_some() && self.progress < 1.0
    }

    /// Get the current interpolated value
    pub fn current_value(&self) -> T {
        if self.progress >= 1.0 {
            self.to.clone()
        } else {
            self.from.clone() // For now, caller must handle interpolation
        }
    }

    /// Get the raw progress value (0.0 to 1.0)
    pub fn get_progress(&self) -> f32 {
        self.progress
    }
}

/// Manages color transitions for the background shader
#[derive(Debug, Clone)]
pub struct ColorTransitionState {
    pub primary: Transition<iced::Color>,
    pub secondary: Transition<iced::Color>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl ColorTransitionState {
    /// Create a new color transition state
    pub fn new(primary: iced::Color, secondary: iced::Color) -> Self {
        Self {
            primary: Transition::new(
                primary,
                COLOR_TRANSITION_DURATION,
                EasingFunction::EaseOutCubic,
            ),
            secondary: Transition::new(
                secondary,
                COLOR_TRANSITION_DURATION,
                EasingFunction::EaseOutCubic,
            ),
        }
    }

    /// Transition to new colors
    pub fn transition_to(&mut self, primary: iced::Color, secondary: iced::Color) {
        self.primary.transition_to(primary);
        self.secondary.transition_to(secondary);
    }

    /// Update all color transitions
    pub fn update(&mut self) {
        self.primary.update();
        self.secondary.update();
    }

    /// Check if any color is transitioning
    pub fn is_transitioning(&self) -> bool {
        self.primary.is_transitioning() || self.secondary.is_transitioning()
    }

    /// Get interpolated colors
    pub fn get_interpolated_colors(&self) -> (iced::Color, iced::Color) {
        let primary_from = self.primary.from;
        let primary_to = self.primary.to;
        let primary_t = self.primary.get_progress();

        let secondary_from = self.secondary.from;
        let secondary_to = self.secondary.to;
        let secondary_t = self.secondary.get_progress();

        (
            interpolate_color(primary_from, primary_to, primary_t),
            interpolate_color(secondary_from, secondary_to, secondary_t),
        )
    }
}

/// Manages backdrop animation state
#[derive(Debug, Clone)]
pub struct BackdropTransitionState {
    pub opacity: Transition<f32>,
    pub slide_offset: Transition<f32>,
    pub scale: Transition<f32>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl Default for BackdropTransitionState {
    fn default() -> Self {
        Self::new()
    }
}

impl BackdropTransitionState {
    /// Create a new backdrop transition state
    pub fn new() -> Self {
        Self {
            opacity: Transition::new(
                1.0,
                BACKDROP_TRANSITION_DURATION,
                EasingFunction::EaseOutCubic,
            ),
            slide_offset: Transition::new(
                0.0,
                BACKDROP_TRANSITION_DURATION,
                EasingFunction::EaseOutQuart,
            ),
            scale: Transition::new(
                1.0,
                BACKDROP_TRANSITION_DURATION,
                EasingFunction::EaseOutCubic,
            ),
        }
    }

    /// Start a fade-in animation
    pub fn start_fade_in(&mut self) {
        self.opacity.from = 0.0;
        self.opacity.to = 1.0;
        self.opacity.start_time = Some(Instant::now());
        self.opacity.progress = 0.0;
    }

    /// Start a slide-down animation
    pub fn start_slide_down(&mut self, offset: f32) {
        self.slide_offset.from = -offset;
        self.slide_offset.to = 0.0;
        self.slide_offset.start_time = Some(Instant::now());
        self.slide_offset.progress = 0.0;
    }

    /// Start a combined fade and slide animation
    pub fn start_fade_slide(&mut self, slide_offset: f32) {
        self.start_fade_in();
        self.start_slide_down(slide_offset);

        // Add subtle scale effect
        self.scale.from = 1.05;
        self.scale.to = 1.0;
        self.scale.start_time = Some(Instant::now());
        self.scale.progress = 0.0;
    }

    /// Update all backdrop transitions
    pub fn update(&mut self) {
        self.opacity.update();
        self.slide_offset.update();
        self.scale.update();
    }

    /// Check if any backdrop animation is active
    pub fn is_transitioning(&self) -> bool {
        self.opacity.is_transitioning()
            || self.slide_offset.is_transitioning()
            || self.scale.is_transitioning()
    }

    /// Get current animation values
    pub fn get_current_values(&self) -> (f32, f32, f32) {
        (
            interpolate_f32(
                self.opacity.from,
                self.opacity.to,
                self.opacity.get_progress(),
            ),
            interpolate_f32(
                self.slide_offset.from,
                self.slide_offset.to,
                self.slide_offset.get_progress(),
            ),
            interpolate_f32(self.scale.from, self.scale.to, self.scale.get_progress()),
        )
    }
}

/// Interpolate between two colors
fn interpolate_color(from: iced::Color, to: iced::Color, t: f32) -> iced::Color {
    iced::Color {
        r: interpolate_f32(from.r, to.r, t),
        g: interpolate_f32(from.g, to.g, t),
        b: interpolate_f32(from.b, to.b, t),
        a: interpolate_f32(from.a, to.a, t),
    }
}

/// Interpolate between two f32 values
fn interpolate_f32(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// Manages gradient center position transitions
#[derive(Debug, Clone)]
pub struct GradientTransitionState {
    pub center: Transition<(f32, f32)>,
}

impl GradientTransitionState {
    /// Create a new gradient transition state
    pub fn new(initial_center: (f32, f32)) -> Self {
        Self {
            center: Transition::new(
                initial_center,
                GRADIENT_TRANSITION_DURATION,
                EasingFunction::EaseInOutCubic,
            ),
        }
    }

    /// Transition to a new gradient center position
    pub fn transition_to(&mut self, new_center: (f32, f32)) {
        self.center.transition_to(new_center);
    }

    /// Update the transition
    pub fn update(&mut self) {
        self.center.update();
    }

    /// Check if transitioning
    pub fn is_transitioning(&self) -> bool {
        self.center.is_transitioning()
    }

    /// Get the current interpolated center position
    pub fn get_interpolated_center(&self) -> (f32, f32) {
        let from = self.center.from;
        let to = self.center.to;
        let t = self.center.get_progress();

        (
            interpolate_f32(from.0, to.0, t),
            interpolate_f32(from.1, to.1, t),
        )
    }
}

// Non-functional
pub fn generate_random_gradient_center() -> (f32, f32) {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (
        0.01 + rng.r#gen::<f32>() * 0.99,
        0.01 + rng.r#gen::<f32>() * 0.9,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_easing_functions() {
        assert_eq!(EasingFunction::Linear.apply(0.5), 0.5);
        assert_eq!(EasingFunction::Linear.apply(0.0), 0.0);
        assert_eq!(EasingFunction::Linear.apply(1.0), 1.0);

        // EaseOutCubic should ease out (slow down) at the end
        let mid = EasingFunction::EaseOutCubic.apply(0.5);
        assert!(mid > 0.5); // Should be past halfway
    }

    #[test]
    fn test_color_interpolation() {
        let black = iced::Color::BLACK;
        let white = iced::Color::WHITE;

        let mid = interpolate_color(black, white, 0.5);
        assert!((mid.r - 0.5).abs() < 0.001);
        assert!((mid.g - 0.5).abs() < 0.001);
        assert!((mid.b - 0.5).abs() < 0.001);
    }
}
