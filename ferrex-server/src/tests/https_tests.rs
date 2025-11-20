//! Comprehensive integration tests for HTTPS functionality
//! 
//! This module provides thorough testing of:
//! - HTTP to HTTPS redirect behavior
//! - HSTS header presence and configuration
//! - Reverse proxy header handling (X-Forwarded-Proto)
//! - TLS configuration validation
//! - Path and query parameter preservation
//! - Error handling for invalid configurations
//!
//! ## Test Coverage
//! 
//! ### Core HTTPS Functionality
//! 1. `test_http_redirects_to_https()` - Verifies HTTP requests redirect to HTTPS with 301 status
//! 2. `test_https_includes_hsts_header()` - Validates HSTS header presence and configuration
//! 3. `test_x_forwarded_proto_handling()` - Tests reverse proxy header trust behavior
//! 4. `test_preserve_path_in_redirect()` - Ensures URL paths are preserved during redirect
//! 5. `test_preserve_query_params()` - Ensures query parameters are preserved during redirect
//! 6. `test_invalid_tls_config_error()` - Tests TLS configuration error handling
//!
//! ### Configuration Testing
//! - HSTS configuration variations (disabled, custom max-age, preload flags)
//! - Proxy trust configuration testing
//! - HTTPS enforcement enable/disable testing
//!
//! ### Edge Cases and Error Handling
//! - Missing Host header scenarios
//! - Malformed X-Forwarded-Proto headers
//! - Case-insensitive header handling
//! - Various host formats (IPv4, IPv6, ports)
//! - URL encoding and special characters
//!
//! ### Performance and Load Testing
//! - Concurrent request handling
//! - High-load performance benchmarks

use crate::middleware::{HttpsConfig, HttpsEnforcementLayer};
use crate::tls::{TlsCertConfig, TlsConfigManager, TlsError};
use axum::{
    body::Body,
    http::{header, Request, Response, StatusCode},
    routing::get,
    Router,
};
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;
use tower::ServiceExt;

/// Create a test app with default HTTPS enforcement
fn create_test_app() -> Router {
    Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/test", get(|| async { "Test endpoint" }))
        .route("/api/v1/status", get(|| async { "API Status" }))
        .layer(HttpsEnforcementLayer::new())
}

/// Create a test app with custom HTTPS configuration
fn create_test_app_with_config(config: HttpsConfig) -> Router {
    Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/test", get(|| async { "Test endpoint" }))
        .layer(HttpsEnforcementLayer::with_config(config))
}

/// Test that HTTP requests are properly redirected to HTTPS
#[tokio::test]
async fn test_http_redirects_to_https() {
    let app = create_test_app();

    let request = Request::builder()
        .uri("http://example.com/test")
        .header("Host", "example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        response.headers().get("Location").unwrap(),
        "https://example.com/test"
    );
    assert_eq!(
        response.headers().get("X-Redirect-Reason").unwrap(),
        "HTTPS-Required"
    );
    
    // Verify empty body for redirect
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(body.is_empty());
}

/// Test that HTTPS requests include HSTS header with proper configuration
#[tokio::test]
async fn test_https_includes_hsts_header() {
    let app = create_test_app();

    let request = Request::builder()
        .uri("https://example.com/test")
        .header("Host", "example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    
    let hsts_header = response.headers().get("Strict-Transport-Security").unwrap();
    let hsts_value = hsts_header.to_str().unwrap();
    
    // Verify HSTS header contains expected directives
    assert!(hsts_value.contains("max-age=31536000"));
    assert!(hsts_value.contains("includeSubDomains"));
    
    // Verify security headers are present
    assert_eq!(response.headers().get("X-Content-Type-Options").unwrap(), "nosniff");
    assert_eq!(response.headers().get("X-Frame-Options").unwrap(), "DENY");
}

/// Test X-Forwarded-Proto header handling for reverse proxy scenarios
#[tokio::test]
async fn test_x_forwarded_proto_handling() {
    let app = create_test_app();

    // Request appears as HTTP but proxy indicates HTTPS
    let request = Request::builder()
        .uri("http://example.com/api/v1/status")
        .header("Host", "example.com")
        .header("X-Forwarded-Proto", "https")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should not redirect because proxy indicates HTTPS
    assert_eq!(response.status(), StatusCode::OK);
    
    // Should include HSTS header since we trust the proxy
    assert!(response.headers().contains_key("Strict-Transport-Security"));
    
    // Verify response body
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body, "API Status");
}

/// Test that paths are preserved in HTTPS redirects
#[tokio::test]
async fn test_preserve_path_in_redirect() {
    let app = create_test_app();

    let test_cases = vec![
        "/",
        "/test",
        "/api/v1/status",
        "/deeply/nested/path/with/segments",
        "/path-with-dashes",
        "/path_with_underscores",
        "/123/numeric/path",
    ];

    for path in test_cases {
        let uri = format!("http://example.com{}", path);
        let expected_redirect = format!("https://example.com{}", path);

        let request = Request::builder()
            .uri(uri)
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            response.headers().get("Location").unwrap(),
            expected_redirect
        );
    }
}

