use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use dialoguer::{Confirm, Input};
use rand::{Rng, distr::Alphanumeric, rng};
use redis::{AsyncCommands, aio::ConnectionManager};
use sqlx::postgres::PgPoolOptions;
use url::Url;
use uuid::Uuid;

use super::{
    loader::{ConfigLoad, ConfigLoader, ConfigLoaderOptions},
    models::Config,
    sources::{
        FileAuthConfig, FileCacheConfig, FileConfig, FileCorsConfig,
        FileDatabaseConfig, FileHstsConfig, FileMediaConfig,
        FileSecurityConfig, FileServerConfig,
    },
};

#[derive(Debug, Clone)]
pub struct InitOptions {
    pub env_path: PathBuf,
    pub force: bool,
    pub non_interactive: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    pub config_path: Option<PathBuf>,
    pub env_path: Option<PathBuf>,
    pub tls_cert_path: Option<PathBuf>,
    pub tls_key_path: Option<PathBuf>,
}

pub async fn run_config_init(opts: &InitOptions) -> Result<()> {
    ensure_writable(&opts.env_path, opts.force)?;

    if let Some(dir) = opts.env_path.parent() {
        fs::create_dir_all(dir).with_context(|| {
            format!("failed to create directory {}", dir.display())
        })?;
    }

    let existing_env = load_env_map(&opts.env_path)?;

    let dev_mode_default = existing_env
        .get("DEV_MODE")
        .and_then(|value| parse_bool(value))
        .unwrap_or(true);

    let mut dev_mode = dev_mode_default;
    if !opts.non_interactive {
        dev_mode = Confirm::new()
            .with_prompt("Use development mode defaults (recommended for localhost setup)?")
            .default(dev_mode_default)
            .interact()
            .context("prompt failed")?;
    }

    let env_server_host = existing_env
        .get("SERVER_HOST")
        .cloned()
        .filter(|value| !value.trim().is_empty());
    let server_host_default = env_server_host
        .clone()
        .unwrap_or_else(|| "0.0.0.0".to_string());
    let server_host: String = if opts.non_interactive {
        server_host_default.clone()
    } else {
        Input::new()
            .with_prompt(
                "Server host (0.0.0.0 for containers/Tailscale; 127.0.0.1 for localhost-only)",
            )
            .default(server_host_default.clone())
            .interact_text()
            .context("prompt failed")?
    };

    let server_port_default = existing_env
        .get("SERVER_PORT")
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(if dev_mode { 3000 } else { 443 });
    let server_port: u16 = if opts.non_interactive {
        server_port_default
    } else {
        Input::new()
            .with_prompt("Server port")
            .default(server_port_default.to_string())
            .validate_with(|input: &String| match input.parse::<u16>() {
                Ok(_) => Ok(()),
                Err(_) => Err("enter a valid port number"),
            })
            .interact_text()
            .context("prompt failed")?
            .parse()
            .expect("validated port to parse")
    };

    let default_media_root = existing_env.get("MEDIA_ROOT").map(PathBuf::from);

    let media_root: Option<PathBuf> = if opts.non_interactive {
        default_media_root.clone()
    } else {
        let answer: String = Input::new()
            .with_prompt("Media library root (leave blank to configure later)")
            .allow_empty(true)
            .default(
                default_media_root
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            )
            .interact_text()
            .context("prompt failed")?;
        let trimmed = answer.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    };

    let managed_database_url = std::env::var("FERREX_CONFIG_INIT_DATABASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty());

    if managed_database_url.is_some() {
        println!(
            "Using managed PostgreSQL connection provided by the host environment."
        );
    }

    let managed_database_url_host =
        std::env::var("FERREX_CONFIG_INIT_HOST_DATABASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());

    let database_url_existing: Option<String> = None;
    let env_database_url_host = existing_env.get("DATABASE_URL").cloned();
    // No separate container DSN; use DATABASE_HOST_CONTAINER to override host in containers

    let mut db_host_default = existing_env
        .get("DATABASE_HOST")
        .cloned()
        .unwrap_or_else(|| "localhost".to_string());
    let mut db_port_default = existing_env
        .get("DATABASE_PORT")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(5432);
    let mut db_user_default = existing_env
        .get("DATABASE_USER")
        .cloned()
        .or_else(|| existing_env.get("FERREX_APP_USER").cloned())
        .unwrap_or_else(|| "ferrex_app".to_string());
    let mut db_name_default = existing_env
        .get("DATABASE_NAME")
        .cloned()
        .or_else(|| existing_env.get("FERREX_DB").cloned())
        .unwrap_or_else(|| "ferrex".to_string());

    for candidate in [
        managed_database_url_host.as_deref(),
        managed_database_url.as_deref(),
        database_url_existing.as_deref(),
        env_database_url_host.as_deref(),
    ] {
        if let Some(parts) = candidate.and_then(parse_postgres_connection_parts)
        {
            db_host_default = parts.host;
            db_port_default = parts.port;
            db_user_default = parts.user;
            db_name_default = parts.database;
            break;
        }
    }

    let db_host: String = if opts.non_interactive {
        db_host_default.clone()
    } else {
        Input::new()
            .with_prompt("PostgreSQL host (from host machine)")
            .default(db_host_default.clone())
            .interact_text()
            .context("prompt failed")?
    };

    let db_port: u16 = if opts.non_interactive {
        db_port_default
    } else {
        Input::new()
            .with_prompt("PostgreSQL port (from host machine)")
            .default(db_port_default.to_string())
            .validate_with(|input: &String| match input.parse::<u16>() {
                Ok(_) => Ok(()),
                Err(_) => Err("enter a valid port number"),
            })
            .interact_text()
            .context("prompt failed")?
            .parse()
            .expect("validated port to parse")
    };

    let db_name: String = if opts.non_interactive {
        db_name_default.clone()
    } else {
        Input::new()
            .with_prompt("Application database name")
            .default(db_name_default.clone())
            .interact_text()
            .context("prompt failed")?
    };

    let db_user: String = if opts.non_interactive {
        db_user_default.clone()
    } else {
        Input::new()
            .with_prompt("Application database user")
            .default(db_user_default.clone())
            .interact_text()
            .context("prompt failed")?
    };

    let db_container_host_default = existing_env
        .get("DATABASE_HOST_CONTAINER")
        .cloned()
        .unwrap_or_else(|| "db".to_string());

    let database_url_host_default = format!(
        "postgresql://{}@{}:{}/{}",
        db_user, db_host, db_port, db_name
    );
    let database_url_host = managed_database_url_host
        .clone()
        .or(env_database_url_host.clone())
        .or(existing_env.get("DATABASE_URL").cloned())
        .unwrap_or_else(|| database_url_host_default.clone());

    let managed_redis_url = std::env::var("FERREX_CONFIG_INIT_REDIS_URL")
        .ok()
        .filter(|value| !value.trim().is_empty());

    if managed_redis_url.is_some() {
        println!(
            "Using managed Redis connection provided by the host environment."
        );
    }

    let managed_redis_url_host =
        std::env::var("FERREX_CONFIG_INIT_HOST_REDIS_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());

    let existing_redis_url: Option<String> = None;
    let env_redis_url_host = existing_env.get("REDIS_URL").cloned();
    let env_redis_url_container =
        existing_env.get("REDIS_URL_CONTAINER").cloned();

    let default_redis_url_host = managed_redis_url_host
        .clone()
        .or(env_redis_url_host.clone())
        .or(existing_redis_url.clone())
        .or_else(|| Some("redis://127.0.0.1:6379".to_string()));

    let redis_url_host: Option<String> = if opts.non_interactive {
        default_redis_url_host.clone()
    } else {
        let answer: String = Input::new()
            .with_prompt(
                "Redis connection URL for rate limiting (leave blank to disable rate limiting)",
            )
            .allow_empty(true)
            .default(default_redis_url_host.clone().unwrap_or_default())
            .interact_text()
            .context("prompt failed")?;
        let trimmed = answer.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    let redis_url_container = managed_redis_url
        .clone()
        .or(env_redis_url_container)
        .or_else(|| {
            redis_url_host
                .as_deref()
                .and_then(|url| derive_internal_connection_url(url, "cache"))
        });

    let default_origins =
        if let Some(raw) = existing_env.get("CORS_ALLOWED_ORIGINS") {
            raw.split(',')
                .filter_map(|s| {
                    let t = s.trim();
                    if t.is_empty() {
                        None
                    } else {
                        Some(t.to_string())
                    }
                })
                .collect()
        } else {
            vec![
                "http://localhost:5173".to_string(),
                "https://localhost:5173".to_string(),
                "http://localhost:3000".to_string(),
                "https://localhost:3000".to_string(),
            ]
        };
    let default_methods = default_cors_methods();
    let default_headers = default_cors_headers();
    let allow_credentials_default = existing_env
        .get("CORS_ALLOW_CREDENTIALS")
        .and_then(|v| parse_bool(v))
        .unwrap_or(false);

    let cors_origins: Vec<String> = if opts.non_interactive {
        default_origins.clone()
    } else {
        let answer: String = Input::new()
            .with_prompt("Allowed CORS origins (comma separated)")
            .default(default_origins.join(","))
            .interact_text()
            .context("prompt failed")?;
        answer
            .split(',')
            .filter_map(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .collect::<Vec<_>>()
    };

    let allow_credentials = if opts.non_interactive {
        allow_credentials_default
    } else {
        Confirm::new()
            .with_prompt("Allow CORS credentials (only when serving a trusted frontend)?")
            .default(allow_credentials_default)
            .interact()
            .context("prompt failed")?
    };

    let enforce_https_default = existing_env
        .get("ENFORCE_HTTPS")
        .and_then(|v| parse_bool(v))
        .unwrap_or(!dev_mode);
    let enforce_https = if opts.non_interactive {
        enforce_https_default
    } else {
        Confirm::new()
            .with_prompt("Enforce HTTPS redirects?")
            .default(enforce_https_default)
            .interact()
            .context("prompt failed")?
    };

    let trust_proxy_default = existing_env
        .get("TRUST_PROXY_HEADERS")
        .and_then(|v| parse_bool(v))
        .unwrap_or(enforce_https);
    let trust_proxy_headers = if enforce_https && !opts.non_interactive {
        Confirm::new()
            .with_prompt(
                "Trust proxy headers like X-Forwarded-Proto? (enable when TLS terminates upstream)",
            )
            .default(trust_proxy_default)
            .interact()
            .context("prompt failed")?
    } else {
        trust_proxy_default
    };

    let hsts_max_age = existing_env
        .get("HSTS_MAX_AGE")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(if enforce_https { 31_536_000 } else { 0 });
    let hsts_include_subdomains = existing_env
        .get("HSTS_INCLUDE_SUBDOMAINS")
        .and_then(|v| parse_bool(v))
        .unwrap_or(false);
    let hsts_preload = existing_env
        .get("HSTS_PRELOAD")
        .and_then(|v| parse_bool(v))
        .unwrap_or(false);

    let cache_root = existing_env
        .get("CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./cache"));
    let cache_transcode = existing_env
        .get("TRANSCODE_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| cache_root.join("transcode"));
    let cache_thumbnails = existing_env
        .get("THUMBNAIL_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| cache_root.join("thumbnails"));

    let existing_ffmpeg = super::sources::FileFfmpegConfig::default();

    let auth_defaults = super::sources::FileAuthConfig::default();
    let auth_password_pepper = auth_defaults
        .password_pepper
        .clone()
        .or(existing_env.get("AUTH_PASSWORD_PEPPER").cloned())
        .unwrap_or_else(|| generate_secret(64));
    let auth_token_key = auth_defaults
        .token_key
        .clone()
        .or(existing_env.get("AUTH_TOKEN_KEY").cloned())
        .unwrap_or_else(|| generate_secret(64));
    let setup_token = auth_defaults
        .setup_token
        .clone()
        .or(existing_env.get("FERREX_SETUP_TOKEN").cloned())
        .or_else(|| Some(generate_secret(48)));

    let _file_config = FileConfig {
        server: FileServerConfig {
            host: Some(server_host.clone()),
            port: Some(server_port),
        },
        database: FileDatabaseConfig {
            url: None,
            ..Default::default()
        },
        redis: redis_url_host
            .clone()
            .map(|url| super::sources::FileRedisConfig { url }),
        media: FileMediaConfig {
            root: media_root.clone(),
        },
        cache: FileCacheConfig {
            root: Some(cache_root.clone()),
            transcode: Some(cache_transcode.clone()),
            thumbnails: Some(cache_thumbnails.clone()),
        },
        ffmpeg: existing_ffmpeg,
        cors: FileCorsConfig {
            allowed_origins: Some(cors_origins.clone()),
            allowed_methods: Some(default_methods.clone()),
            allowed_headers: Some(default_headers.clone()),
            allow_credentials: Some(allow_credentials),
        },
        security: FileSecurityConfig {
            enforce_https: Some(enforce_https),
            trust_proxy_headers: Some(trust_proxy_headers),
            hsts: FileHstsConfig {
                max_age: Some(hsts_max_age),
                include_subdomains: Some(hsts_include_subdomains),
                preload: Some(hsts_preload),
            },
        },
        auth: FileAuthConfig {
            password_pepper: Some(auth_password_pepper.clone()),
            token_key: Some(auth_token_key.clone()),
            setup_token: setup_token.clone(),
        },
        rate_limiter: None,
        scanner: None,
        dev_mode: Some(dev_mode),
    };

    // No TOML file written; environment-only configuration.

    fs::create_dir_all(&cache_root)
        .context("failed to create cache directory")?;
    fs::create_dir_all(&cache_transcode)
        .context("failed to create transcode cache directory")?;
    fs::create_dir_all(&cache_thumbnails)
        .context("failed to create thumbnail cache directory")?;

    let mut env_file = fs::File::create(&opts.env_path).with_context(|| {
        format!("failed to create {}", opts.env_path.display())
    })?;
    writeln!(env_file, "# Ferrex environment (host tooling)")?;
    writeln!(env_file, "DEV_MODE={}", dev_mode)?;
    writeln!(env_file, "SERVER_HOST={}", server_host)?;
    writeln!(env_file, "SERVER_PORT={}", server_port)?;
    writeln!(env_file, "ENFORCE_HTTPS={}", enforce_https)?;
    writeln!(env_file, "TRUST_PROXY_HEADERS={}", trust_proxy_headers)?;
    writeln!(env_file, "HSTS_MAX_AGE={}", hsts_max_age)?;
    writeln!(
        env_file,
        "HSTS_INCLUDE_SUBDOMAINS={}",
        hsts_include_subdomains
    )?;
    writeln!(env_file, "HSTS_PRELOAD={}", hsts_preload)?;

    write_env_entry(
        &mut env_file,
        "DATABASE_URL",
        Some(database_url_host.as_str()),
    )?;
    // Container override host (Compose will set DATABASE_HOST from this inside the container)
    writeln!(
        env_file,
        "DATABASE_HOST_CONTAINER={}",
        db_container_host_default
    )?;

    let media_root_string =
        media_root.as_ref().map(|path| path.display().to_string());
    write_env_entry(&mut env_file, "MEDIA_ROOT", media_root_string.as_deref())?;

    writeln!(env_file, "DATABASE_HOST={}", db_host)?;
    writeln!(env_file, "DATABASE_PORT={}", db_port)?;
    writeln!(env_file, "DATABASE_NAME={}", db_name)?;
    writeln!(env_file, "DATABASE_USER={}", db_user)?;
    // No internal host/port variables; use DATABASE_HOST_CONTAINER instead

    write_env_entry(&mut env_file, "REDIS_URL", redis_url_host.as_deref())?;
    write_env_entry(
        &mut env_file,
        "REDIS_URL_CONTAINER",
        redis_url_container.as_deref(),
    )?;

    writeln!(env_file, "AUTH_PASSWORD_PEPPER={}", auth_password_pepper)?;
    writeln!(env_file, "AUTH_TOKEN_KEY={}", auth_token_key)?;
    write_env_entry(
        &mut env_file,
        "FERREX_SETUP_TOKEN",
        setup_token.as_deref(),
    )?;

    println!("Environment saved to {}", opts.env_path.display());
    println!("Run `ferrex-server config check` before starting the server.");

    Ok(())
}

fn load_existing_file_config(path: &Path) -> Result<Option<FileConfig>> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read existing configuration at {}",
            path.display()
        )
    })?;

    let config: FileConfig = toml::from_str(&contents).with_context(|| {
        format!(
            "failed to parse existing configuration at {}",
            path.display()
        )
    })?;

    Ok(Some(config))
}

fn load_env_map(path: &Path) -> Result<HashMap<String, String>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let mut map = HashMap::new();
    for entry in dotenvy::from_path_iter(path)? {
        let (key, value) = entry?;
        map.insert(key, value);
    }

    Ok(map)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" => Some(true),
        "false" | "0" | "no" | "n" => Some(false),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct PostgresConnectionParts {
    host: String,
    port: u16,
    user: String,
    database: String,
}

fn parse_postgres_connection_parts(
    url: &str,
) -> Option<PostgresConnectionParts> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_string();
    let port = parsed.port().unwrap_or(5432);
    let username = parsed.username();
    if username.is_empty() {
        return None;
    }
    let database = parsed
        .path()
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string();
    if database.is_empty() {
        return None;
    }
    Some(PostgresConnectionParts {
        host,
        port,
        user: username.to_string(),
        database,
    })
}

fn derive_internal_connection_url(
    url: &str,
    internal_host: &str,
) -> Option<String> {
    let mut parsed = Url::parse(url).ok()?;
    if let Some(host) = parsed.host_str()
        && !is_loopback_host(host)
    {
        return Some(url.to_string());
    }
    parsed.set_host(Some(internal_host)).ok()?;
    Some(parsed.to_string())
}

fn is_loopback_host(host: &str) -> bool {
    let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
    normalized == "localhost"
        || normalized == "::1"
        || normalized == "[::1]"
        || normalized == "0.0.0.0"
        || normalized.starts_with("127.")
}

fn write_env_entry(
    file: &mut fs::File,
    key: &str,
    value: Option<&str>,
) -> Result<()> {
    match value {
        Some(value) => writeln!(file, "{}={}", key, value)?,
        None => writeln!(file, "{}=", key)?,
    }
    Ok(())
}

pub async fn run_config_check(opts: &CheckOptions) -> Result<()> {
    let loader = ConfigLoader::with_options(ConfigLoaderOptions {
        config_path: None,
        env_file: opts.env_path.clone(),
    });

    let ConfigLoad { config, warnings } = loader.load()?;

    if !warnings.items.is_empty() {
        println!("Warnings:");
        for warning in &warnings.items {
            match &warning.hint {
                Some(hint) => {
                    println!("  - {} (hint: {})", warning.message, hint)
                }
                None => println!("  - {}", warning.message),
            }
        }
    }

    let mut failures = Vec::new();
    let mut successes = Vec::new();

    match resolve_database_url(&config) {
        Some(url) => match check_database(&url).await {
            Ok(()) => successes.push("database connectivity".to_string()),
            Err(err) => {
                failures.push(format!("database connectivity failed: {err}"))
            }
        },
        None => failures.push("no database configuration found".to_string()),
    }

    match resolve_redis_url(&config) {
        Some(url) => match check_redis(&url).await {
            Ok(()) => successes.push("redis connectivity".to_string()),
            Err(err) => {
                failures.push(format!("redis connectivity failed: {err}"))
            }
        },
        None => successes
            .push("redis not configured (rate limiting disabled)".to_string()),
    }

    match check_tls_strategy(&config, opts) {
        Ok(message) => successes.push(message),
        Err(err) => failures.push(format!("tls configuration invalid: {err}")),
    }

    for success in successes {
        println!("[ok] {success}");
    }

    if failures.is_empty() {
        println!("All checks passed.");
        Ok(())
    } else {
        println!("Configuration check encountered errors:");
        for failure in &failures {
            println!("  - {failure}");
        }
        bail!("configuration check failed")
    }
}

fn ensure_writable(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        bail!(
            "{} already exists (use --force to overwrite)",
            path.display()
        );
    }
    Ok(())
}

