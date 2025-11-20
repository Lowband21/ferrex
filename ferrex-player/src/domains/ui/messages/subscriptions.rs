use super::Message;
use crate::common::messages::DomainMessage;
use crate::domains::ui::kinetic_scroll::messages::{
    Direction as Dir, KineticMessage as KM,
};
use crate::domains::ui::tabs::TabId;
use crate::domains::ui::types::DisplayMode;
use crate::domains::ui::types::ViewState;
use crate::{
    domains::{
        search::messages::Message as SearchMessage, ui::windows::WindowKind,
    },
    state::State,
};
use iced::Subscription;
use iced::event::{self, Event as RuntimeEvent, Status as EventStatus};
use iced::keyboard::{self, Key};

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

    // Delegate window lifecycle subscriptions (resize, move, focus) to the
    // window management module so secondary windows stay isolated
    subscriptions.push(
        crate::domains::ui::windows::subscriptions::subscription(state),
    );

    // Dedicated search window keyboard interactions
    if state.search_window_id.is_some() {
        subscriptions.push(event::listen_with(search_window_key_handler));
    }

    // Watch for close requests and close only our search window
    if let Some(search_id) = state.search_window_id {
        subscriptions.push(iced::window::close_requests().with(search_id).map(
            |(search_id, id)| {
                if id == search_id {
                    DomainMessage::Ui(Message::CloseSearchWindow)
                } else {
                    DomainMessage::NoOp
                }
            },
        ));
    }

    let in_grid_context = state.search_window_id.is_none()
        && matches!(state.domains.ui.state.view, ViewState::Library)
        && matches!(state.domains.ui.state.display_mode, DisplayMode::Library)
        && matches!(state.tab_manager.active_tab_id(), TabId::Library(_));

    if in_grid_context {
        subscriptions.push(event::listen_with(main_window_grid_key_handler));
    }

    if state.domains.ui.state.kinetic_scroll.is_active() {
        subscriptions.push(
            iced::time::every(std::time::Duration::from_nanos(
                crate::infra::constants::performance_config::scrolling::KINETIC_TICK_NS,
            ))
            .map(|_| DomainMessage::Ui(Message::KineticScroll(KM::Tick)) ),
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

fn search_window_key_handler(
    event: RuntimeEvent,
    _status: EventStatus,
    _window: iced::window::Id,
) -> Option<DomainMessage> {
    use iced::keyboard::key::Named;

    if let RuntimeEvent::Keyboard(keyboard::Event::KeyPressed {
        key,
        modifiers,
        ..
    }) = event
    {
        if modifiers.control() || modifiers.alt() || modifiers.logo() {
            return None;
        }

        match key {
            Key::Named(Named::Escape) => {
                Some(DomainMessage::Search(SearchMessage::HandleEscape))
            }
            Key::Named(Named::Enter) => {
                Some(DomainMessage::Search(SearchMessage::SelectCurrent))
            }
            Key::Named(Named::ArrowUp) => {
                Some(DomainMessage::Search(SearchMessage::SelectPrevious))
            }
            Key::Named(Named::ArrowDown) => {
                Some(DomainMessage::Search(SearchMessage::SelectNext))
            }
            Key::Character(value) if modifiers.shift() => None,
            Key::Character(value) if value.eq_ignore_ascii_case("k") => {
                Some(DomainMessage::Search(SearchMessage::SelectPrevious))
            }
            Key::Character(value) if value.eq_ignore_ascii_case("j") => {
                Some(DomainMessage::Search(SearchMessage::SelectNext))
            }
            _ => None,
        }
    } else {
        None
    }
}

fn main_window_grid_key_handler(
    event: RuntimeEvent,
    status: EventStatus,
    _window: iced::window::Id,
) -> Option<DomainMessage> {
    use iced::keyboard::key::Named;

    if !matches!(status, EventStatus::Ignored) {
        return None;
    }

    match event {
        RuntimeEvent::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            ..
        }) => {
            if modifiers.control() || modifiers.alt() || modifiers.logo() {
                return None;
            }
            match key {
                Key::Named(Named::ArrowDown) => Some(DomainMessage::Ui(
                    Message::KineticScroll(KM::Start(Dir::Down)),
                )),
                Key::Named(Named::ArrowUp) => Some(DomainMessage::Ui(
                    Message::KineticScroll(KM::Start(Dir::Up)),
                )),
                Key::Named(Named::Shift) => Some(DomainMessage::Ui(
                    Message::KineticScroll(KM::SetBoost(true)),
                )),
                _ => None,
            }
        }
        RuntimeEvent::Keyboard(keyboard::Event::KeyReleased {
            key,
            modifiers,
            ..
        }) => {
            if modifiers.control() || modifiers.alt() || modifiers.logo() {
                return None;
            }
            match key {
                Key::Named(Named::ArrowDown) => Some(DomainMessage::Ui(
                    Message::KineticScroll(KM::Stop(Dir::Down)),
                )),
                Key::Named(Named::ArrowUp) => Some(DomainMessage::Ui(
                    Message::KineticScroll(KM::Stop(Dir::Up)),
                )),
                Key::Named(Named::Shift) => Some(DomainMessage::Ui(
                    Message::KineticScroll(KM::SetBoost(false)),
                )),
                _ => None,
            }
        }
        _ => None,
    }
}
