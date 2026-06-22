//! # Ferrex Core
//!
//! Core library for the Ferrex Media Server, providing fundamental types, database abstractions,
//! and business logic for media management, user authentication, and playback synchronization.
//!
//! ## Overview
//!
//! `ferrex-core` is the foundation of the Ferrex Media Server ecosystem. It owns
//! the reusable domain, query, database-port, and API DTO surfaces consumed by
//! the HTTP server, player crates, FlatBuffers adapters, and tooling. It offers:
//!
//! - **Media Management**: Comprehensive types for movies, TV shows, episodes, and media files
//! - **User System**: Opaque session tokens with refresh rotation and device management
//! - **Watch Status Tracking**: Track viewing progress and completion status
//! - **Synchronized Playback**: Real-time synchronized viewing sessions
//! - **Database Abstraction**: Trait-based database interface supporting multiple backends
//! - **Metadata Processing**: Integration with TMDB for media metadata
//! - **Query System**: Flexible media querying with filters and sorting
//!
//! ## Feature Flags
//!
//! - `database`: Enables database functionality (PostgreSQL/SQLx support)
//! - `ffmpeg`: Enables FFmpeg-based metadata extraction
//! - `test-utils`: Provides utilities for testing
//!
//! ## Architecture
//!
//! The crate is organized into several key modules:
//!
//! - [`api`]: Versioned routes and cross-service API DTOs
//! - [`domain::users`]: User authentication and session management
//! - [`domain::watch`]: Media playback progress tracking
//! - [`sync_session`]: Synchronized playback session management
//! - [`query`]: Advanced media querying capabilities
//! - [`database`]: Database traits and implementations
//!
//! ## Examples
//!
//! ```ignore
//! use ferrex_core::{
//!     database::DatabaseContext,
//!     player_prelude::{MediaID, MediaIDLike, MovieID, UpdateProgressRequest, UserWatchState},
//!     // user::RegisterRequest,
//! };
//!
//! async fn register_and_track(database_url: &str) -> Result<(), Box<dyn std::error::Error>> {
//!     let db_ctx = DatabaseContext::connect_postgres(database_url, None).await?;
//!     let unit_of_work = db_ctx.unit_of_work();
//!
//!     let request = RegisterRequest {
//!         username: "alice".to_string(),
//!         password: "secure_password".to_string(),
//!         display_name: "Alice".to_string(),
//!     };
//!
//!     let mut watch_state = UserWatchState::new();
//!     let movie = MediaID::Movie(MovieID::new());
//!     let progress = UpdateProgressRequest {
//!         media_id: movie.to_uuid(),
//!         media_type: movie.media_type(),
//!         position: 1800.0,
//!         duration: 7200.0,
//!     };
//!
//!     watch_state.update_progress(progress.media_id, progress.position, progress.duration);
//!     println!("Prepared registration for {}", request.username);
//!     Ok(())
//! }
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
// The core crate still exposes a large pre-alpha public surface inherited by the
// server and player crates. Crate/module docs and curated re-export docs are
// ratcheted here while field-level missing-doc cleanup continues incrementally.
#![allow(missing_docs)]

/// Versioned routes and API data transfer objects.
pub mod api;

/// Domain module grouping core business logic.
pub mod domain;

/// Infrastructure adapters for external services, caches, media analysis, and archive conversion.
pub mod infra;

/// Database abstraction layer and PostgreSQL implementations.
#[cfg(feature = "database")]
#[cfg_attr(docsrs, doc(cfg(feature = "database")))]
pub mod database;

#[cfg(feature = "database")]
/// Embedded SQLx migrations for the core database schema.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Error types and error handling utilities.
pub mod error;

/// rkyv wrapper types for external dependencies
#[cfg(feature = "rkyv")]
#[cfg_attr(docsrs, doc(cfg(feature = "rkyv")))]
pub use ferrex_model::rkyv_wrappers;

/// Advanced media query system with filtering and sorting.
pub mod query;

/// Synchronized playback session management.
pub mod sync_session;

/// Common model types used by both server and clients.
pub use ferrex_model as types;

/// Trait contracts for core model types.
pub use ferrex_contracts as traits;

/// Application-level composition utilities such as units of work and facades.
pub mod application;

/// Curated re-exports for player- and client-facing consumers.
pub mod player_prelude;

#[cfg(test)]
mod collection_schema_migration_tests {
    const COLLECTION_SCHEMA_MIGRATION: &str =
        include_str!("../migrations/011_collection_schema.sql");

    fn normalized_migration() -> String {
        COLLECTION_SCHEMA_MIGRATION.to_ascii_lowercase()
    }

    #[test]
    fn collection_schema_migration_declares_required_tables() {
        let migration = normalized_migration();

        for table in [
            "collection_definitions",
            "collection_manual_memberships",
            "collection_dynamic_rules",
            "collection_materializations",
            "collection_materialized_items",
            "collection_shelf_placements",
            "collection_sources",
            "collection_source_memberships",
        ] {
            assert!(
                migration
                    .contains(&format!("create table if not exists {table}")),
                "missing table declaration for {table}"
            );
        }
    }

    #[test]
    fn collection_schema_enforces_membership_constraints() {
        let migration = normalized_migration();

        for required_fragment in [
            "uq_collection_manual_memberships_media",
            "uq_collection_manual_memberships_position",
            "collection_manual_memberships_media_type_check",
            "uq_collection_materializations_key",
            "collection_materializations_state_metadata_check",
            "uq_collection_materialized_items_position",
            "idx_collection_materialized_items_visible",
            "uq_collection_shelf_placements_collection",
            "uq_collection_shelf_placements_position",
            "idx_collection_shelf_placements_ordered",
        ] {
            assert!(
                migration.contains(required_fragment),
                "missing migration constraint/index fragment {required_fragment}"
            );
        }
    }

    #[test]
    fn collection_schema_keeps_memberships_non_cascading() {
        let migration = normalized_migration();
        let manual_membership = migration
            .split("create table if not exists collection_manual_memberships")
            .nth(1)
            .and_then(|tail| tail.split("create unique index").next())
            .expect("manual membership table block present");
        let materialized_items = migration
            .split("create table if not exists collection_materialized_items")
            .nth(1)
            .and_then(|tail| tail.split("create unique index").next())
            .expect("materialized item table block present");

        for table_block in [manual_membership, materialized_items] {
            assert!(!table_block.contains("references media_files"));
            assert!(!table_block.contains("references movie_references"));
            assert!(!table_block.contains("references series"));
            assert!(!table_block.contains("references season_references"));
            assert!(!table_block.contains("references episode_references"));
        }
    }

    #[test]
    fn collection_schema_migration_preserves_legacy_tmdb_memberships() {
        let migration = normalized_migration();

        assert!(
            migration.contains("comment on table movie_collection_membership")
        );
        assert!(!migration.contains("drop table movie_collection_membership"));
        assert!(!migration.contains("delete from movie_collection_membership"));
        assert!(
            !migration.contains("truncate table movie_collection_membership")
        );
    }
}