/// Test that query parameters are preserved in HTTPS redirects
#[tokio::test]
async fn test_preserve_query_params() {
    let app = create_test_app();

    let test_cases = vec![
        ("http://example.com/test?foo=bar", "https://example.com/test?foo=bar"),
        ("http://example.com/test?foo=bar&baz=qux", "https://example.com/test?foo=bar&baz=qux"),
        ("http://example.com/?search=rust+media+server", "https://example.com/?search=rust+media+server"),
        ("http://example.com/api?limit=10&offset=20", "https://example.com/api?limit=10&offset=20"),
        ("http://example.com/test?empty=&novalue", "https://example.com/test?empty=&novalue"),
    ];

    for (original_uri, expected_redirect) in test_cases {
        let request = Request::builder()
            .uri(original_uri)
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            response.headers().get("Location").unwrap(),
            expected_redirect
        );
    }
}

/// Test invalid TLS configuration error handling
#[tokio::test]
async fn test_invalid_tls_config_error() {
    // Test missing certificate file
    let config = TlsCertConfig {
        cert_path: PathBuf::from("/nonexistent/cert.pem"),
        key_path: PathBuf::from("/nonexistent/key.pem"),
        ..Default::default()
    };

    let result = TlsConfigManager::new(config).await;
    assert!(result.is_err());
    
    match result.unwrap_err() {
        TlsError::CertificateNotFound(path) => {
            assert_eq!(path, PathBuf::from("/nonexistent/cert.pem"));
        }
        _ => panic!("Expected CertificateNotFound error"),
    }
}

/// Test HSTS configuration variations
#[tokio::test]
async fn test_hsts_configuration_variations() {
    // Test HSTS disabled
    let config = HttpsConfig {
        enable_hsts: false,
        force_https: true,
        ..Default::default()
    };
    let app = create_test_app_with_config(config);

    let request = Request::builder()
        .uri("https://example.com/test")
        .header("Host", "example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("Strict-Transport-Security"));

    // Test custom HSTS configuration
    let config = HttpsConfig {
        enable_hsts: true,
        hsts_max_age: 86400, // 1 day
        hsts_include_subdomains: false,
        hsts_preload: true,
        ..Default::default()
    };
    let app = create_test_app_with_config(config);

    let request = Request::builder()
        .uri("https://example.com/test")
        .header("Host", "example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    
    let hsts_header = response.headers().get("Strict-Transport-Security").unwrap();
    let hsts_value = hsts_header.to_str().unwrap();
    
    assert!(hsts_value.contains("max-age=86400"));
    assert!(hsts_value.contains("preload"));
    assert!(!hsts_value.contains("includeSubDomains"));
}

/// Test proxy trust configuration
#[tokio::test]
async fn test_proxy_trust_configuration() {
    // Test with proxy trust disabled
    let config = HttpsConfig {
        trust_proxy: false,
        force_https: true,
        ..Default::default()
    };
    let app = create_test_app_with_config(config);

    let request = Request::builder()
        .uri("http://example.com/test")
        .header("Host", "example.com")
        .header("X-Forwarded-Proto", "https")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should redirect because we don't trust the proxy header
    assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        response.headers().get("Location").unwrap(),
        "https://example.com/test"
    );
}

/// Test HTTPS enforcement disabled
#[tokio::test]
async fn test_https_enforcement_disabled() {
    let config = HttpsConfig {
        force_https: false,
        enable_hsts: true,
        ..Default::default()
    };
    let app = create_test_app_with_config(config);

    let request = Request::builder()
        .uri("http://example.com/test")
        .header("Host", "example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should not redirect when HTTPS enforcement is disabled
    assert_eq!(response.status(), StatusCode::OK);
    
    // Should not have HSTS header for HTTP requests
    assert!(!response.headers().contains_key("Strict-Transport-Security"));
}

/// Test missing Host header handling
#[tokio::test]
async fn test_missing_host_header_error() {
    let app = create_test_app();

    let request = Request::builder()
        .uri("http://example.com/test")
        // Intentionally omit Host header
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body, "Invalid request");
}

/// Test malformed X-Forwarded-Proto header
#[tokio::test]
async fn test_malformed_forwarded_proto_header() {
    let app = create_test_app();

    // Test with invalid UTF-8 in header (simulated with unusual values)
    let request = Request::builder()
        .uri("http://example.com/test")
        .header("Host", "example.com")
        .header("X-Forwarded-Proto", "HTTP") // Should be case-insensitive
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should redirect because X-Forwarded-Proto is not "https"
    assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
}

/// Test case-insensitive X-Forwarded-Proto handling
#[tokio::test]
async fn test_case_insensitive_forwarded_proto() {
    let app = create_test_app();

    let test_cases = vec!["https", "HTTPS", "Https", "HTTPs"];

    for proto_value in test_cases {
        let request = Request::builder()
            .uri("http://example.com/test")
            .header("Host", "example.com")
            .header("X-Forwarded-Proto", proto_value)
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        // Should not redirect for any case variation of "https"
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("Strict-Transport-Security"));
    }
}

