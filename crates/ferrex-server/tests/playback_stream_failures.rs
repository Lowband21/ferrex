use anyhow::{Context, Result};
use axum::Router;
use axum::http::{HeaderName, StatusCode, header};
use axum_test::{TestResponse, TestServer};
use ferrex_core::api::routes::{utils as route_utils, v1};
#[cfg(feature = "native-mpv-e2e")]
use ferrex_player_playback::{
    contract::{
        BackendRequest, EndReason, PlaybackCommand, PlaybackFilePath,
        PlaybackScreenshotMode, PlaybackSnapshot, PlaybackSource,
        PlaybackState, PlaybackTarget, SessionGeneration,
    },
    session::PlaybackSession,
    video::open_playback_session,
};
use ferrex_server::infra::startup::NoopStartupHooks;
use serde_json::json;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::path::Path;
#[cfg(feature = "native-mpv-e2e")]
use std::{path::PathBuf, time::Duration};
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
    // Most transport tests use ten-byte fixtures, while the display-backed
    // acceptance test seeds a real generated media file. Preserve the
    // deliberate missing-file case with the historical placeholder size.
    let file_size = tokio::fs::metadata(path)
        .await
        .ok()
        .and_then(|metadata| i64::try_from(metadata.len()).ok())
        .unwrap_or(10);
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
    .bind(file_size)
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

#[cfg(feature = "native-mpv-e2e")]
fn transcode_start_path(file_id: Uuid) -> String {
    route_utils::replace_param(
        v1::transcode::START,
        "{id}",
        file_id.to_string(),
    )
}

#[cfg(feature = "native-mpv-e2e")]
fn transcode_status_path(job_id: &str) -> String {
    route_utils::replace_param(v1::transcode::STATUS, "{job_id}", job_id)
}

#[cfg(feature = "native-mpv-e2e")]
fn transcode_asset_path(file_id: Uuid, profile: &str, asset: &str) -> String {
    v1::transcode::ASSET
        .replace("{id}", &file_id.to_string())
        .replace("{profile}", profile)
        .replace("{asset}", asset)
}

#[cfg(feature = "native-mpv-e2e")]
async fn issue_playback_ticket(
    server: &TestServer,
    access_token: &str,
    file_id: Uuid,
) -> Result<String> {
    let response = server
        .get(&playback_ticket_path(file_id))
        .add_header("Authorization", bearer(access_token))
        .await;
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    let ticket = body["data"]["access_token"]
        .as_str()
        .context("ticket response should include a playback token")?
        .to_owned();
    anyhow::ensure!(!ticket.is_empty());
    Ok(ticket)
}

