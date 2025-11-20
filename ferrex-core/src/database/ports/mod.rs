//! Repository ports (interfaces) grouped by bounded context.
//! These enable a DDD-style separation between domain/services and infrastructure.
//!
//! NOTE: Existing code still uses `MediaDatabaseTrait` as a façade. These ports
//! are introduced to improve cohesion and guide further refactoring. Implementations
//! live in the Postgres adapter under `database::postgres_ext`.

pub mod folder_inventory;
pub mod images;
pub mod indices;
pub mod library;
pub mod media_files;
pub mod media_references;
pub mod processing_status;
pub mod query;
pub mod rbac;
pub mod security_settings;
pub mod setup_claims;
pub mod sync_sessions;
pub mod users;
pub mod watch_metrics;
pub mod watch_status;
