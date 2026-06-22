//! Database abstractions and PostgreSQL-backed implementations.
//!
//! The database namespace exposes connection/context helpers, repository ports,
//! concrete repositories, Postgres extensions, and traits used by higher-level
//! application services.

/// Database context and connection management.
pub mod context;
/// PostgreSQL database adapter and connection-pool statistics.
pub mod postgres;
/// PostgreSQL-specific extensions that back domain workflows.
pub mod postgres_ext;
/// Concrete repository implementations.
pub mod repositories;
/// Repository trait ports grouped by bounded context.
pub mod repository_ports;
/// Database traits shared by adapters and application services.
pub mod traits;

pub use context::DatabaseContext;
pub use postgres::{PoolStats, PostgresDatabase};