#[cfg(feature = "native-mpv-e2e")]
async fn generate_server_transcode(
    server: &TestServer,
    access_token: &str,
    file_id: Uuid,
    profile: &str,
) -> Result<String> {
    let start = server
        .post(&transcode_start_path(file_id))
        .add_header("Authorization", bearer(access_token))
        .json(&json!({ "profile": profile }))
        .await;
    start.assert_status_ok();
    let start_body: serde_json::Value = start.json();
    let job_id = start_body["data"]["job_id"]
        .as_str()
        .context("transcode start response should include a job ID")?
        .to_owned();
    anyhow::ensure!(!job_id.is_empty(), "transcode job ID was empty");

    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    loop {
        let status = server
            .get(&transcode_status_path(&job_id))
            .add_header("Authorization", bearer(access_token))
            .await;
        status.assert_status_ok();
        let body: serde_json::Value = status.json();
        let data = &body["data"];
        anyhow::ensure!(data["job_id"] == job_id);
        anyhow::ensure!(data["media_id"] == file_id.to_string());
        anyhow::ensure!(data["profile"] == profile);

        match data["state"].as_str() {
            Some("completed") => {
                anyhow::ensure!(data["progress"] == 1.0);
                let playback_path = data["playback_path"]
                    .as_str()
                    .context(
                        "completed transcode should publish a playback path",
                    )?
                    .to_owned();
                anyhow::ensure!(
                    playback_path
                        == transcode_asset_path(file_id, profile, "index.m3u8"),
                    "completed transcode published an unexpected playback path"
                );
                anyhow::ensure!(
                    !playback_path.contains('?')
                        && !playback_path.contains('#'),
                    "transcode playback path contained credentials or a fragment"
                );
                return Ok(playback_path);
            }
            Some("failed") => {
                anyhow::bail!(
                    "server transcode failed: {}",
                    data["message"].as_str().unwrap_or("no status message")
                );
            }
            Some("queued" | "running") => {}
            state => anyhow::bail!(
                "server transcode returned an unknown state: {state:?}"
            ),
        }

        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "server transcode did not complete within 45 seconds"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(feature = "native-mpv-e2e")]
async fn verify_server_transcode_assets(
    server: &TestServer,
    playback_path: &str,
    ticket: &str,
) -> Result<usize> {
    let unauthenticated = server.get(playback_path).await;
    unauthenticated.assert_status(StatusCode::UNAUTHORIZED);

    let manifest_response = server
        .get(playback_path)
        .add_header("Authorization", bearer(ticket))
        .await;
    manifest_response.assert_status_ok();
    assert_eq!(
        manifest_response
            .maybe_header(header::CONTENT_TYPE)
            .context("generated HLS manifest Content-Type is missing")?
            .to_str()?,
        "application/vnd.apple.mpegurl"
    );
    assert_success_has_no_media_error(&manifest_response);
    let manifest = manifest_response.text();
    anyhow::ensure!(manifest.starts_with("#EXTM3U"));
    anyhow::ensure!(manifest.contains("#EXT-X-ENDLIST"));
    anyhow::ensure!(
        !manifest.contains(ticket),
        "generated HLS manifest exposed its playback ticket"
    );

    let (asset_root, _) = playback_path
        .rsplit_once('/')
        .context("generated HLS playback path had no asset root")?;
    let mut segment_count = 0_usize;
    for line in manifest.lines() {
        let asset = line.trim();
        if asset.is_empty() || asset.starts_with('#') {
            continue;
        }
        anyhow::ensure!(
            asset.starts_with("segment-")
                && asset.ends_with(".ts")
                && !asset.contains('/')
                && !asset.contains('\\'),
            "generated HLS manifest contained an unsafe asset reference"
        );
        let asset_path = format!("{asset_root}/{asset}");

        let unauthenticated = server.get(&asset_path).await;
        unauthenticated.assert_status(StatusCode::UNAUTHORIZED);
        let segment = server
            .get(&asset_path)
            .add_header("Authorization", bearer(ticket))
            .await;
        segment.assert_status_ok();
        assert_eq!(
            segment
                .maybe_header(header::CONTENT_TYPE)
                .context("generated HLS segment Content-Type is missing")?
                .to_str()?,
            "video/mp2t"
        );
        assert_success_has_no_media_error(&segment);
        anyhow::ensure!(
            !segment.as_bytes().is_empty(),
            "generated HLS segment was empty"
        );
        segment_count += 1;
    }
    anyhow::ensure!(
        segment_count > 0,
        "generated HLS manifest contained no media segments"
    );
    Ok(segment_count)
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

    // In-process playback backends keep the scoped ticket out of the URL and
    // send it as a bearer header. Verify the exact player transport form, not
    // only the query compatibility path and a full account-session bearer.
    let bearer_ticket_range = server
        .get(&playback_stream_path(file_id))
        .add_header("Authorization", bearer(&ticket_token))
        .add_header("Range", "bytes=2-5")
        .await;
    bearer_ticket_range.assert_status(StatusCode::PARTIAL_CONTENT);
    assert_eq!(bearer_ticket_range.as_bytes().as_ref(), b"2345");
    assert_success_has_no_media_error(&bearer_ticket_range);

    let bearer_session_range = server
        .get(&playback_stream_path(file_id))
        .add_header("Authorization", bearer(&access_token))
        .add_header("Range", "bytes=2-5")
        .await;
    bearer_session_range.assert_status(StatusCode::PARTIAL_CONTENT);
    assert_eq!(bearer_session_range.as_bytes().as_ref(), b"2345");
    assert_success_has_no_media_error(&bearer_session_range);

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

#[cfg(feature = "native-mpv-e2e")]
async fn seed_router_backed_hls_fixture(
    pool: &PgPool,
    server: &TestServer,
    library_id: Uuid,
    source_manifest: &Path,
    output_root: &Path,
) -> Result<(Uuid, Vec<Uuid>)> {
    let source_root = source_manifest
        .parent()
        .context("HLS fixture manifest must have a parent directory")?;
    let source_root = std::fs::canonicalize(source_root)
        .context("could not canonicalize HLS fixture directory")?;
    let manifest = std::fs::read_to_string(source_manifest)
        .context("could not read HLS fixture manifest")?;
    let mut protected_manifest = String::with_capacity(manifest.len() * 2);
    let mut segment_ids = Vec::new();

    for line in manifest.lines() {
        let candidate = line.trim();
        if candidate.is_empty() || candidate.starts_with('#') {
            protected_manifest.push_str(line);
            protected_manifest.push('\n');
            continue;
        }

        let relative = Path::new(candidate);
        anyhow::ensure!(
            !relative.is_absolute()
                && relative.components().all(|component| matches!(
                    component,
                    std::path::Component::Normal(_)
                )),
            "HLS fixture contains a non-local segment path"
        );
        let segment = std::fs::canonicalize(source_root.join(relative))
            .context("HLS fixture segment is missing")?;
        anyhow::ensure!(
            segment.starts_with(&source_root),
            "HLS fixture segment escaped its fixture directory"
        );

        let segment_id = Uuid::new_v4();
        seed_media_file(
            pool,
            library_id,
            Uuid::new_v4(),
            segment_id,
            &segment,
            true,
        )
        .await;
        let segment_url =
            server.server_url(&playback_stream_path(segment_id))?;
        anyhow::ensure!(
            segment_url.query().is_none(),
            "protected HLS segment URL unexpectedly contains credentials"
        );
        protected_manifest.push_str(segment_url.as_str());
        protected_manifest.push('\n');
        segment_ids.push(segment_id);
    }

    anyhow::ensure!(
        !segment_ids.is_empty(),
        "HLS fixture manifest did not contain any segments"
    );
    let manifest_path = output_root.join("router-backed-transcoded-hls.m3u8");
    std::fs::write(&manifest_path, protected_manifest)
        .context("could not write router-backed HLS manifest")?;
    let manifest_id = Uuid::new_v4();
    seed_media_file(
        pool,
        library_id,
        Uuid::new_v4(),
        manifest_id,
        &manifest_path,
        true,
    )
    .await;
    Ok((manifest_id, segment_ids))
}

#[cfg(feature = "native-mpv-e2e")]
fn wait_for_native_mpv(
    session: &mut PlaybackSession,
    label: &str,
    timeout: Duration,
    predicate: impl Fn(&PlaybackSnapshot) -> bool,
) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        session.synchronize_snapshot();
        let snapshot = session.snapshot();
        if predicate(snapshot) {
            return Ok(());
        }
        if snapshot.state == PlaybackState::Failed {
            anyhow::bail!(
                "{label} failed through native mpv: {:?}",
                snapshot.last_error
            );
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!(
                "timed out waiting for {label}: state={:?}, end={:?}",
                snapshot.state,
                snapshot.end_reason
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(feature = "native-mpv-e2e")]
fn run_ferrex_native_mpv_smoke(
    source: PlaybackSource,
    forbidden_ticket: String,
    artifact_root: PathBuf,
) -> Result<()> {
    let mut session = open_playback_session(
        &source,
        Duration::from_millis(250),
        SessionGeneration::INITIAL,
        BackendRequest::Exact(PlaybackTarget::MPV_NATIVE_WINDOW),
    )?;
    anyhow::ensure!(
        session.snapshot().target == PlaybackTarget::MPV_NATIVE_WINDOW,
        "exact native-mpv request fell back: {:?}",
        session.diagnostics()
    );

    wait_for_native_mpv(
        &mut session,
        "authenticated Ferrex playback",
        Duration::from_secs(10),
        |snapshot| {
            matches!(
                snapshot.state,
                PlaybackState::Playing | PlaybackState::Paused
            ) && snapshot.duration.is_some()
                && snapshot.video.is_some()
                && !snapshot.tracks.audio.is_empty()
        },
    )?;
    anyhow::ensure!(
        session.snapshot().position >= Duration::from_millis(200),
        "mpv did not apply the requested resume offset"
    );
    anyhow::ensure!(session.snapshot().capabilities.seek);
    anyhow::ensure!(session.snapshot().capabilities.screenshot);
    anyhow::ensure!(session.snapshot().capabilities.video_shader_passthrough);

    let diagnostics = session.diagnostics();
    anyhow::ensure!(diagnostics.versions.client_api.is_some());
    anyhow::ensure!(diagnostics.versions.mpv.is_some());
    anyhow::ensure!(diagnostics.versions.ffmpeg.is_some());
    anyhow::ensure!(diagnostics.output.vo_configured == Some(true));
    let serialized = serde_json::to_string(&diagnostics)?;
    anyhow::ensure!(
        !serialized.contains(&forbidden_ticket),
        "playback diagnostics exposed the scoped bearer ticket"
    );

    session.apply_command(PlaybackCommand::SetPaused(true))?;
    wait_for_native_mpv(
        &mut session,
        "pause confirmation",
        Duration::from_secs(3),
        |snapshot| snapshot.state == PlaybackState::Paused,
    )?;

    let shader_path = artifact_root.join("ferrex-server-smoke.hook");
    let screenshot_path = artifact_root.join("ferrex-server-smoke.png");
    std::fs::write(
        &shader_path,
        "#!HOOK MAIN\n#!BIND HOOKED\nvec4 hook() { return HOOKED_tex(HOOKED_pos); }\n",
    )?;
    session
        .set_video_shaders(vec![PlaybackFilePath::new(shader_path.clone())])?;
    let shader_deadline = std::time::Instant::now() + Duration::from_secs(3);
    while session
        .diagnostics()
        .mpv_configuration
        .as_ref()
        .and_then(|config| config.active_video_shader_count)
        != Some(1)
        && std::time::Instant::now() < shader_deadline
    {
        session.synchronize_snapshot();
        std::thread::sleep(Duration::from_millis(20));
    }
    anyhow::ensure!(
        session
            .diagnostics()
            .mpv_configuration
            .as_ref()
            .and_then(|config| config.active_video_shader_count)
            == Some(1),
        "mpv did not confirm the smoke shader"
    );

    session.capture_screenshot(
        PlaybackFilePath::new(screenshot_path.clone()),
        PlaybackScreenshotMode::VideoWithSubtitles,
    )?;
    let screenshot_deadline =
        std::time::Instant::now() + Duration::from_secs(3);
    while std::fs::metadata(&screenshot_path)
        .map(|metadata| metadata.len() == 0)
        .unwrap_or(true)
        && std::time::Instant::now() < screenshot_deadline
    {
        session.synchronize_snapshot();
        std::thread::sleep(Duration::from_millis(20));
    }
    anyhow::ensure!(
        std::fs::metadata(&screenshot_path)
            .is_ok_and(|metadata| metadata.len() > 0),
        "mpv did not write the requested screenshot"
    );
    session.set_video_shaders(Vec::new())?;

    session.apply_command(PlaybackCommand::SeekAbsolute(
        Duration::from_millis(500),
    ))?;
    wait_for_native_mpv(
        &mut session,
        "authenticated range seek",
        Duration::from_secs(3),
        |snapshot| {
            snapshot.state != PlaybackState::Seeking
                && snapshot.position >= Duration::from_millis(400)
        },
    )?;
    session.apply_command(PlaybackCommand::SetPaused(false))?;
    wait_for_native_mpv(
        &mut session,
        "play confirmation",
        Duration::from_secs(3),
        |snapshot| snapshot.state == PlaybackState::Playing,
    )?;

    session.apply_command(PlaybackCommand::Stop)?;
    wait_for_native_mpv(
        &mut session,
        "ordered stop",
        Duration::from_secs(5),
        |snapshot| snapshot.end_reason == Some(EndReason::Stopped),
    )?;
    Ok(())
}

#[cfg(feature = "native-mpv-e2e")]
#[ignore = "requires generated fixtures, PostgreSQL, linked libmpv, and a working desktop VO"]
#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn playback_ticket_drives_display_backed_native_mpv_through_ferrex_router(
    pool: PgPool,
) -> Result<()> {
    let fixture = std::env::var_os("FERREX_MPV_SERVER_SMOKE_MEDIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/native-playback-fixtures/h264-sdr-8bit.mkv")
        });
    let fixture = std::fs::canonicalize(&fixture).with_context(|| {
        "native playback fixture is missing; run native_playback_fixtures.py generate"
    })?;

    let (server, tempdir) = build_server(pool.clone()).await?;
    let access_token =
        register_user(&server, "native_mpv_ferrex_router").await?;
    let library_id = Uuid::new_v4();
    seed_library(&pool, library_id).await;
    let file_id = Uuid::new_v4();
    seed_media_file(&pool, library_id, Uuid::new_v4(), file_id, &fixture, true)
        .await;

    let ticket = issue_playback_ticket(&server, &access_token, file_id).await?;

    let stream_url = server.server_url(&playback_stream_path(file_id))?;
    let source = PlaybackSource::new(stream_url)
        .with_header("Authorization", bearer(&ticket))
        .with_title("Ferrex authenticated native-mpv acceptance");
    let artifact_root = tempdir.path().to_path_buf();
    tokio::task::spawn_blocking(move || {
        run_ferrex_native_mpv_smoke(source, ticket, artifact_root)
    })
    .await
    .context("native-mpv acceptance worker panicked")??;

    Ok(())
}

