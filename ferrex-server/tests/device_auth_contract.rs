use anyhow::{Context, Result};
use axum::{Router, http::StatusCode};
use axum_test::TestServer;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signer, SigningKey};
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
const PIN_PROOF: &str = "client-derived-pin-proof";

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
        avatar_url: Some(format!("https://example.invalid/{username}.png")),
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

fn test_device_info(device_id: Uuid, hardware_id: &str) -> Value {
    json!({
        "device_id": device_id,
        "device_name": "Living Room TV",
        "platform": Platform::Android,
        "app_version": "2.3.4",
        "hardware_id": hardware_id,
    })
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7_u8; 32])
}

fn public_key_b64(signing_key: &SigningKey) -> String {
    BASE64.encode(signing_key.verifying_key().to_bytes())
}

fn sign_challenge(
    signing_key: &SigningKey,
    challenge_id: Uuid,
    nonce_b64: &str,
    user_id: Uuid,
) -> Result<String> {
    let nonce = BASE64.decode(nonce_b64)?;
    const CTX: &[u8] = b"Ferrex-PIN-v1";
    let mut msg = Vec::with_capacity(CTX.len() + 16 + nonce.len() + 16);
    msg.extend_from_slice(CTX);
    msg.extend_from_slice(challenge_id.as_bytes());
    msg.extend_from_slice(&nonce);
    msg.extend_from_slice(user_id.as_bytes());
    Ok(BASE64.encode(signing_key.sign(&msg).to_bytes()))
}

async fn issue_challenge(
    server: &TestServer,
    device_session_id: Uuid,
) -> Result<Value> {
    let response = server
        .post(v1::auth::device::PIN_CHALLENGE)
        .json(&json!({ "device_id": device_session_id }))
        .await;
    response.assert_status_ok();
    let body: Value = response.json();
    Ok(body["data"].clone())
}

async fn set_pin(
    server: &TestServer,
    access_token: &str,
    signing_key: &SigningKey,
    user_id: Uuid,
    device_session_id: Uuid,
) -> Result<()> {
    let challenge = issue_challenge(server, device_session_id).await?;
    let challenge_id: Uuid =
        serde_json::from_value(challenge["challenge_id"].clone())?;
    let signature = sign_challenge(
        signing_key,
        challenge_id,
        challenge["nonce"].as_str().context("nonce")?,
        user_id,
    )?;

    let response = server
        .post(v1::auth::device::SET_PIN)
        .add_header("Authorization", bearer(access_token))
        .json(&json!({
            "device_id": device_session_id,
            "client_proof": PIN_PROOF,
            "challenge_id": challenge_id,
            "device_signature": signature,
        }))
        .await;
    response.assert_status_ok();
    Ok(())
}

