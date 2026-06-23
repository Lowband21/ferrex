//! PostgreSQL-backed repository implementations.

mod collection_rule_evaluator;
pub mod collections;
pub mod file_watch;
pub mod folder_inventory;
mod fuzzy_title_search;
pub mod images;
pub mod indices;
pub mod intelligence;
pub mod library;
pub mod media;
pub mod media_references;
pub mod processing_status;
pub mod query;
pub mod rbac;
pub mod security_settings;
pub mod setup_claims;
pub mod sync_sessions;
pub mod transcripts;
pub mod users;
pub mod watch_metrics;
pub mod watch_status;
