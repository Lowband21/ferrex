use crate::common::messages::DomainMessage;
use crate::domains::library::LibrariesLoadState;
use crate::domains::ui::{shell_ui::UiShellMessage, types::ViewState};
use crate::state::State;
use ferrex_player_search::messages::subscriptions::{
    SearchSubscriptionInputs, subscriptions_active,
};
use iced::Subscription;
use iced::event;
use iced::keyboard::{self, Key, Modifiers};

/// Subscribe to top-level keyboard events and seed dropdown search when appropriate.
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    // Only enable search keyboard listening after libraries have successfully loaded
    // and the current UI route can open search. This prevents the search window
    // from popping up during login while keeping the lower search crate free of
    // Iced event types.
    if !subscriptions_active(SearchSubscriptionInputs {
        libraries_loaded: matches!(
            state.domains.library.state.load_state,
            LibrariesLoadState::Succeeded { .. }
        ),
        search_context: is_search_context(&state.domains.ui.state.view),
        search_mode: state.domains.search.state.mode,
        presentation_open: state.domains.search.state.presentation.is_open(),
        tenfoot: state.interface_mode.is_tenfoot(),
        tenfoot_keyboard_open: state
            .domains
            .search
            .state
            .tenfoot_keyboard
            .is_open(),
    }) {
        return Subscription::none();
    }

    if state.interface_mode.is_tenfoot() {
        event::listen().map(|event| {
            let iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                ..
            }) = event
            else {
                return DomainMessage::NoOp;
            };

            handle_tenfoot_search_key_press(key, modifiers)
                .unwrap_or(DomainMessage::NoOp)
        })
    } else {
        event::listen_with(desktop_search_key_handler)
    }
}

fn desktop_search_key_handler(
    event: iced::Event,
    _status: iced::event::Status,
    _id: iced::window::Id,
) -> Option<DomainMessage> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key,
        modifiers,
        ..
    }) = event
    else {
        return None;
    };

    handle_search_key_press(key, modifiers)
}

fn handle_tenfoot_search_key_press(
    key: Key,
    modifiers: Modifiers,
) -> Option<DomainMessage> {
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }

    match key {
        Key::Character(value)
            if value.as_str() == "/"
                || value.as_str().eq_ignore_ascii_case("s") =>
        {
            Some(DomainMessage::Ui(UiShellMessage::OpenSearchOverlay.into()))
        }
        _ => None,
    }
}

fn handle_search_key_press(
    key: Key,
    modifiers: Modifiers,
) -> Option<DomainMessage> {
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }

    match key {
        Key::Character(value) => {
            let text = value.as_str();
            if text.len() != 1 {
                return None;
            }

            let mut chars = text.chars();
            let ch = chars.next().unwrap();

            if !ch.is_ascii_alphanumeric() {
                return None;
            }

            Some(DomainMessage::Ui(
                UiShellMessage::OpenSearchOverlayWithSeed(text.to_string())
                    .into(),
            ))
        }
        _ => None,
    }
}

fn is_search_context(view: &ViewState) -> bool {
    matches!(
        view,
        ViewState::Library
            | ViewState::MovieDetail { .. }
            | ViewState::SeriesDetail { .. }
            | ViewState::SeasonDetail { .. }
            | ViewState::EpisodeDetail { .. }
    )
}
