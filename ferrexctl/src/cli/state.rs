use super::is_placeholder_media_root;

use crate::util::parse_bool;

use anyhow::Result;

use std::{collections::HashMap, path::PathBuf};

/// Mutable draft of user-editable init fields used by the menu UI.
#[derive(Debug, Clone)]
pub struct PromptState {
    pub dev_mode: bool,
    pub server_host: String,
    pub server_port: u16,
    pub media_root: Option<PathBuf>,
    pub ferrex_server_url: String,
    pub tmdb_api_key: String,
    pub cors_allowed_origins: String,
    pub cors_allow_credentials: bool,
    pub enforce_https: bool,
    pub trust_proxy_headers: bool,
    pub hsts_max_age: u64,
    pub hsts_include_subdomains: bool,
    pub hsts_preload: bool,
    pub tls_min_version: String,
    pub tls_cipher_suites: String,
    pub rate_limits_path: String,
    pub rate_limits_json: String,
    pub scanner_config_path: String,
    pub scanner_config_json: String,
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    pub demo_mode: bool,
    pub demo_options: String,
    pub demo_username: String,
    pub demo_password: String,
    pub demo_allow_deviations: String,
    pub demo_deviation_rate: String,
    pub demo_movie_count: String,
    pub demo_series_count: String,
    pub demo_skip_metadata: String,
    pub demo_zero_length: String,
}

impl PromptState {
    pub(crate) fn from_env(
        env: &HashMap<String, String>,
        tailscale: bool,
    ) -> Result<Self> {
        let dev_mode = env
            .get("DEV_MODE")
            .and_then(|v| parse_bool(v))
            .unwrap_or(true);
        let server_host = env
            .get("SERVER_HOST")
            .cloned()
            .unwrap_or_else(|| "0.0.0.0".to_string());
        let server_port = env
            .get("SERVER_PORT")
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(if dev_mode { 3000 } else { 443 });
        let media_root = env
            .get("MEDIA_ROOT")
            .map(String::as_str)
            .filter(|v| !is_placeholder_media_root(v))
            .map(PathBuf::from);
        let ferrex_server_url = env
            .get("FERREX_SERVER_URL")
            .cloned()
            .unwrap_or_else(|| format!("http://localhost:{server_port}"));
        let tmdb_api_key = env.get("TMDB_API_KEY").cloned().unwrap_or_default();
        let cors_allowed_origins = env
            .get("CORS_ALLOWED_ORIGINS")
            .cloned()
            .unwrap_or_else(|| {
                "http://localhost:5173,https://localhost:5173,http://localhost:3000,https://localhost:3000".to_string()
            });
        let cors_allow_credentials = env
            .get("CORS_ALLOW_CREDENTIALS")
            .and_then(|v| parse_bool(v))
            .unwrap_or(false);
        let enforce_https = env
            .get("ENFORCE_HTTPS")
            .and_then(|v| parse_bool(v))
            .unwrap_or(false);
        let trust_proxy_headers = env
            .get("TRUST_PROXY_HEADERS")
            .and_then(|v| parse_bool(v))
            .unwrap_or(enforce_https);
        let hsts_max_age = env
            .get("HSTS_MAX_AGE")
            .and_then(|v| v.parse().ok())
            .unwrap_or(if enforce_https { 31_536_000 } else { 0 });
        let hsts_include_subdomains = env
            .get("HSTS_INCLUDE_SUBDOMAINS")
            .and_then(|v| parse_bool(v))
            .unwrap_or(false);
        let hsts_preload = env
            .get("HSTS_PRELOAD")
            .and_then(|v| parse_bool(v))
            .unwrap_or(false);
        let tls_min_version = env
            .get("TLS_MIN_VERSION")
            .cloned()
            .unwrap_or_else(|| "1.3".to_string());
        let tls_cipher_suites =
            env.get("TLS_CIPHER_SUITES").cloned().unwrap_or_default();
        let rate_limits_path =
            env.get("RATE_LIMITS_PATH").cloned().unwrap_or_default();
        let rate_limits_json =
            env.get("RATE_LIMITS_JSON").cloned().unwrap_or_default();
        let scanner_config_path =
            env.get("SCANNER_CONFIG_PATH").cloned().unwrap_or_default();
        let scanner_config_json =
            env.get("SCANNER_CONFIG_JSON").cloned().unwrap_or_default();
        let ffmpeg_path = env
            .get("FFMPEG_PATH")
            .cloned()
            .unwrap_or_else(|| "ffmpeg".to_string());
        let ffprobe_path = env
            .get("FFPROBE_PATH")
            .cloned()
            .unwrap_or_else(|| "ffprobe".to_string());
        let demo_mode = env
            .get("FERREX_DEMO_MODE")
            .and_then(|v| parse_bool(v))
            .unwrap_or(false);
        let demo_options =
            env.get("FERREX_DEMO_OPTIONS").cloned().unwrap_or_default();
        let demo_username =
            env.get("FERREX_DEMO_USERNAME").cloned().unwrap_or_default();
        let demo_password =
            env.get("FERREX_DEMO_PASSWORD").cloned().unwrap_or_default();
        let demo_allow_deviations = env
            .get("FERREX_DEMO_ALLOW_DEVIATIONS")
            .cloned()
            .unwrap_or_default();
        let demo_deviation_rate = env
            .get("FERREX_DEMO_DEVIATION_RATE")
            .cloned()
            .unwrap_or_default();
        let demo_movie_count = env
            .get("FERREX_DEMO_MOVIE_COUNT")
            .cloned()
            .unwrap_or_default();
        let demo_series_count = env
            .get("FERREX_DEMO_SERIES_COUNT")
            .cloned()
            .unwrap_or_default();
        let demo_skip_metadata = env
            .get("FERREX_DEMO_SKIP_METADATA")
            .cloned()
            .unwrap_or_default();
        let demo_zero_length = env
            .get("FERREX_DEMO_ZERO_LENGTH")
            .cloned()
            .unwrap_or_default();

        let state = Self {
            dev_mode,
            server_host,
            server_port,
            media_root,
            ferrex_server_url,
            tmdb_api_key,
            cors_allowed_origins,
            cors_allow_credentials,
            enforce_https,
            trust_proxy_headers,
            hsts_max_age,
            hsts_include_subdomains,
            hsts_preload,
            tls_min_version,
            tls_cipher_suites,
            rate_limits_path,
            rate_limits_json,
            scanner_config_path,
            scanner_config_json,
            ffmpeg_path,
            ffprobe_path,
            demo_mode,
            demo_options,
            demo_username,
            demo_password,
            demo_allow_deviations,
            demo_deviation_rate,
            demo_movie_count,
            demo_series_count,
            demo_skip_metadata,
            demo_zero_length,
        };
        if tailscale {
            // will still be used later
        }
        Ok(state)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuItem {
    Finish,
    DevMode,
    ServerHost,
    ServerPort,
    ServerUrl,
    MediaRoot,
    TmdbApiKey,
    CorsOrigins,
    CorsAllowCredentials,
    EnforceHttps,
    TrustProxy,
    HstsMaxAge,
    HstsIncludeSub,
    HstsPreload,
    TlsMinVersion,
    TlsCipherSuites,
    RateLimitsPath,
    RateLimitsJson,
    ScannerPath,
    ScannerJson,
    FfmpegPath,
    FfprobePath,
    DemoMode,
    DemoOptions,
    DemoUsername,
    DemoPassword,
    DemoAllowDeviations,
    DemoDeviationRate,
    DemoMovieCount,
    DemoSeriesCount,
    DemoSkipMetadata,
    DemoZeroLength,
}
