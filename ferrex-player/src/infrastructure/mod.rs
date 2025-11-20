//! Infrastructure module containing core utilities and shared components
//!
//! This module provides foundational services used across all domains

pub mod adapters;
pub mod api_client;
pub mod api_types;
pub mod config;
pub mod constants;
pub mod performance_config;
pub mod profiling;
pub mod repositories;
pub mod service_registry;
pub mod services;
pub mod util;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

// Re-export commonly used items
pub use api_client::ApiClient;
pub use api_types::*;
pub use config::Config;
pub use profiling::PROFILER;
pub use services::{ServiceBuilder, CompatToggles};
