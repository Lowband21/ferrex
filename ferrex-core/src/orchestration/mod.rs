//! Orchestrator domain skeleton for the future scan pipeline.
//!
//! This module gathers the foundational types and contracts that describe the
//! scan orchestrator domain. The goal is to provide a compile-time home for the
//! forthcoming implementation without coupling it to today's scanner logic.

pub mod actors;
pub mod budget;
pub mod config;
pub mod correlation;
pub mod dispatcher;
pub mod events;
pub mod job;
pub mod lease;
pub mod persistence;
pub mod queue;
pub mod runtime;
pub mod scan_cursor;
pub mod scheduler;

pub use actors::*;
pub use budget::*;
pub use config::*;
pub use correlation::*;
pub use dispatcher::*;
pub use events::*;
pub use job::*;
pub use lease::*;
pub use persistence::*;
pub use queue::*;
pub use runtime::*;
pub use scan_cursor::*;
pub use scheduler::*;
