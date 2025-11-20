//! Integration tests for HSTS middleware

#[cfg(test)]
mod hsts_tests {
    use crate::middleware::hsts::{HstsLayer, HstsConfig};
    use axum::{body::Body, http::{Request, Response, StatusCode}};
    use tower::{ServiceBuilder, ServiceExt};

    // Helper function to create a simple test service
    fn test_service() -> impl tower::Service<Request<Body>, Response = Response<Body>, Error = Box<dyn std::error::Error + Send + Sync>> + Clone {
        tower::service_fn(|_req: Request<Body>| async {
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                Response::new(Body::from("Hello"))
            )
        })
    }

    #[tokio::test]
    async fn test_hsts_middleware_integration() {
        let service = ServiceBuilder::new()
            .layer(HstsLayer::new())
            .service(test_service());

        // Test HTTPS request - should add HSTS header
        let https_request = Request::builder()
            .uri("https://example.com/test")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.clone().oneshot(https_request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("strict-transport-security"));
        assert_eq!(
            response.headers().get("strict-transport-security").unwrap(),
            "max-age=31536000"
        );

        // Test HTTP request - should not add HSTS header
        let http_request = Request::builder()
            .uri("http://example.com/test")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(http_request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("strict-transport-security"));
    }

    #[tokio::test]
    async fn test_hsts_with_custom_config() {
        let config = HstsConfig::new(63072000) // 2 years
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
            "max-age=63072000; includeSubDomains; preload"
        );
    }

    #[tokio::test]
    async fn test_hsts_with_x_forwarded_proto() {
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
    }
}