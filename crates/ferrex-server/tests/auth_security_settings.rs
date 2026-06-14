use anyhow::{Context, Result};
use axum::Router;
use axum_test::TestServer;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{Duration, Utc};
use ed25519_dalek::SigningKey;
use ferrex_core::{
    api::routes::v1,
    domain::users::{auth::device::Platform, user::User},
};
use ferrex_server::{
    infra::app_state::AppState, infra::startup::NoopStartupHooks,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::net::SocketAddr;
use tempfile::TempDir;
use uuid::Uuid;

mod common;
use common::build_test_app_with_hooks;

const USER_AGENT: &str = "FerrexAndroid/1.0 (Android TV)";

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn spawn(pool: PgPool) -> Result<(TestServer, AppState, TempDir)> {
    let app = build_test_app_with_hooks(pool, &NoopStartupHooks).await?;
    let (router, state, tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state.clone());
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok((server, state, tempdir))
}

async fn create_user(
    state: &AppState,
    username: &str,
    password: &str,
) -> Result<Uuid> {
    let user_id = Uuid::now_v7();
    let password_hash = state
        .auth_crypto()
        .hash_password(password)
        .context("password hash")?;
    let user = User {
        id: user_id,
        username: username.to_string(),
        display_name: format!("{username} Display"),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: None,
        is_active: true,
        email: Some(format!("{username}@example.invalid")),
        preferences: Default::default(),
    };

    state
        .unit_of_work()
        .users
        .create_user_with_password(&user, &password_hash)
        .await?;

    Ok(user_id)
}

async fn promote_to_admin(state: &AppState, user_id: Uuid) -> Result<()> {
    let admin_role = state
        .unit_of_work()
        .rbac
        .get_all_roles()
        .await?
        .into_iter()
        .find(|role| role.name == "admin")
        .context("admin role")?;
    state
        .unit_of_work()
        .rbac
        .assign_user_role(user_id, admin_role.id, user_id)
        .await?;
    Ok(())
}

async fn password_login(
    server: &TestServer,
    username: &str,
    password: &str,
) -> String {
    let response = server
        .post(v1::auth::LOGIN)
        .json(&json!({
            "username": username,
            "password": password,
        }))
        .await;
    response.assert_status_ok();
    let body: Value = response.json();
    body["data"]["access_token"]
        .as_str()
        .expect("access token")
        .to_string()
}

fn test_device_info(device_id: Uuid) -> Value {
    json!({
        "device_id": device_id,
        "device_name": "Security Policy TV",
        "platform": Platform::Android,
        "app_version": "2.3.4",
        "hardware_id": "security-policy-hw",
    })
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn security_settings_persist_pin_and_trust_policy(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = spawn(pool.clone()).await?;
    let admin_password = "AdminSecret#123";
    let admin_id =
        create_user(&state, "security_admin", admin_password).await?;
    promote_to_admin(&state, admin_id).await?;
    let admin_access =
        password_login(&server, "security_admin", admin_password).await;

    let defaults = server
        .get(v1::admin::security::SETTINGS)
        .add_header("Authorization", bearer(&admin_access))
        .await;
    defaults.assert_status_ok();
    let defaults_body: Value = defaults.json();
    assert_eq!(defaults_body["data"]["pin_policy"]["min_length"], json!(4));
    assert_eq!(defaults_body["data"]["pin_policy"]["max_length"], json!(8));
    assert_eq!(
        defaults_body["data"]["device_trust_policy"]["trust_duration_days"],
        json!(30)
    );
    assert_eq!(
        defaults_body["data"]["device_trust_policy"]["admin_pin_unlock_enabled"],
        json!(false)
    );

    let update_payload = json!({
        "admin_password_policy": {
            "enforce": false,
            "min_length": 8,
            "require_uppercase": true,
            "require_lowercase": true,
            "require_number": true,
            "require_special": false
        },
        "user_password_policy": {
            "enforce": false,
            "min_length": 8,
            "require_uppercase": false,
            "require_lowercase": false,
            "require_number": false,
            "require_special": false
        },
        "pin_policy": {
            "min_length": 5,
            "max_length": 6,
            "require_numeric": true,
            "reject_repeated_digits": true,
            "max_consecutive_identical": 2,
            "reject_sequential_digits": true
        },
        "device_trust_policy": {
            "remember_device_default": true,
            "trust_duration_days": 7,
            "pin_max_attempts": 2,
            "pin_lockout_minutes": 2,
            "admin_pin_unlock_enabled": false
        }
    });

    let updated = server
        .put(v1::admin::security::SETTINGS)
        .add_header("Authorization", bearer(&admin_access))
        .json(&update_payload)
        .await;
    updated.assert_status_ok();
    let updated_body: Value = updated.json();
    assert_eq!(updated_body["data"]["pin_policy"]["min_length"], json!(5));
    assert_eq!(
        updated_body["data"]["device_trust_policy"]["pin_max_attempts"],
        json!(2)
    );

    let invalid_admin_pin = server
        .put(v1::admin::security::SETTINGS)
        .add_header("Authorization", bearer(&admin_access))
        .json(&json!({
            "admin_password_policy": update_payload["admin_password_policy"],
            "user_password_policy": update_payload["user_password_policy"],
            "device_trust_policy": {
                "remember_device_default": true,
                "trust_duration_days": 7,
                "pin_max_attempts": 2,
                "pin_lockout_minutes": 2,
                "admin_pin_unlock_enabled": true
            }
        }))
        .await;
    invalid_admin_pin.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let device_user_password = "DeviceSecret#123";
    let user_id =
        create_user(&state, "policy_device_user", device_user_password).await?;
    let device_id = Uuid::now_v7();
    let signing_key = SigningKey::from_bytes(&[9_u8; 32]);
    let public_key = BASE64.encode(signing_key.verifying_key().to_bytes());

    let device_login = server
        .post(v1::auth::device::LOGIN)
        .add_header("user-agent", USER_AGENT)
        .json(&json!({
            "username": "policy_device_user",
            "password": device_user_password,
            "device_info": test_device_info(device_id),
            "remember_device": true,
            "device_public_key": public_key,
            "device_key_alg": "ed25519"
        }))
        .await;
    device_login.assert_status_ok();
    let login_body: Value = device_login.json();
    assert_eq!(login_body["data"]["user_id"], json!(user_id));
    assert_eq!(login_body["data"]["pin_policy"]["min_length"], json!(5));
    assert_eq!(
        login_body["data"]["device_trust_policy"]["trust_duration_days"],
        json!(7)
    );

    let device_session_id: Uuid = serde_json::from_value(
        login_body["data"]["device_session_id"].clone(),
    )?;
    let trusted_until: Option<chrono::DateTime<Utc>> = sqlx::query_scalar!(
        "SELECT trusted_until FROM auth_device_sessions WHERE id = $1",
        device_session_id
    )
    .fetch_one(&pool)
    .await?;
    let trusted_until = trusted_until.context("trusted_until")?;
    assert!(trusted_until > Utc::now() + Duration::days(6));
    assert!(trusted_until < Utc::now() + Duration::days(8));

    let device_status = server
        .get(&format!(
            "{}?device_id={}",
            v1::auth::device::STATUS,
            device_session_id
        ))
        .add_header(
            "Authorization",
            bearer(login_body["data"]["access_token"].as_str().unwrap()),
        )
        .await;
    device_status.assert_status_ok();
    let status_body: Value = device_status.json();
    assert_eq!(status_body["data"]["remaining_attempts"], json!(2));
    assert_eq!(status_body["data"]["pin_policy"]["max_length"], json!(6));

    Ok(())
}
