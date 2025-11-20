//! Rate limit configuration and setup for server endpoints
//!
//! This module provides pre-configured rate limiters for different
//! endpoint categories to prevent abuse and ensure fair usage.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use ferrex_core::auth::rate_limit::RateLimitRule;
use std::pin::Pin;
use std::sync::Arc;
use tracing::{info, warn};

use crate::AppState;

/// Rate limit configuration for different endpoint categories
pub struct RateLimitConfig {
    /// Enable rate limiting globally
    pub enabled: bool,
    /// Redis URL for distributed rate limiting
    pub redis_url: Option<String>,
    /// Use in-memory rate limiting if Redis unavailable
    pub fallback_to_memory: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            redis_url: std::env::var("REDIS_URL").ok(),
            fallback_to_memory: true,
        }
    }
}

/// Apply rate limiting to authentication endpoints
pub fn apply_auth_rate_limits(
    router: Router<AppState>,
    config: &RateLimitConfig,
) -> Router<AppState> {
    if !config.enabled {
        info!("Rate limiting disabled for auth endpoints");
        return router;
    }

    // Since we can't filter on middleware layers, we need to apply rate limiting at route level
    // or handle filtering inside the middleware
    router
}

/// Apply rate limiting to public endpoints
pub fn apply_public_rate_limits(
    router: Router<AppState>,
    config: &RateLimitConfig,
) -> Router<AppState> {
    if !config.enabled {
        info!("Rate limiting disabled for public endpoints");
        return router;
    }

    // Since we can't filter on middleware layers, we need to apply rate limiting at route level
    // or handle filtering inside the middleware
    router
}

/// Apply rate limiting to API endpoints
pub fn apply_api_rate_limits(
    router: Router<AppState>,
    config: &RateLimitConfig,
) -> Router<AppState> {
    if !config.enabled {
        info!("Rate limiting disabled for API endpoints");
        return router;
    }

    // Since we can't filter on middleware layers, we need to apply rate limiting at route level
    // or handle filtering inside the middleware
    router
}

/// Create rate limit middleware for specific endpoint limits
fn rate_limit_middleware(
    limits: RateLimitRule,
) -> impl Fn(Request<Body>, Next) -> Pin<Box<dyn std::future::Future<Output = Response> + Send>>
+ Clone
+ Send
+ 'static {
    move |req: Request<Body>, next: Next| {
        let limits = limits.clone();
        Box::pin(async move {
            // Extract client identifier (IP address, user ID, etc.)
            let client_id = extract_client_id(&req);

            // Check rate limit
            match check_rate_limit(&client_id, &limits).await {
                Ok(true) => {
                    // Request allowed
                    next.run(req).await
                }
                Ok(false) => {
                    // Rate limit exceeded
                    warn!("Rate limit exceeded for client: {}", client_id);
                    rate_limit_exceeded_response(60) // Default to 60 seconds
                }
                Err(e) => {
                    // Rate limiter error - fail open but log
                    warn!("Rate limiter error: {}, allowing request", e);
                    next.run(req).await
                }
            }
        })
    }
}

/// Extract client identifier from request
fn extract_client_id(req: &Request<Body>) -> String {
    // Try to get authenticated user ID from extensions
    if let Some(user) = req.extensions().get::<ferrex_core::user::User>() {
        return format!("user:{}", user.id);
    }

    // Fall back to IP address
    if let Some(addr) = req.extensions().get::<std::net::SocketAddr>() {
        return format!("ip:{}", addr.ip());
    }

    // Last resort - use a header
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(ip) = forwarded.to_str() {
            return format!("ip:{}", ip.split(',').next().unwrap_or("unknown"));
        }
    }

    "unknown".to_string()
}

/// Check rate limit for client
async fn check_rate_limit(client_id: &str, limits: &RateLimitRule) -> Result<bool, String> {
    // TODO: Implement actual rate limiting logic
    // For now, this is a placeholder that always allows requests
    // In production, this would check against Redis or in-memory store

    // Example implementation outline:
    // 1. Connect to Redis or use in-memory store
    // 2. Check current request count for client_id
    // 3. Apply sliding window or token bucket algorithm
    // 4. Return true if under limit, false if exceeded

    Ok(true)
}

/// Create response for rate limit exceeded
fn rate_limit_exceeded_response(window_seconds: u64) -> Response {
    let retry_after = window_seconds.to_string();

    (
        StatusCode::TOO_MANY_REQUESTS,
        [
            ("retry-after", retry_after.as_str()),
            ("x-ratelimit-limit", "exceeded"),
        ],
        axum::Json(serde_json::json!({
            "error": "rate_limit_exceeded",
            "message": format!("Too many requests. Please try again in {} seconds.", window_seconds),
            "retry_after": window_seconds,
        }))
    ).into_response()
}

/// In-memory rate limiter for fallback when Redis unavailable
pub struct InMemoryRateLimiter {
    requests: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<std::time::Instant>>>>,
}

impl Default for InMemoryRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryRateLimiter {
    pub fn new() -> Self {
        Self {
            requests: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn check_limit(&self, key: &str, limit: u32, window: std::time::Duration) -> bool {
        let mut requests = self.requests.write().await;
        let now = std::time::Instant::now();

        // Get or create entry for this key
        let timestamps = requests.entry(key.to_string()).or_insert_with(Vec::new);

        // Remove old timestamps outside the window
        timestamps.retain(|&t| now.duration_since(t) < window);

        // Check if under limit
        if timestamps.len() < limit as usize {
            timestamps.push(now);
            true
        } else {
            false
        }
    }

    /// Clean up old entries periodically
    pub async fn cleanup(&self) {
        let mut requests = self.requests.write().await;
        let now = std::time::Instant::now();

        // Remove entries that haven't been used in the last hour
        requests.retain(|_, timestamps| {
            timestamps
                .iter()
                .any(|&t| now.duration_since(t) < std::time::Duration::from_secs(3600))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_rate_limiter() {
        let limiter = InMemoryRateLimiter::new();
        let key = "test_client";
        let limit = 3;
        let window = std::time::Duration::from_secs(1);

        // First 3 requests should pass
        for i in 1..=3 {
            assert!(
                limiter.check_limit(key, limit, window).await,
                "Request {} should be allowed",
                i
            );
        }

        // 4th request should fail
        assert!(
            !limiter.check_limit(key, limit, window).await,
            "Request 4 should be denied"
        );

        // Wait for window to expire
        tokio::time::sleep(window).await;

        // Should be able to make requests again
        assert!(
            limiter.check_limit(key, limit, window).await,
            "Request after window should be allowed"
        );
    }

    #[test]
    fn test_extract_client_id() {
        // Test with no identifiers
        let req = Request::builder()
            .uri("/api/test")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_id(&req), "unknown");

        // Test with X-Forwarded-For header
        let req = Request::builder()
            .uri("/api/test")
            .header("x-forwarded-for", "192.168.1.1, 10.0.0.1")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_id(&req), "ip:192.168.1.1");
    }
}
