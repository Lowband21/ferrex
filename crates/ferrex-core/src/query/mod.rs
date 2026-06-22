//! Query-building, filtering, sorting, and execution strategy helpers.
//!
//! The query layer lets server and clients describe media searches with bounded
//! complexity and decide whether work should run client-side or server-side.

/// Fluent media-query builder types.
pub mod builder;
/// Query complexity scoring and guard rails.
pub mod complexity_guard;
/// Client/server query execution strategy selection.
pub mod decision_engine;
/// Filter predicates and filter composition helpers.
pub mod filtering;
/// Curated query re-exports for downstream crates.
pub mod prelude;
/// Sorting helpers for hybrid client/server sorting.
pub mod sorting;
/// Query DTOs and enums.
pub mod types;

pub use builder::MediaQueryBuilder;
pub use complexity_guard::{ComplexityConfig, QueryComplexityGuard};
