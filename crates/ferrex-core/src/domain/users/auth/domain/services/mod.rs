// Authentication domain services
// These services orchestrate complex authentication flows that span
// multiple aggregates or require external dependencies

mod authentication_service;
mod device_trust_service;
mod event_context;
mod pin_management_service;

pub(crate) use event_context::map_domain_events;

pub use authentication_service::{
    AuthenticationError, AuthenticationService, PasswordChangeActor,
    PasswordChangeRequest, TokenBundle, ValidatedSession,
};
pub use device_trust_service::{DeviceTrustError, DeviceTrustService};
pub use event_context::AuthEventContext;
pub use pin_management_service::{PinManagementError, PinManagementService};

/// Factory function to create an AuthenticationService with PostgreSQL repository_ports
#[cfg(feature = "database")]
pub fn create_authentication_service(
    pool: sqlx::PgPool,
    crypto: std::sync::Arc<crate::domain::users::auth::AuthCrypto>,
) -> AuthenticationService {
    use crate::domain::users::auth::infrastructure::repositories::{
        PostgresAuthSessionRepository, PostgresDeviceSessionRepository,
        PostgresRefreshTokenRepository, PostgresUserAuthRepository,
    };
    use std::sync::Arc;

    let user_repo = Arc::new(PostgresUserAuthRepository::new(pool.clone()));
    let session_repo = Arc::new(PostgresDeviceSessionRepository::new(
        pool.clone(),
        crypto.clone(),
    ));
    let refresh_repo =
        Arc::new(PostgresRefreshTokenRepository::new(pool.clone()));
    let session_store = Arc::new(PostgresAuthSessionRepository::new(pool));
    AuthenticationService::new(
        user_repo,
        session_repo,
        refresh_repo,
        session_store,
        crypto,
    )
}
