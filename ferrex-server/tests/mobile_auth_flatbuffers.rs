use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{Router, body::Bytes, http::StatusCode, http::header};
use axum_test::{TestResponse, TestServer};
use ferrex_core::api::routes::v1;
use ferrex_flatbuffers::{FLATBUFFERS_MIME, fb, uuid_helpers::fb_to_uuid};
use ferrex_server::infra::{app_state::AppState, startup::NoopStartupHooks};
use flatbuffers::FlatBufferBuilder;
use serde_json::{Value, json};
use sqlx::PgPool;
use tempfile::TempDir;

mod common;
use common::build_test_app_with_hooks;

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn build_server(pool: PgPool) -> Result<(TestServer, AppState, TempDir)> {
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

fn content_type(response: &TestResponse) -> Result<String> {
    response
        .maybe_header(header::CONTENT_TYPE)
        .context("Content-Type header missing")?
        .to_str()
        .context("Content-Type header must be UTF-8")
        .map(ToOwned::to_owned)
}

fn assert_content_type_starts_with(
    response: &TestResponse,
    expected: &str,
) -> Result<()> {
    let content_type = content_type(response)?;
    assert!(
        content_type.starts_with(expected),
        "expected Content-Type {expected}, got {content_type}"
    );
    Ok(())
}

fn fb_login_request(username: &str, password: &str) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let username = builder.create_string(username);
    let password = builder.create_string(password);
    let request = fb::auth::LoginRequest::create(
        &mut builder,
        &fb::auth::LoginRequestArgs {
            username: Some(username),
            password: Some(password),
            device_name: None,
        },
    );
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

fn fb_refresh_request(refresh_token: &str) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let refresh_token = builder.create_string(refresh_token);
    let request = fb::auth::RefreshRequest::create(
        &mut builder,
        &fb::auth::RefreshRequestArgs {
            refresh_token: Some(refresh_token),
        },
    );
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

fn extract_json_token_field<'a>(body: &'a Value, key: &str) -> &'a str {
    body["data"][key]
        .as_str()
        .unwrap_or_else(|| panic!("missing JSON token field {key}"))
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn setup_login_refresh_and_current_user_support_flatbuffers(
    pool: PgPool,
) -> Result<()> {
    let (server, _state, _tempdir) = build_server(pool).await?;

    let json_setup = server
        .get(v1::setup::STATUS)
        .add_header("Accept", "application/json")
        .await;
    json_setup.assert_status_ok();
    assert_content_type_starts_with(&json_setup, "application/json")?;
    let json_setup_body: Value = json_setup.json();
    assert_eq!(json_setup_body["data"]["needs_setup"], true);

    let fb_setup = server
        .get(v1::setup::STATUS)
        .add_header("Accept", FLATBUFFERS_MIME)
        .await;
    fb_setup.assert_status_ok();
    assert_content_type_starts_with(&fb_setup, FLATBUFFERS_MIME)?;
    let setup = flatbuffers::root::<fb::auth::SetupStatus>(
        fb_setup.as_bytes().as_ref(),
    )?;
    assert!(setup.needs_setup());
    assert!(!setup.has_admin());
    assert_eq!(setup.user_count(), 0);
    assert!(setup.admin_password_policy().is_some());
    let setup_pin_policy = setup.pin_policy().expect("setup pin policy");
    assert_eq!(setup_pin_policy.min_length(), 4);
    assert_eq!(setup_pin_policy.max_length(), 8);
    let setup_device_trust_policy = setup
        .device_trust_policy()
        .expect("setup device trust policy");
    assert_eq!(setup_device_trust_policy.pin_max_attempts(), 3);
    assert_eq!(setup_device_trust_policy.trust_duration_days(), 30);

    let username = "mobile_fb_user";
    let password = "Password#123";
    let display_name = "Mobile FlatBuffers User";
    let register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": display_name,
            "password": password,
        }))
        .await;
    register.assert_status_ok();
    let register_body: Value = register.json();
    let registered_user_id = register_body["data"]["user_id"]
        .as_str()
        .expect("register returns user_id")
        .to_string();

    let json_login = server
        .post(v1::auth::LOGIN)
        .add_header("Accept", "application/json")
        .json(&json!({
            "username": username,
            "password": password,
        }))
        .await;
    json_login.assert_status_ok();
    assert_content_type_starts_with(&json_login, "application/json")?;
    let json_login_body: Value = json_login.json();
    assert!(
        !extract_json_token_field(&json_login_body, "access_token").is_empty()
    );

    let login = server
        .post(v1::auth::LOGIN)
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(fb_login_request(username, password)))
        .await;
    login.assert_status_ok();
    assert_content_type_starts_with(&login, FLATBUFFERS_MIME)?;
    let token =
        flatbuffers::root::<fb::auth::AuthToken>(login.as_bytes().as_ref())?;
    assert!(!token.access_token().is_empty());
    assert!(!token.refresh_token().is_empty());
    assert_eq!(
        token.user_id().map(fb_to_uuid).unwrap().to_string(),
        registered_user_id
    );
    let access_token = token.access_token().to_string();
    let refresh_token = token.refresh_token().to_string();

    let refresh = server
        .post(v1::auth::REFRESH)
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(fb_refresh_request(&refresh_token)))
        .await;
    refresh.assert_status_ok();
    assert_content_type_starts_with(&refresh, FLATBUFFERS_MIME)?;
    let refreshed_token =
        flatbuffers::root::<fb::auth::AuthToken>(refresh.as_bytes().as_ref())?;
    assert!(!refreshed_token.access_token().is_empty());
    assert!(!refreshed_token.refresh_token().is_empty());

    let me = server
        .get(v1::users::CURRENT)
        .add_header("Authorization", bearer(&access_token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .await;
    me.assert_status_ok();
    assert_content_type_starts_with(&me, FLATBUFFERS_MIME)?;
    let profile =
        flatbuffers::root::<fb::auth::UserProfile>(me.as_bytes().as_ref())?;
    assert_eq!(fb_to_uuid(profile.id()).to_string(), registered_user_id);
    assert_eq!(profile.username(), username);
    assert_eq!(profile.display_name(), display_name);
    assert!(
        profile
            .created_at()
            .is_some_and(|timestamp| timestamp.millis() > 0),
        "current-user FlatBuffers profile should include created_at"
    );

    let unauthenticated = server
        .get(v1::users::CURRENT)
        .add_header("Accept", FLATBUFFERS_MIME)
        .await;
    unauthenticated.assert_status(StatusCode::UNAUTHORIZED);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn invalid_flatbuffers_login_body_returns_json_bad_request(
    pool: PgPool,
) -> Result<()> {
    let (server, _state, _tempdir) = build_server(pool).await?;

    let response = server
        .post(v1::auth::LOGIN)
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from_static(b"not a flatbuffer"))
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_content_type_starts_with(&response, "application/json")?;
    let body: Value = response.json();
    let message = body["error"]["message"]
        .as_str()
        .expect("error message present");
    assert!(
        message.contains("Invalid FlatBuffers request body"),
        "unexpected error message: {message}"
    );

    Ok(())
}
