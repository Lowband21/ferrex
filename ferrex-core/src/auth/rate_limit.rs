//! Rate limiting for authentication endpoints
//!
//! This module provides distributed rate limiting capabilities for authentication
//! operations with support for multiple algorithms and backends.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use uuid::Uuid;

/// Errors that can occur during rate limiting operations
#[derive(Debug, Error)]
pub enum RateLimitError {
    #[error("Rate limit exceeded: {reason}")]
    RateLimitExceeded {
        reason: String,
        retry_after: Duration,
        violations: u32,
    },

    #[error("Backend error: {0}")]
    BackendError(#[from] anyhow::Error),

    #[error("Invalid configuration: {0}")]
    ConfigError(String),
}

/// Result type for rate limiting operations
pub type RateLimitResult<T> = Result<T, RateLimitError>;

/// Decision returned by rate limiter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitDecision {
    /// Whether the request is allowed
    pub allowed: bool,

    /// Current request count in window
    pub current_count: u32,

    /// Maximum allowed requests
    pub limit: u32,

    /// Time until rate limit resets
    pub reset_after: Duration,

    /// Number of consecutive violations
    pub violation_count: u32,

    /// Additional metadata for monitoring
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Rate limiting algorithm type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum RateLimitAlgorithm {
    /// Sliding window log algorithm (most accurate)
    SlidingWindowLog,

    /// Token bucket algorithm (allows bursts)
    TokenBucket,

    /// Fixed window counter (simplest)
    FixedWindow,

    /// Leaky bucket algorithm (smooth rate)
    LeakyBucket,
}

/// Identifier for rate limiting (can be IP, user_id, device_id, etc.)
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateLimitKey {
    /// IP address based limiting
    IpAddress(String),

    /// User ID based limiting
    UserId(Uuid),

    /// Device ID based limiting
    DeviceId(Uuid),

    /// Combined key for more granular control
    Combined {
        ip: Option<String>,
        user_id: Option<Uuid>,
        device_id: Option<Uuid>,
    },

    /// Custom key for flexibility
    Custom(String),
}

impl RateLimitKey {
    /// Create a cache key for Redis
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

/// Configuration for rate limiting rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitRule {
    /// Name of the rule for identification
    pub name: String,

    /// Algorithm to use
    pub algorithm: RateLimitAlgorithm,

    /// Maximum number of requests allowed
    pub limit: u32,

    /// Time window for the limit
    pub window: Duration,

    /// Whether to apply exponential backoff on violations
    pub exponential_backoff: bool,

    /// Base duration for backoff calculation
    pub backoff_base: Duration,

    /// Maximum backoff duration
    pub max_backoff: Duration,

    /// Number of violations before applying stricter measures
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

/// Endpoint-specific rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointLimits {
    /// Login endpoint limits
    pub login: RateLimitRule,

    /// Registration endpoint limits
    pub register: RateLimitRule,

    /// Password reset limits
    pub password_reset: RateLimitRule,

    /// PIN authentication limits
    pub pin_auth: RateLimitRule,

    /// Device registration limits
    pub device_register: RateLimitRule,

