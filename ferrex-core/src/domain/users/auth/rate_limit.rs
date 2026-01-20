//! Rate limiting for authentication endpoints
//!
//! This module provides distributed rate limiting capabilities for authentication
//! operations with support for multiple algorithms and backends.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use thiserror::Error;

pub use crate::types::rate_limit::{
    EndpointLimits, RateLimitAlgorithm, RateLimitKey, RateLimitRule,
    TrustedSources,
};

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

/// Helper functions for calculating backoff durations
pub mod backoff {
    use super::*;

    /// Calculate exponential backoff duration
    pub fn exponential(
        base: Duration,
        violations: u32,
        max: Duration,
    ) -> Duration {
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

        let user_key = RateLimitKey::UserId(uuid::Uuid::nil());
        assert_eq!(
            user_key.to_cache_key("auth"),
            format!("auth:user:{}", uuid::Uuid::nil())
        );
    }

    #[test]
    fn test_exponential_backoff() {
        let base = Duration::from_secs(60);
        let max = Duration::from_secs(3600);

        assert_eq!(backoff::exponential(base, 1, max), Duration::from_secs(60));
        assert_eq!(
            backoff::exponential(base, 2, max),
            Duration::from_secs(120)
        );
        assert_eq!(
            backoff::exponential(base, 3, max),
            Duration::from_secs(240)
        );
        assert_eq!(backoff::exponential(base, 10, max), max); // Should cap at max
    }
}
