//! # Ferrex Core
//!
//! Core library for the Ferrex Media Server, providing fundamental types, database abstractions,
//! and business logic for media management, user authentication, and playback synchronization.
//!
//! ## Overview
//!
//! `ferrex-core` is the foundation of the Ferrex Media Server ecosystem, offering:
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
//!     let db_ctx = DatabaseContext::connect_postgres(database_url).await?;
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

// TODO: Document properly
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(missing_docs)]

/// Versioned routes and API data transfer objects
pub mod api;

/// Domain module grouping core business logic.
pub mod domain;

/// Infrastructure adapters (database, external services, runtimes).
pub mod infra;

/// Database abstraction layer and implementations
#[cfg(feature = "database")]
#[cfg_attr(docsrs, doc(cfg(feature = "database")))]
pub mod database;

#[cfg(feature = "database")]
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Error types and error handling utilities
pub mod error;

/// rkyv wrapper types for external dependencies
#[cfg(feature = "rkyv")]
#[cfg_attr(docsrs, doc(cfg(feature = "rkyv")))]
pub use ferrex_model::rkyv_wrappers;

/// Advanced media query system with filtering and sorting
pub mod query;

/// Synchronized playback session management
pub mod sync_session;

/// Common types used by both server and client
pub use ferrex_model as types;

/// Traits for core types
pub use ferrex_contracts as traits;

/// Application-level composition utilities (Unit of Work, facades)
pub mod application;

pub mod player_prelude;
