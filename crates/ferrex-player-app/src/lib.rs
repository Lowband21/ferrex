//! Ferrex desktop player application shell.
//!
//! This crate owns the runtime shell around the extracted player UI and domain
//! crates: boot configuration, state construction, cross-domain update/view/
//! subscription composition, Iced application/daemon wiring, runtime presets,
//! and logger/profiling startup hooks.
//!
//! `ferrex-player` keeps the installed binary name and re-exports this crate as
//! a compatibility facade for historical `ferrex_player::*` imports.

pub mod app;
pub mod screenshot;
pub mod subscriptions;
pub mod update;
pub mod view;

/// Compatibility re-export of shared player utilities and message types.
pub mod common {
    pub use ferrex_player_ui::common::*;
}

/// Compatibility re-export of extracted/player domain surfaces used by the app shell.
pub mod domains {
    pub use ferrex_player_ui::domains::*;
}

/// Compatibility re-export of player infrastructure adapters and services.
pub mod infra {
    pub use ferrex_player_ui::infra::*;
}

/// Compatibility re-export of the central player shell state.
pub mod state {
    pub use ferrex_player_ui::state::*;
}

pub use app::{
    AppConfig, application, application_with_presets, init_runtime_hooks, run,
    run_with_config,
};
pub use state::{InterfaceMode, State};

/// Compatibility alias for older `ferrex_player::messages::*` imports.
pub mod messages {
    pub use crate::common::messages::*;
}

/// App-shell composition surfaces re-exported for integration tests and
/// embedders that want explicit access to the assembled shell pieces.
pub mod shell {
    pub use crate::common;
    pub use crate::common::messages::cross_domain;
    pub use crate::domains;
    pub use crate::state;
    pub use crate::subscriptions;
    pub use crate::update;
    pub use crate::view;
}

/// Result type returned by the Ferrex Iced runtime.
pub type Result = iced::Result;
