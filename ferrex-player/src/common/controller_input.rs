//! Runtime-agnostic controller input mapping for 10-foot navigation.
//!
//! Native backends can normalize their controller events into these small enums
//! and feed them through [`ControllerInputMapper`] to produce spatial actions.
//! This module deliberately avoids depending on a gamepad runtime crate.

use std::time::{Duration, Instant};

use crate::common::focus::{SpatialAction, SpatialDirection};

/// Digital controller buttons that participate in 10-foot navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerButton {
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    South,
    East,
    Start,
    Select,
}

impl ControllerButton {
    fn action(self) -> SpatialAction {
        match self {
            Self::DPadUp => SpatialAction::Move(SpatialDirection::Up),
            Self::DPadDown => SpatialAction::Move(SpatialDirection::Down),
            Self::DPadLeft => SpatialAction::Move(SpatialDirection::Left),
            Self::DPadRight => SpatialAction::Move(SpatialDirection::Right),
            Self::South => SpatialAction::Activate,
            Self::East => SpatialAction::Back,
            Self::Start => SpatialAction::Menu,
            Self::Select => SpatialAction::Search,
        }
    }
}

/// Controller axes used for 10-foot spatial movement.
///
/// Values are expected to be normalized to `[-1.0, 1.0]`. The left stick uses
/// UI-coordinate Y, where negative is up and positive is down. Native backends
/// with a different Y convention should invert before constructing this event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerAxis {
    LeftStickX,
    LeftStickY,
}

/// Runtime-neutral controller event consumed by the mapper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControllerEvent {
    ButtonPressed(ControllerButton),
    AxisChanged { axis: ControllerAxis, value: f32 },
}

/// Tunables for analog stick directional mapping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerInputConfig {
    /// Axis magnitude required to begin directional movement.
    pub engage_threshold: f32,
    /// Axis magnitude below which held stick direction is released.
    pub release_threshold: f32,
    /// Minimum time between repeated movement actions for a held stick.
    pub repeat_delay: Duration,
}

impl Default for ControllerInputConfig {
    fn default() -> Self {
        Self {
            engage_threshold: 0.60,
            release_threshold: 0.35,
            repeat_delay: Duration::from_millis(250),
        }
    }
}

/// Maps normalized controller events into 10-foot spatial actions.
#[derive(Debug, Clone)]
pub struct ControllerInputMapper {
    config: ControllerInputConfig,
    left_stick_x: f32,
    left_stick_y: f32,
    held_direction: Option<SpatialDirection>,
    last_stick_emit: Option<Instant>,
}

impl Default for ControllerInputMapper {
    fn default() -> Self {
        Self::new(ControllerInputConfig::default())
    }
}

impl ControllerInputMapper {
    pub fn new(config: ControllerInputConfig) -> Self {
        Self {
            config,
            left_stick_x: 0.0,
            left_stick_y: 0.0,
            held_direction: None,
            last_stick_emit: None,
        }
    }

    /// Consumes one normalized controller event and returns a spatial action
    /// when the event should affect 10-foot focus or activation.
    pub fn handle_event(
        &mut self,
        event: ControllerEvent,
        now: Instant,
    ) -> Option<SpatialAction> {
        match event {
            ControllerEvent::ButtonPressed(button) => Some(button.action()),
            ControllerEvent::AxisChanged { axis, value } => {
                match axis {
                    ControllerAxis::LeftStickX => self.left_stick_x = value,
                    ControllerAxis::LeftStickY => self.left_stick_y = value,
                }
                self.handle_stick_direction(now)
            }
        }
    }

    /// Emits repeated movement for a held analog stick once the configured
    /// repeat delay has elapsed. A backend can call this from its polling loop.
    pub fn tick(&mut self, now: Instant) -> Option<SpatialAction> {
        let direction = self.held_direction?;
        let last = self.last_stick_emit?;
        if now.duration_since(last) >= self.config.repeat_delay {
            self.last_stick_emit = Some(now);
            Some(SpatialAction::Move(direction))
        } else {
            None
        }
    }

    fn handle_stick_direction(
        &mut self,
        now: Instant,
    ) -> Option<SpatialAction> {
        let direction = self.current_stick_direction();

        match (self.held_direction, direction) {
            (_, None) => {
                self.held_direction = None;
                self.last_stick_emit = None;
                None
            }
            (Some(held), Some(next)) if held == next => self.tick(now),
            (_, Some(next)) => {
                self.held_direction = Some(next);
                self.last_stick_emit = Some(now);
                Some(SpatialAction::Move(next))
            }
        }
    }

