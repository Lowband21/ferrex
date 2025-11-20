// Authentication domain services
// These services orchestrate complex authentication flows that span
// multiple aggregates or require external dependencies

mod authentication_service;
mod device_trust_service;
mod pin_management_service;

use std::sync::Arc;

pub use authentication_service::{AuthenticationService, AuthenticationError};
pub use device_trust_service::{DeviceTrustService, DeviceTrustError};
pub use pin_management_service::{PinManagementService, PinManagementError};

/// Factory function to create an AuthenticationService with PostgreSQL repositories
#[cfg(feature = "database")]
pub fn create_authentication_service(pool: Arc<sqlx::PgPool>) -> AuthenticationService {
    use crate::auth::infrastructure::repositories::{
        PostgresUserAuthRepository,
        PostgresDeviceSessionRepository,
    };
    
    let user_repo = Arc::new(PostgresUserAuthRepository::new(pool.clone()));
    let session_repo = Arc::new(PostgresDeviceSessionRepository::new(pool));
    AuthenticationService::new(user_repo, session_repo)
}