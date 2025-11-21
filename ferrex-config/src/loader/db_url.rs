use std::{fs::read_to_string, path::Path};

use url::Url;

use crate::{
    Config,
    ConfigLoadError,
    models::sources::{EnvConfig, FileDatabaseConfig},
};

/// Source of an effective PostgreSQL connection URL.
#[derive(Debug, Clone, Copy)]
pub enum DatabaseUrlSource {
    /// URL came directly from the composed configuration (e.g. `DATABASE_URL`).
    Config,
    /// URL was derived from environment fallbacks such as `PGDATABASE` /
    /// `DATABASE_NAME` when no explicit URL was provided.
    Env,
}

pub fn resolve_database_url(
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
        && let Some(url) = read_secret_file(path)?
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
                resolve_database_password(env, file_database)?
        {
            parsed.set_password(Some(&password)).map_err(|_| {
                ConfigLoadError::InvalidDatabasePassword
            })?;
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
        let mut url = Url::parse(&format!("postgresql://{host}:{port}/{name}"))
            .map_err(|source| ConfigLoadError::InvalidDatabaseUrl {
                source,
            })?;
        url.set_username(&user).map_err(|_| {
            ConfigLoadError::InvalidDatabaseUsername {
                username: user.clone(),
            }
        })?;
        if let Some(password) = resolve_database_password(env, file_database)? {
            url.set_password(Some(&password)).map_err(|_| {
                ConfigLoadError::InvalidDatabasePassword
            })?;
        }
        return Ok(Some(url.to_string()));
    }

    Ok(None)
}

pub fn resolve_database_password(
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
        if let Some(secret) = read_secret_file(path)?
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

pub fn read_secret_file(
    path: &Path,
) -> Result<Option<String>, ConfigLoadError> {
    let contents = read_to_string(path).map_err(|source| {
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

/// Resolve the effective PostgreSQL connection URL and the source used.
///
/// This first prefers `config.database.primary_url` as produced by
/// [`ConfigLoader`]. When that is absent or empty, it falls back to
/// environment-only configuration by checking `PGDATABASE` and then
/// `DATABASE_NAME`, returning a simple `postgresql:///<db>` URL when set.
pub fn resolve_effective_database_url_with_source(
    config: &Config,
) -> Option<(String, DatabaseUrlSource)> {
    if let Some(url) = config
        .database
        .primary_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        return Some((url.to_owned(), DatabaseUrlSource::Config));
    }

    let database = std::env::var("PGDATABASE")
        .or_else(|_| std::env::var("DATABASE_NAME"))
        .ok()?
        .trim()
        .to_owned();

    if database.is_empty() {
        return None;
    }

    Some((format!("postgresql:///{database}"), DatabaseUrlSource::Env))
}

/// Resolve the effective PostgreSQL connection URL, ignoring the source.
pub fn resolve_effective_database_url(config: &Config) -> Option<String> {
    resolve_effective_database_url_with_source(config).map(|(url, _)| url)
}
