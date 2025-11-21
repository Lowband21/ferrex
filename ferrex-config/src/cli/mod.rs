//! CLI-facing init/check helpers shared by `ferrex-init` and `ferrex-server`.
//!
//! The functions here generate managed key/value pairs for `.env`, perform
//! connectivity checks, and expose options structs consumed by the binaries.

pub mod db;
pub mod options;
pub mod prompt_menu;
pub mod specs;
pub mod stack;
pub mod state;
pub mod tui;
pub mod utils;
pub mod validation;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use dialoguer::console::Term;
use rand::{Rng, SeedableRng, distr::Alphanumeric, rng, rngs::StdRng};
use redis::{AsyncCommands, aio::ConnectionManager};
use sqlx::postgres::PgPoolOptions;
use url::Url;
use uuid::Uuid;

pub use options::*;

use crate::cli::{
    prompt_menu::run_prompt_menu, state::PromptState, tui::run_tui_menu,
};

use super::{
    loader::{ConfigLoad, ConfigLoader, ConfigLoaderOptions},
    models::Config,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Secret-rotation scope for init runs.
pub enum RotateTarget {
    None,
    Db,
    Auth,
    All,
}

impl RotateTarget {
    fn rotates_db(self) -> bool {
        matches!(self, RotateTarget::Db | RotateTarget::All)
    }

    fn rotates_auth(self) -> bool {
        matches!(self, RotateTarget::Auth | RotateTarget::All)
    }
}

#[derive(Debug, Clone, Default)]
/// Result of an init run: final key/value pairs and which keys were rotated.
pub struct InitOutcome {
    pub kv: Vec<(String, String)>,
    pub rotated_keys: Vec<String>,
}

pub async fn run_config_init(opts: &InitOptions) -> Result<()> {
    let InitOutcome { kv, .. } = gen_init_merge_env(opts).await?;
    for (key, value) in kv {
        println!("{}={}", key, value);
    }
    Ok(())
}

pub async fn generate_init_kv(
    opts: &InitOptions,
) -> Result<Vec<(String, String)>> {
    Ok(gen_init_merge_env(opts).await?.kv)
}

pub async fn gen_init_merge_env(opts: &InitOptions) -> Result<InitOutcome> {
    let mut kv: Vec<(String, String)> = Vec::new();
    let mut rotated_keys: Vec<String> = Vec::new();
    let mut push = |key: &str, value: String| {
        kv.push((key.to_string(), value));
    };

    // Load existing environment to provide sensible defaults while keeping the map typed.
    let existing_env: HashMap<String, String> = load_env_map(&opts.env_path)?;

    let mut state = PromptState::from_env(&existing_env, opts.tailscale)?;

    if !opts.non_interactive {
        if opts.tui {
            run_tui_menu(&mut state, opts.advanced)?
        } else {
            run_prompt_menu(&mut state, opts.advanced)?;
        }
    }

    let dev_mode = state.dev_mode;
    let server_host = state.server_host.clone();
    let server_port = state.server_port;
    let media_root = state.media_root.clone();
    let ferrex_server_url = state.ferrex_server_url.clone();
    let tmdb_api_key = state.tmdb_api_key.clone();

    let env_database_url = existing_env.get("DATABASE_URL").cloned();
    let env_database_url_admin =
        existing_env.get("DATABASE_URL_ADMIN").cloned();
    let env_database_url_container =
        existing_env.get("DATABASE_URL_CONTAINER").cloned();
    let env_redis_url_host = existing_env.get("REDIS_URL").cloned();
    let env_redis_url_container =
        existing_env.get("REDIS_URL_CONTAINER").cloned();

    let sqlx_offline = existing_env
        .get("DATABASE_HOST")
        .cloned()
        .unwrap_or_else(|| "true".to_string());
    let db_host_default = existing_env
        .get("DATABASE_HOST")
        .cloned()
        .unwrap_or_else(|| "localhost".to_string());
    let mut db_container_host_default = existing_env
        .get("DATABASE_HOST_CONTAINER")
        .cloned()
        .unwrap_or_else(|| "db".to_string());
    let db_port_default = existing_env
        .get("DATABASE_PORT")
        .and_then(|value: &String| value.parse::<u16>().ok())
        .unwrap_or(5432);
    let db_app_user_default = existing_env
        .get("DATABASE_APP_USER")
        .cloned()
        .unwrap_or_else(|| "ferrex_app".to_string());
    let db_admin_user_default = existing_env
        .get("DATABASE_ADMIN_USER")
        .cloned()
        .unwrap_or_else(|| "postgres".to_string());
    let db_name_default = existing_env
        .get("DATABASE_NAME")
        .cloned()
        .unwrap_or_else(|| "ferrex".to_string());
    let app_password_default: Option<String> = None;
    let admin_password_default: Option<String> = None;

    if opts.tailscale {
        db_container_host_default = "127.0.0.1".to_string();
    }

    let db_host: String = db_host_default.clone();

    let db_port: u16 = db_port_default;

    let db_name: String = db_name_default.clone();

    let db_user: String = db_app_user_default.clone();

    let db_admin_user = db_admin_user_default.clone();

    let rotate_db = opts.rotate.rotates_db() || opts.force;

    let (database_app_password, app_rotated) = resolve_secret_with_sources(
        &existing_env,
        "DATABASE_APP_PASSWORD",
        &[
            "DATABASE_APP_PASSWORD_FILE",
            "FERREX_APP_PASSWORD_FILE",
            "DATABASE_PASSWORD_FILE",
        ],
        app_password_default.clone(),
        rotate_db,
        32,
    )?;
    if app_rotated {
        rotated_keys.push("DATABASE_APP_PASSWORD".to_string());
    }

    let (database_admin_password, admin_rotated) = resolve_secret_with_sources(
        &existing_env,
        "DATABASE_ADMIN_PASSWORD",
        &["DATABASE_ADMIN_PASSWORD_FILE"],
        admin_password_default.clone(),
        rotate_db,
        32,
    )?;
    if admin_rotated {
        rotated_keys.push("DATABASE_ADMIN_PASSWORD".to_string());
    }

    let database_url_host = env_database_url.clone().unwrap_or_else(|| {
        build_postgres_url(
            &db_host,
            db_port,
            &db_user,
            &database_app_password,
            &db_name,
        )
        .unwrap_or_else(|| format!(
            "postgresql://{db_user}:{database_app_password}@{db_host}:{db_port}/{db_name}"
        ))
    });

    let database_url_admin = env_database_url_admin.clone().unwrap_or_else(|| {
        build_postgres_url(
            &db_host,
            db_port,
            &db_admin_user,
            &database_admin_password,
            &db_name,
        )
        .unwrap_or_else(|| format!(
            "postgresql://{db_admin_user}:{database_admin_password}@{db_host}:{db_port}/{db_name}"
        ))
    });

    let database_url_container = env_database_url_container.clone().unwrap_or_else(|| {
        build_postgres_url(
            &db_container_host_default,
            db_port,
            &db_user,
            &database_app_password,
            &db_name,
        )
        .unwrap_or_else(|| format!(
            "postgresql://{db_user}:{database_app_password}@{db_container_host_default}:{db_port}/{db_name}"
        ))
    });

    let default_redis_url_host = env_redis_url_host
        .or_else(|| Some("redis://127.0.0.1:6379".to_string()));

    let redis_url_host: Option<String> = default_redis_url_host;

    let redis_internal_host =
        if opts.tailscale { "127.0.0.1" } else { "cache" };
    let redis_url_container = env_redis_url_container
        .or_else(|| {
            redis_url_host.as_deref().and_then(|url| {
                derive_internal_connection_url(url, redis_internal_host)
            })
        })
        .or_else(|| {
            if opts.tailscale {
                Some("redis://127.0.0.1:6379".to_string())
            } else {
                None
            }
        });

    let cors_origins: Vec<String> = state
        .cors_allowed_origins
        .split(',')
        .filter_map(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
        .collect();
    let allow_credentials = state.cors_allow_credentials;
    let enforce_https = state.enforce_https;
    let trust_proxy_headers = state.trust_proxy_headers;
    let hsts_max_age = state.hsts_max_age;
    let hsts_include_subdomains = state.hsts_include_subdomains;
    let hsts_preload = state.hsts_preload;

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

    let auth_defaults = crate::models::sources::FileAuthConfig::default();
    let rotate_auth = opts.rotate.rotates_auth() || opts.force;

    let (auth_password_pepper, pepper_rotated) = resolve_secret_with_sources(
        &existing_env,
        "AUTH_PASSWORD_PEPPER",
        &["AUTH_PASSWORD_PEPPER_FILE"],
        auth_defaults
            .password_pepper
            .clone()
            .and_then(|v: String| normalize_secret_from_env(&v)),
        rotate_auth,
        64,
    )?;
    if pepper_rotated {
        rotated_keys.push("AUTH_PASSWORD_PEPPER".to_string());
    }

    let (auth_token_key, token_rotated) = resolve_secret_with_sources(
        &existing_env,
        "AUTH_TOKEN_KEY",
        &["AUTH_TOKEN_KEY_FILE"],
        auth_defaults
            .token_key
            .clone()
            .and_then(|v: String| normalize_secret_from_env(&v)),
        rotate_auth,
        64,
    )?;
    if token_rotated {
        rotated_keys.push("AUTH_TOKEN_KEY".to_string());
    }

    let (setup_token, setup_rotated) = resolve_secret_with_sources(
        &existing_env,
        "FERREX_SETUP_TOKEN",
        &["FERREX_SETUP_TOKEN_FILE"],
        auth_defaults
            .setup_token
            .clone()
            .and_then(|v: String| normalize_secret_from_env(&v)),
        rotate_auth,
        48,
    )?;
    if setup_rotated {
        rotated_keys.push("FERREX_SETUP_TOKEN".to_string());
    }

    fs::create_dir_all(&cache_root)
        .context("failed to create cache directory")?;
    fs::create_dir_all(&cache_transcode)
        .context("failed to create transcode cache directory")?;
    fs::create_dir_all(&cache_thumbnails)
        .context("failed to create thumbnail cache directory")?;

    push("DEV_MODE", dev_mode.to_string());
    push("SERVER_HOST", server_host.clone());
    push("SERVER_PORT", server_port.to_string());
    push("FERREX_SERVER_URL", ferrex_server_url.clone());
    push("TMDB_API_KEY", tmdb_api_key.clone());
    push("ENFORCE_HTTPS", enforce_https.to_string());
    push("TRUST_PROXY_HEADERS", trust_proxy_headers.to_string());
    push("HSTS_MAX_AGE", hsts_max_age.to_string());
    push(
        "HSTS_INCLUDE_SUBDOMAINS",
        hsts_include_subdomains.to_string(),
    );
    push("HSTS_PRELOAD", hsts_preload.to_string());
    let tls_min_version = state.tls_min_version.clone();
    let tls_cipher_suites = state.tls_cipher_suites.clone();
    push("TLS_MIN_VERSION", tls_min_version);
    push("TLS_CIPHER_SUITES", tls_cipher_suites);

    push(
        "MEDIA_ROOT",
        media_root
            .map(|p: PathBuf| p.display().to_string())
            .unwrap_or_default(),
    );

    push("CACHE_DIR", cache_root.display().to_string());
    push("TRANSCODE_CACHE_DIR", cache_transcode.display().to_string());
    push(
        "THUMBNAIL_CACHE_DIR",
        cache_thumbnails.display().to_string(),
    );

    push("SQLX_OFFLINE", sqlx_offline);

    push("DATABASE_HOST", db_host.clone());
    push("DATABASE_HOST_CONTAINER", db_container_host_default.clone());
    push("DATABASE_PORT", db_port.to_string());
    push("DATABASE_NAME", db_name.clone());
    push("DATABASE_ADMIN_USER", db_admin_user.clone());
    push("DATABASE_ADMIN_PASSWORD", database_admin_password.clone());
    push("DATABASE_APP_USER", db_user.clone());
    push("DATABASE_APP_PASSWORD", database_app_password.clone());

    push("DATABASE_URL", database_url_host.clone());
    push("DATABASE_URL_ADMIN", database_url_admin.clone());
    push("DATABASE_URL_CONTAINER", database_url_container.clone());

    let postgres_initdb_args = existing_env
        .get("POSTGRES_INITDB_ARGS")
        .cloned()
        .unwrap_or_else(|| {
            "--auth-host=scram-sha-256 --auth-local=scram-sha-256".to_string()
        });
    let postgres_initdb_args = ensure_quoted_if_needed(&postgres_initdb_args);
    push("POSTGRES_INITDB_ARGS", postgres_initdb_args);

    push("REDIS_URL", redis_url_host.clone().unwrap_or_default());
    push(
        "REDIS_URL_CONTAINER",
        redis_url_container.clone().unwrap_or_default(),
    );
    let rate_limits_path = state.rate_limits_path.clone();
    let rate_limits_json = state.rate_limits_json.clone();
    push("RATE_LIMITS_PATH", rate_limits_path);
    push("RATE_LIMITS_JSON", rate_limits_json);

    let scanner_path = state.scanner_config_path.clone();
    let scanner_json = state.scanner_config_json.clone();
    push("SCANNER_CONFIG_PATH", scanner_path);
    push("SCANNER_CONFIG_JSON", scanner_json);

    let ffmpeg_path = state.ffmpeg_path.clone();
    let ffprobe_path = state.ffprobe_path.clone();
    push("FFMPEG_PATH", ffmpeg_path);
    push("FFPROBE_PATH", ffprobe_path);

    push("CORS_ALLOWED_ORIGINS", cors_origins.join(","));
    push("CORS_ALLOW_CREDENTIALS", allow_credentials.to_string());

    push("AUTH_PASSWORD_PEPPER", auth_password_pepper);
    push("AUTH_TOKEN_KEY", auth_token_key);
    push("FERREX_SETUP_TOKEN", setup_token);

    let demo_root = existing_env
        .get("FERREX_DEMO_ROOT")
        .cloned()
        .unwrap_or_else(|| "./demo".to_string());
    let demo_language = existing_env
        .get("FERREX_DEMO_LANGUAGE")
        .cloned()
        .unwrap_or_else(|| "us-EN".to_string());
    let demo_region = existing_env
        .get("FERREX_DEMO_REGION")
        .cloned()
        .unwrap_or_else(|| "US".to_string());

    let demo_mode = state.demo_mode.to_string();
    let demo_options = state.demo_options.clone();
    let demo_username = state.demo_username.clone();
    let demo_password = state.demo_password.clone();
    let demo_allow_deviations = state.demo_allow_deviations.clone();
    let demo_deviation_rate = state.demo_deviation_rate.clone();
    let demo_movie_count = state.demo_movie_count.clone();
    let demo_series_count = state.demo_series_count.clone();
    let demo_skip_metadata = state.demo_skip_metadata.clone();
    let demo_zero_length = state.demo_zero_length.clone();

    push("FERREX_DEMO_ROOT", demo_root);
    push("FERREX_DEMO_LANGUAGE", demo_language);
    push("FERREX_DEMO_REGION", demo_region);
    push("FERREX_DEMO_MODE", demo_mode);
    push("FERREX_DEMO_OPTIONS", demo_options);
    push("FERREX_DEMO_USERNAME", demo_username);
    push("FERREX_DEMO_PASSWORD", demo_password);
    push("FERREX_DEMO_ALLOW_DEVIATIONS", demo_allow_deviations);
    push("FERREX_DEMO_DEVIATION_RATE", demo_deviation_rate);
    push("FERREX_DEMO_MOVIE_COUNT", demo_movie_count);
    push("FERREX_DEMO_SERIES_COUNT", demo_series_count);
    push("FERREX_DEMO_SKIP_METADATA", demo_skip_metadata);
    push("FERREX_DEMO_ZERO_LENGTH", demo_zero_length);

    Ok(InitOutcome { kv, rotated_keys })
}

fn load_env_map(path: &Path) -> Result<HashMap<String, String>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let mut map = HashMap::new();

    let env_path = if path.eq(Path::new(".env")) {
        dotenvy::dotenv()?
    } else {
        path.to_path_buf()
    };

    for entry in dotenvy::from_path_iter(env_path)? {
        let (key, value) = entry?;
        map.insert(key, value);
    }

    Ok(map)
}

fn build_postgres_url(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    database: &str,
) -> Option<String> {
    let mut url =
        Url::parse(&format!("postgresql://{host}:{port}/{database}")).ok()?;
    url.set_username(user).ok()?;
    url.set_password(Some(password)).ok()?;
    Some(url.to_string())
}

fn resolve_secret_with_sources(
    existing_env: &HashMap<String, String>,
    key: &str,
    file_keys: &[&str],
    fallback: Option<String>,
    rotate: bool,
    len: usize,
) -> Result<(String, bool)> {
    if rotate {
        return Ok((generate_secret(len), true));
    }

    for file_key in file_keys {
        if let Some(path) = lookup_secret_path(existing_env, file_key)
            && let Some(secret) = read_secret_file(&path)?
            && !is_placeholder_secret(&secret)
        {
            return Ok((secret, false));
        }
    }

    if let Some(existing) = existing_env.get(key)
        && let Some(normalized) = normalize_secret_from_env(existing)
    {
        return Ok((normalized, false));
    }

    if let Some(fallback) = fallback
        && let Some(normalized) = normalize_secret_from_env(&fallback)
    {
        return Ok((normalized, false));
    }

    Ok((generate_secret(len), true))
}

fn lookup_secret_path(
    existing_env: &HashMap<String, String>,
    key: &str,
) -> Option<PathBuf> {
    if let Ok(val) = std::env::var(key)
        && !val.trim().is_empty()
    {
        return Some(PathBuf::from(val));
    }

    if let Some(val) = existing_env.get(key)
        && !val.trim().is_empty()
    {
        return Some(PathBuf::from(val));
    }

    None
}

fn read_secret_file(path: &Path) -> Result<Option<String>> {
    let contents =
        fs::read_to_string(path).context("failed to read secret file")?;
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
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

pub async fn run_config_check(opts: &CheckOptions) -> Result<()> {
    let loader = ConfigLoader::with_options(ConfigLoaderOptions {
        config_path: None,
        env_file: opts.env_file.clone(),
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

fn generate_secret(len: usize) -> String {
    if let Ok(seed_str) = std::env::var("FERREX_INIT_TEST_SEED")
        && let Ok(seed) = seed_str.parse::<u64>()
    {
        let seeded = StdRng::seed_from_u64(seed);
        return seeded
            .sample_iter(&Alphanumeric)
            .take(len)
            .map(char::from)
            .collect();
    }

    let thread_rng = rng();
    thread_rng
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn normalize_secret_from_env(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || is_placeholder_secret(trimmed) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_placeholder_secret(value: &str) -> bool {
    let normalized = value.trim();
    normalized.starts_with("changeme_")
}

fn is_placeholder_media_root(value: &str) -> bool {
    let normalized = value.trim();
    normalized == "/change/me"
}

fn ensure_quoted_if_needed(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    let needs_quotes = trimmed.contains(' ') || trimmed.contains('\t');
    if needs_quotes && !(trimmed.starts_with('"') && trimmed.ends_with('"')) {
        format!("\"{trimmed}\"")
    } else {
        trimmed.to_string()
    }
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
        .or_else(|_| std::env::var("DATABASE_NAME"))
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
