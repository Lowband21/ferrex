use std::sync::Arc;

use anyhow::Result;
use chrono::{Duration, Utc};
use ferrex_core::auth::domain::value_objects::DeviceFingerprint;
use ferrex_core::auth::{
    AuthCrypto,
    domain::services::{AuthenticationError, AuthenticationService, create_authentication_service},
};
use sqlx::PgPool;
use uuid::Uuid;

const TEST_USERNAME: &str = "pinuser";
const TEST_PASSWORD: &str = "StrongPassword123!";
const CLIENT_PROOF: &str = "argon2id$v=19$m=65536,t=3,p=1$Y2xpZW50cHJvb2YtZGV0ZXJtaW5pc3RpYw$U8N9qN5k2mQ3P9mN9EMn3rK9B2oQk0G3t8Jw7wS9C2o";

fn build_service(pool: PgPool) -> Result<(AuthenticationService, Arc<AuthCrypto>)> {
    let crypto = Arc::new(AuthCrypto::new("test-pepper", "test-token-key")?);
    let service = create_authentication_service(pool, crypto.clone());
    Ok((service, crypto))
}

async fn seed_user_with_pin(
    pool: &PgPool,
    crypto: &AuthCrypto,
    username: &str,
) -> Result<Uuid> {
    let user_id = Uuid::now_v7();
    let password_hash = crypto.hash_password(TEST_PASSWORD)?;
    let pin_hash = crypto.hash_password(CLIENT_PROOF)?; // server stores hash(pin_proof)

    // Create user and credentials
    sqlx::query!(
        r#"INSERT INTO users (id, username, display_name) VALUES ($1, $2, $3)"#,
        user_id,
        username,
        "PIN User"
    )
    .execute(pool)
    .await?;

    sqlx::query!(
        r#"INSERT INTO user_credentials (user_id, password_hash, pin_hash, pin_updated_at)
           VALUES ($1, $2, $3, NOW())"#,
        user_id,
        password_hash,
        pin_hash
    )
    .execute(pool)
    .await?;

    Ok(user_id)
}

async fn insert_trusted_device(
    pool: &PgPool,
    user_id: Uuid,
    fingerprint: &DeviceFingerprint,
    device_name: &str,
    last_activity: chrono::DateTime<chrono::Utc>,
) -> Result<Uuid> {
    let device_id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO auth_device_sessions (
            id, user_id, device_fingerprint, device_name, status, failed_attempts,
            first_authenticated_by, first_authenticated_at, last_seen_at, last_activity
        ) VALUES (
            $1, $2, $3, $4, 'trusted'::auth_device_status, 0, $2, NOW(), NOW(), $5
        )
        "#,
        device_id,
        user_id,
        fingerprint.as_str(),
        device_name,
        last_activity
    )
    .execute(pool)
    .await?;
    Ok(device_id)
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn pin_login_rejects_after_30_days_inactive(pool: PgPool) -> Result<()> {
    let (service, crypto) = build_service(pool.clone())?;
    let user_id = seed_user_with_pin(&pool, &crypto, TEST_USERNAME).await?;

    let fingerprint = DeviceFingerprint::new(
        "Linux".to_string(),
        Some("CPU".to_string()),
        None,
        None,
        None,
    )
    .unwrap();

    // Insert device with last_activity older than 30 days
    let old_activity = Utc::now() - Duration::days(31);
    insert_trusted_device(&pool, user_id, &fingerprint, "Test", old_activity).await?;

    // Attempt PIN login using the client proof
    let result = service
        .authenticate_device_with_pin(user_id, &fingerprint, CLIENT_PROOF)
        .await;

    assert!(matches!(result, Err(AuthenticationError::DeviceNotTrusted)));
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn pin_login_succeeds_within_30_days(pool: PgPool) -> Result<()> {
    let (service, crypto) = build_service(pool.clone())?;
    let user_id = seed_user_with_pin(&pool, &crypto, TEST_USERNAME).await?;

    let fingerprint = DeviceFingerprint::new(
        "Linux".to_string(),
        Some("CPU".to_string()),
        None,
        None,
        None,
    )
    .unwrap();

    // Insert device with last_activity within 30 days
    let recent_activity = Utc::now() - Duration::days(1);
    insert_trusted_device(&pool, user_id, &fingerprint, "Test", recent_activity).await?;

    // PIN login using the same client proof string
    let bundle = service
        .authenticate_device_with_pin(user_id, &fingerprint, CLIENT_PROOF)
        .await
        .expect("pin login should succeed within trust window");

    assert_eq!(bundle.scope.as_str(), "playback");
    Ok(())
}

