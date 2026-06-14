use axum::{
    body::Body,
    extract::FromRequestParts,
    http::{HeaderMap, Request, Response, StatusCode, request::Parts},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rng};
use sha2::{Digest, Sha256};

/// Generates a cryptographically secure 32-byte CSRF token
pub fn generate_token() -> String {
    let mut token_bytes = [0u8; 32];
    rng().fill_bytes(&mut token_bytes);
    URL_SAFE_NO_PAD.encode(token_bytes)
}

/// Hashes a CSRF token with SHA256 for secure storage
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

pub fn create_csrf_cookie(token: &str) -> String {
    format!(
        "csrf_token={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=3600",
        token
    )
}

pub fn extract_csrf_from_cookies(headers: &HeaderMap) -> Option<String> {
    headers
        .get("cookie")?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|cookie| {
            let parts: Vec<&str> = cookie.trim().splitn(2, '=').collect();
            if parts.len() == 2 && parts[0] == "csrf_token" {
                Some(parts[1].to_string())
            } else {
                None
            }
        })
}

use std::{
    fmt,
    future::ready,
    task::{Context, Poll},
};
use tower::Layer;

#[derive(Clone, Debug)]
pub struct CsrfLayer;

impl<S> Layer<S> for CsrfLayer {
    type Service = CsrfMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CsrfMiddleware { inner }
    }
}

#[derive(Clone)]
pub struct CsrfMiddleware<S> {
    inner: S,
}

impl<S> fmt::Debug for CsrfMiddleware<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CsrfMiddleware").finish()
    }
}

impl<S> tower::Service<Request<Body>> for CsrfMiddleware<S>
where
    S: tower::Service<Request<Body>, Response = Response<Body>>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        // For now, just pass through - validation logic will be added later
        self.inner.call(req)
    }
}

#[derive(Debug)]
pub struct ValidateCsrf;

impl<S> FromRequestParts<S> for ValidateCsrf
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    fn from_request_parts(
        _parts: &mut Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send
    {
        ready(Ok(ValidateCsrf))
    }
}
