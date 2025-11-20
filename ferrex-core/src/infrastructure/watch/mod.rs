//! Watch-state infrastructure adapters.
//!
//! Provides a namespaced entry point for persistence implementations hooked up
//! to the watch-state domain.

#[cfg(feature = "database")]
pub mod repositories {
    pub use crate::database::infrastructure::postgres::repositories::watch_status::PostgresWatchStatusRepository;
}

// Compatibility re-export so existing imports can flatten during the migration.
#[cfg(feature = "database")]
pub use repositories::*;
