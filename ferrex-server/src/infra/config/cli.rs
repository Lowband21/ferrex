use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use dialoguer::{Confirm, Input};
use rand::{Rng, distr::Alphanumeric, rng};
use redis::{AsyncCommands, aio::ConnectionManager};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use super::{
    loader::{ConfigLoad, ConfigLoader, ConfigLoaderOptions},
    models::Config,
    sources::{
        FileAuthConfig, FileCacheConfig, FileConfig, FileCorsConfig,
        FileDatabaseConfig, FileFfmpegConfig, FileHstsConfig, FileMediaConfig,
        FileSecurityConfig, FileServerConfig,
    },
};

#[derive(Debug, Clone)]
pub struct InitOptions {
    pub config_path: PathBuf,
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
    ensure_writable(&opts.config_path, opts.force)?;
    ensure_writable(&opts.env_path, opts.force)?;

    if let Some(dir) = opts.config_path.parent() {
        fs::create_dir_all(dir).with_context(|| {
            format!("failed to create directory {}", dir.display())
        })?;
    }
    if let Some(dir) = opts.env_path.parent() {
        fs::create_dir_all(dir).with_context(|| {
            format!("failed to create directory {}", dir.display())
        })?;
    }

    let mut dev_mode = true;
    if !opts.non_interactive {
        dev_mode = Confirm::new()
            .with_prompt("Use development mode defaults (recommended for localhost setup)?")
            .default(true)
            .interact()
            .context("prompt failed")?;
    }

    let default_host = if dev_mode { "127.0.0.1" } else { "0.0.0.0" };
    let server_host: String = if opts.non_interactive {
        default_host.into()
    } else {
        Input::new()
            .with_prompt("Server host")
            .default(default_host.to_string())
            .interact_text()
            .context("prompt failed")?
    };

    let default_port = if dev_mode { 3000 } else { 443 };
    let server_port: u16 = if opts.non_interactive {
        default_port
    } else {
        Input::new()
            .with_prompt("Server port")
            .default(default_port.to_string())
            .validate_with(|input: &String| match input.parse::<u16>() {
                Ok(_) => Ok(()),
                Err(_) => Err("enter a valid port number"),
            })
            .interact_text()
            .context("prompt failed")?
            .parse()
            .expect("validated port to parse")
    };

    let default_media_root = if dev_mode {
        Some(PathBuf::from("./media"))
    } else {
        None
    };

