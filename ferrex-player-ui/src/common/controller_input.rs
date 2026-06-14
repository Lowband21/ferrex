//! Runtime-agnostic controller input mapping for 10-foot navigation.
//!
//! Native backends can normalize their controller events into these small enums
//! and feed them through [`ControllerInputMapper`] to produce spatial actions.
//! This module deliberately avoids depending on a gamepad runtime crate.

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

/// Runtime-neutral controller event consumed by the mapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerEvent {
    ButtonPressed(ControllerButton),
}

/// Maps normalized controller events into 10-foot spatial actions.
#[derive(Debug, Clone, Copy, Default)]
pub struct ControllerInputMapper;

impl ControllerInputMapper {
    pub const fn new() -> Self {
        Self
    }

    /// Consumes one normalized controller event and returns the spatial action
    /// that should affect 10-foot focus or activation.
    pub fn handle_event(&self, event: ControllerEvent) -> SpatialAction {
        match event {
            ControllerEvent::ButtonPressed(button) => button.action(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_dpad_to_spatial_movement() {
        let mapper = ControllerInputMapper::new();

        assert_eq!(
            mapper.handle_event(ControllerEvent::ButtonPressed(
                ControllerButton::DPadUp,
            )),
            SpatialAction::Move(SpatialDirection::Up)
        );
        assert_eq!(
            mapper.handle_event(ControllerEvent::ButtonPressed(
                ControllerButton::DPadDown,
            )),
            SpatialAction::Move(SpatialDirection::Down)
        );
        assert_eq!(
            mapper.handle_event(ControllerEvent::ButtonPressed(
                ControllerButton::DPadLeft,
            )),
            SpatialAction::Move(SpatialDirection::Left)
        );
        assert_eq!(
            mapper.handle_event(ControllerEvent::ButtonPressed(
                ControllerButton::DPadRight,
            )),
            SpatialAction::Move(SpatialDirection::Right)
        );
    }

    #[test]
    fn maps_face_and_menu_buttons_to_spatial_actions() {
        let mapper = ControllerInputMapper::new();

        assert_eq!(
            mapper.handle_event(ControllerEvent::ButtonPressed(
                ControllerButton::South,
            )),
            SpatialAction::Activate
        );
        assert_eq!(
            mapper.handle_event(ControllerEvent::ButtonPressed(
                ControllerButton::East,
            )),
            SpatialAction::Back
        );
        assert_eq!(
            mapper.handle_event(ControllerEvent::ButtonPressed(
                ControllerButton::Start,
            )),
            SpatialAction::Menu
        );
        assert_eq!(
            mapper.handle_event(ControllerEvent::ButtonPressed(
                ControllerButton::Select,
            )),
            SpatialAction::Search
        );
    }
}
