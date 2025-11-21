//! Distributed rate limiting middleware for authentication endpoints
//!
//! This module implements Redis-backed rate limiting with support for
//! multiple algorithms and dynamic configuration updates.

use anyhow::{Context, Result};
use async_trait::async_trait;
pub use ferrex_config::RateLimiterConfig;
use ferrex_core::domain::users::auth::rate_limit::{
    EndpointLimits, RateLimitAlgorithm, RateLimitDecision, RateLimitError,
    RateLimitKey, RateLimitResult, RateLimitRule, RateLimiter, TrustedSources,
    backoff,
};
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{RwLock, broadcast},
    time::interval,
};
use tracing::{debug, info, warn};

/// Redis scripts for atomic operations
mod scripts {
    use redis::Script;

    /// Sliding window log implementation
    pub fn sliding_window_log() -> Script {
        Script::new(
            r#"
            local key = KEYS[1]
            local now = tonumber(ARGV[1])
            local window = tonumber(ARGV[2])
            local limit = tonumber(ARGV[3])

            -- Remove old entries
            redis.call('ZREMRANGEBYSCORE', key, 0, now - window)

            -- Count current entries
            local current = redis.call('ZCARD', key)

            if current < limit then
                -- Add new entry
                redis.call('ZADD', key, now, now)
                redis.call('EXPIRE', key, window)
                return {1, current + 1, limit}
            else
                -- Get oldest entry for reset time calculation
                local oldest = redis.call('ZRANGE', key, 0, 0, 'WITHSCORES')
                local reset_time = oldest[2] and (oldest[2] + window - now) or window
                return {0, current, limit, reset_time}
            end
            "#,
        )
    }

    /// Token bucket implementation
    pub fn token_bucket() -> Script {
        Script::new(
            r#"
            local key = KEYS[1]
            local now = tonumber(ARGV[1])
            local rate = tonumber(ARGV[2])
            local capacity = tonumber(ARGV[3])
            local requested = tonumber(ARGV[4]) or 1

            local bucket = redis.call('HMGET', key, 'tokens', 'last_update')
            local tokens = tonumber(bucket[1]) or capacity
            local last_update = tonumber(bucket[2]) or now

            -- Calculate tokens to add
            local elapsed = math.max(0, now - last_update)
            local generated = elapsed * rate
            tokens = math.min(capacity, tokens + generated)

            if tokens >= requested then
                tokens = tokens - requested
                redis.call('HMSET', key, 'tokens', tokens, 'last_update', now)
                redis.call('EXPIRE', key, capacity / rate)
                return {1, tokens, capacity}
            else
                local wait_time = (requested - tokens) / rate
                return {0, tokens, capacity, wait_time}
            end
            "#,
        )
    }
}

/// Redis-backed distributed rate limiter
pub struct RedisRateLimiter {
    /// Redis connection manager
    redis: ConnectionManager,

    /// In-memory cache for hot paths
    cache: Arc<RwLock<HashMap<String, CachedDecision>>>,

    /// Configuration
    config: Arc<RwLock<RateLimiterConfig>>,

    /// Metrics collector
    metrics: Arc<RwLock<RateLimitMetrics>>,

    /// Channel for configuration updates
    update_tx: broadcast::Sender<ConfigUpdate>,
}

impl fmt::Debug for RedisRateLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisRateLimiter").finish()
    }
}

/// Cached rate limit decision
#[derive(Debug, Clone)]
struct CachedDecision {
    decision: RateLimitDecision,
    expires_at: SystemTime,
}

/// Configuration update message
#[derive(Debug, Clone, Serialize, Deserialize)]
enum ConfigUpdate {
    EndpointLimits(EndpointLimits),
    TrustedSources(TrustedSources),
    DynamicRule {
        endpoint: String,
        rule: RateLimitRule,
    },
}

/// Rate limit metrics
#[derive(Debug, Default)]
struct RateLimitMetrics {
    /// Total requests checked
    total_requests: u64,

    /// Requests allowed
    allowed_requests: u64,

    /// Requests denied
    denied_requests: u64,

    /// Cache hits
    cache_hits: u64,

    /// Cache misses
    cache_misses: u64,

    /// Violations by endpoint
    violations_by_endpoint: HashMap<String, u64>,

    /// Average check latency in microseconds
    avg_check_latency_us: u64,
}

