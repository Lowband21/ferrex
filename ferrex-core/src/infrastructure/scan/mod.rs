//! Scan runtime infrastructure adapters.
//!
//! Provides namespaced access to filesystem watch services and Postgres-backed
//! orchestrator repositories while downstream crates migrate away from the
//! legacy root modules.

pub mod fs_watch {
    pub use crate::scan::fs_watch::{
        FsWatchConfig, FsWatchObserver, FsWatchService, NoopFsWatchObserver,
    };
}

#[cfg(feature = "database")]
pub mod orchestration {
    pub use crate::scan::orchestration::persistence::{
        PostgresCursorRepository, PostgresQueueService,
    };
}

// Transitional re-exports so call sites can flatten imports short-term.
pub use fs_watch::*;
#[cfg(feature = "database")]
pub use orchestration::*;
