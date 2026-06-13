//! State types for the color picker widget.

use iced::Point;

use crate::infra::color::ColorPoint;

pub use ferrex_player_settings::color::AccentColorConfig;

/// Transient interaction state (not persisted).
#[derive(Debug, Clone, Default)]
pub struct ColorPickerInteraction {
    /// Current mouse position relative to widget.
    pub mouse_position: Option<Point>,
    /// Which color point is currently hovered.
    pub hovered_point: Option<ColorPoint>,
    /// Which color point is being dragged.
    pub dragging_point: Option<ColorPoint>,
    /// Whether primary mouse button was pressed inside this widget.
    pub pressed_inside: bool,

    /// Animation values for hover state (0.0-1.0) for [primary, comp1, comp2].
    pub hover_animations: [f32; 3],
    /// Animation value for drag state (0.0-1.0).
    pub drag_animation: f32,
}

impl ColorPickerInteraction {
    /// Check if mouse hits any color point handle.
    pub fn hit_test_handles(
        &self,
        mouse: Point,
        center: Point,
        wheel_radius: f32,
        config: &AccentColorConfig,
        handle_radius: f32,
    ) -> Option<ColorPoint> {
        config.hit_test_handles(mouse, center, wheel_radius, handle_radius)
    }

    /// Update hover animations based on current state.
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

    /// Reset interaction state.
    pub fn reset(&mut self) {
        self.mouse_position = None;
        self.hovered_point = None;
        self.dragging_point = None;
        self.pressed_inside = false;
    }
}
