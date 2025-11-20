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
    http::{header, HeaderValue, Response},
};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Layer, Service};

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
#[derive(Clone)]
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
            (31536000, false, false) => HeaderValue::from_static("max-age=31536000"),
            (31536000, true, false) => HeaderValue::from_static("max-age=31536000; includeSubDomains"),
            (31536000, false, true) => HeaderValue::from_static("max-age=31536000; preload"),
            (31536000, true, true) => HeaderValue::from_static("max-age=31536000; includeSubDomains; preload"),
            (63072000, false, false) => HeaderValue::from_static("max-age=63072000"), // 2 years
            (63072000, true, false) => HeaderValue::from_static("max-age=63072000; includeSubDomains"),
            (63072000, false, true) => HeaderValue::from_static("max-age=63072000; preload"),
            (63072000, true, true) => HeaderValue::from_static("max-age=63072000; includeSubDomains; preload"),
            _ => {
                // Dynamic header value for custom configurations
                let mut directives = vec![format!("max-age={}", config.max_age)];
                
                if config.include_subdomains {
                    directives.push("includeSubDomains".to_string());
                }
                
                if config.preload {
                    directives.push("preload".to_string());
                }

                HeaderValue::from_str(&directives.join("; "))
                    .unwrap_or_else(|_| HeaderValue::from_static("max-age=31536000"))
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
        HstsMiddleware {
            inner,
            header_value: self.header_value.clone(),
        }
    }
}

/// HSTS middleware
#[derive(Clone)]
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
        if let Some(proto) = req.headers().get("x-forwarded-proto") {
            if let Ok(proto_str) = proto.to_str() {
                return proto_str.eq_ignore_ascii_case("https");
            }
        }

        // Check X-Forwarded-Ssl header (used by some load balancers)
        if let Some(ssl) = req.headers().get("x-forwarded-ssl") {
            if let Ok(ssl_str) = ssl.to_str() {
                return ssl_str.eq_ignore_ascii_case("on");
            }
        }

        false
    }
}

impl<S> Service<Request<Body>> for HstsMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Send + Clone + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = Response<Body>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
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
                response.headers_mut().insert(
                    header::STRICT_TRANSPORT_SECURITY,
                    header_value,
                );
            }

            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, Response, StatusCode};
    use tower::{ServiceBuilder, ServiceExt};

    // Helper function to create a simple test service
    fn test_service() -> impl Service<Request<Body>, Response = Response<Body>, Error = Box<dyn std::error::Error + Send + Sync>> + Clone {
        tower::service_fn(|_req: Request<Body>| async {
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                Response::new(Body::from("Hello"))
            )
        })
    }

    #[tokio::test]
    async fn test_hsts_header_added_to_https_request() {
        let service = ServiceBuilder::new()
            .layer(HstsLayer::new())
            .service(test_service());

        let request = Request::builder()
            .uri("https://example.com/test")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("strict-transport-security"));
        assert_eq!(
            response.headers().get("strict-transport-security").unwrap(),
            "max-age=31536000"
        );
    }

    #[tokio::test]
    async fn test_hsts_header_not_added_to_http_request() {
        let service = ServiceBuilder::new()
            .layer(HstsLayer::new())
            .service(test_service());

        let request = Request::builder()
            .uri("http://example.com/test")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("strict-transport-security"));
    }

    #[tokio::test]
    async fn test_hsts_header_added_with_x_forwarded_proto() {
        let service = ServiceBuilder::new()
            .layer(HstsLayer::new())
            .service(test_service());

        let request = Request::builder()
            .uri("http://example.com/test")
            .header("Host", "example.com")
            .header("X-Forwarded-Proto", "https")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("strict-transport-security"));
        assert_eq!(
            response.headers().get("strict-transport-security").unwrap(),
            "max-age=31536000"
        );
    }

    #[tokio::test]
    async fn test_hsts_header_added_with_x_forwarded_ssl() {
        let service = ServiceBuilder::new()
            .layer(HstsLayer::new())
            .service(test_service());

        let request = Request::builder()
            .uri("http://example.com/test")
            .header("Host", "example.com")
            .header("X-Forwarded-Ssl", "on")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("strict-transport-security"));
    }

    #[tokio::test]
    async fn test_custom_config_with_include_subdomains() {
        let config = HstsConfig::new(63072000).with_include_subdomains();
        let service = ServiceBuilder::new()
            .layer(HstsLayer::with_config(config))
            .service(test_service());

        let request = Request::builder()
            .uri("https://example.com/test")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("strict-transport-security").unwrap(),
            "max-age=63072000; includeSubDomains"
        );
    }

    #[tokio::test]
    async fn test_custom_config_with_preload() {
        let config = HstsConfig::new(31536000).with_preload();
        let service = ServiceBuilder::new()
            .layer(HstsLayer::with_config(config))
            .service(test_service());

        let request = Request::builder()
            .uri("https://example.com/test")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("strict-transport-security").unwrap(),
            "max-age=31536000; preload"
        );
    }

    #[tokio::test]
    async fn test_custom_config_with_all_options() {
        let config = HstsConfig::new(31536000)
            .with_include_subdomains()
            .with_preload();
        let service = ServiceBuilder::new()
            .layer(HstsLayer::with_config(config))
            .service(test_service());

        let request = Request::builder()
            .uri("https://example.com/test")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("strict-transport-security").unwrap(),
            "max-age=31536000; includeSubDomains; preload"
        );
    }

    #[tokio::test]
    async fn test_custom_max_age() {
        let config = HstsConfig::new(7776000); // 90 days
        let service = ServiceBuilder::new()
            .layer(HstsLayer::with_config(config))
            .service(test_service());

        let request = Request::builder()
            .uri("https://example.com/test")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("strict-transport-security").unwrap(),
            "max-age=7776000"
        );
    }

    #[tokio::test]
    async fn test_case_insensitive_forwarded_proto() {
        let service = ServiceBuilder::new()
            .layer(HstsLayer::new())
            .service(test_service());

        let request = Request::builder()
            .uri("http://example.com/test")
            .header("Host", "example.com")
            .header("X-Forwarded-Proto", "HTTPS")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("strict-transport-security"));
    }

    #[tokio::test]
    async fn test_header_value_performance_optimization() {
        // Test that common configurations use static header values
        let config1 = HstsConfig::default();
        let layer1 = HstsLayer::with_config(config1);
        
        let config2 = HstsConfig::new(31536000).with_include_subdomains();
        let layer2 = HstsLayer::with_config(config2);
        
        // These should use HeaderValue::from_static for performance
        // We can't directly test the static nature, but we can verify the values are correct
        assert_eq!(layer1.header_value, HeaderValue::from_static("max-age=31536000"));
        assert_eq!(layer2.header_value, HeaderValue::from_static("max-age=31536000; includeSubDomains"));
    }
}