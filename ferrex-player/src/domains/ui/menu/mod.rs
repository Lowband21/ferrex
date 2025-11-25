pub mod messages;
pub mod state;
pub mod update;

pub use messages::{MenuButton, PosterMenuMessage};
pub use state::PosterMenuState;
pub use update::poster_menu_update;

pub const MENU_AUTO_CLOSE_MS: u64 = 1000;
pub const MENU_KEEPALIVE_MS: u64 = 1200;
