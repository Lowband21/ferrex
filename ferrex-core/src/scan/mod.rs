//! Scan domain modules.
//!
//! The scan subsystem pulls together folder discovery, filesystem watch pipelines, queue-based
//! orchestrator actors, and supporting fixtures. Over time these modules will become the canonical
//! home for scanning behaviour; top-level `orchestration`, `fs_watch`, and `scanner` exports now act
//! as compatibility shims.

pub mod fs_watch;
pub mod orchestration;
pub mod scanner;

// Re-export key surfaces so new call sites can bind to `crate::scan` immediately while legacy paths
// continue to work through the shims.
pub use fs_watch::*;
pub use orchestration::*;
pub use scanner::*;
