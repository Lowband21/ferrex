use std::fmt;
use std::sync::Arc;

use crate::auth::domain::repositories::UserAuthenticationRepository;

#[derive(Debug, thiserror::Error)]
pub enum PinManagementError {
    #[error("Invalid PIN format")]
    InvalidPinFormat,
    #[error("PIN verification failed")]
    PinVerificationFailed,
    #[error("User not found")]
    UserNotFound,
    #[error("Database error: {0}")]
    DatabaseError(#[from] anyhow::Error),
}

pub struct PinManagementService {
    user_repo: Arc<dyn UserAuthenticationRepository>,
}

impl fmt::Debug for PinManagementService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PinManagementService")
            .field("user_repo_refs", &Arc::strong_count(&self.user_repo))
            .finish()
    }
}

impl PinManagementService {
    pub fn new(user_repo: Arc<dyn UserAuthenticationRepository>) -> Self {
        Self { user_repo }
    }
}
