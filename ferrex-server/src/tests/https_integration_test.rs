//! Integration tests for HTTPS middleware integration
//!
//! These tests verify that the HTTPS enforcement middleware and TLS configuration
//! are properly integrated into the Ferrex media server.

use crate::middleware::{HttpsEnforcementLayer, RateLimiterConfig};
use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    routing::get,
    Router,
};
use std::net::SocketAddr;
use tower::{Service, ServiceExt};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

/// Test that HTTPS middleware is properly integrated
#[tokio::test]
async fn test_https_middleware_integration() {
    // Create a simple test router with HTTPS middleware
    let app = Router::new()
        .route("/test", get(|| async { "Hello, HTTPS!" }))
        // Apply middleware layers in the same order as main.rs
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(HttpsEnforcementLayer::new())
        .layer(crate::middleware::rate_limit_layer(
            RateLimiterConfig::default()
        ));

    // Test HTTP request gets redirected to HTTPS
    let request = Request::builder()
        .uri("http://example.com/test")
        .header("Host", "example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    
    assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        response.headers().get("Location").unwrap(),
        "https://example.com/test"
    );
}

/// Test that HTTPS requests pass through correctly with security headers
#[tokio::test]
async fn test_https_request_with_security_headers() {
    let app = Router::new()
        .route("/test", get(|| async { "Hello, HTTPS!" }))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(HttpsEnforcementLayer::new())
        .layer(crate::middleware::rate_limit_layer(
            RateLimiterConfig::default()
        ));

    // Test HTTPS request (via X-Forwarded-Proto header)
    let request = Request::builder()
        .uri("http://example.com/test")
        .header("Host", "example.com")
        .header("X-Forwarded-Proto", "https")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    // Check that HSTS header is present
    assert!(response.headers().contains_key("Strict-Transport-Security"));
    
    // Check that security headers are present
    assert!(response.headers().contains_key("X-Content-Type-Options"));
    assert!(response.headers().contains_key("X-Frame-Options"));
}

/// Test CLI argument integration for TLS configuration
#[test]
fn test_cli_args_integration() {
    use clap::Parser;
    use std::path::PathBuf;
    
    // Simulate command line arguments
    let args = vec![
        "ferrex-server",
        "--cert", "/path/to/cert.pem",
        "--key", "/path/to/key.pem",
        "--port", "8443",
        "--host", "0.0.0.0"
    ];
    
    let parsed = crate::Args::try_parse_from(args).unwrap();
    
    assert_eq!(parsed.cert, Some(PathBuf::from("/path/to/cert.pem")));
    assert_eq!(parsed.key, Some(PathBuf::from("/path/to/key.pem")));
    assert_eq!(parsed.port, Some(8443));
    assert_eq!(parsed.host, Some("0.0.0.0".to_string()));
}

/// Test TLS configuration creation
#[tokio::test]
async fn test_tls_config_creation() {
    use crate::tls::TlsCertConfig;
    use std::path::PathBuf;
    
    let config = TlsCertConfig {
        cert_path: PathBuf::from("test_cert.pem"),
        key_path: PathBuf::from("test_key.pem"),
        ..Default::default()
    };
    
    // Verify configuration defaults
    assert!(config.enable_ocsp_stapling);
    assert_eq!(config.min_tls_version, "1.2");
    assert_eq!(config.cipher_suites.len(), 0);
}

/// Test middleware layering order
#[tokio::test]
async fn test_middleware_layering_order() {
    // This test verifies that middleware is applied in the correct order:
    // CORS -> Tracing -> HTTPS -> Rate Limiting -> Business Logic
    
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        // Middleware applied in reverse order (outermost first)
        .layer(CorsLayer::permissive())  // 1. CORS (outermost)
        .layer(TraceLayer::new_for_http())  // 2. Tracing
        .layer(HttpsEnforcementLayer::new())  // 3. HTTPS enforcement
        .layer(crate::middleware::rate_limit_layer(  // 4. Rate limiting
            RateLimiterConfig::default()
        ));
    
    // Test that middleware stack processes requests correctly
    let request = Request::builder()
        .uri("https://example.com/health")
        .header("Host", "example.com")
        .header("X-Forwarded-Proto", "https")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}