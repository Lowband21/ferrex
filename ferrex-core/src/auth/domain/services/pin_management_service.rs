use anyhow::Result;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::domain::repositories::UserAuthenticationRepository;
use crate::auth::domain::value_objects::PinCode;

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

impl PinManagementService {
    pub fn new(user_repo: Arc<dyn UserAuthenticationRepository>) -> Self {
        Self { user_repo }
    }
}
