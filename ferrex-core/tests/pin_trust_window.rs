//! Validates the PIN device trust window enforcement in authentication.

use anyhow::Result;
use chrono::{Duration, Utc};
use ferrex_core::domain::users::auth::domain::{
    services::AuthenticationError, value_objects::DeviceFingerprint,
};
use sqlx::PgPool;

#[path = "support/mod.rs"]
mod support;

use support::auth::TestAuthHarness;

const TEST_USERNAME: &str = "pinuser";
const TEST_PASSWORD: &str = "StrongPassword123!";
const TEST_PIN: &str = "4821";
const TEST_DEVICE_NAME: &str = "Test Device";

fn sample_fingerprint() -> DeviceFingerprint {
    DeviceFingerprint::new(
        "Linux".to_string(),
        Some("CPU".to_string()),
        None,
        None,
        None,
    )
    .expect("valid fingerprint")
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn pin_login_rejects_after_30_days_inactive(pool: PgPool) -> Result<()> {
    let harness = TestAuthHarness::new(pool.clone())?;
    let user_id = harness.create_user(TEST_USERNAME, TEST_PASSWORD).await?;
    let fingerprint = sample_fingerprint();

    let device_id = harness
        .register_device_with_pin(
            user_id,
            fingerprint.clone(),
            TEST_DEVICE_NAME,
            TEST_PIN,
        )
        .await?;

    let inactive_since = Utc::now() - Duration::days(31);
    harness
        .backdate_device_activity(device_id, inactive_since)
        .await?;

    let result = harness
        .auth()
        .authenticate_device_with_pin(user_id, &fingerprint, TEST_PIN)
        .await;

    assert!(matches!(result, Err(AuthenticationError::DeviceNotTrusted)));
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn pin_login_succeeds_within_30_days(pool: PgPool) -> Result<()> {
    let harness = TestAuthHarness::new(pool.clone())?;
    let user_id = harness.create_user(TEST_USERNAME, TEST_PASSWORD).await?;
    let fingerprint = sample_fingerprint();

    let device_id = harness
        .register_device_with_pin(
            user_id,
            fingerprint.clone(),
            TEST_DEVICE_NAME,
            TEST_PIN,
        )
        .await?;

    // Simulate recent activity to keep the device within the trust window
    let recent_activity = Utc::now() - Duration::days(1);
    harness
        .backdate_device_activity(device_id, recent_activity)
        .await?;

    let bundle = harness
        .auth()
        .authenticate_device_with_pin(user_id, &fingerprint, TEST_PIN)
        .await
        .expect("pin login should succeed within trust window");

    assert_eq!(bundle.scope.as_str(), "playback");
    Ok(())
}
