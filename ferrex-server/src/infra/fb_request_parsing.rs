use axum::{body::Bytes, http::HeaderMap};
use serde::de::DeserializeOwned;
use std::fmt;

use crate::infra::{
    content_negotiation::RequestBodyFormat,
    errors::{AppError, AppResult},
};

pub fn parse_json_or_flatbuffers<T, F, E>(
    headers: &HeaderMap,
    body: Bytes,
    parse_flatbuffers: F,
) -> AppResult<T>
where
    T: DeserializeOwned,
    F: FnOnce(&[u8]) -> Result<T, E>,
    E: fmt::Display,
{
    match RequestBodyFormat::from_headers(headers) {
        RequestBodyFormat::FlatBuffers => parse_flatbuffers(body.as_ref())
            .map_err(|err| {
                AppError::bad_request(format!(
                    "Invalid FlatBuffers request body: {err}"
                ))
            }),
        RequestBodyFormat::Json => serde_json::from_slice(body.as_ref())
            .map_err(|err| {
                AppError::bad_request(format!(
                    "Invalid JSON request body: {err}"
                ))
            }),
        format @ (RequestBodyFormat::RkyvOctetStream
        | RequestBodyFormat::Unsupported(_)) => {
            Err(AppError::bad_request(format!(
                "Unsupported request content type for this endpoint: {}",
                format.label()
            )))
        }
    }
}