/// Test multiple host scenarios
#[tokio::test]
async fn test_different_host_scenarios() {
    let app = create_test_app();

    let test_cases = vec![
        ("localhost", "https://localhost/test"),
        ("example.com", "https://example.com/test"),
        ("sub.example.com", "https://sub.example.com/test"),
        ("example.com:8080", "https://example.com:8080/test"),
        ("192.168.1.1", "https://192.168.1.1/test"),
        ("[::1]", "https://[::1]/test"),
    ];

    for (host, expected_redirect) in test_cases {
        let request = Request::builder()
            .uri("http://placeholder/test") // URI host will be overridden by Host header
            .header("Host", host)
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            response.headers().get("Location").unwrap(),
            expected_redirect
        );
    }
}

/// Test concurrent requests handling
#[tokio::test]
async fn test_concurrent_requests() {
    let app = create_test_app();

    let mut handles = vec![];

    // Create 100 concurrent requests
    for i in 0..100 {
        let app_clone = app.clone();
        let handle = tokio::spawn(async move {
            let request = Request::builder()
                .uri(format!("http://example.com/test?id={}", i))
                .header("Host", "example.com")
                .body(Body::empty())
                .unwrap();

            app_clone.oneshot(request).await.unwrap()
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    let responses = futures::future::try_join_all(handles).await.unwrap();

    // Verify all responses are correct redirects
    for (i, response) in responses.into_iter().enumerate() {
        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            response.headers().get("Location").unwrap(),
            format!("https://example.com/test?id={}", i)
        );
    }
}

/// Test URL edge cases and special characters
#[tokio::test]
async fn test_url_edge_cases() {
    let app = create_test_app();

    let test_cases = vec![
        // Encoded characters
        ("http://example.com/test%20with%20spaces", "https://example.com/test%20with%20spaces"),
        ("http://example.com/test?q=hello%20world", "https://example.com/test?q=hello%20world"),
        // Fragment should not appear in Location header (fragments are client-side only)
        ("http://example.com/test#fragment", "https://example.com/test"),
        // Complex query parameters
        ("http://example.com/search?q=rust&lang=en&page=1", "https://example.com/search?q=rust&lang=en&page=1"),
    ];

    for (original_uri, expected_redirect) in test_cases {
        let request = Request::builder()
            .uri(original_uri)
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            response.headers().get("Location").unwrap(),
            expected_redirect
        );
    }
}

/// Test TLS configuration validation with temporary certificate files
#[tokio::test]
async fn test_tls_config_with_temp_certs() -> Result<(), Box<dyn std::error::Error>> {
    // Create temporary certificate files
    let temp_dir = TempDir::new()?;
    let cert_path = temp_dir.path().join("cert.pem");
    let key_path = temp_dir.path().join("key.pem");
    
    // Test certificate content (self-signed for testing)
    let test_cert = r#"-----BEGIN CERTIFICATE-----
<REDACTED>
-----END CERTIFICATE-----"#;

    let test_key = r#"-----BEGIN RSA PRIVATE KEY-----
<REDACTED>
-----END RSA PRIVATE KEY-----"#;

    // Write test certificate and key files
    fs::write(&cert_path, test_cert).await?;
    fs::write(&key_path, test_key).await?;

    // Test valid TLS configuration
    let config = TlsCertConfig {
        cert_path: cert_path.clone(),
        key_path: key_path.clone(),
        min_tls_version: "1.2".to_string(),
        ..Default::default()
    };

    let tls_manager = TlsConfigManager::new(config).await;
    assert!(tls_manager.is_ok());

    // Test with TLS 1.3 only
    let config_tls13 = TlsCertConfig {
        cert_path,
        key_path,
        min_tls_version: "1.3".to_string(),
        ..Default::default()
    };

    let tls_manager_13 = TlsConfigManager::new(config_tls13).await;
    assert!(tls_manager_13.is_ok());

    Ok(())
}

/// Test performance under high load
#[tokio::test]
async fn test_high_load_performance() {
    let app = create_test_app();
    let start_time = std::time::Instant::now();

    let mut handles = vec![];

    // Create 1000 concurrent requests to test performance
    for i in 0..1000 {
        let app_clone = app.clone();
        let handle = tokio::spawn(async move {
            let request = Request::builder()
                .uri(format!("http://example.com/test?load_test_id={}", i))
                .header("Host", "example.com")
                .body(Body::empty())
                .unwrap();

            let start = std::time::Instant::now();
            let response = app_clone.oneshot(request).await.unwrap();
            let duration = start.elapsed();
            
            (response.status(), duration)
        });
        handles.push(handle);
    }

    let results = futures::future::try_join_all(handles).await.unwrap();
    let total_duration = start_time.elapsed();

    // Verify all requests succeeded
    let mut total_request_time = std::time::Duration::ZERO;
    for (status, duration) in results {
        assert_eq!(status, StatusCode::MOVED_PERMANENTLY);
        total_request_time += duration;
    }

    // Performance assertions (adjust thresholds as needed)
    assert!(total_duration.as_millis() < 5000, "Total time should be under 5 seconds");
    
    let avg_request_time = total_request_time / 1000;
    assert!(avg_request_time.as_millis() < 10, "Average request time should be under 10ms");
}