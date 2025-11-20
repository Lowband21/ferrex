// Authentication domain services
// These services orchestrate complex authentication flows that span
// multiple aggregates or require external dependencies

mod authentication_service;
mod device_trust_service;
mod pin_management_service;

use std::sync::Arc;

pub use authentication_service::{AuthenticationError, AuthenticationService};
pub use device_trust_service::{DeviceTrustError, DeviceTrustService};
pub use pin_management_service::{PinManagementError, PinManagementService};

/// Factory function to create an AuthenticationService with PostgreSQL repositories
#[cfg(feature = "database")]
pub fn create_authentication_service(pool: Arc<sqlx::PgPool>) -> AuthenticationService {
    use crate::auth::infrastructure::repositories::{
        PostgresDeviceSessionRepository, PostgresUserAuthRepository,
    };

    let user_repo = Arc::new(PostgresUserAuthRepository::new(pool.clone()));
    let session_repo = Arc::new(PostgresDeviceSessionRepository::new(pool));
    AuthenticationService::new(user_repo, session_repo)
}
