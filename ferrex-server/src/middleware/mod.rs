pub mod csrf;
pub mod hsts;
/// Middleware modules for the Ferrex media server
///
/// This module contains all middleware implementations including:
/// - HTTPS enforcement and TLS security
/// - HSTS (HTTP Strict Transport Security) headers
/// - Request/response logging
/// - Rate limiting
/// - Security headers
pub mod https;
pub mod rate_limit;
pub mod rate_limit_setup;

pub use csrf::{
    create_csrf_cookie, extract_csrf_from_cookies, generate_token, hash_token, CsrfLayer,
    CsrfMiddleware, ValidateCsrf,
};
pub use hsts::{HstsConfig, HstsLayer, HstsMiddleware};
pub use https::{
    HttpsEnforcementLayer, HttpsEnforcementMiddleware, HttpsRedirectLayer, HttpsRedirectMiddleware,
};
pub use rate_limit::{create_rate_limiter, RateLimiterConfig};
