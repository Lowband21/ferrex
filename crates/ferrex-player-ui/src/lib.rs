//! Ferrex player UI and presentation library.
//!
//! This crate owns the desktop player's UI-facing library surfaces: design
//! tokens, theme helpers, shader widgets and WGSL assets, Iced views/widgets,
//! UI state/messages, window/focus helpers, and 10-foot surfaces.
//!
//! The `ferrex-player-app` crate owns the runtime shell and re-exports these
//! UI/domain surfaces for compatibility. The `ferrex-player` package keeps the
//! installed binary/facade.

/// Common UI messages, helpers, and presentation primitives.
pub mod common;
/// UI-facing domain modules and compatibility surfaces.
pub mod domains;
/// UI infrastructure such as shader widgets, testing helpers, and services.
pub mod infra;
/// Central UI state facade used by views and reducers.
pub mod state;
/// UI subscription helpers.
pub mod subscriptions;
/// UI update/routing helpers.
pub mod update;
/// Iced view modules and widgets.
pub mod view;