impl RedisRateLimiter {
    /// Create a new Redis-backed rate limiter
    pub async fn new(
        redis_url: &str,
        config: RateLimiterConfig,
    ) -> Result<Self> {
        let client = redis::Client::open(redis_url)
            .context("Failed to create Redis client")?;

        let redis = ConnectionManager::new(client)
            .await
            .context("Failed to create Redis connection manager")?;

        let (update_tx, _) = broadcast::channel(100);

        let limiter = Self {
            redis,
            cache: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(RwLock::new(config)),
            metrics: Arc::new(RwLock::new(RateLimitMetrics::default())),
            update_tx,
        };

        // Start background tasks
        limiter.start_background_tasks();

        Ok(limiter)
    }

    /// Start background maintenance tasks
    fn start_background_tasks(&self) {
        let cache = Arc::clone(&self.cache);
        let metrics = Arc::clone(&self.metrics);
        let config_updates = Arc::clone(&self.config);
        let cache_for_updates = Arc::clone(&self.cache);
        let mut update_rx = self.update_tx.subscribe();

        // Cache cleanup task
        tokio::spawn(async move {
            let mut cleanup_interval = interval(Duration::from_secs(60));

            loop {
                cleanup_interval.tick().await;

                let now = SystemTime::now();
                let mut cache_guard = cache.write().await;
                let before_size = cache_guard.len();

                cache_guard.retain(|_, v| v.expires_at > now);

                let removed = before_size - cache_guard.len();
                if removed > 0 {
                    debug!(
                        "Rate limiter cache cleanup: removed {} expired entries",
                        removed
                    );
                }
            }
        });

        // Metrics reporting task
        tokio::spawn(async move {
            let mut report_interval = interval(Duration::from_secs(300)); // 5 minutes

            loop {
                report_interval.tick().await;

                let metrics_guard = metrics.read().await;
                info!(
                    "Rate limiter metrics - Total: {}, Allowed: {}, Denied: {}, Cache hit rate: {:.2}%",
                    metrics_guard.total_requests,
                    metrics_guard.allowed_requests,
                    metrics_guard.denied_requests,
                    if metrics_guard.total_requests > 0 {
                        (metrics_guard.cache_hits as f64
                            / metrics_guard.total_requests as f64)
                            * 100.0
                    } else {
                        0.0
                    }
                );

                if !metrics_guard.violations_by_endpoint.is_empty() {
                    warn!(
                        "Violations by endpoint: {:?}",
                        metrics_guard.violations_by_endpoint
                    );
                }
            }
        });

        // Configuration update task
        tokio::spawn(async move {
            while let Ok(update) = update_rx.recv().await {
                debug!(?update, "Applying rate limiter configuration update");
                {
                    let mut guard = config_updates.write().await;
                    match update {
                        ConfigUpdate::EndpointLimits(limits) => {
                            guard.endpoint_limits = limits;
                        }
                        ConfigUpdate::TrustedSources(sources) => {
                            guard.trusted_sources = sources;
                        }
                        ConfigUpdate::DynamicRule { endpoint, rule } => {
                            apply_dynamic_rule(
                                &mut guard.endpoint_limits,
                                &endpoint,
                                rule,
                            );
                        }
                    }
                }

                // Drop cached decisions so future checks pick up new configuration
                cache_for_updates.write().await.clear();
            }
        });
    }
    /// Get cache key with namespace
    fn get_cache_key(
        &self,
        key: &RateLimitKey,
        rule: &RateLimitRule,
    ) -> String {
        format!(
            "{}:{}:{}",
            self.config.blocking_read().key_prefix,
            rule.name,
            key.to_cache_key(&rule.name)
        )
    }

    /// Check cache for decision
    async fn check_cache(&self, cache_key: &str) -> Option<RateLimitDecision> {
        let cache_guard = self.cache.read().await;

        if let Some(cached) = cache_guard.get(cache_key)
            && cached.expires_at > SystemTime::now()
        {
            let mut metrics = self.metrics.write().await;
            metrics.cache_hits += 1;
            return Some(cached.decision.clone());
        }

        let mut metrics = self.metrics.write().await;
        metrics.cache_misses += 1;
        None
    }

    /// Update cache with decision
    async fn update_cache(
        &self,
        cache_key: String,
        decision: RateLimitDecision,
    ) {
        let config = self.config.read().await;
        let expires_at = SystemTime::now() + config.cache_ttl;

        let mut cache_guard = self.cache.write().await;
        cache_guard.insert(
            cache_key,
            CachedDecision {
                decision,
                expires_at,
            },
        );
    }

    /// Record violation for monitoring
    async fn record_violation(&self, key: &RateLimitKey, endpoint: &str) {
        let mut metrics = self.metrics.write().await;

        let counter = metrics
            .violations_by_endpoint
            .entry(endpoint.to_string())
            .or_insert(0);
        *counter += 1;

        // TODO: Send to monitoring system
        warn!(
            "Rate limit violation - Endpoint: {}, Key: {:?}",
            endpoint, key
        );
    }
}

