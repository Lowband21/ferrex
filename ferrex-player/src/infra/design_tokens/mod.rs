//! Design tokens for consistent, scalable UI sizing
//!
//! This module provides a comprehensive design token system for UI scaling.
//! All dimensions, fonts, spacing, and timing values flow through these tokens.
//!
//! ## Architecture
//!
//! - `ScalingContext` - UI scale configuration with optional per-view override
//! - `SizeProvider` - Central aggregator for all scaled token values
//! - Token modules - Semantic definitions for fonts, spacing, icons, animations
//! - `ScalePreset` - Named configurations for common use cases
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Access via UI state
//! let font_size = state.domains.ui.state.size_provider.font.body;
//! let spacing = state.domains.ui.state.size_provider.spacing.md;
//!
//! // Quick scaling for arbitrary values
//! let scaled_value = state.domains.ui.state.size_provider.scale(100.0);
//! ```

pub mod animation;
pub mod fonts;
pub mod icons;
pub mod presets;
pub mod spacing;

pub use animation::AnimationTokens;
pub use fonts::FontTokens;
pub use icons::IconTokens;
pub use presets::ScalePreset;
pub use spacing::SpacingTokens;

/// Scaling context for UI elements
///
/// The effective scale is the user_scale value clamped to [0.5, 3.0],
/// unless `view_override` is set, which takes precedence.
///
/// Note: System DPI scale is handled by Iced/winit at the rendering level,
/// so it's not included here to avoid double-scaling on HiDPI displays.
#[derive(Debug, Clone, Copy)]
pub struct ScalingContext {
    /// User preference scale (0.5 to 3.0)
    pub user_scale: f32,
    /// Optional per-view override (None = use user_scale, Some = override)
    pub view_override: Option<f32>,
}

impl ScalingContext {
    /// Create a new default scaling context (scale at 1.0)
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: set user preference scale
    #[must_use]
    pub fn with_user_scale(mut self, scale: f32) -> Self {
        self.user_scale = scale;
        self
    }

    /// Builder: set a per-view override that bypasses user scale
    #[must_use]
    pub fn with_override(mut self, scale: f32) -> Self {
        self.view_override = Some(scale);
        self
    }

    /// Clear any view override
    #[must_use]
    pub fn without_override(mut self) -> Self {
        self.view_override = None;
        self
    }

    /// Compute effective scale
    ///
    /// If `view_override` is set, returns that value (clamped).
    /// Otherwise, returns user_scale clamped to [0.5, 3.0].
    pub fn effective_scale(&self) -> f32 {
        if let Some(override_scale) = self.view_override {
            return override_scale.clamp(0.5, 3.0);
        }
        self.user_scale.clamp(0.5, 3.0)
    }

    /// Create from a named preset
    pub fn from_preset(preset: ScalePreset) -> Self {
        preset.to_context()
    }

    /// Update user scale in place (for settings changes)
    pub fn set_user_scale(&mut self, scale: f32) {
        self.user_scale = scale;
    }
}

impl Default for ScalingContext {
    fn default() -> Self {
        Self {
            user_scale: 1.0,
            view_override: None,
        }
    }
}

/// Central size provider - the main interface for views
///
/// Provides pre-computed token values for the current scale, avoiding repeated
/// calculations during rendering.
///
/// ## Usage
///
/// Access via `state.domains.ui.state.size_provider`:
///
/// ```rust,ignore
/// // Font sizes
/// text("Title").size(size_provider.font.title)
///
/// // Spacing
/// column![...].spacing(size_provider.spacing.md)
///
/// // Arbitrary scaling
/// .width(Length::Fixed(size_provider.scale(200.0)))
/// ```
///
/// ## Extension
///
/// Add new token categories as fields (e.g., `pub shadows: ShadowTokens`).
#[derive(Debug, Clone)]
pub struct SizeProvider {
    /// The effective scale factor
    pub scale: f32,
    /// The source context (for debugging/logging)
    pub context: ScalingContext,
    /// Font size tokens
    pub font: FontTokens,
    /// Spacing tokens
    pub spacing: SpacingTokens,
    /// Icon size tokens
    pub icon: IconTokens,
    /// Animation duration tokens
    pub animation: AnimationTokens,
}

impl SizeProvider {
    /// Create a new size provider from a scaling context
    pub fn new(context: ScalingContext) -> Self {
        let scale = context.effective_scale();
        Self {
            scale,
            context,
            font: FontTokens::scaled(scale),
            spacing: SpacingTokens::scaled(scale),
            icon: IconTokens::scaled(scale),
            animation: AnimationTokens::scaled(scale),
        }
    }

    /// Create from a named preset
    pub fn from_preset(preset: ScalePreset) -> Self {
        Self::new(ScalingContext::from_preset(preset))
    }

    /// Quick scale helper for arbitrary f32 values
    #[inline]
    pub fn scale(&self, value: f32) -> f32 {
        value * self.scale
    }

    /// Scale a font size value, with 8px minimum
    #[inline]
    pub fn scale_font(&self, value: f32) -> f32 {
        (value * self.scale).round().max(8.0)
    }

    /// Scale and round to nearest integer
    #[inline]
    pub fn scale_int(&self, value: i32) -> i32 {
        ((value as f32) * self.scale).round() as i32
    }

    /// Check if currently at default scale (1.0)
    #[inline]
    pub fn is_default_scale(&self) -> bool {
        (self.scale - 1.0).abs() < 0.001
    }
}

impl Default for SizeProvider {
    fn default() -> Self {
        Self::new(ScalingContext::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_scale_is_one() {
        let provider = SizeProvider::default();
        assert!((provider.scale - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_user_scale_affects_effective() {
        let context = ScalingContext::new().with_user_scale(1.5);
        assert!((context.effective_scale() - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_user_scale_is_effective() {
        let context = ScalingContext::new().with_user_scale(1.2);
        assert!((context.effective_scale() - 1.2).abs() < 0.001);
    }

    #[test]
    fn test_override_takes_precedence() {
        let context = ScalingContext::new()
            .with_user_scale(0.5)
            .with_override(1.0);
        assert!((context.effective_scale() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_scale_clamping() {
        let too_small = ScalingContext::new().with_user_scale(0.1);
        assert!((too_small.effective_scale() - 0.5).abs() < 0.001);

        let too_large = ScalingContext::new().with_user_scale(10.0);
        assert!((too_large.effective_scale() - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_font_minimum() {
        let provider =
            SizeProvider::new(ScalingContext::new().with_user_scale(0.5));
        // Even at 0.5 scale, a 10px font should floor to 8px
        assert!(provider.scale_font(10.0) >= 8.0);
    }
}
