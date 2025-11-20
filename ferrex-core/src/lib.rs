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
//! - **User System**: JWT-based authentication with session management
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
//! - [`api_types`]: Common types used across API boundaries
//! - [`user`]: User authentication and session management
//! - [`watch_status`]: Media playback progress tracking
//! - [`sync_session`]: Synchronized playback session management
//! - [`query`]: Advanced media querying capabilities
//! - [`database`]: Database traits and implementations
//!
//! ## Examples
//!
//! ```no_run
//! use ferrex_core::{MediaDatabase, user::RegisterRequest};
//!
//! async fn register_user(db: &MediaDatabase) -> Result<(), Box<dyn std::error::Error>> {
//!     let request = RegisterRequest {
//!         username: "alice".to_string(),
//!         password: "secure_password".to_string(),
//!         display_name: Some("Alice".to_string()),
//!     };
//!
//!     let user = db.backend().create_user(request).await?;
//!     println!("Created user: {}", user.username);
//!     Ok(())
//! }
//! ```

// TODO: Document properly
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(missing_docs)]

/// Common API routes used across Ferrex services
pub mod api_routes;

/// Domain-specific scan API payloads shared between server and player
pub mod api_scan;
/// Common API types used across the Ferrex ecosystem
pub mod api_types;

/// Database abstraction layer and implementations
#[cfg(feature = "database")]
#[cfg_attr(docsrs, doc(cfg(feature = "database")))]
pub mod database;

/// Database abstraction layer and implementations
#[cfg(feature = "database")]
#[cfg_attr(docsrs, doc(cfg(feature = "database")))]
pub mod persistence;

#[cfg(feature = "database")]
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Error types and error handling utilities
pub mod error;

/// Parser for identifying and organizing media extras (trailers, featurettes, etc.)
pub mod extras_parser;

/// Image caching and serving service
#[cfg(feature = "database")]
#[cfg_attr(docsrs, doc(cfg(feature = "database")))]
pub mod image_service;

/// Shared image domain records
#[cfg(feature = "database")]
pub mod image;

/// rkyv wrapper types for external dependencies
pub mod rkyv_wrappers;

/// FFmpeg-based metadata extraction
#[cfg(feature = "ffmpeg")]
#[cfg_attr(docsrs, doc(cfg(feature = "ffmpeg")))]
pub mod metadata;

/// External metadata providers (TMDB integration)
pub mod providers;

/// Advanced media query system with filtering and sorting
pub mod query;

/// Sorted indices for efficient media sorting
#[cfg(feature = "database")]
pub mod indices;

/// Role-Based Access Control (RBAC) system
pub mod rbac;

/// Public scanner interface (wraps streaming_scanner_v2)
#[cfg(feature = "database")]
#[cfg_attr(docsrs, doc(cfg(feature = "database")))]
pub mod scanner;

/// Scan orchestrator domain scaffolding
#[cfg(feature = "database")]
#[cfg_attr(docsrs, doc(cfg(feature = "database")))]
pub mod orchestration;

/// Filesystem watch adapters feeding the orchestrator actors
#[cfg(feature = "database")]
#[cfg_attr(docsrs, doc(cfg(feature = "database")))]
pub mod fs_watch;

/// Synchronized playback session management
pub mod sync_session;

/// TV show filename parser for extracting episode information
pub mod tv_parser;

/// Common types used by both server and client
pub mod types;

/// Traits for core types
pub mod traits;

/// User authentication and session management
pub mod user;

/// Enhanced authentication with device trust and PIN support
pub mod auth;

/// User management domain module with CRUD operations
pub mod user_management;

/// Media watch status and progress tracking
pub mod watch_status;

pub use api_scan::*;
pub use api_types::*;
pub use auth::*;
#[cfg(feature = "database")]
pub use database::*;
pub use error::*;
pub use extras_parser::ExtrasParser;
#[cfg(feature = "database")]
pub use fs_watch::*;
#[cfg(feature = "database")]
pub use image_service::{ImageService, TmdbImageSize};
#[cfg(feature = "ffmpeg")]
pub use metadata::*;
#[cfg(feature = "database")]
pub use orchestration::*;
pub use providers::{ProviderError, TmdbApiProvider};
pub use query::*;
pub use rbac::*;
pub use sync_session::*;
pub use tv_parser::{EpisodeInfo, TvParser};
pub use types::library::*;
pub use types::transcoding::{
    TranscodingJobResponse, TranscodingProgressDetails, TranscodingStatus,
};

// Core exports
pub use traits::*;
pub use types::*;
pub use user::*;
pub use watch_status::*;
// user_management is available as a module but not re-exported to avoid conflicts