#[async_trait]
impl RateLimiter for RedisRateLimiter {
    async fn check_and_update(
        &self,
        key: &RateLimitKey,
        rule: &RateLimitRule,
    ) -> RateLimitResult<RateLimitDecision> {
        let start_time = std::time::Instant::now();

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.total_requests += 1;
        }

        // Check if trusted source
        let config = self.config.read().await;
        if config.trusted_sources.is_trusted(key) {
            let decision = RateLimitDecision {
                allowed: true,
                current_count: 0,
                limit: rule.limit,
                reset_after: Duration::from_secs(0),
                violation_count: 0,
                metadata: HashMap::new(),
            };

            let mut metrics = self.metrics.write().await;
            metrics.allowed_requests += 1;

            return Ok(decision);
        }
        drop(config);

        let cache_key = self.get_cache_key(key, rule);

        // Check cache first
        if let Some(cached) = self.check_cache(&cache_key).await {
            return Ok(cached);
        }

        // Perform Redis operation based on algorithm
        let redis_key = cache_key.clone();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut conn = self.redis.clone();

        let result = match rule.algorithm {
            RateLimitAlgorithm::SlidingWindowLog => {
                let script = scripts::sliding_window_log();
                script
                    .arg(now)
                    .arg(rule.window.as_secs())
                    .arg(rule.limit)
                    .key(&redis_key)
                    .invoke_async::<Vec<i64>>(&mut conn)
                    .await
                    .map_err(|e| RateLimitError::BackendError(e.into()))?
            }
            RateLimitAlgorithm::TokenBucket => {
                let rate = rule.limit as f64 / rule.window.as_secs_f64();
                let script = scripts::token_bucket();
                script
                    .arg(now)
                    .arg(rate)
                    .arg(rule.limit)
                    .arg(1)
                    .key(&redis_key)
                    .invoke_async::<Vec<i64>>(&mut conn)
                    .await
                    .map_err(|e| RateLimitError::BackendError(e.into()))?
            }
            _ => {
                // Fallback to simple counter for other algorithms
                let count: i64 = conn
                    .incr(&redis_key, 1)
                    .await
                    .map_err(|e| RateLimitError::BackendError(e.into()))?;

                if count == 1 {
                    conn.expire::<_, ()>(
                        &redis_key,
                        rule.window.as_secs() as i64,
                    )
                    .await
                    .map_err(|e| RateLimitError::BackendError(e.into()))?;
                }

                if count <= rule.limit as i64 {
                    vec![1, count, rule.limit as i64]
                } else {
                    let ttl: i64 = conn
                        .ttl(&redis_key)
                        .await
                        .map_err(|e| RateLimitError::BackendError(e.into()))?;
                    vec![0, count, rule.limit as i64, ttl.max(0)]
                }
            }
        };

        // Parse result
        let allowed = result[0] == 1;
        let current_count = result[1] as u32;
        let limit = result[2] as u32;
        let reset_after = if !allowed && result.len() > 3 {
            Duration::from_secs(result[3] as u64)
        } else {
            rule.window
        };

        // Check violations for exponential backoff
        let violation_count = if !allowed {
            let violation_key = format!("{}:violations", redis_key);
            let count: u32 = conn
                .incr(&violation_key, 1)
                .await
                .map_err(|e| RateLimitError::BackendError(e.into()))?;
            conn.expire::<_, ()>(&violation_key, 86400)
                .await // 24 hours
                .map_err(|e| RateLimitError::BackendError(e.into()))?;
            count
        } else {
            0
        };

        // Calculate actual reset time with backoff
        let actual_reset_after = if !allowed
            && rule.exponential_backoff
            && violation_count > rule.violation_threshold
        {
            backoff::exponential(
                rule.backoff_base,
                violation_count - rule.violation_threshold,
                rule.max_backoff,
            )
        } else {
            reset_after
        };

        let decision = RateLimitDecision {
            allowed,
            current_count,
            limit,
            reset_after: actual_reset_after,
            violation_count,
            metadata: HashMap::new(),
        };

        // Update cache
        self.update_cache(cache_key, decision.clone()).await;

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            if allowed {
                metrics.allowed_requests += 1;
            } else {
                metrics.denied_requests += 1;
            }

