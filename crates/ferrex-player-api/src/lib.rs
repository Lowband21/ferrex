//! HTTP API boundary for Ferrex player clients.
//!
//! This crate owns the concrete HTTP client, trait-based API service contracts,
//! API-only adapters, authentication DTOs, API-facing type re-exports, and test
//! stubs shared by the desktop player and future clients.

/// Concrete service adapters built on the HTTP client.
pub mod adapters;
/// Low-level HTTP client for the Ferrex server API.
pub mod api_client;
/// Curated API DTO re-export surface for player crates.
pub mod api_types;
/// Authentication DTOs shared at the player API boundary.
pub mod auth;
/// Trait-based API service contracts.
pub mod services;
/// API-focused test stubs for player clients.
pub mod testing;

/// Default API adapter implementation used by the player shell.
pub use adapters::ApiClientAdapter;
/// HTTP client and refresh-token callback type.
pub use api_client::{ApiClient, RefreshTokenCallback};
/// Player-facing API DTOs re-exported from core/model crates.
pub use api_types::*;
/// Server setup status DTO.
pub use auth::SetupStatus;
