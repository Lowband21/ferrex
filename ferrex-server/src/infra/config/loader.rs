use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;
use tracing::error;
use url::Url;

use super::{
    models::{
        AuthConfig, CacheConfig, Config, ConfigMetadata, CorsConfig,
        DatabaseConfig, FfmpegConfig, HstsSettings, MediaConfig,
        RateLimiterSettings, RedisConfig, SecurityConfig, ServerConfig,
    },
    rate_limits::RateLimitSpec,
    scanner::{ScannerConfig, ScannerConfigSource},
    sources::{EnvConfig, FileConfig, FileDatabaseConfig},
    validation::{self, ConfigGuardRailError, ConfigWarnings},
};
use crate::infra::constants::{DEFAULT_PASSWORD_PEPPER, DEFAULT_TOKEN_KEY};

#[derive(Debug, Default, Clone)]
pub struct ConfigLoaderOptions {
    pub config_path: Option<PathBuf>,
    pub env_file: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct ConfigLoader {
    options: ConfigLoaderOptions,
}

impl ConfigLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: ConfigLoaderOptions) -> Self {
        Self { options }
    }

    pub fn with_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.options.env_file = Some(path.into());
        self
    }

    pub fn load(&self) -> Result<ConfigLoad, ConfigLoadError> {
        // Load .env with the following precedence:
        // 1) Explicit file path provided via options.env_file
        // 2) $FERREX_CONFIG_DIR/.env when FERREX_CONFIG_DIR is set
        // 3) Default dotenv discovery from current working directory
        let env_file_loaded = match (
            &self.options.env_file,
            std::env::var("FERREX_CONFIG_DIR"),
        ) {
            // Explicit .env path
            (Some(path), _) => dotenvy::from_path(path).map(|_| true).or_else(
                |err| match err {
                    dotenvy::Error::Io(_) => Ok(false),
                    _ => Err(err),
                },
            )?,
            // Config directory provided via environment; resolve to <dir>/.env
            (None, Ok(dir)) => {
                let candidate = {
                    let p = Path::new(&dir);
                    // If the provided path is a directory (or does not exist yet but looks like a dir),
                    // look for a .env inside it. Otherwise, assume the path is already a file.
                    if p.is_dir() {
                        p.join(".env")
                    } else {
                        p.to_path_buf()
                    }
                };
                dotenvy::from_path(&candidate)
                    .map(|_| true)
                    .or_else(|err| match err {
                        dotenvy::Error::Io(e) => {
                            error!("{}", e);
                            Ok(false)
                        }
                        _ => Err(err),
                    })?
            }
            // Fallback: dotenv discovery from CWD
            (None, Err(_)) => {
                dotenvy::dotenv().map(|_| true).or_else(|err| match err {
                    dotenvy::Error::Io(_) => Ok(false),
                    _ => Err(err),
                })?
            }
        };

        let env_config = EnvConfig::gather();

        let (file_config, config_path, config_present) = (None, None, false);

        let (config, warnings) = self.compose_config(
            file_config,
            env_config,
            config_path.clone(),
            env_file_loaded,
            config_present,
        )?;

        Ok(ConfigLoad { config, warnings })
    }

    fn load_file_config(
        &self,
        _env_config: &EnvConfig,
    ) -> Result<(Option<FileConfig>, Option<PathBuf>, bool), ConfigLoadError>
    {
        Ok((None, None, false))
    }

    #[allow(clippy::too_many_arguments)]
    fn compose_config(
        &self,
        file_config: Option<FileConfig>,
        env: EnvConfig,
        config_path: Option<PathBuf>,
        env_file_loaded: bool,
        config_present: bool,
    ) -> Result<(Config, ConfigWarnings), ConfigLoadError> {
        let mut warnings = ConfigWarnings::default();

        // No file-based config; environment-only configuration.

        let file = file_config.unwrap_or_default();
        let FileConfig {
            server: file_server,
            database: file_database,
            redis: file_redis,
            media: file_media,
            cache: file_cache,
            ffmpeg: file_ffmpeg,
            cors: file_cors,
            security: file_security,
            auth: file_auth,
            rate_limiter: file_rate_limiter,
            scanner: file_scanner,
            dev_mode: file_dev_mode,
        } = file;
        let file_media_root = file_media.root;

        let env = env.clone();

        let server = ServerConfig {
            host: env
                .server_host
                .clone()
                .or(file_server.host.clone())
                .unwrap_or_else(|| "0.0.0.0".to_string()),
            port: env.server_port.or(file_server.port).unwrap_or(3000),
        };

        let database = DatabaseConfig {
            primary_url: self.resolve_database_url(&env, &file_database)?,
        };

        let redis = env
            .redis_url
            .clone()
            .map(|url| RedisConfig { url })
            .or_else(|| file_redis.map(|r| RedisConfig { url: r.url }));

        let media_root = match (file_media_root, env.media_root.clone()) {
            (Some(file_root), _) => Some(file_root),
            (None, env_root) => env_root,
        };
        let media = MediaConfig { root: media_root };

        let cache_root = env
            .cache_root
            .clone()
            .or(file_cache.root.clone())
            .unwrap_or_else(|| PathBuf::from("./cache"));
        let transcode = env
            .cache_transcode
            .clone()
            .or(file_cache.transcode.clone())
            .unwrap_or_else(|| cache_root.join("transcode"));
        let thumbnails = env
            .cache_thumbnails
            .clone()
            .or(file_cache.thumbnails.clone())
            .unwrap_or_else(|| cache_root.join("thumbnails"));
        let cache = CacheConfig {
            root: cache_root,
            transcode,
            thumbnails,
        };

        let ffmpeg = FfmpegConfig {
            ffmpeg_path: env
                .ffmpeg_path
                .clone()
                .or(file_ffmpeg.ffmpeg_path.clone())
                .unwrap_or_else(|| "ffmpeg".into()),
            ffprobe_path: env
                .ffprobe_path
                .clone()
                .or(file_ffmpeg.ffprobe_path.clone())
                .unwrap_or_else(|| "ffprobe".into()),
        };

        let cors = CorsConfig {
            allowed_origins: env
                .cors_allowed_origins
                .clone()
                .or(file_cors.allowed_origins.clone())
                .unwrap_or_else(default_cors_origins),
            allowed_methods: env
                .cors_allowed_methods
                .clone()
                .or(file_cors.allowed_methods.clone())
                .unwrap_or_else(default_cors_methods),
            allowed_headers: env
                .cors_allowed_headers
                .clone()
                .or(file_cors.allowed_headers.clone())
                .unwrap_or_else(default_cors_headers),
            allow_credentials: env
                .cors_allow_credentials
                .or(file_cors.allow_credentials)
                .unwrap_or(false),
        };

        let dev_mode = env.dev_mode.or(file_dev_mode).unwrap_or(false);

        let security = SecurityConfig {
            enforce_https: env
                .enforce_https
                .or(file_security.enforce_https)
                .unwrap_or(false),
            trust_proxy_headers: env
                .trust_proxy_headers
                .or(file_security.trust_proxy_headers)
                .unwrap_or(false),
            hsts: HstsSettings {
                max_age: env
                    .hsts_max_age
                    .or(file_security.hsts.max_age)
                    .unwrap_or(31_536_000),
                include_subdomains: env
                    .hsts_include_subdomains
                    .or(file_security.hsts.include_subdomains)
                    .unwrap_or(false),
                preload: env
                    .hsts_preload
                    .or(file_security.hsts.preload)
                    .unwrap_or(false),
            },
        };

        let auth = AuthConfig {
            password_pepper: env
                .auth_password_pepper
                .clone()
                .or(file_auth.password_pepper.clone())
                .unwrap_or_else(|| DEFAULT_PASSWORD_PEPPER.to_string()),
            token_key: env
                .auth_token_key
                .clone()
                .or(file_auth.token_key.clone())
                .unwrap_or_else(|| DEFAULT_TOKEN_KEY.to_string()),
            setup_token: env.setup_token.or(file_auth.setup_token),
        };

        let (scanner, scanner_source) =
            ScannerConfig::load_from_env().map_err(ConfigLoadError::Scanner)?;

        let (rate_limiter, rate_limit_source) =
            if let Some(env_spec) = env.rate_limits {
                let (config, source) = env_spec
                    .load_from_env()
                    .map_err(ConfigLoadError::RateLimiter)?;
                let settings = RateLimiterSettings {
                    config,
                    source: source.clone(),
                };
                (Some(settings), Some(source))
            } else {
                (None, None)
            };

        let metadata = ConfigMetadata {
            config_path: None,
            env_file_loaded,
            scanner_source,
            rate_limit_source,
        };

        let mut config = Config {
            server,
            database,
            redis,
            media,
            cache,
            ffmpeg,
            cors,
            security,
            dev_mode,
            auth,
            scanner,
            rate_limiter,
            metadata,
        };

        config
            .ensure_directories()
            .map_err(|err| ConfigLoadError::Filesystem { source: err })?;
        config
            .normalize_paths()
            .map_err(|err| ConfigLoadError::Filesystem { source: err })?;

        let guard_warnings = validation::apply_guard_rails(&config)?;
        warnings.extend(guard_warnings);

        Ok((config, warnings))
    }

    fn resolve_database_url(
        &self,
        env: &EnvConfig,
        file_database: &FileDatabaseConfig,
    ) -> Result<Option<String>, ConfigLoadError> {
        if let Some(url) = env
            .database_url
            .clone()
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(Some(url));
        }

        if let Some(path) = env.database_url_file.as_ref()
            && let Some(url) = Self::read_secret_file(path)?
        {
            return Ok(Some(url));
        }

        if let Some(ref stored_url) = file_database.url {
            let trimmed = stored_url.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            let mut parsed = Url::parse(trimmed).map_err(|source| {
                ConfigLoadError::InvalidDatabaseUrl { source }
            })?;
            if parsed.password().is_none()
                && let Some(password) =
                    self.resolve_database_password(env, file_database)?
            {
                parsed
                    .set_password(Some(&password))
                    .map_err(|_| ConfigLoadError::InvalidDatabasePassword)?;
            }
            return Ok(Some(parsed.to_string()));
        }

        let host = env
            .database_host
            .clone()
            .filter(|value| !value.trim().is_empty());
        let user = env
            .database_user
            .clone()
            .filter(|value| !value.trim().is_empty());
        let name = env
            .database_name
            .clone()
            .filter(|value| !value.trim().is_empty());

        if let (Some(host), Some(user), Some(name)) = (host, user, name) {
            let port = env.database_port.unwrap_or(5432);
            let mut url =
                Url::parse(&format!("postgresql://{host}:{port}/{name}"))
                    .map_err(|source| ConfigLoadError::InvalidDatabaseUrl {
                        source,
                    })?;
            url.set_username(&user).map_err(|_| {
                ConfigLoadError::InvalidDatabaseUsername {
                    username: user.clone(),
                }
            })?;
            if let Some(password) =
                self.resolve_database_password(env, file_database)?
            {
                url.set_password(Some(&password))
                    .map_err(|_| ConfigLoadError::InvalidDatabasePassword)?;
            }
            return Ok(Some(url.to_string()));
        }

        Ok(None)
    }

    fn resolve_database_password(
        &self,
        env: &EnvConfig,
        file_database: &FileDatabaseConfig,
    ) -> Result<Option<String>, ConfigLoadError> {
        if let Some(password) = env
            .database_password
            .clone()
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(Some(password));
        }

        for path in [
            env.database_password_file.as_ref(),
            env.ferrex_app_password_file.as_ref(),
            file_database.password_file.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(secret) = Self::read_secret_file(path)?
                && !secret.is_empty()
            {
                return Ok(Some(secret));
            }
        }

        if let Some(password) = env
            .ferrex_app_password
            .clone()
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(Some(password));
        }

        Ok(None)
    }

    fn read_secret_file(
        path: &Path,
    ) -> Result<Option<String>, ConfigLoadError> {
        let contents = fs::read_to_string(path).map_err(|source| {
            ConfigLoadError::SecretFileIo {
                path: path.to_path_buf(),
                source,
            }
        })?;
        let trimmed = contents.trim();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed.to_string()))
        }
    }
}

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

// File-based config resolution removed; environment-only configuration is used.

fn default_cors_origins() -> Vec<String> {
    vec![
        "http://localhost:3000".to_string(),
        "http://localhost:5173".to_string(),
    ]
}

fn default_cors_methods() -> Vec<String> {
    vec![
        "GET".to_string(),
        "POST".to_string(),
        "PUT".to_string(),
        "PATCH".to_string(),
        "DELETE".to_string(),
        "OPTIONS".to_string(),
    ]
}

fn default_cors_headers() -> Vec<String> {
    vec![
        "Authorization".to_string(),
        "Content-Type".to_string(),
        "X-CSRF-Token".to_string(),
    ]
}

#[derive(Debug)]
pub struct ConfigLoad {
    pub config: Config,
    pub warnings: ConfigWarnings,
}
