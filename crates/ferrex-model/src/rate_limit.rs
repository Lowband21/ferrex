use std::time::Duration;

use uuid::Uuid;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Rate limiting algorithm type.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RateLimitAlgorithm {
    /// Sliding window log algorithm (most accurate).
    SlidingWindowLog,
    /// Token bucket algorithm (allows bursts).
    TokenBucket,
    /// Fixed window counter (simplest).
    FixedWindow,
    /// Leaky bucket algorithm (smooth rate).
    LeakyBucket,
}

/// Identifier for rate limiting (IP, user_id, device_id, etc.).
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RateLimitKey {
    /// IP address based limiting.
    IpAddress(String),
    /// User ID based limiting.
    UserId(Uuid),
    /// Device ID based limiting.
    DeviceId(Uuid),
    /// Combined key for more granular control.
    Combined {
        ip: Option<String>,
        user_id: Option<Uuid>,
        device_id: Option<Uuid>,
    },
    /// Custom key for flexibility.
    Custom(String),
}

impl RateLimitKey {
    /// Create a cache key for Redis.
    pub fn to_cache_key(&self, namespace: &str) -> String {
        match self {
            Self::IpAddress(ip) => format!("{}:ip:{}", namespace, ip),
            Self::UserId(id) => format!("{}:user:{}", namespace, id),
            Self::DeviceId(id) => format!("{}:device:{}", namespace, id),
            Self::Combined {
                ip,
                user_id,
                device_id,
            } => {
                let parts: Vec<String> = vec![
                    ip.as_ref().map(|i| format!("ip:{}", i)),
                    user_id.map(|u| format!("user:{}", u)),
                    device_id.map(|d| format!("device:{}", d)),
                ]
                .into_iter()
                .flatten()
                .collect();
                format!("{}:combined:{}", namespace, parts.join(":"))
            }
            Self::Custom(key) => format!("{}:custom:{}", namespace, key),
        }
    }
}

/// Configuration for a single rate limiting rule.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct RateLimitRule {
    /// Name of the rule for identification.
    pub name: String,
    /// Algorithm to use.
    pub algorithm: RateLimitAlgorithm,
    /// Maximum number of requests allowed.
    pub limit: u32,
    /// Time window for the limit.
    pub window: Duration,
    /// Whether to apply exponential backoff on violations.
    pub exponential_backoff: bool,
    /// Base duration for backoff calculation.
    pub backoff_base: Duration,
    /// Maximum backoff duration.
    pub max_backoff: Duration,
    /// Number of violations before applying stricter measures.
    pub violation_threshold: u32,
}

impl Default for RateLimitRule {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            algorithm: RateLimitAlgorithm::SlidingWindowLog,
            limit: 10,
            window: Duration::from_secs(60),
            exponential_backoff: true,
            backoff_base: Duration::from_secs(60),
            max_backoff: Duration::from_secs(3600),
            violation_threshold: 3,
        }
    }
}

/// Endpoint-specific rate limit configuration.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct EndpointLimits {
    /// Login endpoint limits.
    pub login: RateLimitRule,
    /// Registration endpoint limits.
    pub register: RateLimitRule,
    /// Password reset limits.
    pub password_reset: RateLimitRule,
    /// PIN authentication limits.
    pub pin_auth: RateLimitRule,
    /// Device registration limits.
    pub device_register: RateLimitRule,
    /// Token refresh limits.
    pub token_refresh: RateLimitRule,
    /// Setup claim start limits (LAN-only endpoint).
    pub setup_start: RateLimitRule,
    /// Setup claim confirm limits (LAN-only endpoint).
    pub setup_confirm: RateLimitRule,
    /// Setup create admin limits.
    pub setup_create_admin: RateLimitRule,
}

impl Default for EndpointLimits {
    fn default() -> Self {
        Self {
            login: RateLimitRule {
                name: "login".to_string(),
                limit: 5,
                window: Duration::from_secs(300),
                violation_threshold: 3,
                ..Default::default()
            },
            register: RateLimitRule {
                name: "register".to_string(),
                limit: 3,
                window: Duration::from_secs(3600),
                violation_threshold: 2,
                ..Default::default()
            },
            password_reset: RateLimitRule {
                name: "password_reset".to_string(),
                limit: 3,
                window: Duration::from_secs(3600),
                violation_threshold: 2,
                ..Default::default()
            },
            pin_auth: RateLimitRule {
                name: "pin_auth".to_string(),
                limit: 10,
                window: Duration::from_secs(300),
                violation_threshold: 5,
                ..Default::default()
            },
            device_register: RateLimitRule {
                name: "device_register".to_string(),
                limit: 5,
                window: Duration::from_secs(86400),
                violation_threshold: 2,
                ..Default::default()
            },
            token_refresh: RateLimitRule {
                name: "token_refresh".to_string(),
                limit: 100,
                window: Duration::from_secs(3600),
                exponential_backoff: false,
                ..Default::default()
            },
            setup_start: RateLimitRule {
                name: "setup_start".to_string(),
                limit: 5,
                window: Duration::from_secs(120),
                violation_threshold: 3,
                ..Default::default()
            },
            setup_confirm: RateLimitRule {
                name: "setup_confirm".to_string(),
                limit: 5,
                window: Duration::from_secs(120),
                violation_threshold: 3,
                ..Default::default()
            },
            setup_create_admin: RateLimitRule {
                name: "setup_create_admin".to_string(),
                limit: 2,
                window: Duration::from_secs(3600),
                violation_threshold: 1,
                ..Default::default()
            },
        }
    }
}

/// Configuration for trusted sources that bypass rate limiting.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TrustedSources {
    /// Trusted IP addresses or CIDR blocks.
    pub ip_addresses: Vec<String>,
    /// Trusted user IDs.
    pub user_ids: Vec<Uuid>,
    /// Trusted device IDs.
    pub device_ids: Vec<Uuid>,
}

impl TrustedSources {
    /// Check if a key is from a trusted source.
    pub fn is_trusted(&self, key: &RateLimitKey) -> bool {
        match key {
            RateLimitKey::IpAddress(ip) => self.ip_addresses.contains(ip),
            RateLimitKey::UserId(id) => self.user_ids.contains(id),
            RateLimitKey::DeviceId(id) => self.device_ids.contains(id),
            RateLimitKey::Combined {
                ip,
                user_id,
                device_id,
            } => {
                ip.as_ref()
                    .map(|i| self.ip_addresses.contains(i))
                    .unwrap_or(false)
                    || user_id
                        .map(|u| self.user_ids.contains(&u))
                        .unwrap_or(false)
                    || device_id
                        .map(|d| self.device_ids.contains(&d))
                        .unwrap_or(false)
            }
            RateLimitKey::Custom(_) => false,
        }
    }
}
