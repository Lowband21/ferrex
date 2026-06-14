//! HTTP API boundary for Ferrex player clients.
//!
//! This crate owns the concrete HTTP client, trait-based API service
//! contracts, API-only adapters, and client-facing DTO re-exports shared by
//! the desktop player and future clients.

pub mod adapters;
pub mod api_client;
pub mod api_types;
pub mod auth;
pub mod services;
pub mod testing;

pub use adapters::ApiClientAdapter;
pub use api_client::{ApiClient, RefreshTokenCallback};
pub use api_types::*;
pub use auth::SetupStatus;
