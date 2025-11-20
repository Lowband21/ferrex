//! Infrastructure module containing core utilities and shared components
//!
//! This module provides foundational services used across all domains

pub mod adapters;
pub mod api_client;
pub mod api_types;
pub mod config;
pub mod constants;

// New profiling modules (feature-gated)
#[cfg(any(
    feature = "profile-with-puffin",
    feature = "profile-with-tracy",
    feature = "profile-with-tracing",
    feature = "profiling-stats"
))]
pub mod profiling;

pub mod profiling_scopes;
pub mod repository;
pub mod service_registry;
pub mod services;

#[cfg(any(test, feature = "iced_tester"))]
pub mod testing;

// Re-export commonly used items
pub use api_client::ApiClient;
pub use api_types::*;
pub use config::Config;

// Export the main profiler
#[cfg(any(
    feature = "profile-with-puffin",
    feature = "profile-with-tracy",
    feature = "profile-with-tracing",
    feature = "profiling-stats"
))]
pub use profiling::PROFILER;

// Export profiling scope definitions
pub use profiling_scopes::{analyze_performance, scopes, PerformanceTargets};

// For backward compatibility when no profiling features are enabled
#[cfg(not(any(
    feature = "profile-with-puffin",
    feature = "profile-with-tracy",
    feature = "profile-with-tracing",
    feature = "profiling-stats"
)))]
pub mod profiling {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    pub struct Profiler {
        enabled: AtomicBool,
    }

    impl Profiler {
        pub fn new() -> Arc<Self> {
            Arc::new(Self {
                enabled: AtomicBool::new(false),
            })
        }

        pub fn is_enabled(&self) -> bool {
            false
        }

        pub fn begin_frame(&self) {}

        pub fn set_enabled(&self, _enabled: bool) {}
    }

    lazy_static::lazy_static! {
        pub static ref PROFILER: Arc<Profiler> = Profiler::new();
    }

    pub fn init() {}
}

#[cfg(not(any(
    feature = "profile-with-puffin",
    feature = "profile-with-tracy",
    feature = "profile-with-tracing",
    feature = "profiling-stats"
)))]
pub use self::profiling::PROFILER;

pub use services::{CompatToggles, ServiceBuilder};
