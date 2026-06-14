//! Settings-owned scale preset DTOs shared between settings messages and UI.

/// Named scale presets for common player UI configurations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScalePreset {
    /// Compact mode - 80% scale for dense layouts.
    Compact,
    /// Default - 100% scale, standard desktop usage.
    #[default]
    Default,
    /// Large - 120% scale for easier reading.
    Large,
    /// Huge - 150% scale for accessibility.
    Huge,
    /// TV - 200% scale for 10-foot UI.
    TV,
}

impl ScalePreset {
    /// Get the user scale factor for this preset.
    pub const fn scale_factor(self) -> f32 {
        match self {
            Self::Compact => 0.8,
            Self::Default => 1.0,
            Self::Large => 1.2,
            Self::Huge => 1.5,
            Self::TV => 2.0,
        }
    }

    /// Get a human-readable name for this preset.
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Default => "Default",
            Self::Large => "Large",
            Self::Huge => "Huge",
            Self::TV => "TV",
        }
    }

    /// Get a description of this preset.
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Compact => "Dense layout for power users",
            Self::Default => "Standard desktop interface",
            Self::Large => "Larger elements for easier reading",
            Self::Huge => "Extra large for accessibility needs",
            Self::TV => "Optimized for TV and couch viewing",
        }
    }

    /// All available presets.
    pub const ALL: &'static [ScalePreset] = &[
        Self::Compact,
        Self::Default,
        Self::Large,
        Self::Huge,
        Self::TV,
    ];
}

impl std::fmt::Display for ScalePreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::ScalePreset;

    #[test]
    fn preset_scale_factors_match_named_options() {
        assert!((ScalePreset::Compact.scale_factor() - 0.8).abs() < 0.001);
        assert!((ScalePreset::Default.scale_factor() - 1.0).abs() < 0.001);
        assert!((ScalePreset::Large.scale_factor() - 1.2).abs() < 0.001);
        assert!((ScalePreset::Huge.scale_factor() - 1.5).abs() < 0.001);
        assert!((ScalePreset::TV.scale_factor() - 2.0).abs() < 0.001);
    }

    #[test]
    fn all_presets_remain_available_for_settings_ui() {
        assert_eq!(ScalePreset::ALL.len(), 5);
        assert!(ScalePreset::ALL.contains(&ScalePreset::Default));
    }
}
