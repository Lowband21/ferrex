//! Local message types for virtual carousel interactions (scaffold)

use std::time::Instant;

use iced::widget::scrollable;

use super::types::CarouselKey;

#[derive(Debug, Clone)]
pub enum VirtualCarouselMessage {
    // Navigation
    NextPage(CarouselKey),
    PrevPage(CarouselKey),
    NextItem(CarouselKey),
    PrevItem(CarouselKey),
    // Active-context navigation (key-less)
    NextPageActive,
    PrevPageActive,
    NextItemActive,
    PrevItemActive,

    // Focus management
    FocusKey(CarouselKey),
    BlurKey(CarouselKey),

    // Kinetic
    // Active-context variants avoid keys to keep Subscription::map closures non-capturing
    StartRightActive,
    StartLeftActive,
    StopRightActive,
    StopLeftActive,
    /// Frame-synchronized tick with timestamp from window::frames()
    MotionTick(Instant),
    SetBoostActive(bool),

    // Viewport / scroll reporting
    ViewportChanged(CarouselKey, scrollable::Viewport),
}
