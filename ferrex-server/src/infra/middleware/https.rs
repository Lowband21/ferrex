//! HTTPS redirect middleware for Ferrex media server
//!
//! This module provides a focused HTTP to HTTPS redirect middleware that:
//! - Checks if request is HTTP (not HTTPS)
//! - Returns 301 redirect to HTTPS version
//! - Handles X-Forwarded-Proto header for reverse proxies
//! - Implements tower::Service trait for axum compatibility

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, Response, StatusCode, Uri, header},
};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Layer, Service};

/// Layer for HTTPS redirect middleware
#[derive(Clone, Debug)]
pub struct HttpsRedirectLayer;

impl Default for HttpsRedirectLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpsRedirectLayer {
    /// Create a new HTTPS redirect layer
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for HttpsRedirectLayer {
    type Service = HttpsRedirectMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpsRedirectMiddleware { inner }
    }
}

/// HTTPS redirect middleware
#[derive(Clone, Debug)]
pub struct HttpsRedirectMiddleware<S> {
    inner: S,
}

impl<S> HttpsRedirectMiddleware<S> {
    /// Check if the request is using HTTPS
    fn is_https(&self, req: &Request<Body>) -> bool {
        // Check direct connection scheme
        if req.uri().scheme_str() == Some("https") {
            return true;
        }

        // Check X-Forwarded-Proto header for reverse proxies
        if let Some(proto) = req.headers().get("x-forwarded-proto")
            && let Ok(proto_str) = proto.to_str()
        {
            return proto_str.eq_ignore_ascii_case("https");
        }

        false
    }

    /// Build HTTPS redirect URL preserving path and query
    fn build_https_url(
        &self,
        req: &Request<Body>,
    ) -> Result<Uri, Box<dyn std::error::Error + Send + Sync>> {
        let uri = req.uri();
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .ok_or("Missing Host header")?;

        // Build HTTPS URL preserving path and query
        let path_and_query =
            uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");

        let https_url = format!("https://{}{}", host, path_and_query);
        Ok(https_url.parse()?)
    }
}

impl<S> Service<Request<Body>> for HttpsRedirectMiddleware<S>
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
        // Check if request is HTTP (not HTTPS)
        if !self.is_https(&req) {
            // Build redirect URL
            match self.build_https_url(&req) {
                Ok(https_url) => {
                    let location = https_url.to_string();
                    match HeaderValue::from_str(&location) {
                        Ok(loc) => {
                            let mut response = Response::new(Body::empty());
                            *response.status_mut() =
                                StatusCode::MOVED_PERMANENTLY;
                            response
                                .headers_mut()
                                .insert(header::LOCATION, loc);
                            return Box::pin(async move { Ok(response) });
                        }
                        Err(_) => {
                            let mut response = Response::new(Body::from(
                                "Invalid redirect location",
                            ));
                            *response.status_mut() = StatusCode::BAD_REQUEST;
                            return Box::pin(async move { Ok(response) });
                        }
                    }
                }
                Err(_) => {
                    let mut response =
                        Response::new(Body::from("Invalid request"));
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Box::pin(async move { Ok(response) });
                }
            }
        }

        // Pass through HTTPS requests
        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await.map_err(Into::into) })
    }
}

// Legacy exports for backward compatibility
pub use HttpsRedirectLayer as HttpsEnforcementLayer;
pub use HttpsRedirectMiddleware as HttpsEnforcementMiddleware;

// Alternative implementation using axum middleware is now inlined in main.rs
// due to type inference issues with axum 0.7

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, Response, StatusCode};
    use tower::{ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn test_http_to_https_redirect() {
        let service = ServiceBuilder::new()
            .layer(HttpsRedirectLayer::new())
            .service_fn(|_req: Request<Body>| async {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                    Response::new(Body::from("Hello")),
                )
            });

        let request = Request::builder()
            .uri("http://example.com/test")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            response.headers().get("Location").unwrap(),
            "https://example.com/test"
        );
    }

    #[tokio::test]
    async fn test_https_passes_through() {
        let service = ServiceBuilder::new()
            .layer(HttpsRedirectLayer::new())
            .service_fn(|_req: Request<Body>| async {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                    Response::new(Body::from("Hello HTTPS")),
                )
            });

        let request = Request::builder()
            .uri("https://example.com/test")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_proxy_forwarded_proto_https() {
        let service = ServiceBuilder::new()
            .layer(HttpsRedirectLayer::new())
            .service_fn(|_req: Request<Body>| async {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                    Response::new(Body::from("Hello")),
                )
            });

        let request = Request::builder()
            .uri("http://example.com/test")
            .header("Host", "example.com")
            .header("X-Forwarded-Proto", "https")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();

        // Should not redirect because X-Forwarded-Proto says it's HTTPS
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_proxy_forwarded_proto_http() {
        let service = ServiceBuilder::new()
            .layer(HttpsRedirectLayer::new())
            .service_fn(|_req: Request<Body>| async {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                    Response::new(Body::from("Hello")),
                )
            });

        let request = Request::builder()
            .uri("http://example.com/test")
            .header("Host", "example.com")
            .header("X-Forwarded-Proto", "http")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();

        // Should redirect because X-Forwarded-Proto says it's HTTP
        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            response.headers().get("Location").unwrap(),
            "https://example.com/test"
        );
    }

    #[tokio::test]
    async fn test_redirect_preserves_path_and_query() {
        let service = ServiceBuilder::new()
            .layer(HttpsRedirectLayer::new())
            .service_fn(|_req: Request<Body>| async {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                    Response::new(Body::from("Hello")),
                )
            });

        let request = Request::builder()
            .uri("http://example.com/api/v1/users?page=1&limit=10")
            .header("Host", "example.com")
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            response.headers().get("Location").unwrap(),
            "https://example.com/api/v1/users?page=1&limit=10"
        );
    }

    #[tokio::test]
    async fn test_missing_host_header() {
        let service = ServiceBuilder::new()
            .layer(HttpsRedirectLayer::new())
            .service_fn(|_req: Request<Body>| async {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                    Response::new(Body::from("Hello")),
                )
            });

        let request = Request::builder()
            .uri("http://example.com/test")
            // No Host header
            .body(Body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();

        // Should return 400 Bad Request when Host header is missing
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
