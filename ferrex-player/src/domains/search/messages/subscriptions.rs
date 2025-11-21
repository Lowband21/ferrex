use crate::common::messages::DomainMessage;
use crate::domains::ui::{messages::UiMessage, types::ViewState};
use crate::state::State;
use iced::Subscription;

/// Subscribe to top-level keyboard events and seed dropdown search when appropriate.
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    if !is_search_context(&state.domains.ui.state.view) {
        return Subscription::none();
    }

    if state.search_window_id.is_some() {
        return Subscription::none();
    }

    iced::keyboard::on_key_press(|key, modifiers| {
        use iced::keyboard::Key;

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

                Some(DomainMessage::Ui(UiMessage::OpenSearchWindowWithSeed(
                    text.to_string(),
                )))
            }
            _ => None,
        }
    })
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
