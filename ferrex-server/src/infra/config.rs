use anyhow::{Context, anyhow};
use ferrex_core::{orchestration::config::OrchestratorConfig, scanner::settings};
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Server configuration loaded via environment variables (and optionally a
/// scanner config file).
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    // Server settings
    pub server_host: String,
    pub server_port: u16,

    // Database settings
    pub database_url: Option<String>,

    // Redis settings
    pub redis_url: Option<String>,

    // Media settings
    pub media_root: Option<PathBuf>,
    pub transcode_cache_dir: PathBuf,
    pub thumbnail_cache_dir: PathBuf,
    pub cache_dir: PathBuf,

    // FFmpeg settings
    pub ffmpeg_path: String,
    pub ffprobe_path: String,

    // CORS settings
    pub cors_allowed_origins: Vec<String>,

    // Development settings
    pub dev_mode: bool,

    // Authentication secrets (pepper for Argon2 + HMAC key for tokens)
    pub auth_password_pepper: String,
    pub auth_token_key: String,

    /// Scanner/orchestrator settings used to tune queue depth, concurrency,
    /// and maintenance behaviour.
    #[serde(default)]
    pub scanner: ScannerConfig,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        // Load .env file if present
        dotenv::dotenv().ok();

        let scanner = ScannerConfig::load_from_env()?;

        Ok(Self {
            server_host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            server_port: env::var("SERVER_PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .unwrap_or(3000),

            database_url: env::var("DATABASE_URL").ok(),
            redis_url: env::var("REDIS_URL").ok(),

            media_root: env::var("MEDIA_ROOT").ok().map(PathBuf::from),
            transcode_cache_dir: env::var("TRANSCODE_CACHE_DIR")
                .unwrap_or_else(|_| "./cache/transcode".to_string())
                .into(),
            thumbnail_cache_dir: env::var("THUMBNAIL_CACHE_DIR")
                .unwrap_or_else(|_| "./cache/thumbnails".to_string())
                .into(),
            cache_dir: env::var("CACHE_DIR")
                .unwrap_or_else(|_| "./cache".to_string())
                .into(),

            ffmpeg_path: env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string()),
            ffprobe_path: env::var("FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string()),

            cors_allowed_origins: env::var("CORS_ALLOWED_ORIGINS")
                .unwrap_or_else(|_| "http://localhost:3000,http://localhost:5173".to_string())
                .split(',')
                .map(|s| s.trim().to_string())
                .collect(),

            dev_mode: env::var("DEV_MODE")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),

            auth_password_pepper: env::var("AUTH_PASSWORD_PEPPER")
                .unwrap_or_else(|_| "change-me-password-pepper".to_string()),
            auth_token_key: env::var("AUTH_TOKEN_KEY")
                .unwrap_or_else(|_| "change-me-hmac-key".to_string()),

            scanner,
        })
    }

    pub fn ensure_directories(&self) -> anyhow::Result<()> {
        // Create cache directories if they don't exist
        std::fs::create_dir_all(&self.cache_dir)?;
        std::fs::create_dir_all(&self.transcode_cache_dir)?;
        std::fs::create_dir_all(&self.thumbnail_cache_dir)?;
        Ok(())
    }

    /// Canonicalize cache-related directories so downstream services work with
    /// absolute paths. The server calls this once during startup immediately
    /// after `ensure_directories`, so handlers can safely assume the cache root
    /// never needs further normalization.
    pub fn normalize_paths(&mut self) -> anyhow::Result<()> {
        self.cache_dir = std::fs::canonicalize(&self.cache_dir)?;
        self.transcode_cache_dir = std::fs::canonicalize(&self.transcode_cache_dir)?;
        self.thumbnail_cache_dir = std::fs::canonicalize(&self.thumbnail_cache_dir)?;
        Ok(())
    }
}

fn default_video_extensions() -> Vec<String> {
    settings::default_video_file_extensions_vec()
}

