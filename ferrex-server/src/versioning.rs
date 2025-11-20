use axum::{
    extract::Request,
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents an API version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApiVersion {
    V1,
    // Future versions can be added here
    // V2,
}

impl ApiVersion {
    /// Get the URL path segment for this version
    pub fn path_segment(&self) -> &'static str {
        match self {
            ApiVersion::V1 => "v1",
            // ApiVersion::V2 => "v2",
        }
    }

    /// Parse a version string into an ApiVersion
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "v1" | "1" | "1.0" => Some(ApiVersion::V1),
            // "v2" | "2" | "2.0" => Some(ApiVersion::V2),
            _ => None,
        }
    }

    /// Check if this version is deprecated
    pub fn is_deprecated(&self) -> bool {
        match self {
            ApiVersion::V1 => false, // V1 is still active
            // ApiVersion::V2 => false,
        }
    }

    /// Get deprecation notice if applicable
    pub fn deprecation_notice(&self) -> Option<&'static str> {
        match self {
            ApiVersion::V1 => None,
            // When V2 is released, V1 might become deprecated:
            // ApiVersion::V1 => Some("API v1 is deprecated. Please migrate to v2."),
        }
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path_segment())
    }
}

impl Default for ApiVersion {
    fn default() -> Self {
        ApiVersion::V1
    }
}

/// Extension type for storing API version in request
#[derive(Debug, Clone, Copy)]
pub struct RequestApiVersion(pub ApiVersion);

/// Extract API version from Accept header
/// Format: application/vnd.ferrex.v1+json
fn extract_version_from_accept(accept: &str) -> Option<ApiVersion> {
    // Look for vendor-specific media type
    if accept.contains("application/vnd.ferrex.") {
        // Extract version from vendor type
        let parts: Vec<&str> = accept.split('.').collect();
        if parts.len() >= 3 {
            // Extract "v1" from "application/vnd.ferrex.v1+json"
            let version_part = parts[2].split('+').next()?;
            return ApiVersion::from_str(version_part);
        }
    }
    None
}

/// Middleware for API version negotiation
pub async fn version_middleware(
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    // Try to extract version from Accept header first
    let requested_version = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .and_then(extract_version_from_accept)
        .unwrap_or_default();

    // Store version in request extensions
    request.extensions_mut().insert(RequestApiVersion(requested_version));

    let mut response = next.run(request).await;

    // Add version headers to response
    let headers = response.headers_mut();
    headers.insert(
        "X-API-Version",
        requested_version.to_string().parse().unwrap(),
    );

    // Add deprecation header if applicable
    if let Some(notice) = requested_version.deprecation_notice() {
        headers.insert("X-API-Deprecation", notice.parse().unwrap());
        headers.insert("X-API-Deprecation-Date", "2025-12-31".parse().unwrap());
    }

    response
}

/// Helper function to add deprecation headers to a response
pub fn add_deprecation_headers(response: &mut Response, version: ApiVersion) {
    if let Some(notice) = version.deprecation_notice() {
        let headers = response.headers_mut();
        headers.insert("X-API-Deprecation", notice.parse().unwrap());
        headers.insert("Sunset", "2025-12-31T00:00:00Z".parse().unwrap());
    }
}

/// Response for unsupported API version
pub struct UnsupportedVersionError;

impl IntoResponse for UnsupportedVersionError {
    fn into_response(self) -> Response {
        (
            StatusCode::NOT_ACCEPTABLE,
            [("Content-Type", "application/json")],
            r#"{"error":"Unsupported API version","supported_versions":["v1"]}"#,
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        assert_eq!(ApiVersion::from_str("v1"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::from_str("V1"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::from_str("1"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::from_str("1.0"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::from_str("v3"), None);
    }

    #[test]
    fn test_accept_header_parsing() {
        assert_eq!(
            extract_version_from_accept("application/vnd.ferrex.v1+json"),
            Some(ApiVersion::V1)
        );
        assert_eq!(
            extract_version_from_accept("application/json"),
            None
        );
    }
}