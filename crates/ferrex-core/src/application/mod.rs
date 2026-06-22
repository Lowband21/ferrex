//! Application-level composition services for core workflows.
//!
//! This layer coordinates domain services, repository ports, and database units
//! of work without depending on HTTP transport details.

/// Bounded grounded LLM run orchestration over providers and tools.
#[cfg(feature = "database")]
pub mod intelligence_runtime;
/// Bounded grounded LLM tool registry over intelligence/query repositories.
#[cfg(feature = "database")]
pub mod intelligence_tools;
/// Bootstrap helpers for role-based access control state.
#[cfg(feature = "database")]
pub mod rbac_bootstrap;
/// Unit-of-work facade over repository implementations.
#[cfg(feature = "database")]
pub mod unit_of_work;
