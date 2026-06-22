//! Test service stubs for API and settings boundaries.

/// Test implementation of the general API service.
pub mod api;
/// Test implementation of the settings service.
pub mod settings;

pub use api::TestApiService;
pub use settings::TestSettingsService;
