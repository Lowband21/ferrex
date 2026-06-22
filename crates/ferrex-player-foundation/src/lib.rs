//! Dependency-light player foundation primitives.
//!
//! `ferrex-player-foundation` is the bottom of the planned player crate stack.
//! It is allowed to depend on small Rust ecosystem crates such as `serde` and
//! `thiserror`, but it must not depend on the desktop player, UI frameworks,
//! video backends, or Ferrex domain crates. See
//! `docs/player-dependency-boundaries.md` in the workspace for the full policy.

#![forbid(unsafe_code)]

/// Authentication policy DTOs and official-client PIN helpers.
pub mod auth;
/// Generic domain update/effect/event containers.
pub mod domain;
/// Shared repository error/result primitives.
pub mod repository;
/// Unit-formatting helpers shared by player crates.
pub mod units;
