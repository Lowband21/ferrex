use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::util::{parse_bool_var, parse_csv_var, rate_limit_spec_from_env};

use super::{rate_limits::RateLimitSpec, scanner::ScannerConfig};

/// Raw configuration as defined in a TOML file.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FileConfig {
    #[serde(default)]
    pub server: FileServerConfig,
    #[serde(default)]
    pub database: FileDatabaseConfig,
    pub redis: Option<FileRedisConfig>,
    #[serde(default)]
    pub media: FileMediaConfig,
    #[serde(default)]
    pub cache: FileCacheConfig,
    #[serde(default)]
    pub ffmpeg: FileFfmpegConfig,
    #[serde(default)]
    pub cors: FileCorsConfig,
    #[serde(default)]
    pub security: FileSecurityConfig,
    #[serde(default)]
    pub auth: FileAuthConfig,
    pub rate_limiter: Option<FileRateLimiterConfig>,
    pub scanner: Option<ScannerConfig>,
    pub dev_mode: Option<bool>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileServerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileDatabaseConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileRedisConfig {
    pub url: String,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileMediaConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileCacheConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcode: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnails: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileFfmpegConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ffmpeg_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ffprobe_path: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileCorsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_origins: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_methods: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_headers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_credentials: Option<bool>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileSecurityConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforce_https: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_proxy_headers: Option<bool>,
    #[serde(default)]
    pub hsts: FileHstsConfig,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileHstsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_subdomains: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preload: Option<bool>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileAuthConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_pepper: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_token: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileRateLimiterConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_json: Option<String>,
}

/// Environment-derived configuration values.
#[derive(Debug, Default, Clone)]
pub struct EnvConfig {
    pub server_host: Option<String>,
    pub server_port: Option<u16>,
    pub database_url: Option<String>,
    pub database_url_file: Option<PathBuf>,
    pub database_host: Option<String>,
    pub database_port: Option<u16>,
    pub database_user: Option<String>,
    pub database_name: Option<String>,
    pub database_password: Option<String>,
    pub database_password_file: Option<PathBuf>,
    pub ferrex_app_password: Option<String>,
    pub ferrex_app_password_file: Option<PathBuf>,
    pub redis_url: Option<String>,
    pub media_root: Option<PathBuf>,
    pub cache_root: Option<PathBuf>,
    pub cache_images: Option<PathBuf>,
    pub cache_transcode: Option<PathBuf>,
    pub cache_thumbnails: Option<PathBuf>,
    pub ffmpeg_path: Option<String>,
    pub ffprobe_path: Option<String>,
    pub cors_allowed_origins: Option<Vec<String>>,
    pub cors_allowed_methods: Option<Vec<String>>,
    pub cors_allowed_headers: Option<Vec<String>>,
    pub cors_allow_credentials: Option<bool>,
    pub dev_mode: Option<bool>,
    pub enforce_https: Option<bool>,
    pub trust_proxy_headers: Option<bool>,
    pub hsts_max_age: Option<u64>,
    pub hsts_include_subdomains: Option<bool>,
    pub hsts_preload: Option<bool>,
    pub auth_password_pepper: Option<String>,
    pub auth_token_key: Option<String>,
    pub setup_token: Option<String>,
    pub rate_limits: Option<RateLimitSpec>,
    pub scanner_config_path: Option<PathBuf>,
    pub scanner_config_json: Option<String>,
}

impl EnvConfig {
    pub fn gather() -> Self {
        Self {
            server_host: std::env::var("SERVER_HOST").ok(),
            server_port: std::env::var("SERVER_PORT")
                .ok()
                .and_then(|s| s.parse().ok()),
            database_url: std::env::var("DATABASE_URL").ok(),
            database_url_file: std::env::var("DATABASE_URL_FILE")
                .ok()
                .map(PathBuf::from),
            database_host: std::env::var("DATABASE_HOST").ok(),
            database_port: std::env::var("DATABASE_PORT")
                .ok()
                .and_then(|s| s.parse().ok()),
            database_user: std::env::var("DATABASE_USER").ok(),
            database_name: std::env::var("DATABASE_NAME").ok(),
            database_password: std::env::var("DATABASE_PASSWORD").ok(),
            database_password_file: std::env::var("DATABASE_PASSWORD_FILE")
                .ok()
                .map(PathBuf::from),
            ferrex_app_password: std::env::var("FERREX_APP_PASSWORD").ok(),
            ferrex_app_password_file: std::env::var("FERREX_APP_PASSWORD_FILE")
                .ok()
                .map(PathBuf::from),
            redis_url: std::env::var("REDIS_URL").ok(),
            media_root: std::env::var("MEDIA_ROOT").ok().map(PathBuf::from),
            cache_root: std::env::var("CACHE_DIR").ok().map(PathBuf::from),
            cache_images: std::env::var("IMAGE_CACHE_DIR")
                .ok()
                .map(PathBuf::from),
            cache_transcode: std::env::var("TRANSCODE_CACHE_DIR")
                .ok()
                .map(PathBuf::from),
            cache_thumbnails: std::env::var("THUMBNAIL_CACHE_DIR")
                .ok()
                .map(PathBuf::from),
            ffmpeg_path: std::env::var("FFMPEG_PATH").ok(),
            ffprobe_path: std::env::var("FFPROBE_PATH").ok(),

            cors_allowed_origins: parse_csv_var("CORS_ALLOWED_ORIGINS"),
            cors_allowed_methods: parse_csv_var("CORS_ALLOWED_METHODS"),
            cors_allowed_headers: parse_csv_var("CORS_ALLOWED_HEADERS"),
            cors_allow_credentials: parse_bool_var("CORS_ALLOW_CREDENTIALS"),

            dev_mode: parse_bool_var("DEV_MODE"),
            enforce_https: parse_bool_var("ENFORCE_HTTPS"),
            trust_proxy_headers: parse_bool_var("TRUST_PROXY_HEADERS"),
            hsts_max_age: std::env::var("HSTS_MAX_AGE")
                .ok()
                .and_then(|s| s.parse().ok()),
            hsts_include_subdomains: parse_bool_var("HSTS_INCLUDE_SUBDOMAINS"),
            hsts_preload: parse_bool_var("HSTS_PRELOAD"),

            auth_password_pepper: std::env::var("AUTH_PASSWORD_PEPPER").ok(),
            auth_token_key: std::env::var("AUTH_TOKEN_KEY").ok(),
            setup_token: std::env::var("FERREX_SETUP_TOKEN").ok(),

            rate_limits: rate_limit_spec_from_env(),

            scanner_config_path: std::env::var("SCANNER_CONFIG_PATH")
                .ok()
                .map(PathBuf::from),
            scanner_config_json: std::env::var("SCANNER_CONFIG_JSON").ok(),
        }
    }
}
