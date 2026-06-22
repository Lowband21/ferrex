//! Application-level composition services for core workflows.
//!
//! This layer coordinates domain services, repository ports, and database units
//! of work without depending on HTTP transport details.

/// Bootstrap helpers for role-based access control state.
#[cfg(feature = "database")]
pub mod rbac_bootstrap;
/// Unit-of-work facade over repository implementations.
#[cfg(feature = "database")]
pub mod unit_of_work;
