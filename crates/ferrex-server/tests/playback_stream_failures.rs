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
    tokio::fs::write(&media_path, b"0123456789").await?;
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
    assert!(
        ticket_body["data"]["access_token"]
            .as_str()
            .is_some_and(|token| !token.is_empty()),
        "ticket response should include a playback token"
    );

    let range = server
        .get(&playback_stream_path(file_id))
        .add_header("Authorization", bearer(&access_token))
        .add_header("Range", "bytes=2-5")
        .await;
    range.assert_status(StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        range
            .maybe_header(header::CONTENT_RANGE)
            .context("Content-Range header missing")?
            .to_str()?,
        "bytes 2-5/10",
    );
    assert!(
        range
            .maybe_header(HeaderName::from_static("x-media-error"))
            .is_none(),
        "successful playback responses must not carry recovery errors"
    );

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
    let missing_file_stream = server
        .get(&playback_stream_path(missing_file_id))
        .add_header("Authorization", bearer(&access_token))
        .await;
    missing_file_stream.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(media_error(&missing_file_stream)?, "file-missing");

    Ok(())
}
