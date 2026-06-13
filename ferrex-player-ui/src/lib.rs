//! Ferrex player UI and presentation library.
//!
//! This crate owns the desktop player's UI-facing library surfaces: design
//! tokens, theme helpers, shader widgets and WGSL assets, Iced views/widgets,
//! UI state/messages, window/focus helpers, and 10-foot surfaces.
//!
//! The `ferrex-player-app` crate owns the runtime shell and re-exports these
//! UI/domain surfaces for compatibility. The `ferrex-player` package keeps the
//! installed binary/facade.

/// Core module declaration as library to enable application module reuse in tests.
pub mod common;
pub mod domains;
pub mod infra;
pub mod state;
pub mod subscriptions;
pub mod update;
pub mod view;
