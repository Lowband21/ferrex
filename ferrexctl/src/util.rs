use std::path::PathBuf;

use crate::models::rate_limits::RateLimitSpec;

pub fn parse_csv_var(name: &str) -> Option<Vec<String>> {
    std::env::var(name).ok().map(|raw| {
        raw.split(',')
            .filter_map(|part| {
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .collect()
    })
}

pub fn rate_limit_spec_from_env() -> Option<RateLimitSpec> {
    if let Ok(path) = std::env::var("RATE_LIMITS_PATH")
        && !path.trim().is_empty()
    {
        return Some(RateLimitSpec::Path(PathBuf::from(path)));
    }

    if let Ok(raw) = std::env::var("RATE_LIMITS_JSON") {
        if raw.trim().is_empty() {
            None
        } else {
            Some(RateLimitSpec::Inline(raw))
        }
    } else {
        None
    }
}

/// Parse a boolean value from a raw string, accepting common env-style forms.
///
/// Accepted truthy values (case-insensitive): `"1"`, `"true"`, `"yes"`, `"on"`.
/// Accepted falsy values: `"0"`, `"false"`, `"no"`, `"off"`.
pub fn parse_bool(raw: &str) -> Option<bool> {
    match raw.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn parse_bool_var(name: &str) -> Option<bool> {
    std::env::var(name).ok().and_then(|raw| parse_bool(&raw))
}
