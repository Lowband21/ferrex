//! Compatibility facade for the Ferrex desktop player.
//!
//! The application shell now lives in `ferrex-player-app`, which assembles the
//! extracted UI, domain, and API crates. This package keeps the installed
//! `ferrex-player` binary and historical `ferrex_player::*` imports working
//! during the extraction stack.

pub use ferrex_player_app::*;
