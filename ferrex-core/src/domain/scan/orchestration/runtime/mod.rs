//! In-memory runtime components for the scan orchestrator.
//!
//! These building blocks let us stand up a single-process runtime while the
//! durable queue and scheduler implementations incubate. They intentionally
//! keep business logic light so higher-level services can be wired together and
//! iterated on without committing to a storage schema yet.

mod event_bus;
mod supervisor;

pub use event_bus::*;
pub use supervisor::*;
