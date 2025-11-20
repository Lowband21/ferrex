use super::Message;
use crate::messages::DomainMessage;
use crate::state::State;
use iced::Subscription;

/// Creates all UI-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];

    // Always subscribe to window resize events
    subscriptions.push(
        iced::window::resize_events()
            .map(|(_id, size)| DomainMessage::Ui(Message::WindowResized(size))),
    );

    // Animation transitions subscription
    if state
        .background_shader_state
        .color_transitions
        .is_transitioning()
        || state
            .background_shader_state
            .backdrop_transitions
            .is_transitioning()
        || state
            .background_shader_state
            .gradient_transitions
            .is_transitioning()
    {
        subscriptions.push(
            iced::time::every(std::time::Duration::from_millis(16)) // ~60 FPS
                .map(|_| DomainMessage::Ui(Message::UpdateTransitions)),
        );
    }

    Subscription::batch(subscriptions)
}
