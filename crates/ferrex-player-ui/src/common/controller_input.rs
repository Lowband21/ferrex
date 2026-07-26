//! Controller input mapping and native event delivery for 10-foot navigation.
//!
//! Native backends can normalize their controller events into these small enums
//! and feed them through [`ControllerInputMapper`] to produce spatial actions.
//! The normalized contract remains runtime-neutral; macOS additionally owns a
//! native event subscription so a physical controller reaches the same
//! canonical action path as keyboard navigation.

use crate::common::focus::{SpatialAction, SpatialDirection};
use iced::Subscription;

#[cfg(target_os = "macos")]
use futures::{StreamExt, stream::BoxStream};
#[cfg(target_os = "macos")]
use std::{sync::OnceLock, time::Duration};
#[cfg(target_os = "macos")]
use tokio::sync::broadcast;

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

/// Delivers normalized button presses from the platform controller runtime.
///
/// macOS subscribes to one process-wide, bounded native gamepad event hub.
/// Other targets return no events until their platform backends are wired; the
/// public normalized contract remains identical.
pub fn native_controller_subscription() -> Subscription<ControllerButton> {
    #[cfg(target_os = "macos")]
    {
        Subscription::run(native_controller_stream)
    }

    #[cfg(not(target_os = "macos"))]
    {
        Subscription::none()
    }
}

#[cfg(target_os = "macos")]
const CONTROLLER_EVENT_CAPACITY: usize = 64;

#[cfg(target_os = "macos")]
static CONTROLLER_EVENTS: OnceLock<broadcast::Sender<ControllerButton>> =
    OnceLock::new();

#[cfg(target_os = "macos")]
fn native_controller_stream() -> BoxStream<'static, ControllerButton> {
    let receiver = controller_events().subscribe();

    futures::stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(button) => return Some((button, receiver)),
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    log::warn!(
                        "macOS controller input dropped {skipped} stale events"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
    .boxed()
}

#[cfg(target_os = "macos")]
fn controller_events() -> &'static broadcast::Sender<ControllerButton> {
    CONTROLLER_EVENTS.get_or_init(|| {
        let (sender, initial_receiver) =
            broadcast::channel(CONTROLLER_EVENT_CAPACITY);
        drop(initial_receiver);
        spawn_controller_worker(sender.clone());
        sender
    })
}

#[cfg(target_os = "macos")]
fn spawn_controller_worker(sender: broadcast::Sender<ControllerButton>) {
    let spawn_result = std::thread::Builder::new()
        .name("ferrex-macos-controller".to_string())
        .spawn(move || {
            let mut controllers = match gilrs::GilrsBuilder::new()
                .with_force_feedback(false)
                .build()
            {
                Ok(controllers) => controllers,
                Err(error) => {
                    log::error!(
                        "macOS controller input initialization failed: {error}"
                    );
                    return;
                }
            };

            // gilrs-core's macOS IOHID implementation owns a CFRunLoop thread
            // without a public cancellation handle. Keep exactly one gilrs
            // instance for the process lifetime instead of constructing one
            // whenever an Iced subscription snapshot changes.
            loop {
                let Some(event) = controllers
                    .next_event_blocking(Some(Duration::from_millis(250)))
                else {
                    continue;
                };
                let gilrs::EventType::ButtonPressed(button, _) = event.event
                else {
                    continue;
                };
                let Some(button) = normalize_gilrs_button(button) else {
                    continue;
                };
                // The broadcast ring is bounded. With no active player input
                // subscription this simply reports no receivers and retains
                // no queued button history.
                let _ = sender.send(button);
            }
        });

    if let Err(error) = spawn_result {
        log::error!("could not start macOS controller input worker: {error}");
    }
}

#[cfg(target_os = "macos")]
fn normalize_gilrs_button(button: gilrs::Button) -> Option<ControllerButton> {
    match button {
        gilrs::Button::DPadUp => Some(ControllerButton::DPadUp),
        gilrs::Button::DPadDown => Some(ControllerButton::DPadDown),
        gilrs::Button::DPadLeft => Some(ControllerButton::DPadLeft),
        gilrs::Button::DPadRight => Some(ControllerButton::DPadRight),
        gilrs::Button::South => Some(ControllerButton::South),
        gilrs::Button::East => Some(ControllerButton::East),
        gilrs::Button::Start => Some(ControllerButton::Start),
        gilrs::Button::Select => Some(ControllerButton::Select),
        _ => None,
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

    #[cfg(target_os = "macos")]
    #[test]
    fn gilrs_buttons_normalize_to_the_runtime_neutral_contract() {
        for (native, normalized) in [
            (gilrs::Button::DPadUp, ControllerButton::DPadUp),
            (gilrs::Button::DPadDown, ControllerButton::DPadDown),
            (gilrs::Button::DPadLeft, ControllerButton::DPadLeft),
            (gilrs::Button::DPadRight, ControllerButton::DPadRight),
            (gilrs::Button::South, ControllerButton::South),
            (gilrs::Button::East, ControllerButton::East),
            (gilrs::Button::Start, ControllerButton::Start),
            (gilrs::Button::Select, ControllerButton::Select),
        ] {
            assert_eq!(normalize_gilrs_button(native), Some(normalized));
        }
        assert_eq!(normalize_gilrs_button(gilrs::Button::North), None);
    }
}
