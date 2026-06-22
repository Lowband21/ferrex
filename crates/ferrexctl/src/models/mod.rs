//! Runtime configuration model types shared by `ferrexctl` and `ferrex-server`.

/// Rate-limit config models.
pub mod rate_limits;
/// Scanner config models.
pub mod scanner;
/// Value-source metadata for configuration diagnostics.
pub mod sources;

use crate::constants::{DEFAULT_PASSWORD_PEPPER, DEFAULT_TOKEN_KEY};

use rate_limits::{RateLimitSource, RateLimiterConfig};
use scanner::{ScannerConfig, ScannerConfigSource};

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

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
    pub intelligence: IntelligenceRuntimeConfig,
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

    pub fn image_cache_dir(&self) -> &Path {
        &self.cache.images
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
    pub images: PathBuf,
    pub transcode: PathBuf,
    pub thumbnails: PathBuf,
}

impl CacheConfig {
    fn ensure_directories(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(&self.images)?;
        std::fs::create_dir_all(&self.transcode)?;
        std::fs::create_dir_all(&self.thumbnails)?;
        Ok(())
    }

    fn normalize_paths(&mut self) -> anyhow::Result<()> {
        self.root = std::fs::canonicalize(&self.root)?;
        self.images = std::fs::canonicalize(&self.images)?;
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
pub struct IntelligenceRuntimeConfig {
    pub enabled: bool,
    pub provider: IntelligenceProviderConfig,
    pub limits: IntelligenceRuntimeLimits,
    pub retry: IntelligenceRetryConfig,
}

impl Default for IntelligenceRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: IntelligenceProviderConfig::default(),
            limits: IntelligenceRuntimeLimits::default(),
            retry: IntelligenceRetryConfig::default(),
        }
    }
}

#[derive(Clone)]
pub struct IntelligenceProviderConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

impl fmt::Debug for IntelligenceProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntelligenceProviderConfig")
            .field("base_url", &self.base_url)
            .field("api_key_configured", &self.api_key.is_some())
            .field("model", &self.model)
            .finish()
    }
}

impl Default for IntelligenceProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8081/v1".to_string(),
            api_key: None,
            model: Some("gemma-4-12b".to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IntelligenceRuntimeLimits {
    pub model_timeout: Duration,
    pub tool_timeout: Duration,
    pub total_timeout: Duration,
    pub max_steps: u32,
    pub max_tool_calls: u32,
    pub max_output_bytes: usize,
    pub max_tool_result_bytes: usize,
    pub per_user_concurrency: u32,
}

impl Default for IntelligenceRuntimeLimits {
    fn default() -> Self {
        Self {
            model_timeout: Duration::from_secs(60),
            tool_timeout: Duration::from_secs(20),
            total_timeout: Duration::from_secs(180),
            max_steps: 12,
            max_tool_calls: 24,
            max_output_bytes: 64 * 1024,
            max_tool_result_bytes: 256 * 1024,
            per_user_concurrency: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IntelligenceRetryConfig {
    pub max_retries: u32,
}

impl Default for IntelligenceRetryConfig {
    fn default() -> Self {
        Self { max_retries: 1 }
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
