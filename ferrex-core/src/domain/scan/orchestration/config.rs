//! Orchestrator configuration types.
//!
//! These configuration structures are shared across crates (config loading, server runtime,
//! and any tooling) so they live in `ferrex-model` and are re-exported here for backwards
//! compatibility with the `ferrex-core` domain module path.

pub use crate::types::scan::orchestration::config::*;
