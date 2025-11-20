//! Ferrex Player library
//!
//! This crate contains the desktop player's library surfaces used by the
//! executable in `src/main.rs`. Modules here are primarily application glue,
//! UI domains, and infrastructure helpers that are still evolving.
//!
//! Notes
//! - Public items are subject to change while the UI and domains stabilize.
//! - Most consumers should use the `ferrex-player` binary; the library is
//!   exposed mainly to enable testing and internal reuse.

pub mod app;
/// Core module declaration as library to enable utilizing application modules for testing
pub mod common;
pub mod domains;
pub mod infra;
pub mod state;
pub mod subscriptions;
pub mod update;
pub mod view;