#[cfg(feature = "native-mpv-e2e")]
#[ignore = "requires generated fixtures, PostgreSQL, FFmpeg, linked libmpv, and a working desktop VO"]
#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn server_generated_transcode_plays_through_display_backed_native_mpv(
    pool: PgPool,
) -> Result<()> {
    let fixture = std::env::var_os("FERREX_MPV_SERVER_TRANSCODE_MEDIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/native-playback-fixtures/h264-sdr-8bit.mkv")
        });
    let fixture = std::fs::canonicalize(&fixture).with_context(|| {
        "native playback fixture is missing; run native_playback_fixtures.py generate"
    })?;

    let (server, tempdir) = build_server(pool.clone()).await?;
    let access_token =
        register_user(&server, "native_mpv_server_transcode").await?;
    let library_id = Uuid::new_v4();
    seed_library(&pool, library_id).await;
    let file_id = Uuid::new_v4();
    seed_media_file(&pool, library_id, Uuid::new_v4(), file_id, &fixture, true)
        .await;

    let playback_path =
        generate_server_transcode(&server, &access_token, file_id, "360p")
            .await?;
    let ticket = issue_playback_ticket(&server, &access_token, file_id).await?;
    let segment_count =
        verify_server_transcode_assets(&server, &playback_path, &ticket)
            .await?;

    // Starting the same profile again must reuse the atomically published
    // rendition rather than exposing a partially regenerated playlist.
    let cached = server
        .post(&transcode_start_path(file_id))
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({ "profile": "360p" }))
        .await;
    cached.assert_status_ok();
    let cached_body: serde_json::Value = cached.json();
    anyhow::ensure!(cached_body["data"]["state"] == "completed");
    anyhow::ensure!(cached_body["data"]["progress"] == 1.0);
    anyhow::ensure!(cached_body["data"]["playback_path"] == playback_path);

    let stream_url = server.server_url(&playback_path)?;
    let source = PlaybackSource::new(stream_url)
        .with_header("Authorization", bearer(&ticket))
        .with_title("Ferrex server-generated transcode acceptance");
    let artifact_root = tempdir.path().to_path_buf();
    tokio::task::spawn_blocking(move || {
        run_ferrex_native_mpv_smoke(source, ticket, artifact_root)
    })
    .await
    .context("server-transcode native-mpv acceptance worker panicked")??;

    eprintln!(
        "server-generated transcode acceptance passed: profile=360p, segments={segment_count}"
    );
    Ok(())
}

