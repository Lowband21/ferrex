//! Color harmony algorithms
//!
//! Implements color harmony theory for generating complementary color palettes
//! that are aesthetically pleasing.

use super::HsluvColor;
use serde::{Deserialize, Serialize};

/// Color harmony mode for generating complementary colors
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize,
)]
pub enum HarmonyMode {
    /// Single color only (no complements)
    #[default]
    None,
    /// Complementary: 180 degrees apart (maximum contrast)
    Complementary,
    /// Triadic: 120 degrees apart (balanced triangle)
    Triadic,
    /// Split-Complementary: 150 and 210 degrees (softer contrast)
    SplitComplementary,
}

impl HarmonyMode {
    /// All available harmony modes
    pub const ALL: [HarmonyMode; 4] = [
        Self::None,
        Self::Complementary,
        Self::Triadic,
        Self::SplitComplementary,
    ];

    /// Get the hue offsets for this harmony mode (in degrees)
    pub fn offsets(&self) -> &'static [f32] {
        match self {
            Self::None => &[],
            Self::Complementary => &[180.0],
            Self::Triadic => &[120.0, 240.0],
            Self::SplitComplementary => &[150.0, 210.0],
        }
    }

    /// Calculate complementary hue values from a primary hue
    pub fn complementary_hues(&self, primary_hue: f32) -> Vec<f32> {
        self.offsets()
            .iter()
            .map(|offset| (primary_hue + offset).rem_euclid(360.0))
            .collect()
    }

    /// Get the number of color points for this mode (including primary)
    pub fn point_count(&self) -> usize {
        1 + self.offsets().len()
    }
}

impl std::fmt::Display for HarmonyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Complementary => write!(f, "Complementary"),
            Self::Triadic => write!(f, "Triadic"),
            Self::SplitComplementary => write!(f, "Split-Comp"),
        }
    }
}

/// Identifies which color point in the harmony
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorPoint {
    /// Primary user-selected color
    Primary,
    /// First complementary color
    Complement1,
    /// Second complementary color (for triadic/split-comp)
    Complement2,
}

impl ColorPoint {
    /// Get the index of this point (0 = primary, 1 = comp1, 2 = comp2)
    pub fn index(&self) -> usize {
        match self {
            Self::Primary => 0,
            Self::Complement1 => 1,
            Self::Complement2 => 2,
        }
    }

    /// Create from index
    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Primary),
            1 => Some(Self::Complement1),
            2 => Some(Self::Complement2),
            _ => None,
        }
    }
}

/// Handle linked drag adjustment
///
/// When any point is dragged, all points rotate together maintaining harmony offsets.
/// This updates the primary hue such that the dragged point ends up at `new_hue`.
pub fn handle_linked_drag(
    primary_hue: &mut f32,
    dragged_point: ColorPoint,
    new_hue: f32,
    harmony: HarmonyMode,
) {
    // Calculate the current hue of the dragged point
    let current_hue = match dragged_point {
        ColorPoint::Primary => *primary_hue,
        ColorPoint::Complement1 => (*primary_hue
            + harmony.offsets().first().copied().unwrap_or(0.0))
        .rem_euclid(360.0),
        ColorPoint::Complement2 => (*primary_hue
            + harmony.offsets().get(1).copied().unwrap_or(0.0))
        .rem_euclid(360.0),
    };

    // Calculate shortest path delta (handles wraparound at 0/360)
    let mut delta = new_hue - current_hue;
    if delta > 180.0 {
        delta -= 360.0;
    }
    if delta < -180.0 {
        delta += 360.0;
    }

    // Rotate primary hue (complements follow automatically via fixed offsets)
    *primary_hue = (*primary_hue + delta).rem_euclid(360.0);
}

/// Get all colors for a harmony configuration
pub fn get_harmony_colors(
    primary: HsluvColor,
    harmony: HarmonyMode,
) -> Vec<HsluvColor> {
    let mut colors = vec![primary];

    for offset in harmony.offsets() {
        colors.push(primary.rotate_hue(*offset));
    }

    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_harmony_offsets() {
        assert_eq!(HarmonyMode::None.offsets(), &[]);
        assert_eq!(HarmonyMode::Complementary.offsets(), &[180.0]);
        assert_eq!(HarmonyMode::Triadic.offsets(), &[120.0, 240.0]);
        assert_eq!(HarmonyMode::SplitComplementary.offsets(), &[150.0, 210.0]);
    }

    #[test]
    fn test_complementary_hues() {
        let hues = HarmonyMode::Complementary.complementary_hues(60.0);
        assert_eq!(hues.len(), 1);
        assert!((hues[0] - 240.0).abs() < 0.001);

        let hues = HarmonyMode::Triadic.complementary_hues(0.0);
        assert_eq!(hues.len(), 2);
        assert!((hues[0] - 120.0).abs() < 0.001);
        assert!((hues[1] - 240.0).abs() < 0.001);
    }

    #[test]
    fn test_linked_drag_primary() {
        let mut primary = 90.0;
        handle_linked_drag(
            &mut primary,
            ColorPoint::Primary,
            120.0,
            HarmonyMode::Complementary,
        );
        assert!((primary - 120.0).abs() < 0.001);
    }

    #[test]
    fn test_linked_drag_complement() {
        let mut primary = 0.0; // Complement is at 180
        handle_linked_drag(
            &mut primary,
            ColorPoint::Complement1,
            200.0, // Drag complement from 180 to 200 (delta = +20)
            HarmonyMode::Complementary,
        );
        // Primary should also shift by +20
        assert!((primary - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_linked_drag_wraparound() {
        let mut primary = 350.0; // Complement is at 170
        handle_linked_drag(
            &mut primary,
            ColorPoint::Primary,
            10.0, // Drag from 350 to 10 (shortest path is +20, not -340)
            HarmonyMode::Complementary,
        );
        assert!((primary - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_get_harmony_colors() {
        let primary = HsluvColor::new(60.0, 80.0, 50.0);
        let colors = get_harmony_colors(primary, HarmonyMode::Complementary);

        assert_eq!(colors.len(), 2);
        assert!((colors[0].hue - 60.0).abs() < 0.001);
        assert!((colors[1].hue - 240.0).abs() < 0.001);
    }
}
