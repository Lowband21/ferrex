use axum::http::{Method, header::HeaderName};
use thiserror::Error;

use super::models::{AuthConfig, Config, CorsConfig, RateLimiterSettings};

/// Errors returned when configuration violates non-negotiable safety checks.
#[derive(Debug, Error)]
pub enum ConfigGuardRailError {
    /// A required authentication secret is unset, too short, or still uses a placeholder.
    #[error("authentication secret {field} {reason}")]
    WeakSecret {
        /// The config field or environment variable name.
        field: &'static str,
        /// Why the secret was rejected.
        reason: String,
    },
    /// Wildcard CORS origins were enabled outside development mode.
    #[error("CORS wildcard origins are not allowed when DEV_MODE is false")]
    DangerousCorsWildcard,
    /// The CORS configuration is structurally invalid.
    #[error("invalid CORS configuration: {reason}")]
    InvalidCorsConfig {
        /// A human-readable explanation of the invalid setting.
        reason: String,
    },
    /// Rate limiting was enabled without a production-safe backend.
    #[error(
        "rate limiter configured but no supported backend is available in non-dev mode"
    )]
    MissingRateLimiterBackend,
}

/// A non-fatal configuration issue along with an optional remediation hint.
#[derive(Debug, Clone)]
pub struct ConfigWarning {
    /// Human-readable warning text.
    pub message: String,
    /// Optional follow-up guidance for fixing the warning.
    pub hint: Option<String>,
}

/// Collection of non-fatal configuration warnings gathered during validation.
#[derive(Debug, Default, Clone)]
pub struct ConfigWarnings {
    /// Warning items accumulated while checking config guard rails.
    pub items: Vec<ConfigWarning>,
}

impl ConfigWarnings {
    /// Appends a warning without additional remediation guidance.
    pub fn push<S: Into<String>>(&mut self, message: S) {
        self.items.push(ConfigWarning {
            message: message.into(),
            hint: None,
        });
    }

    /// Appends a warning paired with a suggested follow-up action.
    pub fn push_with_hint<S: Into<String>, H: Into<String>>(
        &mut self,
        message: S,
        hint: H,
    ) {
        self.items.push(ConfigWarning {
            message: message.into(),
            hint: Some(hint.into()),
        });
    }

    /// Returns true when no warnings were recorded.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Appends warnings from another collection.
    pub fn extend(&mut self, other: ConfigWarnings) {
        self.items.extend(other.items);
    }
}

/// Applies hard guard rails and advisory checks to the resolved runtime configuration.
pub fn apply_guard_rails(
    config: &Config,
) -> Result<ConfigWarnings, ConfigGuardRailError> {
    let mut warnings = ConfigWarnings::default();

    if !config.dev_mode {
        enforce_secret(&config.auth, &mut warnings)?;
        if config.cors.is_wildcard_included() {
            return Err(ConfigGuardRailError::DangerousCorsWildcard);
        }
    }

    validate_cors(&config.cors)?;

    if config.redis.is_none() {
        if !config.dev_mode && rate_limiter_configured(&config.rate_limiter) {
            // In production posture, do not allow an enabled limiter without a proper backend.
            return Err(ConfigGuardRailError::MissingRateLimiterBackend);
        }
        warnings.push_with_hint(
            "REDIS_URL not configured; Redis-backed rate limiting and caching are disabled",
            "Set REDIS_URL or disable limiter in prod; memory fallback is not allowed in non-dev",
        );
    } else if !rate_limiter_configured(&config.rate_limiter) {
        warnings.push_with_hint(
            "Rate limiter not configured; auth endpoints will run without throttling",
            "Provide RATE_LIMITS_PATH or RATE_LIMITS_JSON, or add a rate_limiter section to the config file",
        );
    }

    if config.security.enforce_https && !config.security.trust_proxy_headers {
        warnings.push_with_hint(
            "ENFORCE_HTTPS is true but TRUST_PROXY_HEADERS is false",
            "If TLS terminates at a proxy, enable TRUST_PROXY_HEADERS to respect X-Forwarded-*",
        );
    }

    if config.cors.allow_credentials && config.cors.is_wildcard_included() {
        warnings.push(
            "CORS credentials allowed alongside wildcard origin; browsers will reject such configuration",
        );
    }

    Ok(warnings)
}

fn enforce_secret(
    auth: &AuthConfig,
    warnings: &mut ConfigWarnings,
) -> Result<(), ConfigGuardRailError> {
    const MIN_SECRET_LENGTH: usize = 32;

    if auth.is_default_pepper() {
        return Err(ConfigGuardRailError::WeakSecret {
            field: "AUTH_PASSWORD_PEPPER",
            reason: "uses the default placeholder value".into(),
        });
    }

    if auth.password_pepper.len() < MIN_SECRET_LENGTH {
        return Err(ConfigGuardRailError::WeakSecret {
            field: "AUTH_PASSWORD_PEPPER",
            reason: format!("must be at least {MIN_SECRET_LENGTH} characters"),
        });
    }

    if auth.is_default_token_key() {
        return Err(ConfigGuardRailError::WeakSecret {
            field: "AUTH_TOKEN_KEY",
            reason: "uses the default placeholder value".into(),
        });
    }

    if auth.token_key.len() < MIN_SECRET_LENGTH {
        return Err(ConfigGuardRailError::WeakSecret {
            field: "AUTH_TOKEN_KEY",
            reason: format!("must be at least {MIN_SECRET_LENGTH} characters"),
        });
    }

    if auth.setup_token.is_none() {
        warnings.push_with_hint(
            "FERREX_SETUP_TOKEN not set; first admin creation endpoint is open",
            "Set FERREX_SETUP_TOKEN to gate initial admin provisioning",
        );
    }

    Ok(())
}

fn rate_limiter_configured(rate_limiter: &Option<RateLimiterSettings>) -> bool {
    rate_limiter.as_ref().map(|_| true).unwrap_or(false)
}

fn validate_cors(cors: &CorsConfig) -> Result<(), ConfigGuardRailError> {
    if cors.allowed_methods.is_empty() {
        return Err(ConfigGuardRailError::InvalidCorsConfig {
            reason:
                "CORS_ALLOWED_METHODS must include at least one HTTP method"
                    .into(),
        });
    }

    for method in &cors.allowed_methods {
        Method::from_bytes(method.as_bytes()).map_err(|_| {
            ConfigGuardRailError::InvalidCorsConfig {
                reason: format!(
                    "invalid HTTP method `{}` in CORS_ALLOWED_METHODS",
                    method
                ),
            }
        })?;
    }

    if cors.allowed_headers.is_empty() {
        return Err(ConfigGuardRailError::InvalidCorsConfig {
            reason:
                "CORS_ALLOWED_HEADERS must include at least one header name"
                    .into(),
        });
    }

    for header in &cors.allowed_headers {
        HeaderName::from_bytes(header.as_bytes()).map_err(|_| {
            ConfigGuardRailError::InvalidCorsConfig {
                reason: format!(
                    "invalid header name `{}` in CORS_ALLOWED_HEADERS",
                    header
                ),
            }
        })?;
    }

    Ok(())
}
