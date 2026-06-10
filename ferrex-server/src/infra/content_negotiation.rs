use std::future::ready;

use axum::{
    Json,
    body::Bytes,
    extract::FromRequestParts,
    http::{HeaderMap, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use ferrex_flatbuffers::FLATBUFFERS_MIME;
use serde::Serialize;

pub const JSON_MIME: &str = "application/json";
pub const RKYV_OCTET_STREAM_MIME: &str = "application/octet-stream";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    Json,
    FlatBuffers,
    RkyvOctetStream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptedFormat(pub WireFormat);

impl AcceptedFormat {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        Self(negotiate_response_format(headers))
    }
}

impl<S> FromRequestParts<S> for AcceptedFormat
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send
    {
        ready(Ok(Self::from_headers(&parts.headers)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestBodyFormat {
    Json,
    FlatBuffers,
    RkyvOctetStream,
    Unsupported(String),
}

impl RequestBodyFormat {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let Some(value) = headers.get(header::CONTENT_TYPE) else {
            return Self::Json;
        };

        let Ok(raw_value) = value.to_str() else {
            return Self::Unsupported("<invalid Content-Type>".to_string());
        };

        match normalize_media_type(raw_value).as_str() {
            FLATBUFFERS_MIME => Self::FlatBuffers,
            RKYV_OCTET_STREAM_MIME => Self::RkyvOctetStream,
            media_type if is_json_media_type(media_type) => Self::Json,
            other => Self::Unsupported(other.to_string()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Json => JSON_MIME,
            Self::FlatBuffers => FLATBUFFERS_MIME,
            Self::RkyvOctetStream => RKYV_OCTET_STREAM_MIME,
            Self::Unsupported(value) => value,
        }
    }
}

pub fn negotiate_response_format(headers: &HeaderMap) -> WireFormat {
    let mut best: Option<AcceptCandidate> = None;
    let mut order = 0usize;

    for value in headers.get_all(header::ACCEPT) {
        let Ok(raw_value) = value.to_str() else {
            continue;
        };

        for item in raw_value.split(',') {
            if let Some(candidate) = parse_accept_item(item, order) {
                if candidate.q > 0
                    && best
                        .is_none_or(|current| candidate.is_better_than(current))
                {
                    best = Some(candidate);
                }
            }
            order += 1;
        }
    }

    best.map_or(WireFormat::Json, |candidate| candidate.format)
}

pub fn json_or_flatbuffers<T, F>(
    format: WireFormat,
    json_body: T,
    flatbuffers_body: F,
) -> Response
where
    T: Serialize,
    F: FnOnce() -> Vec<u8>,
{
    match format {
        WireFormat::FlatBuffers => (
            [(header::CONTENT_TYPE, FLATBUFFERS_MIME)],
            Bytes::from(flatbuffers_body()),
        )
            .into_response(),
        WireFormat::Json | WireFormat::RkyvOctetStream => {
            Json(json_body).into_response()
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AcceptCandidate {
    format: WireFormat,
    q: u16,
    specificity: u8,
    order: usize,
}

impl AcceptCandidate {
    fn is_better_than(self, current: Self) -> bool {
        self.q > current.q
            || (self.q == current.q && self.specificity > current.specificity)
            || (self.q == current.q
                && self.specificity == current.specificity
                && self.order < current.order)
    }
}

fn parse_accept_item(raw_item: &str, order: usize) -> Option<AcceptCandidate> {
    let mut parts = raw_item.split(';');
    let media_type = normalize_media_type(parts.next()?);
    let (format, specificity) = media_type_to_format(&media_type)?;
    let mut q = 1000u16;

    for parameter in parts {
        let mut key_value = parameter.splitn(2, '=');
        let key = key_value.next()?.trim();
        let value = key_value.next().unwrap_or_default().trim();
        if key.eq_ignore_ascii_case("q") {
            q = parse_q(value);
        }
    }

    Some(AcceptCandidate {
        format,
        q,
        specificity,
        order,
    })
}

fn media_type_to_format(media_type: &str) -> Option<(WireFormat, u8)> {
    match media_type {
        FLATBUFFERS_MIME => Some((WireFormat::FlatBuffers, 3)),
        RKYV_OCTET_STREAM_MIME => Some((WireFormat::RkyvOctetStream, 3)),
        value if is_json_media_type(value) => Some((WireFormat::Json, 3)),
        "application/*" => Some((WireFormat::Json, 1)),
        "*/*" => Some((WireFormat::Json, 0)),
        _ => None,
    }
}

fn is_json_media_type(media_type: &str) -> bool {
    media_type == JSON_MIME || media_type.ends_with("+json")
}

fn normalize_media_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase()
}

fn parse_q(value: &str) -> u16 {
    value
        .parse::<f32>()
        .map(|q| (q.clamp(0.0, 1.0) * 1000.0).round() as u16)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn negotiates_flatbuffers_when_explicitly_preferred() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static(
                "application/json;q=0.5, application/x-flatbuffers",
            ),
        );

        assert_eq!(
            negotiate_response_format(&headers),
            WireFormat::FlatBuffers
        );
    }

    #[test]
    fn defaults_to_json_for_wildcard_or_missing_accept() {
        let headers = HeaderMap::new();
        assert_eq!(negotiate_response_format(&headers), WireFormat::Json);

        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static("*/*"));
        assert_eq!(negotiate_response_format(&headers), WireFormat::Json);
    }

    #[test]
    fn preserves_octet_stream_as_legacy_binary_format() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("application/octet-stream"),
        );

        assert_eq!(
            negotiate_response_format(&headers),
            WireFormat::RkyvOctetStream
        );
    }
}
