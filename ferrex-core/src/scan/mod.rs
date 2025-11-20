//! Scan domain modules.
//!
//! The scan subsystem pulls together folder discovery, filesystem watch pipelines, queue-based
//! orchestrator actors, and supporting fixtures. These modules now form the canonical home for
//! scanning behaviour; downstream crates import from this namespace directly instead of the former
//! root-level shims.

pub mod fs_watch;
pub mod orchestration;
pub mod scanner;

// Re-export key surfaces so downstream code can write `crate::scan::*`.
pub use fs_watch::*;
pub use orchestration::*;
pub use scanner::*;
