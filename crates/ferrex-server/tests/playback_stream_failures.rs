use anyhow::{Context, Result};
use axum::Router;
use axum::http::{HeaderName, StatusCode, header};
use axum_test::{TestResponse, TestServer};
use ferrex_core::api::routes::{utils as route_utils, v1};
use ferrex_server::infra::startup::NoopStartupHooks;
use serde_json::json;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::path::Path;
use tempfile::TempDir;
use uuid::Uuid;

mod common;
use common::build_test_app_with_hooks;

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn extract_token_field<'a>(body: &'a serde_json::Value, key: &str) -> &'a str {
    body["data"][key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} missing"))
}

async fn build_server(pool: PgPool) -> Result<(TestServer, TempDir)> {
    let app = build_test_app_with_hooks(pool, &NoopStartupHooks).await?;
    let (router, state, tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state);
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok((server, tempdir))
}

async fn register_user(server: &TestServer, username: &str) -> Result<String> {
    let register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": username.replace('_', " "),
            "password": "Password#123"
        }))
        .await;
    register.assert_status_ok();
    let body: serde_json::Value = register.json();
    Ok(extract_token_field(&body, "access_token").to_string())
}

async fn device_login_user(
    server: &TestServer,
    username: &str,
) -> Result<serde_json::Value> {
    let response = server
        .post(v1::auth::device::LOGIN)
        .add_header("user-agent", "FerrexAndroid/1.0 (Android TV)")
        .json(&json!({
            "username": username,
            "password": "Password#123",
            "remember_device": false,
            "device_info": {
                "device_id": Uuid::new_v4(),
                "device_name": "Living Room TV",
                "platform": "android",
                "app_version": "2.3.4",
                "hardware_id": "playback-ticket-test-hw"
            }
        }))
        .await;
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    Ok(body["data"].clone())
}

async fn seed_library(pool: &PgPool, id: Uuid) {
    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, $2, 'movies', ARRAY['/tmp'])
        "#,
    )
    .bind(id)
    .bind(format!("playback-test-{id}"))
    .execute(pool)
    .await
    .expect("insert library");
}

async fn seed_media_file(
    pool: &PgPool,
    library_id: Uuid,
    logical_media_id: Uuid,
    file_id: Uuid,
    path: &Path,
    is_available: bool,
) {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("playback-test.mkv");
    sqlx::query(
        r#"
        INSERT INTO media_files (
            id, library_id, media_id, media_type, file_path, filename,
            file_size, technical_metadata, is_available
        ) VALUES ($1, $2, $3, 'movie', $4, $5, $6, $7, $8)
        "#,
    )
    .bind(file_id)
    .bind(library_id)
    .bind(logical_media_id)
    .bind(path.to_string_lossy().to_string())
    .bind(filename)
    .bind(10_i64)
    .bind(json!({ "duration": "legacy-invalid-shape" }))
    .bind(is_available)
    .execute(pool)
    .await
    .expect("insert media_file");
}

fn playback_ticket_path(file_id: Uuid) -> String {
    route_utils::replace_param(
        v1::stream::PLAYBACK_TICKET,
        "{id}",
        file_id.to_string(),
    )
}

fn playback_stream_path(file_id: Uuid) -> String {
    route_utils::replace_param(v1::stream::PLAY, "{id}", file_id.to_string())
}

fn playback_stream_ticket_path(file_id: Uuid, access_token: &str) -> String {
    route_utils::with_query(
        &playback_stream_path(file_id),
        &[("access_token", access_token)],
    )
}

fn assert_success_has_no_media_error(response: &TestResponse) {
    assert!(
        response
            .maybe_header(HeaderName::from_static("x-media-error"))
            .is_none(),
        "successful playback responses must not carry recovery errors"
    );
}

fn assert_not_media_response(response: &TestResponse, media_bytes: &[u8]) {
    assert_ne!(
        response.as_bytes().as_ref(),
        media_bytes,
        "auth failures must not stream media bytes"
    );
    assert!(
        response.maybe_header(header::ACCEPT_RANGES).is_none(),
        "auth failures must not advertise media streaming headers"
    );
}

