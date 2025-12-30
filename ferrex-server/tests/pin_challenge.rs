use anyhow::Result;
use axum::Router;
use axum_test::TestServer;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ferrex_core::{
    api::routes::v1,
    domain::users::auth::domain::{
        AuthEventContext, DeviceFingerprint, DeviceSession,
    },
    player_prelude::User,
};
use ferrex_server::infra::startup::NoopStartupHooks;
use serde_json::json;
use sqlx::PgPool;
use std::net::SocketAddr;
use uuid::Uuid;

mod common;
use common::build_test_app_with_hooks;

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn pin_challenge_returns_server_managed_salt(pool: PgPool) -> Result<()> {
    let app = build_test_app_with_hooks(pool, &NoopStartupHooks).await?;
    let (router, state, tempdir) = app.into_parts();
    assert!(
        tempdir.path().join("cache").exists(),
        "test app should create cache directory structure"
    );
    let router: Router<()> = router.with_state(state.clone());
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let user_id = Uuid::now_v7();
    let username = "pin_challenge_user".to_string();
    let password = "PinSecret#123";
    let password_hash = state
        .auth_crypto()
        .hash_password(password)
        .expect("password hash");
    let user = User {
        id: user_id,
        username: username.clone(),
        display_name: "Pin Challenge".into(),
        avatar_url: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        last_login: None,
        is_active: true,
        email: None,
        preferences: Default::default(),
    };

    state
        .unit_of_work()
        .users
        .create_user_with_password(&user, &password_hash)
        .await?;

    let fingerprint = DeviceFingerprint::from_hash("a".repeat(64))?;
    let session: DeviceSession = state
        .auth_facade()
        .device_trust_service()
        .register_device(
            user_id,
            fingerprint.clone(),
            "Test Device".into(),
            Some(AuthEventContext::default()),
        )
        .await?;

    let device_id = session.id();
    let device_fingerprint = fingerprint;

    let initial_salt = state.auth_facade().get_pin_client_salt(user_id).await?;

    let challenge = server
        .post(v1::auth::device::PIN_CHALLENGE)
        .json(&json!({ "device_id": device_id }))
        .await;
    challenge.assert_status_ok();
    let body: serde_json::Value = challenge.json();
    let salt_b64 = body["data"]["pin_salt"]
        .as_str()
        .expect("pin_salt field present");
    let decoded = BASE64.decode(salt_b64).expect("valid base64 salt");
    assert_eq!(
        decoded, initial_salt,
        "challenge should return current server salt"
    );

    state
        .auth_facade()
        .pin_management_service()
        .force_clear_pin(user_id, &device_fingerprint, None)
        .await?;

    let rotated_salt = state.auth_facade().get_pin_client_salt(user_id).await?;
    assert_ne!(
        rotated_salt, initial_salt,
        "force_clear_pin rotates the salt"
    );

    let challenge_after_rotation = server
        .post(v1::auth::device::PIN_CHALLENGE)
        .json(&json!({ "device_id": device_id }))
        .await;
    challenge_after_rotation.assert_status_ok();
    let body_rotated: serde_json::Value = challenge_after_rotation.json();
    let rotated_b64 = body_rotated["data"]["pin_salt"]
        .as_str()
        .expect("pin_salt field present after rotation");
    let decoded_rotated = BASE64
        .decode(rotated_b64)
        .expect("valid base64 salt after rotation");
    assert_eq!(
        decoded_rotated, rotated_salt,
        "challenge exposes rotated salt"
    );

    Ok(())
}
