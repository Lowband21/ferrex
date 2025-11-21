use super::super::validation::ConfigGuardRailError;

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigLoadError {
    #[error("invalid database URL")]
    InvalidDatabaseUrl {
        #[source]
        source: url::ParseError,
    },
    #[error("invalid database username '{username}'")]
    InvalidDatabaseUsername { username: String },
    #[error("failed to encode database password into URL")]
    InvalidDatabasePassword,
    #[error("failed to read secret file {path}")]
    SecretFileIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to load scanner configuration: {0}")]
    Scanner(#[source] anyhow::Error),
    #[error("failed to load rate limiter configuration: {0}")]
    RateLimiter(#[source] anyhow::Error),
    #[error("filesystem initialization failed")]
    Filesystem { source: anyhow::Error },
    #[error(transparent)]
    GuardRail(#[from] ConfigGuardRailError),
    #[error(transparent)]
    EnvFile(#[from] dotenvy::Error),
}