#[cfg(feature = "native-mpv-e2e")]
#[ignore = "requires generated fixtures, PostgreSQL, linked libmpv, and a working desktop VO"]
#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn playback_ticket_propagates_to_every_router_backed_hls_segment(
    pool: PgPool,
) -> Result<()> {
    let source_manifest =
        std::env::var_os("FERREX_MPV_SERVER_SMOKE_HLS")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
                    "../../target/native-playback-fixtures/transcoded-hls/index.m3u8",
                )
            });
    let source_manifest = std::fs::canonicalize(&source_manifest)
        .with_context(|| {
            "native HLS fixture is missing; run native_playback_fixtures.py generate"
        })?;

    let (server, tempdir) = build_server(pool.clone()).await?;
    let access_token = register_user(&server, "native_mpv_hls_router").await?;
    let library_id = Uuid::new_v4();
    seed_library(&pool, library_id).await;
    let (manifest_id, segment_ids) = seed_router_backed_hls_fixture(
        &pool,
        &server,
        library_id,
        &source_manifest,
        tempdir.path(),
    )
    .await?;
    let ticket =
        issue_playback_ticket(&server, &access_token, manifest_id).await?;

    let manifest_response = server
        .get(&playback_stream_path(manifest_id))
        .add_header("Authorization", bearer(&ticket))
        .await;
    manifest_response.assert_status_ok();
    assert_eq!(
        manifest_response
            .maybe_header(header::CONTENT_TYPE)
            .context("HLS manifest Content-Type is missing")?
            .to_str()?,
        "application/vnd.apple.mpegurl"
    );
    let protected_manifest = manifest_response.text();
    anyhow::ensure!(
        !protected_manifest.contains(&ticket),
        "router-backed HLS manifest exposed its playback ticket"
    );

    for segment_id in &segment_ids {
        let unauthorized = server.get(&playback_stream_path(*segment_id)).await;
        unauthorized.assert_status(StatusCode::UNAUTHORIZED);

        let segment = server
            .get(&playback_stream_path(*segment_id))
            .add_header("Authorization", bearer(&ticket))
            .await;
        segment.assert_status_ok();
        assert_eq!(
            segment
                .maybe_header(header::CONTENT_TYPE)
                .context("HLS segment Content-Type is missing")?
                .to_str()?,
            "video/mp2t"
        );
        assert_success_has_no_media_error(&segment);
    }

    let stream_url = server.server_url(&playback_stream_path(manifest_id))?;
    let source = PlaybackSource::new(stream_url)
        .with_header("Authorization", bearer(&ticket))
        .with_title("Ferrex authenticated transcoded-HLS transport acceptance");
    let artifact_root = tempdir.path().to_path_buf();
    tokio::task::spawn_blocking(move || {
        run_ferrex_native_mpv_smoke(source, ticket, artifact_root)
    })
    .await
    .context("native-mpv HLS acceptance worker panicked")??;

    Ok(())
}
