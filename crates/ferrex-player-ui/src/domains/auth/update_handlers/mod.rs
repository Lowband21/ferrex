//! Authentication update handlers owned by the UI crate.
//!
//! These handlers touch desktop `State`, focus, and cross-domain loading glue,
//! so they live with the presentation/app shell instead of the dependency-light
//! `ferrex-player-auth` data crate.

pub mod auth_flow;
pub mod first_run;

// Re-export update functions for the update router
pub use auth_flow::*;
pub use first_run::*;
