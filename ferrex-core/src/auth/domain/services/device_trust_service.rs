use std::fmt;
use std::sync::Arc;

use crate::auth::domain::repositories::DeviceSessionRepository;

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

impl fmt::Debug for DeviceTrustService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceTrustService")
            .field("session_repo_refs", &Arc::strong_count(&self.session_repo))
            .finish()
    }
}

impl DeviceTrustService {
    pub fn new(session_repo: Arc<dyn DeviceSessionRepository>) -> Self {
        Self { session_repo }
    }
}
