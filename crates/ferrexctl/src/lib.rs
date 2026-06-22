//! Shared configuration and orchestration library for Ferrex tooling.
//!
//! This crate centralizes `.env` generation/rotation, config loading/validation,
//! runner selection (host vs docker), packaging metadata, and lightweight stack
//! orchestration. Both the `ferrexctl` binary and `ferrex-server` re-export these
//! utilities so there is a single source of truth for config defaults, managed
//! keys, and validation rules.

/// CLI-facing init/check helpers and option structs.
pub mod cli;
/// Shared configuration defaults and managed key names.
pub mod constants;
/// `.env` merge/write utilities that preserve unmanaged user values.
pub mod env_writer;
/// Configuration loading, source tracking, and validation entry points.
pub mod loader;
/// Runtime configuration model types.
pub mod models;
/// Packaging/release metadata loaded from `packaging.toml`.
pub mod packaging_config;
/// Host/Docker runner selection and command execution helpers.
pub mod runner;
/// Miscellaneous utility helpers used by config loading.
pub mod util;
/// Configuration guard rails and warning generation.
pub mod validation;

/// Config loader entry points and load errors.
pub use loader::{ConfigLoad, ConfigLoader, error::ConfigLoadError};
/// Rate-limit configuration model re-exports.
pub use models::rate_limits::{
    RateLimitSource, RateLimitSpec, RateLimiterConfig,
};
/// Scanner configuration model re-exports.
pub use models::scanner::{ScannerConfig, ScannerConfigSource};
/// Top-level server configuration model re-exports.
pub use models::{
    AuthConfig, CacheConfig, Config, ConfigMetadata, CorsConfig,
    DatabaseConfig, FfmpegConfig, HstsLayerConfig, HstsSettings,
    IntelligenceProviderConfig, IntelligenceRetryConfig,
    IntelligenceRuntimeConfig, IntelligenceRuntimeLimits, MediaConfig,
    RateLimiterSettings, RedisConfig, SecurityConfig, ServerConfig,
};
/// Packaging configuration model re-exports.
pub use packaging_config::{
    FlatpakConfig, PackagingConfig, PackagingConfigError, PreflightConfig,
    ReleaseConfig, VersionConfig, VersionSource,
};
/// Config validation errors and warnings.
pub use validation::{ConfigGuardRailError, ConfigWarning, ConfigWarnings};
