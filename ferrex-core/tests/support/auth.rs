use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use ferrex_core::auth::domain::value_objects::{DeviceFingerprint, PinPolicy};
use ferrex_core::auth::infrastructure::repositories::{
    PostgresAuthEventRepository, PostgresAuthSessionRepository, PostgresDeviceSessionRepository,
    PostgresRefreshTokenRepository, PostgresUserAuthRepository,
};
use ferrex_core::auth::{
    AuthCrypto,
    domain::services::{
        AuthenticationService, DeviceTrustService, PinManagementService,
        create_authentication_service,
    },
};

/// End-to-end authentication harness for integration tests.
pub struct TestAuthHarness {
    pool: PgPool,
    crypto: Arc<AuthCrypto>,
    auth_service: AuthenticationService,
    device_trust_service: DeviceTrustService,
    pin_service: PinManagementService,
}

impl TestAuthHarness {
    /// Construct a new harness backed by the provided pool.
    pub fn new(pool: PgPool) -> Result<Self> {
        let crypto = Arc::new(AuthCrypto::new("test-pepper", "test-token-key")?);

        let auth_service = create_authentication_service(pool.clone(), crypto.clone());

        let user_repo = Arc::new(PostgresUserAuthRepository::new(pool.clone()));
        let session_repo = Arc::new(PostgresDeviceSessionRepository::new(
            pool.clone(),
            crypto.clone(),
        ));
        let event_repo = Arc::new(PostgresAuthEventRepository::new(pool.clone()));
        let session_store = Arc::new(PostgresAuthSessionRepository::new(pool.clone()));
        let refresh_repo = Arc::new(PostgresRefreshTokenRepository::new(pool.clone()));

        let device_trust_service = DeviceTrustService::new(
            user_repo.clone(),
            session_repo.clone(),
            event_repo.clone(),
            session_store,
            refresh_repo,
        );

        let pin_service =
            PinManagementService::new(user_repo, session_repo, event_repo, crypto.clone());

        Ok(Self {
            pool,
            crypto,
            auth_service,
            device_trust_service,
            pin_service,
        })
    }

    /// Direct access to the underlying connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Access the test crypto instance.
    pub fn crypto(&self) -> &AuthCrypto {
        self.crypto.as_ref()
    }

    /// Borrow the authentication service for login flows.
    pub fn auth(&self) -> &AuthenticationService {
        &self.auth_service
    }

    /// Create a user with the supplied credentials, returning the new user ID.
    pub async fn create_user_with_password(
        &self,
        username: &str,
        display_name: &str,
        password: &str,
    ) -> Result<Uuid> {
        let user_id = Uuid::now_v7();
        let password_hash = self
            .crypto
            .hash_password(password)
            .context("failed to hash test password")?;

        sqlx::query(
            r#"
            INSERT INTO users (id, username, display_name)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(user_id)
        .bind(username)
        .bind(display_name)
        .execute(self.pool())
        .await
        .context("failed to insert test user")?;

        sqlx::query(
            r#"
            INSERT INTO user_credentials (user_id, password_hash)
            VALUES ($1, $2)
            "#,
        )
        .bind(user_id)
        .bind(password_hash)
        .execute(self.pool())
        .await
        .context("failed to insert test user credentials")?;

        Ok(user_id)
    }

    /// Convenience for creating a user where the display name matches the username.
    pub async fn create_user(&self, username: &str, password: &str) -> Result<Uuid> {
        self.create_user_with_password(username, username, password)
            .await
    }

    /// Register a device for the user and trust it by configuring a PIN.
    pub async fn register_device_with_pin(
        &self,
        user_id: Uuid,
        fingerprint: DeviceFingerprint,
        device_name: &str,
        pin: &str,
    ) -> Result<Uuid> {
        let registered = self
            .device_trust_service
            .register_device(user_id, fingerprint.clone(), device_name.to_string(), None)
            .await
            .context("failed to register test device")?;

        let (last_activity,): (DateTime<Utc>,) =
            sqlx::query_as("SELECT last_activity FROM auth_device_sessions WHERE id = $1")
                .bind(registered.id())
                .fetch_one(self.pool())
                .await
                .context("failed to inspect device session prior to pin set")?;
        println!("registered device last_activity: {last_activity:?}");

        self.pin_service
            .set_pin(
                user_id,
                &fingerprint,
                pin.to_string(),
                &PinPolicy::default(),
                None,
            )
            .await
            .context("failed to set test device PIN")?;

        Ok(registered.id())
    }

    /// Set explicit last_activity for a device session to simulate inactivity windows.
    pub async fn backdate_device_activity(
        &self,
        device_session_id: Uuid,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE auth_device_sessions
            SET last_activity = $1,
                last_seen_at = $1,
                updated_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(timestamp)
        .bind(device_session_id)
        .execute(self.pool())
        .await
        .context("failed to backdate device activity for test")?;

        Ok(())
    }
}