            let elapsed = start_time.elapsed().as_micros() as u64;
            metrics.avg_check_latency_us = (metrics.avg_check_latency_us
                * (metrics.total_requests - 1)
                + elapsed)
                / metrics.total_requests;
        }

        if !allowed {
            self.record_violation(key, &rule.name).await;

            return Err(RateLimitError::RateLimitExceeded {
                reason: format!(
                    "Exceeded {} requests per {:?}",
                    rule.limit, rule.window
                ),
                retry_after: actual_reset_after,
                violations: violation_count,
            });
        }

        Ok(decision)
    }

    async fn reset(&self, key: &RateLimitKey) -> RateLimitResult<()> {
        let config = self.config.read().await;
        let pattern =
            format!("{}:*:{}", config.key_prefix, key.to_cache_key("*"));

        let mut conn = self.redis.clone();

        // Find all keys matching the pattern
        let keys: Vec<String> = conn
            .keys(&pattern)
            .await
            .map_err(|e| RateLimitError::BackendError(e.into()))?;

        // Delete all matching keys
        if !keys.is_empty() {
            conn.del::<_, ()>(keys)
                .await
                .map_err(|e| RateLimitError::BackendError(e.into()))?;
        }

        // Clear from cache
        let mut cache_guard = self.cache.write().await;
        cache_guard.retain(|k, _| !k.contains(&key.to_cache_key("")));

        Ok(())
    }

    async fn get_current_state(
        &self,
        key: &RateLimitKey,
        rule: &RateLimitRule,
    ) -> RateLimitResult<RateLimitDecision> {
        let cache_key = self.get_cache_key(key, rule);

        // Check cache first
        if let Some(cached) = self.check_cache(&cache_key).await {
            return Ok(cached);
        }

        // Query Redis without updating
        let redis_key = cache_key.clone();
        let mut conn = self.redis.clone();

        let current_count: u32 = match rule.algorithm {
            RateLimitAlgorithm::SlidingWindowLog => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                conn.zcount(
                    &redis_key,
                    (now - rule.window.as_secs()).to_string(),
                    "+inf",
                )
                .await
                .map_err(|e| RateLimitError::BackendError(e.into()))?
            }
            _ => conn.get(&redis_key).await.unwrap_or(0),
        };

        let ttl: i64 = conn
            .ttl(&redis_key)
            .await
            .map_err(|e| RateLimitError::BackendError(e.into()))?;

        let decision = RateLimitDecision {
            allowed: current_count < rule.limit,
            current_count,
            limit: rule.limit,
            reset_after: Duration::from_secs(ttl.max(0) as u64),
            violation_count: 0,
            metadata: HashMap::new(),
        };

        Ok(decision)
    }

    async fn batch_check(
        &self,
        requests: Vec<(&RateLimitKey, &RateLimitRule)>,
    ) -> RateLimitResult<Vec<RateLimitDecision>> {
        // Use pipelining for efficiency
        let mut decisions = Vec::with_capacity(requests.len());

        for (key, rule) in requests {
            // Check each one (could be optimized with Lua scripts)
            let decision = self.check_and_update(key, rule).await?;
            decisions.push(decision);
        }

        Ok(decisions)
    }

    async fn cleanup_expired(&self) -> RateLimitResult<u64> {
        let config = self.config.read().await;
        let pattern = format!("{}:*", config.key_prefix);

        let mut conn = self.redis.clone();
        let mut cleaned = 0u64;

        // Scan for keys (use SCAN in production, not KEYS)
        let mut cursor = 0;
        loop {
            let (new_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut conn)
                .await
                .map_err(|e| RateLimitError::BackendError(e.into()))?;

            // Check TTL for each key
            for key in keys {
                let ttl: i64 = conn
                    .ttl(&key)
                    .await
                    .map_err(|e| RateLimitError::BackendError(e.into()))?;

                if ttl == -1 {
                    // No expiry set, add one
                    conn.expire::<_, ()>(&key, 3600)
                        .await
                        .map_err(|e| RateLimitError::BackendError(e.into()))?;
                    cleaned += 1;
                }
            }

            cursor = new_cursor;
            if cursor == 0 {
                break;
            }
        }

        Ok(cleaned)
    }
}

fn apply_dynamic_rule(
    limits: &mut EndpointLimits,
    endpoint: &str,
    rule: RateLimitRule,
) {
    match endpoint {
        "login" => limits.login = rule,
        "register" => limits.register = rule,
        "password_reset" => limits.password_reset = rule,
        "pin_auth" => limits.pin_auth = rule,
        "device_register" => limits.device_register = rule,
        "token_refresh" => limits.token_refresh = rule,
        other => warn!("Unknown rate limit endpoint '{other}'"),
    }
}

/// Create endpoint-specific rate limiter
pub fn create_rate_limiter(
    redis_url: &str,
    config: RateLimiterConfig,
) -> Result<Arc<dyn RateLimiter>> {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let limiter = RedisRateLimiter::new(redis_url, config).await?;
            Ok(Arc::new(limiter) as Arc<dyn RateLimiter>)
        })
    })
}
