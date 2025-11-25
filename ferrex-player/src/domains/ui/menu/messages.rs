use uuid::Uuid;

use crate::infra::constants::menu;

/// Menu button identifiers matching shader constants
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuButton {
    Play = 0,
    Details = 1,
    Watched = 2,
    Watchlist = 3, // Grayed out for alpha
    Edit = 4,      // Grayed out for alpha
}

impl MenuButton {
    /// Returns true if this button is disabled (grayed out) for alpha
    pub fn is_disabled(&self) -> bool {
        matches!(self, MenuButton::Watchlist | MenuButton::Edit)
    }

    /// Get button index from normalized y position.
    /// Uses centralized constants from `crate::infra::constants::menu`.
    pub fn from_position(y: f32) -> Option<Self> {
        menu::button_from_y(y).and_then(|i| match i {
            0 => Some(MenuButton::Play),
            1 => Some(MenuButton::Details),
            2 => Some(MenuButton::Watched),
            3 => Some(MenuButton::Watchlist),
            4 => Some(MenuButton::Edit),
            _ => None,
        })
    }

    /// Check if position is within button x bounds.
    /// Uses centralized constants from `crate::infra::constants::menu`.
    pub fn in_x_bounds(x: f32) -> bool {
        menu::in_x_bounds(x)
    }
}

#[derive(Clone, Debug)]
pub enum PosterMenuMessage {
    Toggle(Uuid),
    Close(Uuid),
    HoldStart(Uuid),
    HoldEnd(Uuid),
    /// Button clicked on backface menu
    ButtonClicked(Uuid, MenuButton),
}