fn assert_response_does_not_expose_token(response: &TestResponse, token: &str) {
    assert!(
        !response.text().contains(token),
        "response body exposed a raw token"
    );
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            assert!(
                !value.contains(token),
                "response header {name} exposed a raw token"
            );
        }
    }
}

fn media_error(response: &TestResponse) -> Result<String> {
    response
        .maybe_header(HeaderName::from_static("x-media-error"))
        .context("X-Media-Error header missing")?
        .to_str()
        .context("X-Media-Error header must be UTF-8")
        .map(ToOwned::to_owned)
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn playback_ticket_and_range_stream_ignore_corrupt_technical_metadata(
    pool: PgPool,
) -> Result<()> {
    let (server, tempdir) = build_server(pool.clone()).await?;
    let access_token =
        register_user(&server, "playback_legacy_metadata").await?;
    let library_id = Uuid::new_v4();
    seed_library(&pool, library_id).await;

    let logical_media_id = Uuid::new_v4();
    let file_id = Uuid::new_v4();
    let media_path = tempdir.path().join("legacy-corrupt-metadata.mkv");
    let media_bytes = b"0123456789";
    tokio::fs::write(&media_path, media_bytes).await?;
    seed_media_file(
        &pool,
        library_id,
        logical_media_id,
        file_id,
        &media_path,
        true,
    )
    .await;

    let ticket = server
        .get(&playback_ticket_path(file_id))
        .add_header("Authorization", bearer(&access_token))
        .await;
    ticket.assert_status_ok();
    let ticket_body: serde_json::Value = ticket.json();
    assert_eq!(ticket_body["status"], "success");
    let ticket_token = ticket_body["data"]["access_token"]
        .as_str()
        .context("ticket response should include a playback token")?
        .to_string();
    assert!(!ticket_token.is_empty());
    let expires_in = ticket_body["data"]["expires_in"]
        .as_i64()
        .context("ticket response should include an expiry")?;
    assert!(
        (1..=6 * 60 * 60).contains(&expires_in),
        "ticket expiry should be positive and no longer than the configured lifetime"
    );

    let full_ticket_path = playback_stream_ticket_path(file_id, &ticket_token);
    let full_ticket_stream = server.get(&full_ticket_path).await;
    full_ticket_stream.assert_status_ok();
    assert_eq!(full_ticket_stream.as_bytes().as_ref(), media_bytes);
    assert_success_has_no_media_error(&full_ticket_stream);

    let ranged_ticket_path =
        playback_stream_ticket_path(file_id, &ticket_token);
    let ranged_ticket_stream = server
        .get(&ranged_ticket_path)
        .add_header("Range", "bytes=2-5")
        .await;
    ranged_ticket_stream.assert_status(StatusCode::PARTIAL_CONTENT);
    assert_eq!(ranged_ticket_stream.as_bytes().as_ref(), b"2345");
    assert_eq!(
        ranged_ticket_stream
            .maybe_header(header::CONTENT_RANGE)
            .context("Content-Range header missing")?
            .to_str()?,
        "bytes 2-5/10",
    );
    assert_success_has_no_media_error(&ranged_ticket_stream);

    let bearer_range = server
        .get(&playback_stream_path(file_id))
        .add_header("Authorization", bearer(&access_token))
        .add_header("Range", "bytes=2-5")
        .await;
    bearer_range.assert_status(StatusCode::PARTIAL_CONTENT);
    assert_eq!(bearer_range.as_bytes().as_ref(), b"2345");
    assert_success_has_no_media_error(&bearer_range);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn device_bound_playback_tickets_coexist_with_full_device_session(
    pool: PgPool,
) -> Result<()> {
    let (server, tempdir) = build_server(pool.clone()).await?;
    let username = "playback_device_ticket";
    register_user(&server, username).await?;
    let device_login = device_login_user(&server, username).await?;
    assert_eq!(device_login["scope"], "full");
    let access_token = device_login["access_token"]
        .as_str()
        .context("device login should return an access token")?
        .to_string();
    let device_session_id: Uuid =
        serde_json::from_value(device_login["device_session_id"].clone())?;

    let library_id = Uuid::new_v4();
    seed_library(&pool, library_id).await;

    let file_id = Uuid::new_v4();
    let media_path = tempdir.path().join("device-bound-ticket.mkv");
    let media_bytes = b"0123456789";
    tokio::fs::write(&media_path, media_bytes).await?;
    seed_media_file(
        &pool,
        library_id,
        Uuid::new_v4(),
        file_id,
        &media_path,
        true,
    )
    .await;

    let first_ticket = server
        .get(&playback_ticket_path(file_id))
        .add_header("Authorization", bearer(&access_token))
        .await;
    first_ticket.assert_status_ok();
    let first_ticket_body: serde_json::Value = first_ticket.json();
    let first_ticket_token = first_ticket_body["data"]["access_token"]
        .as_str()
        .context("ticket response should include a playback token")?
        .to_string();

    let current_user = server
        .get(v1::users::CURRENT)
        .add_header("Authorization", bearer(&access_token))
        .await;
    current_user.assert_status_ok();

    let second_ticket = server
        .get(&playback_ticket_path(file_id))
        .add_header("Authorization", bearer(&access_token))
        .await;
    second_ticket.assert_status_ok();
    let second_ticket_body: serde_json::Value = second_ticket.json();
    let second_ticket_token = second_ticket_body["data"]["access_token"]
        .as_str()
        .context("second ticket response should include a playback token")?
        .to_string();

    let full_ticket_path =
        playback_stream_ticket_path(file_id, &first_ticket_token);
    let full_ticket_stream = server.get(&full_ticket_path).await;
    full_ticket_stream.assert_status_ok();
    assert_eq!(full_ticket_stream.as_bytes().as_ref(), media_bytes);
    assert_success_has_no_media_error(&full_ticket_stream);

    let ranged_ticket_path =
        playback_stream_ticket_path(file_id, &second_ticket_token);
    let ranged_ticket_stream = server
        .get(&ranged_ticket_path)
        .add_header("Range", "bytes=2-5")
        .await;
    ranged_ticket_stream.assert_status(StatusCode::PARTIAL_CONTENT);
    assert_eq!(ranged_ticket_stream.as_bytes().as_ref(), b"2345");
    assert_success_has_no_media_error(&ranged_ticket_stream);

    let revoke = server
        .post(v1::auth::device::REVOKE)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({ "device_id": device_session_id }))
        .await;
    revoke.assert_status_ok();

    let revoked_ticket_stream = server.get(&full_ticket_path).await;
    revoked_ticket_stream.assert_status(StatusCode::UNAUTHORIZED);
    assert_not_media_response(&revoked_ticket_stream, media_bytes);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn playback_stream_auth_rejects_missing_and_invalid_tokens_without_serving_media(
    pool: PgPool,
) -> Result<()> {
    let (server, tempdir) = build_server(pool.clone()).await?;
    let library_id = Uuid::new_v4();
    seed_library(&pool, library_id).await;

    let file_id = Uuid::new_v4();
    let media_path = tempdir.path().join("protected-media.mkv");
    let media_bytes = b"0123456789";
    tokio::fs::write(&media_path, media_bytes).await?;
    seed_media_file(
        &pool,
        library_id,
        Uuid::new_v4(),
        file_id,
        &media_path,
        true,
    )
    .await;

    let stream_path = playback_stream_path(file_id);
    let missing = server.get(&stream_path).await;
    missing.assert_status(StatusCode::UNAUTHORIZED);
    assert_not_media_response(&missing, media_bytes);

    let invalid_token = "invalid-stream-token-secret";
    let invalid_query_path =
        playback_stream_ticket_path(file_id, invalid_token);
    let invalid_query = server.get(&invalid_query_path).await;
    invalid_query.assert_status(StatusCode::UNAUTHORIZED);
    assert_not_media_response(&invalid_query, media_bytes);
    assert_response_does_not_expose_token(&invalid_query, invalid_token);

    let invalid_bearer = server
        .get(&stream_path)
        .add_header("Authorization", bearer(invalid_token))
        .await;
    invalid_bearer.assert_status(StatusCode::UNAUTHORIZED);
    assert_not_media_response(&invalid_bearer, media_bytes);
    assert_response_does_not_expose_token(&invalid_bearer, invalid_token);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn playback_tickets_are_rejected_by_account_apis(
    pool: PgPool,
) -> Result<()> {
    let (server, tempdir) = build_server(pool.clone()).await?;
    let access_token =
        register_user(&server, "playback_scope_account_guard").await?;
    let library_id = Uuid::new_v4();
    seed_library(&pool, library_id).await;

    let file_id = Uuid::new_v4();
    let media_path = tempdir.path().join("scope-guard.mkv");
    tokio::fs::write(&media_path, b"0123456789").await?;
    seed_media_file(
        &pool,
        library_id,
        Uuid::new_v4(),
        file_id,
        &media_path,
        true,
    )
    .await;

    let ticket = server
        .get(&playback_ticket_path(file_id))
        .add_header("Authorization", bearer(&access_token))
        .await;
    ticket.assert_status_ok();
    let ticket_body: serde_json::Value = ticket.json();
    let ticket_token = ticket_body["data"]["access_token"]
        .as_str()
        .context("ticket response should include a playback token")?
        .to_string();

    let current_user = server
        .get(v1::users::CURRENT)
        .add_header("Authorization", bearer(&ticket_token))
        .await;
    current_user.assert_status(StatusCode::FORBIDDEN);
    assert_response_does_not_expose_token(&current_user, &ticket_token);

    let admin_users = server
        .get(v1::admin::USERS)
        .add_header("Authorization", bearer(&ticket_token))
        .await;
    admin_users.assert_status(StatusCode::FORBIDDEN);
    assert_response_does_not_expose_token(&admin_users, &ticket_token);

    let query_account_path = route_utils::with_query(
        v1::users::CURRENT,
        &[("access_token", &ticket_token)],
    );
    let query_account = server.get(&query_account_path).await;
    query_account.assert_status(StatusCode::UNAUTHORIZED);
    assert_response_does_not_expose_token(&query_account, &ticket_token);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn playback_availability_failures_return_typed_recovery_headers(
    pool: PgPool,
) -> Result<()> {
    let (server, tempdir) = build_server(pool.clone()).await?;
    let access_token = register_user(&server, "playback_typed_errors").await?;
    let library_id = Uuid::new_v4();
    seed_library(&pool, library_id).await;

    let missing_media_id = Uuid::new_v4();
    let missing_ticket = server
        .get(&playback_ticket_path(missing_media_id))
        .add_header("Authorization", bearer(&access_token))
        .await;
    missing_ticket.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(media_error(&missing_ticket)?, "media-not-found");

    let unavailable_id = Uuid::new_v4();
    let unavailable_path = tempdir.path().join("unavailable.mkv");
    tokio::fs::write(&unavailable_path, b"0123456789").await?;
    seed_media_file(
        &pool,
        library_id,
        Uuid::new_v4(),
        unavailable_id,
        &unavailable_path,
        false,
    )
    .await;
    let unavailable_ticket = server
        .get(&playback_ticket_path(unavailable_id))
        .add_header("Authorization", bearer(&access_token))
        .await;
    unavailable_ticket.assert_status(StatusCode::GONE);
    assert_eq!(media_error(&unavailable_ticket)?, "media-unavailable");

    let missing_file_id = Uuid::new_v4();
    let missing_file_path = tempdir.path().join("missing-on-disk.mkv");
    seed_media_file(
        &pool,
        library_id,
        Uuid::new_v4(),
        missing_file_id,
        &missing_file_path,
        true,
    )
    .await;
    let missing_file_ticket = server
        .get(&playback_ticket_path(missing_file_id))
        .add_header("Authorization", bearer(&access_token))
        .await;
    missing_file_ticket.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(media_error(&missing_file_ticket)?, "file-missing");

    let missing_file_stream = server
        .get(&playback_stream_path(missing_file_id))
        .add_header("Authorization", bearer(&access_token))
        .await;
    missing_file_stream.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(media_error(&missing_file_stream)?, "file-missing");

    Ok(())
}