    let media_root: Option<PathBuf> = if opts.non_interactive {
        default_media_root
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

    let default_database_url = if dev_mode {
        Some("postgres://localhost/ferrex".to_string())
    } else {
        None
    };

    let database_url: Option<String> = if let Some(url) = managed_database_url {
        Some(url)
    } else if opts.non_interactive {
        default_database_url
    } else {
        let answer: String = Input::new()
            .with_prompt("PostgreSQL connection URL")
            .allow_empty(true)
            .default(default_database_url.clone().unwrap_or_default())
            .interact_text()
            .context("prompt failed")?;
        let trimmed = answer.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    let managed_redis_url = std::env::var("FERREX_CONFIG_INIT_REDIS_URL")
        .ok()
        .filter(|value| !value.trim().is_empty());

    if managed_redis_url.is_some() {
        println!(
            "Using managed Redis connection provided by the host environment."
        );
    }

    let redis_url: Option<String> = if let Some(url) = managed_redis_url {
        Some(url)
    } else if opts.non_interactive {
        None
    } else {
        let answer: String = Input::new()
            .with_prompt("Redis connection URL for rate limiting (leave blank to disable rate limiting)")
            .allow_empty(true)
            .interact_text()
            .context("prompt failed")?;
        let trimmed = answer.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    let default_origins = vec![
        "http://localhost:5173".to_string(),
        "https://localhost:5173".to_string(),
        "http://localhost:3000".to_string(),
        "https://localhost:3000".to_string(),
    ];

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
        false
    } else {
        Confirm::new()
            .with_prompt("Allow CORS credentials (only when serving a trusted frontend)?")
            .default(false)
            .interact()
            .context("prompt failed")?
    };

    let mut enforce_https = !dev_mode;
    if !opts.non_interactive {
        enforce_https = Confirm::new()
            .with_prompt("Enforce HTTPS redirects?")
            .default(!dev_mode)
            .interact()
            .context("prompt failed")?;
    }

    let mut trust_proxy_headers = enforce_https;
    if enforce_https && !opts.non_interactive {
        trust_proxy_headers = Confirm::new()
            .with_prompt(
                "Trust proxy headers like X-Forwarded-Proto? (enable when TLS terminates upstream)",
            )
            .default(trust_proxy_headers)
            .interact()
            .context("prompt failed")?;
    }

    let cache_root = PathBuf::from("./cache");

    let file_config = FileConfig {
        server: FileServerConfig {
            host: Some(server_host.clone()),
            port: Some(server_port),
        },
        database: FileDatabaseConfig {
            url: database_url.clone(),
        },
        redis: redis_url
            .clone()
            .map(|url| super::sources::FileRedisConfig { url }),
        media: FileMediaConfig {
            root: media_root.clone(),
        },
        cache: FileCacheConfig {
            root: Some(cache_root.clone()),
            transcode: Some(cache_root.join("transcode")),
            thumbnails: Some(cache_root.join("thumbnails")),
        },
        ffmpeg: FileFfmpegConfig::default(),
        cors: FileCorsConfig {
            allowed_origins: Some(cors_origins.clone()),
            allowed_methods: Some(default_cors_methods()),
            allowed_headers: Some(default_cors_headers()),
            allow_credentials: Some(allow_credentials),
        },
        security: FileSecurityConfig {
            enforce_https: Some(enforce_https),
            trust_proxy_headers: Some(trust_proxy_headers),
            hsts: FileHstsConfig {
                max_age: Some(if enforce_https { 31_536_000 } else { 0 }),
                include_subdomains: Some(false),
                preload: Some(false),
            },
        },
        auth: FileAuthConfig::default(),
        rate_limiter: None,
        scanner: None,
        dev_mode: Some(dev_mode),
    };

    let toml = toml::to_string(&file_config)
        .context("failed to serialize configuration")?;
    fs::write(&opts.config_path, toml).with_context(|| {
        format!(
            "failed to write configuration to {}",
            opts.config_path.display()
        )
    })?;

    // Prepare cache directories
    fs::create_dir_all(cache_root.join("transcode"))
        .context("failed to create transcode cache directory")?;
    fs::create_dir_all(cache_root.join("thumbnails"))
        .context("failed to create thumbnail cache directory")?;

    let auth_password_pepper = generate_secret(64);
    let auth_token_key = generate_secret(64);
    let setup_token = generate_secret(48);

    let mut env_file = fs::File::create(&opts.env_path).with_context(|| {
        format!("failed to create {}", opts.env_path.display())
    })?;
    writeln!(env_file, "# Ferrex environment secrets")?;
    writeln!(env_file, "DEV_MODE={}", dev_mode)?;
    writeln!(env_file, "AUTH_PASSWORD_PEPPER={}", auth_password_pepper)?;
    writeln!(env_file, "AUTH_TOKEN_KEY={}", auth_token_key)?;
    writeln!(env_file, "FERREX_SETUP_TOKEN={}", setup_token)?;
    if let Some(db) = &database_url {
        writeln!(env_file, "DATABASE_URL={}", db)?;
    }
    if let Some(redis) = &redis_url {
        writeln!(env_file, "REDIS_URL={}", redis)?;
    }
    writeln!(
        env_file,
        "FERREX_CONFIG_PATH={}",
        opts.config_path.display()
    )?;

    println!(
        "Configuration written to {} and environment secrets saved to {}",
        opts.config_path.display(),
        opts.env_path.display()
    );
    println!("Run `ferrex-server config check` before starting the server.");

    Ok(())
}

pub async fn run_config_check(opts: &CheckOptions) -> Result<()> {
    let loader = ConfigLoader::with_options(ConfigLoaderOptions {
        config_path: opts.config_path.clone(),
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
