//! Repository repository_ports (interfaces) grouped by bounded context.
//! These enable a DDD-style separation between domain/services and infra.
//!
//! Implementations live in the Postgres adapter under `database::infra::postgres`.

pub mod file_watch;
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
pub mod tmdb_metadata;
pub mod tmdb_metadata_batch_bulk;
pub mod tmdb_metadata_bulk;
pub mod users;
pub mod watch_metrics;
pub mod watch_status;
