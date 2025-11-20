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
//! use ferrex_core::{
//!     database::MediaDatabase,
//!     player_prelude::{MediaID, MediaIDLike, MovieID, UpdateProgressRequest, UserWatchState},
//!     user::RegisterRequest,
//! };
//!
//! async fn register_and_track(
//!     _db: &MediaDatabase,
//! ) -> Result<(), Box<dyn std::error::Error>> {
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

#[cfg(feature = "database")]
pub use image::MediaImageKind;

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

/// Demo-mode helpers for quickly seeding fake media libraries.
#[cfg(feature = "demo")]
pub mod demo;

/// Scan domain entrypoint bundling orchestrator, filesystem watch, and helper modules.
#[cfg(feature = "scan-runtime")]
#[cfg_attr(docsrs, doc(cfg(feature = "scan-runtime")))]
pub mod scan;

/// Scan orchestrator domain scaffolding
#[cfg(feature = "scan-runtime")]
#[cfg_attr(docsrs, doc(cfg(feature = "scan-runtime")))]
pub mod orchestration;

/// Filesystem watch adapters feeding the orchestrator actors
#[cfg(feature = "scan-runtime")]
#[cfg_attr(docsrs, doc(cfg(feature = "scan-runtime")))]
pub mod fs_watch;

/// Public scanner interface (legacy helpers and fixtures).
#[cfg(feature = "scan-runtime")]
#[cfg_attr(docsrs, doc(cfg(feature = "scan-runtime")))]
pub mod scanner;

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

/// First-run setup flows (claim codes, binding)
#[cfg(feature = "database")]
pub mod setup;

/// User management domain module with CRUD operations
pub mod user_management;

/// Media watch status and progress tracking
pub mod watch_status;

/// Application-level composition utilities (Unit of Work, facades)
pub mod application;

pub mod player_prelude;

/// Query helper exports grouped for UI/search crates
pub use query::prelude as query_prelude;

// #[cfg(feature = "compat")]
// mod compat {
//     pub use super::api_scan::*;
//     pub use super::api_types::*;
//     #[cfg(feature = "database")]
//     pub use super::database::*;
//     pub use super::error::*;
//     pub use super::extras_parser::ExtrasParser;
//     #[cfg(feature = "database")]
//     pub use super::fs_watch::*;
//     #[cfg(feature = "database")]
//     pub use super::image_service::{ImageService, TmdbImageSize};
//     #[cfg(feature = "ffmpeg")]
//     pub use super::metadata::*;
//     #[cfg(feature = "database")]
//     pub use super::orchestration::events::stable_path_key;
//     #[cfg(feature = "database")]
//     pub use super::orchestration::events::{
//         DomainEvent, DomainEventPublisher, EventBus, EventMeta, JobEvent, JobEventPayload,
//         JobEventPublisher, ManualEnqueueRequest, ManualEnqueueResponse,
//     };
//     #[cfg(feature = "database")]
//     pub use super::orchestration::{
//         actors::*, budget::*, classification::*, config::*, correlation::*, dispatcher::*, job::*,
//         lease::*, persistence::*, queue::*, runtime::*, scan_cursor::*, scheduler::*, series::*,
//     };
//     pub use super::providers::{ProviderError, TmdbApiProvider};
//     pub use super::query::*;
//     pub use super::rbac::*;
//     pub use super::sync_session::*;
//     pub use super::tv_parser::{EpisodeInfo, TvParser};
//     pub use super::types::library::*;
//     pub use super::types::transcoding::{
//         TranscodingJobResponse, TranscodingProgressDetails, TranscodingStatus,
//     };

//     // Authentication exports
//     pub use super::auth::AuthError as DeviceAuthError;
//     #[cfg(feature = "database")]
//     pub use super::auth::infrastructure::*;
//     #[cfg(feature = "database")]
//     pub use super::auth::pin::*;
//     pub use super::auth::rate_limit::*;
//     pub use super::auth::session::{
//         CreateSessionRequest, CreateSessionResponse, DeviceSession as SessionDeviceSession,
//         ListSessionsRequest, RevokeSessionRequest, SessionActivity, SessionConfig, SessionSummary,
//         SessionValidationResult, generate_session_token,
//     };
//     pub use super::auth::state::{
//         AuthEvent as DeviceAuthEvent, AuthState as DeviceAuthState,
//         TransitionResult as DeviceAuthTransitionResult,
//     };
//     pub use super::auth::state_machine::AuthState as AuthStateTrait;
//     pub use super::auth::state_machine::TransitionResult as AuthStateMachineResult;
//     pub use super::auth::state_machine::{
//         AuthConfig, AuthStateMachine, AuthTransitionError, Authenticated, AwaitingPassword,
//         AwaitingPin, Refreshing, SerializedAuthState, SettingUpPin, Unauthenticated, UserSelected,
//     };
//     pub use super::auth::{
//         AuthContext, AuthEvent, AuthEventType, AuthResult, AuthenticationMethod,
//     };

//     // Core exports
//     pub use super::traits::*;
//     pub use super::types::*;
//     pub use super::user::AuthError as UserAuthError;
//     pub use super::user::{
//         AuthToken, Claims, GridSize, LoginRequest, PlaybackPreferences, PlaybackQuality,
//         RegisterRequest, ResumeBehavior, SubtitlePreferences, ThemePreference, UiPreferences, User,
//         UserPreferences, UserSession, UserUpdateRequest, ValidationError,
//     };
//     pub use super::watch_status::*;
//     // user_management is available as a module but not re-exported to avoid conflicts
// }

// #[cfg(feature = "compat")]
// pub use compat::*;
