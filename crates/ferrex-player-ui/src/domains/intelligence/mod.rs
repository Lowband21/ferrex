//! UI-facing compatibility surface for smart-shelf intelligence state.
//!
//! The reducer and DTO-shaped command/intent values live in
//! `ferrex-player-intelligence` so desktop views can render and schedule the
//! flow without duplicating smart-shelf business logic or depending on Iced from
//! the lower-level domain crate.

pub use ferrex_player_intelligence::*;
