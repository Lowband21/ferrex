use serde::Deserialize;
use std::env;
use std::path::PathBuf;

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
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        // Load .env file if present
        dotenv::dotenv().ok();

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
        })
    }

    pub fn ensure_directories(&self) -> anyhow::Result<()> {
        // Create cache directories if they don't exist
        std::fs::create_dir_all(&self.cache_dir)?;
        std::fs::create_dir_all(&self.transcode_cache_dir)?;
        std::fs::create_dir_all(&self.thumbnail_cache_dir)?;
        Ok(())
    }
}