    /// Token refresh limits
    pub token_refresh: RateLimitRule,
}

impl Default for EndpointLimits {
    fn default() -> Self {
        Self {
            login: RateLimitRule {
                name: "login".to_string(),
                limit: 5,
                window: Duration::from_secs(300), // 5 attempts per 5 minutes
                violation_threshold: 3,
                ..Default::default()
            },
            register: RateLimitRule {
                name: "register".to_string(),
                limit: 3,
                window: Duration::from_secs(3600), // 3 registrations per hour
                violation_threshold: 2,
                ..Default::default()
            },
            password_reset: RateLimitRule {
                name: "password_reset".to_string(),
                limit: 3,
                window: Duration::from_secs(3600), // 3 resets per hour
                violation_threshold: 2,
                ..Default::default()
            },
            pin_auth: RateLimitRule {
                name: "pin_auth".to_string(),
                limit: 10,
                window: Duration::from_secs(300), // 10 attempts per 5 minutes
                violation_threshold: 5,
                ..Default::default()
            },
            device_register: RateLimitRule {
                name: "device_register".to_string(),
                limit: 5,
                window: Duration::from_secs(86400), // 5 devices per day
                violation_threshold: 2,
                ..Default::default()
            },
            token_refresh: RateLimitRule {
                name: "token_refresh".to_string(),
                limit: 100,
                window: Duration::from_secs(3600), // 100 refreshes per hour
                exponential_backoff: false,
                ..Default::default()
            },
        }
    }
}

/// Rate limiter trait for implementing different backends
#[async_trait]
pub trait RateLimiter: Send + Sync {
    /// Check if a request is allowed and update counters
    async fn check_and_update(
        &self,
        key: &RateLimitKey,
        rule: &RateLimitRule,
    ) -> RateLimitResult<RateLimitDecision>;

    /// Reset rate limit for a specific key
    async fn reset(&self, key: &RateLimitKey) -> RateLimitResult<()>;

    /// Get current state without updating counters
    async fn get_current_state(
        &self,
        key: &RateLimitKey,
        rule: &RateLimitRule,
    ) -> RateLimitResult<RateLimitDecision>;

    /// Batch check multiple keys (for efficiency)
    async fn batch_check(
        &self,
        requests: Vec<(&RateLimitKey, &RateLimitRule)>,
    ) -> RateLimitResult<Vec<RateLimitDecision>>;

    /// Clean up expired entries (maintenance operation)
    async fn cleanup_expired(&self) -> RateLimitResult<u64>;
}

/// Metadata for rate limit violations (for monitoring)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationMetadata {
    pub key: RateLimitKey,
    pub endpoint: String,
    pub timestamp: SystemTime,
    pub violation_count: u32,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

/// Configuration for trusted sources that bypass rate limiting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedSources {
    /// Trusted IP addresses or CIDR blocks
    pub ip_addresses: Vec<String>,

    /// Trusted user IDs
    pub user_ids: Vec<Uuid>,

    /// Trusted device IDs
    pub device_ids: Vec<Uuid>,
}

impl TrustedSources {
    /// Check if a key is from a trusted source
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
                    || user_id.map(|u| self.user_ids.contains(&u)).unwrap_or(false)
                    || device_id
                        .map(|d| self.device_ids.contains(&d))
                        .unwrap_or(false)
            }
            RateLimitKey::Custom(_) => false,
        }
    }
}

/// Helper functions for calculating backoff durations
pub mod backoff {
    use super::*;

    /// Calculate exponential backoff duration
    pub fn exponential(base: Duration, violations: u32, max: Duration) -> Duration {
        let multiplier = 2_u32.saturating_pow(violations.saturating_sub(1));
        let backoff = base.saturating_mul(multiplier);
        backoff.min(max)
    }

    /// Calculate linear backoff duration
    pub fn linear(base: Duration, violations: u32, max: Duration) -> Duration {
        let backoff = base.saturating_mul(violations);
        backoff.min(max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_key_cache_key() {
        let ip_key = RateLimitKey::IpAddress("192.168.1.1".to_string());
        assert_eq!(ip_key.to_cache_key("auth"), "auth:ip:192.168.1.1");

        let user_key = RateLimitKey::UserId(Uuid::nil());
        assert_eq!(
            user_key.to_cache_key("auth"),
            format!("auth:user:{}", Uuid::nil())
        );
    }

    #[test]
    fn test_exponential_backoff() {
        let base = Duration::from_secs(60);
        let max = Duration::from_secs(3600);

        assert_eq!(backoff::exponential(base, 1, max), Duration::from_secs(60));
        assert_eq!(backoff::exponential(base, 2, max), Duration::from_secs(120));
        assert_eq!(backoff::exponential(base, 3, max), Duration::from_secs(240));
        assert_eq!(backoff::exponential(base, 10, max), max); // Should cap at max
    }
}