/// Top-level scanner settings. Use these to tune how quickly new folders are
/// queued, how many workers run in parallel, and when a bulk scan is considered
/// finished.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ScannerConfig {
    /// Full orchestrator tuning: queue parallelism, priority weights, retry and
    /// lease policy, metadata throttles, maintenance scheduling, and filesystem
    /// watch debouncing. Raise the `queue` or `budget` limits to process more
    /// folders/files in parallel, but keep an eye on disk and network pressure.
    pub orchestrator: OrchestratorConfig,
    /// Per-library limit for queued maintenance jobs after the initial bulk
    /// sweep. Increase to let a library keep more follow-up scans pending; too
    /// high can starve other libraries on busy disks.
    pub library_actor_max_outstanding_jobs: usize,
    /// Idle window (ms) the aggregator waits after the queue drains before it
    /// declares the bulk scan complete. Shorter windows flip to maintenance
    /// faster; longer windows help when the filesystem reports changes slowly.
    pub quiescence_window_ms: u64,
    /// File extensions treated as video assets by the filesystem watcher.
    /// Defaults mirror the core's built-in allow-list so future user overrides
    /// can flow through without diverging behaviour.
    #[serde(default = "default_video_extensions")]
    pub video_extensions: Vec<String>,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        let mut orchestrator = OrchestratorConfig::default();
        if orchestrator.budget.image_fetch_limit < orchestrator.queue.max_parallel_image_fetch {
            orchestrator.budget.image_fetch_limit = orchestrator.queue.max_parallel_image_fetch;
        }

        Self {
            orchestrator,
            // Should make this num_cpus?
            library_actor_max_outstanding_jobs: 32,
            quiescence_window_ms: 5_000,
            video_extensions: default_video_extensions(),
        }
    }
}

impl ScannerConfig {
    /// Load scanner configuration overrides. Evaluation order:
    /// 1) `$SCANNER_CONFIG_PATH` (TOML or JSON file),
    /// 2) `$SCANNER_CONFIG_JSON` (inline JSON),
    /// 3) defaults if neither is set.
    fn load_from_env() -> anyhow::Result<Self> {
        if let Ok(path) = env::var("SCANNER_CONFIG_PATH") {
            return Self::load_from_file(Path::new(&path));
        }

        if let Ok(raw) = env::var("SCANNER_CONFIG_JSON") {
            return Self::parse_json(&raw).context("failed to parse SCANNER_CONFIG_JSON");
        }

        if let Some(path) = Self::find_default_file() {
            return Self::load_from_file(&path);
        }

        Ok(Self::default())
    }

    fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read scanner config from {}", path.display()))?;

        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Self::parse_json(&contents)
                .with_context(|| format!("invalid scanner config {}", path.display())),
            Some("toml") | Some("tml") => toml::from_str(&contents)
                .map_err(|err| anyhow!("invalid scanner config {}: {}", path.display(), err)),
            _ => Self::parse_from_str(&contents, &path.display().to_string()),
        }
    }

    fn parse_from_str(contents: &str, origin: &str) -> anyhow::Result<Self> {
        // Try TOML first, then JSON for convenience.
        toml::from_str(contents).or_else(|toml_err| {
            serde_json::from_str(contents).map_err(|json_err| {
                anyhow!(
                    "failed to parse scanner config {}: toml error: {}; json error: {}",
                    origin,
                    toml_err,
                    json_err
                )
            })
        })
    }

    fn parse_json(raw: &str) -> anyhow::Result<Self> {
        serde_json::from_str(raw).map_err(|err| anyhow!("invalid scanner config json: {err}"))
    }

    fn find_default_file() -> Option<PathBuf> {
        const CANDIDATES: &[&str] = &[
            "scanner.toml",
            "scanner.json",
            "config/scanner.toml",
            "config/scanner.json",
        ];

        CANDIDATES
            .iter()
            .map(Path::new)
            .find(|path| path.exists())
            .map(|path| path.to_path_buf())
    }
}
