use crate::common::messages::DomainMessage;
use crate::domains::library::LibrariesLoadState;
use crate::domains::ui::{shell_ui::UiShellMessage, types::ViewState};
use crate::state::State;

use iced::Subscription;
use iced::event;
use iced::keyboard::{self, Key, Modifiers};

/// Subscribe to top-level keyboard events and seed dropdown search when appropriate.
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    // Only enable search keyboard listening after libraries have successfully loaded
    // This prevents the search window from popping up during user login
    if !matches!(
        state.domains.library.state.load_state,
        LibrariesLoadState::Succeeded { .. }
    ) {
        return Subscription::none();
    }

    if !is_search_context(&state.domains.ui.state.view) {
        return Subscription::none();
    }

    if state.domains.search.state.presentation.is_open() {
        return Subscription::none();
    }

    event::listen_with(|event, _status, _id| {
        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            ..
        }) = event
        else {
            return None;
        };
        handle_search_key_press(key, modifiers)
    })
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
