//! Adapter implementations that wrap existing services
//!
//! These adapters implement the new trait-based interfaces using
//! the existing concrete implementations (ApiClient, AuthManager, etc.)
//! This is part of the gradual migration to Ports & Adapters pattern.

pub mod media_store_adapter;
pub mod api_client_adapter;
pub mod auth_manager_adapter;

pub use media_store_adapter::MediaStoreAdapter;
pub use api_client_adapter::ApiClientAdapter;
pub use auth_manager_adapter::AuthManagerAdapter;