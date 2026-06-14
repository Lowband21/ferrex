use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};
use ferrex_model::rate_limit::{EndpointLimits, TrustedSources};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitSource {
    EnvPath(PathBuf),
    EnvInline,
    FilePath(PathBuf),
    FileInline(PathBuf),
}

#[derive(Debug, Clone)]
pub enum RateLimitSpec {
    Path(PathBuf),
    Inline(String),
}

/// Rate limiter configuration (shared between config and middleware)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimiterConfig {
    /// Endpoint-specific limits
    pub endpoint_limits: EndpointLimits,

    /// Trusted sources that bypass rate limiting
    pub trusted_sources: TrustedSources,

    /// Cache TTL for decisions
    pub cache_ttl: Duration,

    /// Enable distributed synchronization
    pub enable_sync: bool,

    /// Redis key prefix
    pub key_prefix: String,

    /// Clock skew tolerance
    pub clock_skew_tolerance: Duration,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            endpoint_limits: EndpointLimits::default(),
            trusted_sources: TrustedSources {
                ip_addresses: vec![],
                user_ids: vec![],
                device_ids: vec![],
            },
            cache_ttl: Duration::from_millis(100),
            enable_sync: true,
            key_prefix: "ferrex:ratelimit".to_string(),
            clock_skew_tolerance: Duration::from_secs(5),
        }
    }
}

impl RateLimitSpec {
    pub fn load_from_env(
        &self,
    ) -> anyhow::Result<(RateLimiterConfig, RateLimitSource)> {
        match self {
            RateLimitSpec::Path(path) => {
                let config = load_from_path(path)?;
                Ok((config, RateLimitSource::EnvPath(path.clone())))
            }
            RateLimitSpec::Inline(raw) => {
                let config = parse_inline(raw)?;
                Ok((config, RateLimitSource::EnvInline))
            }
        }
    }

    pub fn load_from_file(
        &self,
        config_path: &Path,
    ) -> anyhow::Result<(RateLimiterConfig, RateLimitSource)> {
        match self {
            RateLimitSpec::Path(path) => {
                let resolved = resolve_relative(config_path, path);
                let config = load_from_path(&resolved)?;
                Ok((config, RateLimitSource::FilePath(resolved)))
            }
            RateLimitSpec::Inline(raw) => {
                let config = parse_inline(raw)?;
                Ok((
                    config,
                    RateLimitSource::FileInline(config_path.to_path_buf()),
                ))
            }
        }
    }
}

fn resolve_relative(base: &Path, value: &Path) -> PathBuf {
    if value.is_relative() {
        let base_dir = base.parent().unwrap_or_else(|| Path::new("."));
        base_dir.join(value)
    } else {
        value.to_path_buf()
    }
}

fn load_from_path(path: &Path) -> anyhow::Result<RateLimiterConfig> {
    let raw = fs::read_to_string(path).with_context(|| {
        format!("failed to read rate limiter config from {}", path.display())
    })?;
    parse_config(&raw, Some(path))
}

fn parse_inline(raw: &str) -> anyhow::Result<RateLimiterConfig> {
    parse_config(raw, None)
}

fn parse_config(
    raw: &str,
    origin: Option<&Path>,
) -> anyhow::Result<RateLimiterConfig> {
    serde_json::from_str(raw)
        .or_else(|json_err| {
            toml::from_str(raw).map_err(|toml_err| {
                let origin = origin
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "inline".into());
                anyhow!(
                    "failed to parse rate limiter config {}: json error: {}; toml error: {}",
                    origin,
                    json_err,
                    toml_err
                )
            })
        })
        .or_else(|_| parse_lenient_duration_json(raw, origin))
}

fn parse_lenient_duration_json(
    raw: &str,
    origin: Option<&Path>,
) -> anyhow::Result<RateLimiterConfig> {
    let value: Value = serde_json::from_str(raw).with_context(|| {
        format!(
            "failed to parse rate limiter json from {}",
            origin_display(origin)
        )
    })?;
    let materialized = humantime_value::expand(value).context(
        "failed to normalize human-readable durations in rate limiter config",
    )?;
    serde_json::from_value(materialized).with_context(|| {
        format!("invalid rate limiter config in {}", origin_display(origin))
    })
}

fn origin_display(origin: Option<&Path>) -> String {
    origin
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "inline".to_string())
}

mod humantime_value {
    use std::time::Duration;

    use anyhow::{Context, Result};
    use serde_json::Value;

    pub fn expand(value: Value) -> Result<Value> {
        expand_recursive(value)
    }

    fn expand_object(mut map: serde_json::Map<String, Value>) -> Result<Value> {
        for (_, value) in map.iter_mut() {
            *value = expand_recursive(value.take())?;
        }
        Ok(Value::Object(map))
    }

    fn expand_array(mut items: Vec<Value>) -> Result<Value> {
        for item in items.iter_mut() {
            *item = expand_recursive(item.take())?;
        }
        Ok(Value::Array(items))
    }

    fn expand_recursive(value: Value) -> Result<Value> {
        match value {
            Value::String(s) => parse_duration(&s)
                .map(|duration| duration_to_value(&duration))
                .or(Ok(Value::String(s))),
            Value::Object(map) => expand_object(map),
            Value::Array(items) => expand_array(items),
            other => Ok(other),
        }
    }

    fn parse_duration(input: &str) -> Result<Duration> {
        humantime::parse_duration(input).context("failed to parse duration")
    }

    fn duration_to_value(duration: &Duration) -> Value {
        serde_json::json!({
            "secs": duration.as_secs(),
            "nanos": duration.subsec_nanos(),
        })
    }
}
