use anyhow::Result;
use axum::Router;
use axum::http::StatusCode;
use axum_test::TestServer;
use chrono::Utc;
use serde_json::json;
use sqlx::PgPool;
use std::net::SocketAddr;

use ferrex_core::api_routes::v1;
use ferrex_server::users::setup::claim::reset_claim_rate_limiter_for_tests;

#[path = "support/mod.rs"]
mod support;
use support::build_test_app;

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn claim_flow_allows_admin_creation(pool: PgPool) -> Result<()> {
    reset_claim_rate_limiter_for_tests().await;
    let app = build_test_app(pool).await?;
    let (router, state, tempdir) = app.into_parts();
    let _tempdir = tempdir; // keep temp directory alive for test lifetime
    let router: Router<()> = router.with_state(state.clone());
    let make_service = router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let start = server
        .post(v1::setup::CLAIM_START)
        .json(&json!({"device_name": "Player"}))
        .await;
    start.assert_status_ok();
    let start_body: serde_json::Value = start.json();
    let claim_code = start_body["data"]["claim_code"]
        .as_str()
        .expect("claim code present")
        .to_string();

    let confirm = server
        .post(v1::setup::CLAIM_CONFIRM)
        .json(&json!({"claim_code": claim_code}))
        .await;
    confirm.assert_status_ok();
    let confirm_body: serde_json::Value = confirm.json();
    let claim_token = confirm_body["data"]["claim_token"]
        .as_str()
        .expect("claim token present")
        .to_string();

    let create_admin = server
        .post(v1::setup::CREATE_ADMIN)
        .json(&json!({
            "username": "primaryadmin",
            "display_name": "Administrator",
            "password": "StrongPass9",
            "claim_token": claim_token
        }))
        .await;
    create_admin.assert_status_ok();

    let second_attempt = server
        .post(v1::setup::CREATE_ADMIN)
        .json(&json!({
            "username": "admin2",
            "display_name": "Duplicate",
            "password": "StrongPass9",
            "claim_token": "ignored"
        }))
        .await;
    second_attempt.assert_status(StatusCode::FORBIDDEN);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn claim_reset_revokes_pending_codes(pool: PgPool) -> Result<()> {
    reset_claim_rate_limiter_for_tests().await;
    let app = build_test_app(pool).await?;
    let (router, state, tempdir) = app.into_parts();
    let _tempdir = tempdir; // keep temp directory alive for test lifetime
    let router: Router<()> = router.with_state(state.clone());
    let make_service = router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let start = server
        .post(v1::setup::CLAIM_START)
        .json(&json!({"device_name": "Living Room"}))
        .await;
    start.assert_status_ok();
    let start_body: serde_json::Value = start.json();
    let claim_code = start_body["data"]["claim_code"]
        .as_str()
        .expect("claim code present")
        .to_string();

    let confirm = server
        .post(v1::setup::CLAIM_CONFIRM)
        .json(&json!({"claim_code": claim_code}))
        .await;
    confirm.assert_status_ok();

    let revoked = state
        .setup_claim_service()
        .revoke_all(Some("integration-test reset"))
        .await?;
    assert_eq!(revoked, 1);

    state.setup_claim_service().purge_stale(Utc::now()).await?;

    let reuse = server
        .post(v1::setup::CLAIM_CONFIRM)
        .json(&json!({"claim_code": claim_code}))
        .await;
    reuse.assert_status(StatusCode::BAD_REQUEST);

    reset_claim_rate_limiter_for_tests().await;

    let restart = server
        .post(v1::setup::CLAIM_START)
        .json(&json!({"device_name": "Living Room"}))
        .await;
    restart.assert_status_ok();
    let restart_body: serde_json::Value = restart.json();
    let new_code = restart_body["data"]["claim_code"]
        .as_str()
        .expect("new claim code present");
    assert_ne!(new_code, claim_code);

    Ok(())
}
