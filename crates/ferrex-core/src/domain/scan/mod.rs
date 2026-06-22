//! Scan domain modules.
//!
//! Deep-module map:
//! - `actors` owns scanner actor ports, commands, and actor-local messages.
//! - `fs_watch` owns filesystem watcher debouncing, durable watch-event replay,
//!   and watcher-to-library-command handoff.
//! - `orchestration` owns durable queue ports/adapters, runtime supervision,
//!   work planning, event/correlation contracts, and scan-run read models.
//! - `scanner` owns legacy scanner settings and compatibility helpers.
//!
//! The production invariant is that scan work enters through the orchestration
//! facades (`LibraryCommandExecutor`, `QueueService`, and the server
//! `ScanOrchestrator`) and progress/catalog projections are driven from the
//! scan control facade. Root re-exports below are compatibility shims for
//! existing callers; new code should prefer the owning deep module.

pub mod actors;
pub mod fs_watch;
pub mod orchestration;
pub mod scanner;

// Re-export key surfaces so downstream code can write `crate::scan::*`.
pub use fs_watch::*;
pub use orchestration::*;
pub use scanner::*;
