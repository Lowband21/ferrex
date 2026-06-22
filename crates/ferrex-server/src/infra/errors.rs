use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::fmt;

use ferrex_core::{
    domain::intelligence::IntelligenceProviderError, error::MediaError,
};

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl AppError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, message)
    }

    pub fn gone(message: impl Into<String>) -> Self {
        Self::new(StatusCode::GONE, message)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "error": {
                "message": self.message,
                "status": self.status.as_u16(),
            }
        }));

        (self.status, body).into_response()
    }
}

// Convert from various error types
impl From<MediaError> for AppError {
    fn from(err: MediaError) -> Self {
        match err {
            MediaError::NotFound(msg) => Self::not_found(msg),
            MediaError::Internal(msg) => Self::internal(msg),
            _ => Self::internal(err.to_string()),
        }
    }
}

impl From<IntelligenceProviderError> for AppError {
    fn from(err: IntelligenceProviderError) -> Self {
        let message = err.to_string();
        match err {
            IntelligenceProviderError::NotConfigured { .. } => {
                Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
            }
            IntelligenceProviderError::Unavailable { .. } => {
                Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
            }
            IntelligenceProviderError::Unauthorized { .. } => {
                Self::new(StatusCode::BAD_GATEWAY, message)
            }
            IntelligenceProviderError::RateLimited { .. } => {
                Self::rate_limited(message)
            }
            IntelligenceProviderError::Timeout { .. } => {
                Self::new(StatusCode::GATEWAY_TIMEOUT, message)
            }
            IntelligenceProviderError::Cancelled { .. } => {
                Self::new(StatusCode::REQUEST_TIMEOUT, message)
            }
            IntelligenceProviderError::ModelUnavailable { .. } => {
                Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
            }
            IntelligenceProviderError::InvalidRequest { .. } => {
                Self::bad_request(message)
            }
            IntelligenceProviderError::MalformedOutput { .. }
            | IntelligenceProviderError::SchemaViolation { .. }
            | IntelligenceProviderError::RetryExhausted { .. }
            | IntelligenceProviderError::ProviderRejectedOptions { .. }
            | IntelligenceProviderError::ProviderStatus { .. } => {
                Self::new(StatusCode::BAD_GATEWAY, message)
            }
            IntelligenceProviderError::Internal { .. } => {
                Self::internal(message)
            }
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        Self::internal(err.to_string())
    }
}

impl From<ferrex_core::domain::users::user::ValidationError> for AppError {
    fn from(err: ferrex_core::domain::users::user::ValidationError) -> Self {
        Self::bad_request(err.to_string())
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!(error = ?err, "database operation failed");
        Self::internal("Database operation failed")
    }
}
