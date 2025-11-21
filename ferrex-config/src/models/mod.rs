pub mod rate_limits;
pub mod scanner;
pub mod sources;

use crate::constants::{DEFAULT_PASSWORD_PEPPER, DEFAULT_TOKEN_KEY};

use rate_limits::{RateLimitSource, RateLimiterConfig};
use scanner::{ScannerConfig, ScannerConfigSource};

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub redis: Option<RedisConfig>,
    pub media: MediaConfig,
    pub cache: CacheConfig,
    pub ffmpeg: FfmpegConfig,
    pub cors: CorsConfig,
    pub security: SecurityConfig,
    pub dev_mode: bool,
    pub auth: AuthConfig,
    pub scanner: ScannerConfig,
    pub rate_limiter: Option<RateLimiterSettings>,
    pub metadata: ConfigMetadata,
}

impl Config {
    pub fn ensure_directories(&self) -> anyhow::Result<()> {
        self.cache.ensure_directories()
    }

    pub fn normalize_paths(&mut self) -> anyhow::Result<()> {
        self.cache.normalize_paths()
    }

    pub fn cache_root(&self) -> &Path {
        &self.cache.root
    }

    pub fn transcode_cache_dir(&self) -> &Path {
        &self.cache.transcode
    }

    pub fn thumbnail_cache_dir(&self) -> &Path {
        &self.cache.thumbnails
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub primary_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RedisConfig {
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct MediaConfig {
    pub root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub root: PathBuf,
    pub transcode: PathBuf,
    pub thumbnails: PathBuf,
}

impl CacheConfig {
    fn ensure_directories(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(&self.transcode)?;
        std::fs::create_dir_all(&self.thumbnails)?;
        Ok(())
    }

    fn normalize_paths(&mut self) -> anyhow::Result<()> {
        self.root = std::fs::canonicalize(&self.root)?;
        self.transcode = std::fs::canonicalize(&self.transcode)?;
        self.thumbnails = std::fs::canonicalize(&self.thumbnails)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FfmpegConfig {
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
}

#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub allow_credentials: bool,
}

impl CorsConfig {
    pub fn is_wildcard_included(&self) -> bool {
        self.allowed_origins
            .iter()
            .any(|origin| origin.trim() == "*")
    }
}

#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub enforce_https: bool,
    pub trust_proxy_headers: bool,
    pub hsts: HstsSettings,
}

#[derive(Debug, Clone)]
pub struct HstsSettings {
    pub max_age: u64,
    pub include_subdomains: bool,
    pub preload: bool,
}

#[derive(Debug, Clone, Default)]
pub struct HstsLayerConfig {
    pub max_age: u64,
    pub include_subdomains: bool,
    pub preload: bool,
}

impl From<&HstsSettings> for HstsLayerConfig {
    fn from(value: &HstsSettings) -> Self {
        HstsLayerConfig {
            max_age: value.max_age,
            include_subdomains: value.include_subdomains,
            preload: value.preload,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub password_pepper: String,
    pub token_key: String,
    pub setup_token: Option<String>,
}

impl AuthConfig {
    pub fn is_default_pepper(&self) -> bool {
        self.password_pepper == DEFAULT_PASSWORD_PEPPER
    }

    pub fn is_default_token_key(&self) -> bool {
        self.token_key == DEFAULT_TOKEN_KEY
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiterSettings {
    pub config: RateLimiterConfig,
    pub source: RateLimitSource,
}

#[derive(Debug, Clone)]
pub struct ConfigMetadata {
    pub config_path: Option<PathBuf>,
    pub env_file_loaded: bool,
    pub scanner_source: ScannerConfigSource,
    pub rate_limit_source: Option<RateLimitSource>,
}

impl Default for ConfigMetadata {
    fn default() -> Self {
        Self {
            config_path: None,
            env_file_loaded: false,
            scanner_source: ScannerConfigSource::Default,
            rate_limit_source: None,
        }
    }
}