    fn current_stick_direction(&self) -> Option<SpatialDirection> {
        let x = self.left_stick_x.clamp(-1.0, 1.0);
        let y = self.left_stick_y.clamp(-1.0, 1.0);
        let magnitude = x.abs().max(y.abs());

        let threshold = if self.held_direction.is_some() {
            self.config.release_threshold
        } else {
            self.config.engage_threshold
        };

        if magnitude < threshold {
            return None;
        }

        if x.abs() > y.abs() {
            Some(if x < 0.0 {
                SpatialDirection::Left
            } else {
                SpatialDirection::Right
            })
        } else {
            Some(if y < 0.0 {
                SpatialDirection::Up
            } else {
                SpatialDirection::Down
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_time() -> Instant {
        Instant::now()
    }

    #[test]
    fn maps_dpad_to_spatial_movement() {
        let mut mapper = ControllerInputMapper::default();
        let now = base_time();

        assert_eq!(
            mapper.handle_event(
                ControllerEvent::ButtonPressed(ControllerButton::DPadUp),
                now,
            ),
            Some(SpatialAction::Move(SpatialDirection::Up))
        );
        assert_eq!(
            mapper.handle_event(
                ControllerEvent::ButtonPressed(ControllerButton::DPadDown),
                now,
            ),
            Some(SpatialAction::Move(SpatialDirection::Down))
        );
        assert_eq!(
            mapper.handle_event(
                ControllerEvent::ButtonPressed(ControllerButton::DPadLeft),
                now,
            ),
            Some(SpatialAction::Move(SpatialDirection::Left))
        );
        assert_eq!(
            mapper.handle_event(
                ControllerEvent::ButtonPressed(ControllerButton::DPadRight),
                now,
            ),
            Some(SpatialAction::Move(SpatialDirection::Right))
        );
    }

    #[test]
    fn maps_face_and_menu_buttons_to_spatial_actions() {
        let mut mapper = ControllerInputMapper::default();
        let now = base_time();

        assert_eq!(
            mapper.handle_event(
                ControllerEvent::ButtonPressed(ControllerButton::South),
                now,
            ),
            Some(SpatialAction::Activate)
        );
        assert_eq!(
            mapper.handle_event(
                ControllerEvent::ButtonPressed(ControllerButton::East),
                now,
            ),
            Some(SpatialAction::Back)
        );
        assert_eq!(
            mapper.handle_event(
                ControllerEvent::ButtonPressed(ControllerButton::Start),
                now,
            ),
            Some(SpatialAction::Menu)
        );
        assert_eq!(
            mapper.handle_event(
                ControllerEvent::ButtonPressed(ControllerButton::Select),
                now,
            ),
            Some(SpatialAction::Search)
        );
    }

    #[test]
    fn maps_left_stick_to_dominant_spatial_direction() {
        let mut mapper = ControllerInputMapper::default();
        let now = base_time();

        assert_eq!(
            mapper.handle_event(
                ControllerEvent::AxisChanged {
                    axis: ControllerAxis::LeftStickX,
                    value: 0.8,
                },
                now,
            ),
            Some(SpatialAction::Move(SpatialDirection::Right))
        );

        assert_eq!(
            mapper.handle_event(
                ControllerEvent::AxisChanged {
                    axis: ControllerAxis::LeftStickX,
                    value: 0.0,
                },
                now,
            ),
            None
        );

        assert_eq!(
            mapper.handle_event(
                ControllerEvent::AxisChanged {
                    axis: ControllerAxis::LeftStickY,
                    value: -0.9,
                },
                now,
            ),
            Some(SpatialAction::Move(SpatialDirection::Up))
        );
    }

    #[test]
    fn ignores_left_stick_noise_below_engage_threshold() {
        let mut mapper = ControllerInputMapper::default();

        assert_eq!(
            mapper.handle_event(
                ControllerEvent::AxisChanged {
                    axis: ControllerAxis::LeftStickX,
                    value: 0.4,
                },
                base_time(),
            ),
            None
        );
    }

    #[test]
    fn held_left_stick_repeats_only_after_delay() {
        let mut mapper = ControllerInputMapper::default();
        let now = base_time();

        assert_eq!(
            mapper.handle_event(
                ControllerEvent::AxisChanged {
                    axis: ControllerAxis::LeftStickX,
                    value: -0.8,
                },
                now,
            ),
            Some(SpatialAction::Move(SpatialDirection::Left))
        );
        assert_eq!(mapper.tick(now + Duration::from_millis(100)), None);
        assert_eq!(
            mapper.tick(now + Duration::from_millis(250)),
            Some(SpatialAction::Move(SpatialDirection::Left))
        );
    }

    #[test]
    fn held_left_stick_releases_below_release_threshold() {
        let mut mapper = ControllerInputMapper::default();
        let now = base_time();

        assert_eq!(
            mapper.handle_event(
                ControllerEvent::AxisChanged {
                    axis: ControllerAxis::LeftStickY,
                    value: 0.8,
                },
                now,
            ),
            Some(SpatialAction::Move(SpatialDirection::Down))
        );
        assert_eq!(
            mapper.handle_event(
                ControllerEvent::AxisChanged {
                    axis: ControllerAxis::LeftStickY,
                    value: 0.2,
                },
                now + Duration::from_millis(10),
            ),
            None
        );
        assert_eq!(mapper.tick(now + Duration::from_millis(500)), None);
    }
}