fn generate_secret(len: usize) -> String {
    rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn default_cors_methods() -> Vec<String> {
    vec![
        "GET".into(),
        "POST".into(),
        "PUT".into(),
        "PATCH".into(),
        "DELETE".into(),
        "OPTIONS".into(),
    ]
}

fn default_cors_headers() -> Vec<String> {
    vec![
        "Authorization".into(),
        "Content-Type".into(),
        "X-CSRF-Token".into(),
    ]
}

fn resolve_database_url(config: &Config) -> Option<String> {
    config
        .database
        .primary_url
        .clone()
        .or_else(derive_database_url_from_env)
}

fn resolve_redis_url(config: &Config) -> Option<String> {
    config
        .redis
        .as_ref()
        .map(|redis| redis.url.clone())
        .or_else(|| std::env::var("REDIS_URL").ok())
}

fn derive_database_url_from_env() -> Option<String> {
    let database = std::env::var("PGDATABASE")
        .or_else(|_| std::env::var("POSTGRES_DB"))
        .ok()?
        .trim()
        .to_owned();

    if database.is_empty() {
        return None;
    }

    Some(format!("postgresql:///{database}"))
}

async fn check_database(url: &str) -> Result<()> {
    let pool = match PgPoolOptions::new().max_connections(1).connect(url).await
    {
        Ok(pool) => pool,
        Err(err) => {
            return Err(anyhow!(
                "failed to connect to database at {url}: {err}"
            ));
        }
    };

    if let Err(err) = sqlx::query("SELECT 1").execute(&pool).await {
        return Err(anyhow!("failed to execute test query: {err}"));
    }

    Ok(())
}

async fn check_redis(url: &str) -> Result<()> {
    let client = redis::Client::open(url)
        .with_context(|| format!("failed to parse redis url {url}"))?;
    let mut connection = ConnectionManager::new(client)
        .await
        .map_err(|err| anyhow!("failed to connect to redis: {err}"))?;

    let check_key = format!("ferrex:config-check:{}", Uuid::new_v4());
    if let Err(err) = connection
        .set_ex::<String, &str, ()>(check_key, "ok", 30)
        .await
    {
        return Err(anyhow!("failed to write probe key to redis: {err}"));
    }

    Ok(())
}

fn check_tls_strategy(config: &Config, opts: &CheckOptions) -> Result<String> {
    let cert_path = opts
        .tls_cert_path
        .clone()
        .or_else(|| std::env::var("TLS_CERT_PATH").ok().map(PathBuf::from));
    let key_path = opts
        .tls_key_path
        .clone()
        .or_else(|| std::env::var("TLS_KEY_PATH").ok().map(PathBuf::from));

    if config.security.enforce_https && !config.dev_mode {
        match (cert_path, key_path) {
            (Some(cert), Some(key)) => {
                if !cert.exists() {
                    bail!("TLS certificate {} does not exist", cert.display());
                }
                if !key.exists() {
                    bail!("TLS private key {} does not exist", key.display());
                }
                Ok("https termination configured locally".to_string())
            }
            _ if config.security.trust_proxy_headers => {
                Ok("https enforced via proxy headers; ensure upstream terminates TLS".to_string())
            }
            _ => bail!(
                "ENFORCE_HTTPS=true but no TLS assets found. Provide --cert/--key or enable TRUST_PROXY_HEADERS."
            ),
        }
    } else {
        Ok("https enforcement disabled".to_string())
    }
}
