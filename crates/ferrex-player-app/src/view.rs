//! Root-level view composition for the Ferrex player app shell.

use iced::{Element, Theme};

use crate::{common::messages::DomainMessage, state::State};

/// Render the app shell for the given window.
pub fn view(
    state: &State,
    window_id: iced::window::Id,
) -> Element<'_, DomainMessage, Theme, iced::Renderer> {
    ferrex_player_ui::view::view(state, window_id)
}

/// Get the lucide font used by player UI surfaces.
pub fn lucide_font() -> iced::Font {
    ferrex_player_ui::view::lucide_font()
}
