//! State types for the color picker widget

use iced::Point;
use serde::{Deserialize, Serialize};

use crate::infra::color::{ColorPoint, HarmonyMode, HsluvColor};

/// Persistent color picker configuration (saved to preferences)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccentColorConfig {
    /// Primary hue in degrees (0-360)
    pub primary_hue: f32,
    /// Primary saturation as percentage (0-100)
    pub primary_saturation: f32,
    /// Lightness as percentage (0-100)
    pub lightness: f32,
    /// Color harmony mode
    pub harmony_mode: HarmonyMode,
}

impl Default for AccentColorConfig {
    fn default() -> Self {
        Self {
            primary_hue: 300.0,        // Magenta (current theme)
            primary_saturation: 100.0, // Full saturation
            lightness: 50.0,           // Medium lightness
            harmony_mode: HarmonyMode::None,
        }
    }
}

impl AccentColorConfig {
    /// Get the primary color as HSLuv
    pub fn primary_hsluv(&self) -> HsluvColor {
        HsluvColor::new(
            self.primary_hue,
            self.primary_saturation,
            self.lightness,
        )
    }

    /// Get the primary color as sRGB
    pub fn primary_color(&self) -> iced::Color {
        self.primary_hsluv().to_srgb()
    }

    /// Get complement 1 color (if harmony mode includes it)
    pub fn complement1_color(&self) -> Option<iced::Color> {
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

    /// Get complement 2 color (if harmony mode includes it)
    pub fn complement2_color(&self) -> Option<iced::Color> {
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

    /// Get all colors as sRGB (primary + complements)
    pub fn all_colors(&self) -> Vec<iced::Color> {
        let mut colors = vec![self.primary_color()];
        if let Some(c1) = self.complement1_color() {
            colors.push(c1);
        }
        if let Some(c2) = self.complement2_color() {
            colors.push(c2);
        }
        colors
    }

    /// Convert wheel position to hue (degrees) and saturation (0-100)
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

    /// Convert hue/saturation to position on wheel
    pub fn hue_sat_to_position(
        hue: f32,
        saturation: f32,
        center: Point,
        wheel_radius: f32,
    ) -> Point {
        let angle = hue.to_radians() - std::f32::consts::PI;
        let r = (saturation / 100.0) * wheel_radius;
        Point::new(center.x + angle.cos() * r, center.y + angle.sin() * r)
    }

    /// Get position for the primary handle
    pub fn primary_position(&self, center: Point, wheel_radius: f32) -> Point {
        Self::hue_sat_to_position(
            self.primary_hue,
            self.primary_saturation,
            center,
            wheel_radius,
        )
    }

    /// Get positions for all active handles (primary + complements)
    pub fn all_handle_positions(
        &self,
        center: Point,
        wheel_radius: f32,
    ) -> [Option<Point>; 3] {
        let primary = Some(self.primary_position(center, wheel_radius));

        let offsets = self.harmony_mode.offsets();
        let comp1 = offsets.first().map(|off| {
            Self::hue_sat_to_position(
                (self.primary_hue + off).rem_euclid(360.0),
                self.primary_saturation,
                center,
                wheel_radius,
            )
        });
        let comp2 = offsets.get(1).map(|off| {
            Self::hue_sat_to_position(
                (self.primary_hue + off).rem_euclid(360.0),
                self.primary_saturation,
                center,
                wheel_radius,
            )
        });

        [primary, comp1, comp2]
    }
}

/// Transient interaction state (not persisted)
#[derive(Debug, Clone, Default)]
pub struct ColorPickerInteraction {
    /// Current mouse position relative to widget
    pub mouse_position: Option<Point>,
    /// Which color point is currently hovered
    pub hovered_point: Option<ColorPoint>,
    /// Which color point is being dragged
    pub dragging_point: Option<ColorPoint>,
    /// Whether primary mouse button was pressed inside this widget
    pub pressed_inside: bool,

    /// Animation values for hover state (0.0-1.0) for [primary, comp1, comp2]
    pub hover_animations: [f32; 3],
    /// Animation value for drag state (0.0-1.0)
    pub drag_animation: f32,
}

impl ColorPickerInteraction {
    /// Check if mouse hits any color point handle
    pub fn hit_test_handles(
        &self,
        mouse: Point,
        center: Point,
        wheel_radius: f32,
        config: &AccentColorConfig,
        handle_radius: f32,
    ) -> Option<ColorPoint> {
        let points = config.all_handle_positions(center, wheel_radius);

        for (i, pos) in points.iter().enumerate() {
            if let Some(p) = pos {
                let dist_sq = (mouse.x - p.x).powi(2) + (mouse.y - p.y).powi(2);
                let hit_radius = handle_radius * 1.3; // Slightly larger hit area
                if dist_sq <= hit_radius.powi(2) {
                    return ColorPoint::from_index(i);
                }
            }
        }
        None
    }

    /// Update hover animations based on current state
    pub fn update_animations(&mut self, dt: f32) {
        const ANIM_SPEED: f32 = 10.0;

        for (i, anim) in self.hover_animations.iter_mut().enumerate() {
            let target = if self.hovered_point.map(|p| p.index()) == Some(i) {
                1.0
            } else {
                0.0
            };
            *anim += (target - *anim) * ANIM_SPEED * dt;
        }

        let drag_target = if self.dragging_point.is_some() {
            1.0
        } else {
            0.0
        };
        self.drag_animation +=
            (drag_target - self.drag_animation) * ANIM_SPEED * dt;
    }

    /// Reset interaction state
    pub fn reset(&mut self) {
        self.mouse_position = None;
        self.hovered_point = None;
        self.dragging_point = None;
        self.pressed_inside = false;
    }
}
