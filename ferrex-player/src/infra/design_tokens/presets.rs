//! Named scale presets for common configurations
//!
//! Presets encapsulate common scaling configurations, making it easy to
//! switch between modes.

use super::ScalingContext;

/// Named scale presets for common configurations
///
/// ## Available Presets
///
/// | Preset   | Scale | Use Case                      |
/// |----------|-------|-------------------------------|
/// | `Compact`| 0.8x  | Dense layouts, power users    |
/// | `Default`| 1.0x  | Standard desktop usage        |
/// | `Large`  | 1.2x  | Easier reading, larger UI     |
/// | `Huge`   | 1.5x  | Extra large, accessibility    |
/// | `TV`     | 2.0x  | TV/10-foot UI, couch viewing  |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScalePreset {
    /// Compact mode - 80% scale for dense layouts
    Compact,
    /// Default - 100% scale, standard desktop usage
    Default,
    /// Large - 120% scale for easier reading
    Large,
    /// Huge - 150% scale for accessibility
    Huge,
    /// TV - 200% scale for 10-foot UI
    TV,
}

impl ScalePreset {
    /// Convert preset to a ScalingContext
    pub fn to_context(self) -> ScalingContext {
        match self {
            Self::Compact => ScalingContext::new().with_user_scale(0.8),
            Self::Default => ScalingContext::new(),
            Self::Large => ScalingContext::new().with_user_scale(1.2),
            Self::Huge => ScalingContext::new().with_user_scale(1.5),
            Self::TV => ScalingContext::new().with_user_scale(2.0),
        }
    }

    /// Get the effective scale factor for this preset
    pub fn scale_factor(self) -> f32 {
        self.to_context().effective_scale()
    }

    /// Get a human-readable name for this preset
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Default => "Default",
            Self::Large => "Large",
            Self::Huge => "Huge",
            Self::TV => "TV",
        }
    }

    /// Get a description of this preset
    pub fn description(&self) -> &'static str {
        match self {
            Self::Compact => "Dense layout for power users",
            Self::Default => "Standard desktop interface",
            Self::Large => "Larger elements for easier reading",
            Self::Huge => "Extra large for accessibility needs",
            Self::TV => "Optimized for TV and couch viewing",
        }
    }

    /// All available presets (for settings UI)
    pub const ALL: &'static [ScalePreset] = &[
        Self::Compact,
        Self::Default,
        Self::Large,
        Self::Huge,
        Self::TV,
    ];
}

impl Default for ScalePreset {
    fn default() -> Self {
        Self::Default
    }
}

impl std::fmt::Display for ScalePreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_scale_factors() {
        assert!((ScalePreset::Compact.scale_factor() - 0.8).abs() < 0.001);
        assert!((ScalePreset::Default.scale_factor() - 1.0).abs() < 0.001);
        assert!((ScalePreset::Large.scale_factor() - 1.2).abs() < 0.001);
        assert!((ScalePreset::Huge.scale_factor() - 1.5).abs() < 0.001);
        assert!((ScalePreset::TV.scale_factor() - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_all_presets() {
        assert_eq!(ScalePreset::ALL.len(), 5);
        assert!(ScalePreset::ALL.contains(&ScalePreset::Default));
    }

    #[test]
    fn test_display_name() {
        assert_eq!(ScalePreset::Compact.display_name(), "Compact");
        assert_eq!(ScalePreset::TV.display_name(), "TV");
    }
}
