//! Shared configuration library for Ferrex.
//!
//! This crate centralizes `.env` generation/rotation, config loading/validation,
//! runner selection (host vs docker), and lightweight stack orchestration. Both
//! the `ferrex-init` binary and `ferrex-server` re-export these utilities so
//! there is a single source of truth for config defaults, managed keys, and
//! validation rules.

pub mod cli;
pub mod constants;
pub mod env_writer;
pub mod loader;
pub mod models;
pub mod runner;
pub mod util;
pub mod validation;

pub use loader::{ConfigLoad, ConfigLoader, error::ConfigLoadError};
pub use models::rate_limits::{
    RateLimitSource, RateLimitSpec, RateLimiterConfig,
};
pub use models::scanner::{ScannerConfig, ScannerConfigSource};
pub use models::{
    AuthConfig, CacheConfig, Config, ConfigMetadata, CorsConfig,
    DatabaseConfig, FfmpegConfig, HstsLayerConfig, HstsSettings, MediaConfig,
    RateLimiterSettings, RedisConfig, SecurityConfig, ServerConfig,
};
pub use validation::{ConfigGuardRailError, ConfigWarning, ConfigWarnings};
