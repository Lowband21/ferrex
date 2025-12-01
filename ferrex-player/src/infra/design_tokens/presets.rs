//! Named scale presets for common configurations
//!
//! Presets encapsulate common scaling configurations, making it easy to
//! switch between modes (e.g., compact vs accessibility).

use super::ScalingContext;

/// Named scale presets for common configurations
///
/// ## Available Presets
///
/// | Preset              | Effective Scale | Use Case                      |
/// |---------------------|-----------------|-------------------------------|
/// | `Compact`           | 0.8x            | Dense layouts, power users    |
/// | `Default`           | 1.0x            | Standard desktop usage        |
/// | `Large`             | 1.2x            | Easier reading, larger UI     |
/// | `AccessibilityLarge`| 1.5x            | Accessibility, low vision     |
/// | `TenFoot`           | 1.5x            | TV/10-foot UI, couch viewing  |
///
/// ## Extension
///
/// Add new presets by adding variants to this enum and implementing
/// the corresponding `to_context()` match arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScalePreset {
    /// Compact mode - 80% scale for dense layouts
    Compact,
    /// Default - 100% scale, standard desktop usage
    Default,
    /// Large - 120% scale for easier reading
    Large,
    /// Accessibility large text - 150% scale
    AccessibilityLarge,
    /// TV/10-foot UI - optimized for couch viewing
    TenFoot,
}

impl ScalePreset {
    /// Convert preset to a ScalingContext
    pub fn to_context(self) -> ScalingContext {
        match self {
            Self::Compact => ScalingContext::new().with_user_scale(0.8),
            Self::Default => ScalingContext::new(),
            Self::Large => ScalingContext::new().with_user_scale(1.2),
            Self::AccessibilityLarge => {
                ScalingContext::new().with_accessibility_scale(1.5)
            }
            Self::TenFoot => ScalingContext::new()
                .with_user_scale(1.2)
                .with_accessibility_scale(1.25),
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
            Self::AccessibilityLarge => "Accessibility (Large)",
            Self::TenFoot => "TV / 10-foot",
        }
    }

    /// Get a description of this preset
    pub fn description(&self) -> &'static str {
        match self {
            Self::Compact => "Dense layout for power users",
            Self::Default => "Standard desktop interface",
            Self::Large => "Larger elements for easier reading",
            Self::AccessibilityLarge => "Extra large for accessibility needs",
            Self::TenFoot => "Optimized for TV and couch viewing",
        }
    }

    /// All available presets (for settings UI)
    pub const ALL: &'static [ScalePreset] = &[
        Self::Compact,
        Self::Default,
        Self::Large,
        Self::AccessibilityLarge,
        Self::TenFoot,
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
        assert!(
            (ScalePreset::AccessibilityLarge.scale_factor() - 1.5).abs()
                < 0.001
        );
        // TenFoot: 1.2 * 1.25 = 1.5
        assert!((ScalePreset::TenFoot.scale_factor() - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_all_presets() {
        assert_eq!(ScalePreset::ALL.len(), 5);
        assert!(ScalePreset::ALL.contains(&ScalePreset::Default));
    }

    #[test]
    fn test_display_name() {
        assert_eq!(ScalePreset::Compact.display_name(), "Compact");
        assert_eq!(ScalePreset::TenFoot.display_name(), "TV / 10-foot");
    }
}