async fn device_login(
    server: &TestServer,
    username: &str,
    password: &str,
    device_info: Value,
    public_key: Option<&str>,
    remember_device: bool,
) -> Value {
    let mut request = json!({
        "username": username,
        "password": password,
        "device_info": device_info,
        "remember_device": remember_device,
    });
    if let Some(public_key) = public_key {
        request["device_public_key"] = json!(public_key);
        request["device_key_alg"] = json!("ed25519");
    }

    let response = server
        .post(v1::auth::device::LOGIN)
        .add_header("user-agent", USER_AGENT)
        .json(&request)
        .await;
    response.assert_status_ok();
    let body: Value = response.json();
    body["data"].clone()
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn password_device_login_returns_auth_token_and_persists_metadata(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = spawn(pool.clone()).await?;
    let username = "device_contract_user";
    let password = "DeviceSecret#123";
    let user_id = create_user(&state, username, password).await?;
    let device_id = Uuid::now_v7();
    let device_info = test_device_info(device_id, "living-room-hw");
    let signing_key = signing_key();
    let public_key = public_key_b64(&signing_key);

    let missing_key = server
        .post(v1::auth::device::LOGIN)
        .add_header("user-agent", USER_AGENT)
        .json(&json!({
            "username": username,
            "password": password,
            "device_info": device_info,
            "remember_device": true,
        }))
        .await;
    missing_key.assert_status(StatusCode::BAD_REQUEST);

    let data = device_login(
        &server,
        username,
        password,
        test_device_info(device_id, "living-room-hw"),
        Some(&public_key),
        true,
    )
    .await;

    assert!(data["access_token"].as_str().is_some_and(|s| !s.is_empty()));
    assert_eq!(data["session_token"], data["access_token"]);
    assert!(
        data["refresh_token"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    assert!(data["expires_in"].as_u64().is_some_and(|v| v > 0));
    assert_eq!(data["user_id"], json!(user_id));
    assert_eq!(data["scope"], json!("full"));
    assert_eq!(data["requires_pin_setup"], json!(true));

    let session_id: Uuid = serde_json::from_value(data["session_id"].clone())?;
    let device_session_id: Uuid =
        serde_json::from_value(data["device_session_id"].clone())?;
    let registration = &data["device_registration"];
    assert_eq!(registration["id"], json!(device_session_id));
    assert_eq!(registration["device_id"], json!(device_id));
    assert_eq!(registration["platform"], json!("android"));
    assert_eq!(registration["app_version"], json!("2.3.4"));

    let row = sqlx::query!(
        r#"
        SELECT platform,
               app_version,
               hardware_id,
               auto_login_enabled,
               trusted_until,
               device_public_key,
               device_key_alg::text AS "device_key_alg?",
               metadata
        FROM auth_device_sessions
        WHERE id = $1
        "#,
        device_session_id
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(row.platform.as_deref(), Some("android"));
    assert_eq!(row.app_version.as_deref(), Some("2.3.4"));
    assert_eq!(row.hardware_id.as_deref(), Some("living-room-hw"));
    assert!(
        row.auto_login_enabled,
        "remember_device should be per-device"
    );
    assert!(
        row.trusted_until.is_some(),
        "trusted_until should be persisted"
    );
    assert_eq!(row.device_public_key.as_deref(), Some(public_key.as_str()));
    assert_eq!(row.device_key_alg.as_deref(), Some("ed25519"));
    assert_eq!(row.metadata["device_id"], json!(device_id));

    let refreshed = server
        .post(v1::auth::REFRESH)
        .json(&json!({ "refresh_token": data["refresh_token"] }))
        .await;
    refreshed.assert_status_ok();
    let refreshed_body: Value = refreshed.json();
    assert_eq!(
        refreshed_body["data"]["device_session_id"],
        json!(device_session_id)
    );
    assert_ne!(refreshed_body["data"]["session_id"], json!(session_id));
    assert_eq!(refreshed_body["data"]["scope"], json!("full"));

    let unknown_profiles = server
        .post(v1::auth::device::KNOWN_USERS)
        .add_header("user-agent", USER_AGENT)
        .json(&json!({
            "device_info": test_device_info(Uuid::now_v7(), "unknown-hw")
        }))
        .await;
    unknown_profiles.assert_status_ok();
    let unknown_body: Value = unknown_profiles.json();
    assert_eq!(unknown_body["data"]["known_device"], json!(false));
    assert_eq!(unknown_body["data"]["users"].as_array().unwrap().len(), 0);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn pin_login_profiles_lockout_and_revoked_fallback(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = spawn(pool.clone()).await?;
    let username = "pin_contract_user";
    let password = "PinSecret#123";
    let user_id = create_user(&state, username, password).await?;
    let device_id = Uuid::now_v7();
    let signing_key = signing_key();
    let public_key = public_key_b64(&signing_key);

    let login = device_login(
        &server,
        username,
        password,
        test_device_info(device_id, "pin-hw"),
        Some(&public_key),
        true,
    )
    .await;
    let access_token = login["access_token"].as_str().unwrap().to_string();
    let device_session_id: Uuid =
        serde_json::from_value(login["device_session_id"].clone())?;

    set_pin(
        &server,
        &access_token,
        &signing_key,
        user_id,
        device_session_id,
    )
    .await?;

    let profiles = server
        .post(v1::auth::device::KNOWN_USERS)
        .add_header("user-agent", USER_AGENT)
        .json(&json!({
            "device_info": test_device_info(device_id, "pin-hw")
        }))
        .await;
    profiles.assert_status_ok();
    let profiles_body: Value = profiles.json();
    assert_eq!(profiles_body["data"]["known_device"], json!(true));
    let users = profiles_body["data"]["users"].as_array().unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0]["id"], json!(user_id));
    assert_eq!(users[0]["username"], json!(username));
    assert_eq!(users[0]["has_pin"], json!(true));
    assert!(
        users[0].get("email").is_none(),
        "profile card must be minimal"
    );

    let challenge = issue_challenge(&server, device_session_id).await?;
    let challenge_id: Uuid =
        serde_json::from_value(challenge["challenge_id"].clone())?;
    let signature = sign_challenge(
        &signing_key,
        challenge_id,
        challenge["nonce"].as_str().context("nonce")?,
        user_id,
    )?;
    let pin_login = server
        .post(v1::auth::device::PIN_LOGIN)
        .json(&json!({
            "device_id": device_session_id,
            "client_proof": PIN_PROOF,
            "challenge_id": challenge_id,
            "device_signature": signature,
        }))
        .await;
    pin_login.assert_status_ok();
    let pin_body: Value = pin_login.json();
    assert_eq!(pin_body["data"]["user_id"], json!(user_id));
    assert_eq!(
        pin_body["data"]["device_session_id"],
        json!(device_session_id)
    );
    assert_eq!(pin_body["data"]["scope"], json!("playback"));
    assert!(
        pin_body["data"]["refresh_token"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    assert!(pin_body["data"]["session_id"].as_str().is_some());
    assert_eq!(
        pin_body["data"]["device_registration"]["id"],
        json!(device_session_id)
    );

    let consumed_reuse = server
        .post(v1::auth::device::PIN_LOGIN)
        .json(&json!({
            "device_id": device_session_id,
            "client_proof": PIN_PROOF,
            "challenge_id": challenge_id,
            "device_signature": signature,
        }))
        .await;
    consumed_reuse.assert_status(StatusCode::UNAUTHORIZED);

    let pin_refresh = server
        .post(v1::auth::REFRESH)
        .json(&json!({
            "refresh_token": pin_body["data"]["refresh_token"],
        }))
        .await;
    pin_refresh.assert_status_ok();
    let pin_refresh_body: Value = pin_refresh.json();
    assert_eq!(
        pin_refresh_body["data"]["device_session_id"],
        json!(device_session_id)
    );
    assert_eq!(pin_refresh_body["data"]["scope"], json!("playback"));

    for attempt in 1..=3 {
        let challenge = issue_challenge(&server, device_session_id).await?;
        let challenge_id: Uuid =
            serde_json::from_value(challenge["challenge_id"].clone())?;
        let signature = sign_challenge(
            &signing_key,
            challenge_id,
            challenge["nonce"].as_str().context("nonce")?,
            user_id,
        )?;
        let response = server
            .post(v1::auth::device::PIN_LOGIN)
            .json(&json!({
                "device_id": device_session_id,
                "client_proof": "wrong-proof",
                "challenge_id": challenge_id,
                "device_signature": signature,
            }))
            .await;
        if attempt < 3 {
            response.assert_status(StatusCode::UNAUTHORIZED);
        } else {
            response.assert_status(StatusCode::TOO_MANY_REQUESTS);
        }
    }

    let locked_until: Option<DateTime<Utc>> = sqlx::query_scalar!(
        "SELECT locked_until FROM auth_device_sessions WHERE id = $1",
        device_session_id
    )
    .fetch_one(&pool)
    .await?;
    assert!(locked_until.is_some(), "lockout deadline should persist");

    let refreshed_access = pin_refresh_body["data"]["access_token"]
        .as_str()
        .context("refreshed access token")?;
    let revoke = server
        .post(v1::auth::device::REVOKE)
        .add_header("Authorization", bearer(refreshed_access))
        .json(&json!({ "device_id": device_session_id }))
        .await;
    revoke.assert_status_ok();

    let profiles_after_revoke = server
        .post(v1::auth::device::KNOWN_USERS)
        .add_header("user-agent", USER_AGENT)
        .json(&json!({
            "device_info": test_device_info(device_id, "pin-hw")
        }))
        .await;
    profiles_after_revoke.assert_status_ok();
    let after_revoke_body: Value = profiles_after_revoke.json();
    assert_eq!(after_revoke_body["data"]["known_device"], json!(false));
    assert_eq!(
        after_revoke_body["data"]["users"].as_array().unwrap().len(),
        0
    );

    let stale_refresh = server
        .post(v1::auth::REFRESH)
        .json(&json!({
            "refresh_token": pin_refresh_body["data"]["refresh_token"],
        }))
        .await;
    stale_refresh.assert_status(StatusCode::UNAUTHORIZED);

    Ok(())
}
