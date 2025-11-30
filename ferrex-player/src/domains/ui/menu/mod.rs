pub mod messages;
pub mod state;
pub mod update;

pub use messages::PosterMenuMessage;
pub use state::PosterMenuState;
pub use update::poster_menu_update;

use crate::infra::constants;

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
        constants::menu::button_from_y(y).and_then(|i| match i {
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
        constants::menu::in_x_bounds(x)
    }
}

/// Explicit interaction phase - replaces boolean flags for clearer state management.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InteractionPhase {
    /// Animating toward target via direct spring physics.
    /// Used after single clicks or when settled.
    Idle,

    /// Right mouse button being held - continuous acceleration.
    Holding,

    /// Released after hold - uses periodic potential (sin-based gravity wells)
    /// for multi-rotation settling. If must_reach_opposite is true, will nudge
    /// toward the opposite face if settling on the same face we started from.
    Settling { must_reach_opposite: bool },
}
