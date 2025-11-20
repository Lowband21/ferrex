//! Integration tests for HTTPS enforcement middleware

use crate::middleware::{HttpsConfig, HttpsEnforcementLayer};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use tower::ServiceExt;

/// Create a test app with HTTPS enforcement
fn create_test_app() -> Router {
    Router::new()
        .route("/test", get(|| async { "Hello, HTTPS!" }))
        .layer(HttpsEnforcementLayer::new())
}

#[tokio::test]
async fn test_http_request_redirects_to_https() {
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
}

#[tokio::test]
async fn test_https_request_adds_hsts_header() {
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
    
    assert!(hsts_value.contains("max-age=31536000"));
    assert!(hsts_value.contains("includeSubDomains"));
}

#[tokio::test]
async fn test_proxy_forwarded_proto_https() {
    let app = create_test_app();

    // Request comes in as HTTP but proxy says it's HTTPS
    let request = Request::builder()
        .uri("http://example.com/test")
        .header("Host", "example.com")
        .header("X-Forwarded-Proto", "https")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("Strict-Transport-Security"));
}

#[tokio::test]
async fn test_custom_config_disabled_hsts() {
    let app = Router::new()
        .route("/test", get(|| async { "Hello!" }))
        .layer(HttpsEnforcementLayer::with_config(HttpsConfig {
            enable_hsts: false,
            force_https: false,
            ..Default::default()
        }));

    let request = Request::builder()
        .uri("http://example.com/test")
        .header("Host", "example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("Strict-Transport-Security"));
}

#[tokio::test]
async fn test_security_headers_added() {
    let app = create_test_app();

    let request = Request::builder()
        .uri("https://example.com/test")
        .header("Host", "example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.headers().get("X-Content-Type-Options").unwrap(), "nosniff");
    assert_eq!(response.headers().get("X-Frame-Options").unwrap(), "DENY");
}

#[tokio::test]
async fn test_preserve_query_parameters_in_redirect() {
    let app = create_test_app();

    let request = Request::builder()
        .uri("http://example.com/test?foo=bar&baz=qux")
        .header("Host", "example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        response.headers().get("Location").unwrap(),
        "https://example.com/test?foo=bar&baz=qux"
    );
}

#[tokio::test]
async fn test_missing_host_header_returns_bad_request() {
    let app = create_test_app();

    let request = Request::builder()
        .uri("http://example.com/test")
        // No Host header
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}