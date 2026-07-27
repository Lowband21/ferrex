//! Compatibility facade for the Ferrex desktop player.
//!
//! The application shell now lives in `ferrex-player-app`, which assembles the
//! extracted UI, domain, and API crates. This package keeps the installed
//! `ferrex-player` binary and historical `ferrex_player::*` imports working
//! during the extraction stack.

#[cfg(any(target_os = "macos", test))]
pub mod macos_bundle_runtime;

pub use ferrex_player_app::*;
