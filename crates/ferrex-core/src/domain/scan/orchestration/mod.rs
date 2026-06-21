//! Scan orchestration domain ports, adapters, and runtime wiring.
//!
//! `BOUNDARY_CONTRACTS.md` maps the public black-box seams in this module to
//! the focused characterization tests that pin current behavior before further
//! extraction work.

pub mod budget;
pub mod config;
pub mod context;
pub mod correlation;
pub mod delta;
pub mod dispatcher;
pub mod events;
pub mod job;
pub mod lease;
pub mod maintenance;
pub mod persistence;
pub mod queue;
pub mod runtime;
pub mod scan_cursor;
pub mod scan_run;
pub mod scheduler;
pub mod series;
pub mod series_state;

pub use crate::domain::scan::actors::*;
pub use budget::*;
pub use config::*;
pub use correlation::*;
pub use delta::*;
pub use dispatcher::*;
pub use events::*;
pub use job::*;
pub use lease::*;
pub use maintenance::*;
pub use persistence::*;
pub use queue::*;
pub use runtime::*;
pub use scan_cursor::*;
pub use scan_run::*;
pub use scheduler::*;
pub use series::*;
pub use series_state::*;
