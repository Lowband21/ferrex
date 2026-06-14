//! HSLuv color type and conversions
//!
//! HSLuv is a perceptually uniform color space where equal changes in hue
//! produce equal perceived color differences across the entire gamut.

use iced::Color;

/// A color in HSLuv color space
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HsluvColor {
    /// Hue in degrees (0-360)
    pub hue: f32,
    /// Saturation as percentage (0-100)
    pub saturation: f32,
    /// Lightness as percentage (0-100)
    pub lightness: f32,
}

impl HsluvColor {
    /// Create a new HSLuv color
    pub fn new(hue: f32, saturation: f32, lightness: f32) -> Self {
        Self {
            hue: hue.rem_euclid(360.0),
            saturation: saturation.clamp(0.0, 100.0),
            lightness: lightness.clamp(0.0, 100.0),
        }
    }

    /// Convert to sRGB Color (using hsluv crate)
    pub fn to_srgb(&self) -> Color {
        let (r, g, b) = hsluv::hsluv_to_rgb(
            self.hue as f64,
            self.saturation as f64,
            self.lightness as f64,
        );
        // Clamp to handle floating point precision issues from hsluv conversion
        Color::from_rgb(
            (r as f32).clamp(0.0, 1.0),
            (g as f32).clamp(0.0, 1.0),
            (b as f32).clamp(0.0, 1.0),
        )
    }

    /// Create from sRGB Color
    pub fn from_srgb(color: Color) -> Self {
        let (h, s, l) =
            hsluv::rgb_to_hsluv(color.r as f64, color.g as f64, color.b as f64);
        Self {
            hue: h as f32,
            saturation: s as f32,
            lightness: l as f32,
        }
    }

    /// Create from wheel position (angle + normalized radius)
    ///
    /// # Arguments
    /// * `angle_deg` - Hue angle in degrees (0-360)
    /// * `radius_normalized` - Saturation as radius (0.0 = center/gray, 1.0 = edge/vivid)
    /// * `lightness` - Lightness percentage (0-100)
    pub fn from_wheel(
        angle_deg: f32,
        radius_normalized: f32,
        lightness: f32,
    ) -> Self {
        Self {
            hue: angle_deg.rem_euclid(360.0),
            saturation: (radius_normalized * 100.0).clamp(0.0, 100.0),
            lightness: lightness.clamp(0.0, 100.0),
        }
    }

    /// Get wheel position (angle in degrees, radius normalized 0-1)
    pub fn to_wheel(&self) -> (f32, f32) {
        (self.hue, self.saturation / 100.0)
    }

    /// Create a new color with adjusted hue
    pub fn with_hue(&self, hue: f32) -> Self {
        Self::new(hue, self.saturation, self.lightness)
    }

    /// Create a new color with adjusted saturation
    pub fn with_saturation(&self, saturation: f32) -> Self {
        Self::new(self.hue, saturation, self.lightness)
    }

    /// Create a new color with adjusted lightness
    pub fn with_lightness(&self, lightness: f32) -> Self {
        Self::new(self.hue, self.saturation, lightness)
    }

    /// Rotate hue by given degrees
    pub fn rotate_hue(&self, degrees: f32) -> Self {
        Self::new(self.hue + degrees, self.saturation, self.lightness)
    }
}

impl Default for HsluvColor {
    fn default() -> Self {
        // Magenta (current theme accent)
        Self::new(300.0, 100.0, 50.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hsluv_roundtrip() {
        let original = HsluvColor::new(180.0, 80.0, 50.0);
        let rgb = original.to_srgb();
        let recovered = HsluvColor::from_srgb(rgb);

        // Allow small floating point differences
        assert!((original.hue - recovered.hue).abs() < 1.0);
        assert!((original.saturation - recovered.saturation).abs() < 1.0);
        assert!((original.lightness - recovered.lightness).abs() < 1.0);
    }

    #[test]
    fn test_hue_wrapping() {
        let color = HsluvColor::new(400.0, 50.0, 50.0);
        assert!((color.hue - 40.0).abs() < 0.001);

        let color = HsluvColor::new(-30.0, 50.0, 50.0);
        assert!((color.hue - 330.0).abs() < 0.001);
    }

    #[test]
    fn test_rotate_hue() {
        let color = HsluvColor::new(350.0, 100.0, 50.0);
        let rotated = color.rotate_hue(20.0);
        assert!((rotated.hue - 10.0).abs() < 0.001);
    }
}
