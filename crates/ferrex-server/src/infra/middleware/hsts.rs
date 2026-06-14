//! HSTS (HTTP Strict Transport Security) middleware for Ferrex media server
//!
//! This module provides a tower middleware that:
//! - Adds Strict-Transport-Security header to HTTPS responses only
//! - Configurable max-age (default 1 year)
//! - Optional includeSubDomains directive
//! - Optional preload directive
//! - Uses HeaderValue::from_static for performance optimization
//! - Follows single responsibility principle

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, Response, header},
};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Layer, Service};
use tracing::debug;

/// Configuration for HSTS middleware
#[derive(Clone, Debug)]
pub struct HstsConfig {
    /// HSTS max-age in seconds (default: 31536000 = 1 year)
    pub max_age: u64,
    /// Include subdomains in HSTS policy
    pub include_subdomains: bool,
    /// Enable HSTS preload flag
    pub preload: bool,
}

impl Default for HstsConfig {
    fn default() -> Self {
        Self {
            max_age: 31536000, // 1 year in seconds
            include_subdomains: false,
            preload: false,
        }
    }
}

impl HstsConfig {
    /// Create a new HSTS config with custom max-age
    pub fn new(max_age: u64) -> Self {
        Self {
            max_age,
            ..Default::default()
        }
    }

    /// Enable includeSubDomains directive
    pub fn with_include_subdomains(mut self) -> Self {
        self.include_subdomains = true;
        self
    }

    /// Enable preload directive
    pub fn with_preload(mut self) -> Self {
        self.preload = true;
        self
    }
}

/// Layer for HSTS middleware
#[derive(Clone, Debug)]
pub struct HstsLayer {
    config: HstsConfig,
    header_value: HeaderValue,
}

impl HstsLayer {
    /// Create a new HSTS layer with default configuration
    pub fn new() -> Self {
        let config = HstsConfig::default();
        let header_value = Self::build_header_value(&config);
        Self {
            config,
            header_value,
        }
    }

    /// Create a new HSTS layer with custom configuration
    pub fn with_config(config: HstsConfig) -> Self {
        let header_value = Self::build_header_value(&config);
        Self {
            config,
            header_value,
        }
    }

    /// Build the HSTS header value from configuration
    /// Uses HeaderValue::from_static for common configurations for performance
    fn build_header_value(config: &HstsConfig) -> HeaderValue {
        // Common configurations using static strings for performance
        match (config.max_age, config.include_subdomains, config.preload) {
            (31536000, false, false) => {
                HeaderValue::from_static("max-age=31536000")
            }
            (31536000, true, false) => {
                HeaderValue::from_static("max-age=31536000; includeSubDomains")
            }
            (31536000, false, true) => {
                HeaderValue::from_static("max-age=31536000; preload")
            }
            (31536000, true, true) => HeaderValue::from_static(
                "max-age=31536000; includeSubDomains; preload",
            ),
            (63072000, false, false) => {
                HeaderValue::from_static("max-age=63072000")
            } // 2 years
            (63072000, true, false) => {
                HeaderValue::from_static("max-age=63072000; includeSubDomains")
            }
            (63072000, false, true) => {
                HeaderValue::from_static("max-age=63072000; preload")
            }
            (63072000, true, true) => HeaderValue::from_static(
                "max-age=63072000; includeSubDomains; preload",
            ),
            _ => {
                // Dynamic header value for custom configurations
                let mut directives =
                    vec![format!("max-age={}", config.max_age)];

                if config.include_subdomains {
                    directives.push("includeSubDomains".to_string());
                }

                if config.preload {
                    directives.push("preload".to_string());
                }

                HeaderValue::from_str(&directives.join("; ")).unwrap_or_else(
                    |_| HeaderValue::from_static("max-age=31536000"),
                )
            }
        }
    }
}

impl Default for HstsLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for HstsLayer {
    type Service = HstsMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        debug!(config = ?self.config, "installing HSTS middleware layer");
        HstsMiddleware {
            inner,
            header_value: self.header_value.clone(),
        }
    }
}

/// HSTS middleware
#[derive(Clone, Debug)]
pub struct HstsMiddleware<S> {
    inner: S,
    header_value: HeaderValue,
}

impl<S> HstsMiddleware<S> {
    /// Check if the request is using HTTPS
    fn is_https(&self, req: &Request<Body>) -> bool {
        // Check direct connection scheme
        if req.uri().scheme_str() == Some("https") {
            return true;
        }

        // Check X-Forwarded-Proto header for proxy scenarios
        if let Some(proto) = req.headers().get("x-forwarded-proto")
            && let Ok(proto_str) = proto.to_str()
        {
            return proto_str.eq_ignore_ascii_case("https");
        }

        // Check X-Forwarded-Ssl header (used by some load balancers)
        if let Some(ssl) = req.headers().get("x-forwarded-ssl")
            && let Ok(ssl_str) = ssl.to_str()
        {
            return ssl_str.eq_ignore_ascii_case("on");
        }

        false
    }
}

impl<S> Service<Request<Body>> for HstsMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>>
        + Send
        + Clone
        + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = Response<Body>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<
        Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let is_https = self.is_https(&req);

        // Clone the service and header value for use in the async block
        let mut inner = self.inner.clone();
        let header_value = self.header_value.clone();

        Box::pin(async move {
            // Call the inner service
            let mut response = inner.call(req).await.map_err(Into::into)?;

            // Add HSTS header only to HTTPS responses
            if is_https {
                response
                    .headers_mut()
                    .insert(header::STRICT_TRANSPORT_SECURITY, header_value);
            }

            Ok(response)
        })
    }
}
