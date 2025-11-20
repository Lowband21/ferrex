use anyhow::Result;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::domain::repositories::DeviceSessionRepository;
use crate::auth::domain::value_objects::DeviceFingerprint;

#[derive(Debug, thiserror::Error)]
pub enum DeviceTrustError {
    #[error("Device not found")]
    DeviceNotFound,
    #[error("Device not trusted")]
    DeviceNotTrusted,
    #[error("Database error: {0}")]
    DatabaseError(#[from] anyhow::Error),
}

pub struct DeviceTrustService {
    session_repo: Arc<dyn DeviceSessionRepository>,
}

impl DeviceTrustService {
    pub fn new(session_repo: Arc<dyn DeviceSessionRepository>) -> Self {
        Self { session_repo }
    }
}
