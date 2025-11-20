use super::Message;
use crate::common::messages::DomainMessage;
use crate::state::State;
use iced::Subscription;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];

    subscriptions.push(
        iced::window::resize_events()
            .map(|(_id, size)| DomainMessage::Ui(Message::WindowResized(size))),
    );

    // Close search window on Esc/Enter when it is open
    if state.search_window_id.is_some() {
        subscriptions.push(iced::keyboard::on_key_press(|key, _modifiers| {
            use iced::keyboard::key::Named;
            use iced::keyboard::Key;
            match key {
                Key::Named(Named::Escape) | Key::Named(Named::Enter) => {
                    Some(DomainMessage::Ui(Message::CloseSearchWindow))
                }
                _ => None,
            }
        }));
    }

    // Watch for close requests and close only our search window
    if let Some(search_id) = state.search_window_id {
        subscriptions.push(
            iced::window::close_requests().map(move |(id, _)| {
                if id == search_id {
                    DomainMessage::Ui(Message::CloseSearchWindow)
                } else {
                    DomainMessage::NoOp
                }
            }),
        );
    }

    let poster_anim_active = state
        .domains
        .ui
        .state
        .poster_anim_active_until
        .map(|until| until > std::time::Instant::now())
        .unwrap_or(false);

    if state
        .domains
        .ui
        .state
        .background_shader_state
        .color_transitions
        .is_transitioning()
        || state
            .domains
            .ui
            .state
            .background_shader_state
            .backdrop_transitions
            .is_transitioning()
        || state
            .domains
            .ui
            .state
            .background_shader_state
            .gradient_transitions
            .is_transitioning()
        || poster_anim_active
    {
        subscriptions.push(
            iced::time::every(std::time::Duration::from_nanos(8_333_333)) // ~120 FPS
                .map(|_| DomainMessage::Ui(Message::UpdateTransitions)),
        );
    }

    Subscription::batch(subscriptions)
}
