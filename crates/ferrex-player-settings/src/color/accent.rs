//! Accent color configuration shared by settings state and color picker UI.

use iced_core::{Color, Point};
use serde::{Deserialize, Serialize};

use super::{ColorPoint, HarmonyMode, HsluvColor};

/// Persistent color picker configuration saved with settings preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccentColorConfig {
    /// Primary hue in degrees (0-360).
    pub primary_hue: f32,
    /// Primary saturation as percentage (0-100).
    pub primary_saturation: f32,
    /// Lightness as percentage (0-100).
    pub lightness: f32,
    /// Color harmony mode.
    pub harmony_mode: HarmonyMode,
}

impl Default for AccentColorConfig {
    fn default() -> Self {
        Self {
            primary_hue: 300.0,
            primary_saturation: 100.0,
            lightness: 50.0,
            harmony_mode: HarmonyMode::None,
        }
    }
}

impl AccentColorConfig {
    /// Get the primary color as HSLuv.
    pub fn primary_hsluv(&self) -> HsluvColor {
        HsluvColor::new(
            self.primary_hue,
            self.primary_saturation,
            self.lightness,
        )
    }

    /// Get the primary color as sRGB.
    pub fn primary_color(&self) -> Color {
        self.primary_hsluv().to_srgb()
    }

    /// Get complement 1 color if the active harmony mode includes it.
    pub fn complement1_color(&self) -> Option<Color> {
        let offsets = self.harmony_mode.offsets();
        offsets.first().map(|offset| {
            HsluvColor::new(
                (self.primary_hue + offset).rem_euclid(360.0),
                self.primary_saturation,
                self.lightness,
            )
            .to_srgb()
        })
    }

    /// Get complement 2 color if the active harmony mode includes it.
    pub fn complement2_color(&self) -> Option<Color> {
        let offsets = self.harmony_mode.offsets();
        offsets.get(1).map(|offset| {
            HsluvColor::new(
                (self.primary_hue + offset).rem_euclid(360.0),
                self.primary_saturation,
                self.lightness,
            )
            .to_srgb()
        })
    }

    /// Get all colors as sRGB (primary + active complements).
    pub fn all_colors(&self) -> Vec<Color> {
        let mut colors = vec![self.primary_color()];
        if let Some(c1) = self.complement1_color() {
            colors.push(c1);
        }
        if let Some(c2) = self.complement2_color() {
            colors.push(c2);
        }
        colors
    }

    /// Convert wheel position to hue (degrees) and saturation (0-100).
    pub fn position_to_hue_sat(
        pos: Point,
        center: Point,
        wheel_radius: f32,
    ) -> (f32, f32) {
        let offset_x = pos.x - center.x;
        let offset_y = pos.y - center.y;
        let dist = (offset_x.powi(2) + offset_y.powi(2)).sqrt();

        let hue = (offset_y.atan2(offset_x) + std::f32::consts::PI)
            / (2.0 * std::f32::consts::PI)
            * 360.0;
        let saturation = (dist / wheel_radius * 100.0).clamp(0.0, 100.0);

        (hue.rem_euclid(360.0), saturation)
    }

    /// Convert hue/saturation to position on the color wheel.
    pub fn hue_sat_to_position(
        hue: f32,
        saturation: f32,
        center: Point,
        wheel_radius: f32,
    ) -> Point {
        let angle = hue.to_radians() - std::f32::consts::PI;
        let radius = (saturation / 100.0) * wheel_radius;
        Point::new(
            center.x + angle.cos() * radius,
            center.y + angle.sin() * radius,
        )
    }

    /// Get position for the primary handle.
    pub fn primary_position(&self, center: Point, wheel_radius: f32) -> Point {
        Self::hue_sat_to_position(
            self.primary_hue,
            self.primary_saturation,
            center,
            wheel_radius,
        )
    }

    /// Get positions for all active handles (primary + complements).
    pub fn all_handle_positions(
        &self,
        center: Point,
        wheel_radius: f32,
    ) -> [Option<Point>; 3] {
        let primary = Some(self.primary_position(center, wheel_radius));

        let offsets = self.harmony_mode.offsets();
        let comp1 = offsets.first().map(|offset| {
            Self::hue_sat_to_position(
                (self.primary_hue + offset).rem_euclid(360.0),
                self.primary_saturation,
                center,
                wheel_radius,
            )
        });
        let comp2 = offsets.get(1).map(|offset| {
            Self::hue_sat_to_position(
                (self.primary_hue + offset).rem_euclid(360.0),
                self.primary_saturation,
                center,
                wheel_radius,
            )
        });

        [primary, comp1, comp2]
    }

    /// Return which color point is hit by a cursor position, if any.
    pub fn hit_test_handles(
        &self,
        mouse: Point,
        center: Point,
        wheel_radius: f32,
        handle_radius: f32,
    ) -> Option<ColorPoint> {
        let points = self.all_handle_positions(center, wheel_radius);

        for (index, position) in points.iter().enumerate() {
            if let Some(point) = position {
                let dist_sq =
                    (mouse.x - point.x).powi(2) + (mouse.y - point.y).powi(2);
                let hit_radius = handle_radius * 1.3;
                if dist_sq <= hit_radius.powi(2) {
                    return ColorPoint::from_index(index);
                }
            }
        }
        None
    }
}
