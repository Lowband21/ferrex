//! Content negotiation for multi-format responses (JSON, rkyv, FlatBuffers).
//!
//! # Usage
//!
//! ```rust,ignore
//! use crate::infra::content_negotiation::{AcceptFormat, NegotiatedResponse};
//!
//! async fn list_libraries(
//!     State(state): State<AppState>,
//!     accept: AcceptFormat,
//! ) -> NegotiatedResponse {
//!     let libraries = /* ... */;
//!     // For FlatBuffers: serialize with the dedicated serializer
//!     // For JSON: serialize with serde_json via ApiResponse wrapper
//!     // For rkyv: serialize with rkyv (existing path)
//!     accept.respond_flatbuffers_or_json(fb_bytes, &json_data)
//! }
//! ```

use axum::{
    extract::FromRequestParts,
    http::{
        StatusCode,
        header::{ACCEPT, CONTENT_TYPE},
        request::Parts,
    },
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// Recognized wire formats, ordered by preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptFormat {
    /// `application/x-flatbuffers` — zero-copy mobile format
    FlatBuffers,
    /// `application/x-rkyv` — zero-copy desktop format (existing)
    Rkyv,
    /// `application/json` — fallback / debug
    Json,
}

/// MIME type constants.
pub mod mime {
    pub const FLATBUFFERS: &str = "application/x-flatbuffers";
    pub const RKYV: &str = "application/x-rkyv";
    pub const JSON: &str = "application/json";
}

impl<S> FromRequestParts<S> for AcceptFormat
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let accept = parts
            .headers
            .get(ACCEPT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // Parse Accept header — check for our custom types first, then fall
        // back to JSON. We do simple substring matching since these MIME types
        // are unambiguous and the full quality-value parsing isn't needed.
        let format = if accept.contains(mime::FLATBUFFERS) {
            AcceptFormat::FlatBuffers
        } else if accept.contains(mime::RKYV) {
            AcceptFormat::Rkyv
        } else {
            // Default to JSON for browser, curl, and unknown clients
            AcceptFormat::Json
        };

        Ok(format)
    }
}

impl AcceptFormat {
    /// Content-Type header value for the selected format.
    pub fn content_type(&self) -> &'static str {
        match self {
            AcceptFormat::FlatBuffers => mime::FLATBUFFERS,
            AcceptFormat::Rkyv => mime::RKYV,
            AcceptFormat::Json => mime::JSON,
        }
    }
}

/// A response that carries pre-serialized bytes with the correct Content-Type.
#[derive(Debug)]
pub struct NegotiatedResponse {
    pub status: StatusCode,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl NegotiatedResponse {
    /// Create a 200 OK response with FlatBuffers bytes.
    pub fn flatbuffers(bytes: Vec<u8>) -> Self {
        Self {
            status: StatusCode::OK,
            content_type: mime::FLATBUFFERS,
            body: bytes,
        }
    }

    /// Create a 200 OK response with JSON-serialized data.
    pub fn json<T: Serialize>(data: &T) -> Self {
        let body = serde_json::to_vec(data).unwrap_or_else(|_| b"{}".to_vec());
        Self {
            status: StatusCode::OK,
            content_type: mime::JSON,
            body,
        }
    }

    /// Create a 200 OK response with rkyv bytes.
    pub fn rkyv(bytes: Vec<u8>) -> Self {
        Self {
            status: StatusCode::OK,
            content_type: mime::RKYV,
            body: bytes,
        }
    }

    /// Create an error response (always JSON for readability).
    pub fn error(status: StatusCode, message: &str) -> Self {
        let body = serde_json::to_vec(&serde_json::json!({
            "status": "error",
            "error": message,
        }))
        .unwrap_or_else(|_| b"{}".to_vec());

        Self {
            status,
            content_type: mime::JSON,
            body,
        }
    }
}

impl IntoResponse for NegotiatedResponse {
    fn into_response(self) -> Response {
        (
            self.status,
            [(CONTENT_TYPE, self.content_type)],
            self.body,
        )
            .into_response()
    }
}

/// Convenience: respond with FlatBuffers or JSON based on the `AcceptFormat`.
///
/// `fb_serializer` is called lazily only when FlatBuffers is requested.
/// This avoids serializing FlatBuffers when the client wants JSON.
pub fn respond<T: Serialize, F>(
    accept: AcceptFormat,
    json_data: &T,
    fb_serializer: F,
) -> NegotiatedResponse
where
    F: FnOnce() -> Vec<u8>,
{
    match accept {
        AcceptFormat::FlatBuffers => NegotiatedResponse::flatbuffers(fb_serializer()),
        AcceptFormat::Rkyv => {
            // rkyv path is handled by existing handlers — this fallback
            // produces JSON so callers that use this helper get a safe default.
            NegotiatedResponse::json(json_data)
        }
        AcceptFormat::Json => NegotiatedResponse::json(json_data),
    }
}
