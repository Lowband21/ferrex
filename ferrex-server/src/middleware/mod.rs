/// Middleware modules for the Ferrex media server
/// 
/// This module contains all middleware implementations including:
/// - HTTPS enforcement and TLS security
/// - HSTS (HTTP Strict Transport Security) headers
/// - Request/response logging
/// - Rate limiting
/// - Security headers

pub mod https;
pub mod hsts;
pub mod rate_limit;
pub mod rate_limit_setup;
pub mod csrf;

pub use https::{HttpsRedirectLayer, HttpsRedirectMiddleware, HttpsEnforcementLayer, HttpsEnforcementMiddleware};
pub use hsts::{HstsLayer, HstsMiddleware, HstsConfig};
pub use rate_limit::{create_rate_limiter, RateLimiterConfig};
pub use csrf::{CsrfLayer, CsrfMiddleware, ValidateCsrf, generate_token, hash_token, create_csrf_cookie, extract_csrf_from_cookies};