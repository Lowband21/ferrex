use anyhow::Result;
use axum::Router;
use axum::http::{StatusCode, header};
use axum_test::TestServer;
use ferrex_core::api::routes::{utils as route_utils, v1};
use ferrex_server::infra::startup::NoopStartupHooks;
use serde_json::json;
use sqlx::PgPool;
use std::{net::SocketAddr, path::Path};
use uuid::Uuid;

mod common;
use common::build_test_app_with_hooks;

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn register_user(server: &TestServer, username: &str) -> String {
    let response = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": username,
            "password": "Password#123"
        }))
        .await;
    response.assert_status_ok();
    response.json::<serde_json::Value>()["data"]["access_token"]
        .as_str()
        .expect("registration token")
        .to_string()
}

async fn seed_library(pool: &PgPool, id: Uuid) {
    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, $2, 'movies', ARRAY['/tmp'])
        "#,
    )
    .bind(id)
    .bind(format!("transcode-test-{id}"))
    .execute(pool)
    .await
    .expect("insert library");
}

async fn seed_media_file(
    pool: &PgPool,
    library_id: Uuid,
    file_id: Uuid,
    path: &Path,
) {
    let size = tokio::fs::metadata(path)
        .await
        .expect("source metadata")
        .len();
    sqlx::query(
        r#"
        INSERT INTO media_files (
            id, library_id, media_id, media_type, file_path, filename,
            file_size, technical_metadata, is_available
        ) VALUES ($1, $2, $3, 'movie', $4, $5, $6, '{}'::jsonb, true)
        "#,
    )
    .bind(file_id)
    .bind(library_id)
    .bind(Uuid::new_v4())
    .bind(path.to_string_lossy().to_string())
    .bind("transcode-source.mkv")
    .bind(i64::try_from(size).expect("fixture size fits i64"))
    .execute(pool)
    .await
    .expect("insert media file");
}

fn start_path(media_id: Uuid) -> String {
    route_utils::replace_param(
        v1::transcode::START,
        "{id}",
        media_id.to_string(),
    )
}

fn status_path(job_id: &str) -> String {
    route_utils::replace_param(v1::transcode::STATUS, "{job_id}", job_id)
}

fn asset_path(media_id: Uuid, profile: &str, asset: &str) -> String {
    v1::transcode::ASSET
        .replace("{id}", &media_id.to_string())
        .replace("{profile}", profile)
        .replace("{asset}", asset)
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn cached_transcode_job_and_every_hls_asset_require_scoped_auth(
    pool: PgPool,
) -> Result<()> {
    let app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (router, state, _tempdir) = app.into_parts();

    let library_id = Uuid::new_v4();
    let media_id = Uuid::new_v4();
    seed_library(&pool, library_id).await;
    let source = state.config().cache_root().join("transcode-source.mkv");
    tokio::fs::write(&source, b"source fixture").await?;
    seed_media_file(&pool, library_id, media_id, &source).await;

    let rendition = state
        .config()
        .transcode_cache_dir()
        .join(media_id.to_string())
        .join("480p");
    tokio::fs::create_dir_all(&rendition).await?;
    tokio::fs::write(rendition.join("segment-00000.ts"), b"segment bytes")
        .await?;
    tokio::fs::write(
        rendition.join("index.m3u8"),
        b"#EXTM3U\n#EXT-X-VERSION:3\n#EXTINF:1.0,\nsegment-00000.ts\n#EXT-X-ENDLIST\n",
    )
    .await?;

    let router: Router<()> = router.with_state(state);
    let server = TestServer::builder()
        .http_transport()
        .build(router.into_make_service_with_connect_info::<SocketAddr>())
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let owner_token = register_user(&server, "transcode_owner").await;
    let other_token = register_user(&server, "transcode_other").await;

    let start = server
        .post(&start_path(media_id))
        .add_header("Authorization", bearer(&owner_token))
        .json(&json!({ "profile": "480p" }))
        .await;
    start.assert_status_ok();
    let start_body: serde_json::Value = start.json();
    assert_eq!(start_body["data"]["state"], "completed");
    assert_eq!(start_body["data"]["profile"], "480p");
    assert_eq!(
        start_body["data"]["playback_path"],
        asset_path(media_id, "480p", "index.m3u8")
    );
    let job_id = start_body["data"]["job_id"].as_str().expect("job id");

    let forbidden_status = server
        .get(&status_path(job_id))
        .add_header("Authorization", bearer(&other_token))
        .await;
    forbidden_status.assert_status(StatusCode::FORBIDDEN);

    let status = server
        .get(&status_path(job_id))
        .add_header("Authorization", bearer(&owner_token))
        .await;
    status.assert_status_ok();
    assert_eq!(
        status.json::<serde_json::Value>()["data"]["state"],
        "completed"
    );

    let ticket = server
        .get(&route_utils::replace_param(
            v1::stream::PLAYBACK_TICKET,
            "{id}",
            media_id.to_string(),
        ))
        .add_header("Authorization", bearer(&owner_token))
        .await;
    ticket.assert_status_ok();
    let ticket = ticket.json::<serde_json::Value>()["data"]["access_token"]
        .as_str()
        .expect("playback ticket")
        .to_string();

    for (asset, content_type, expected) in [
        (
            "index.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U".as_slice(),
        ),
        (
            "segment-00000.ts",
            "video/mp2t",
            b"segment bytes".as_slice(),
        ),
    ] {
        let path = asset_path(media_id, "480p", asset);
        let unauthenticated = server.get(&path).await;
        unauthenticated.assert_status(StatusCode::UNAUTHORIZED);

        let response = server
            .get(&path)
            .add_header("Authorization", bearer(&ticket))
            .await;
        response.assert_status_ok();
        assert_eq!(
            response
                .maybe_header(header::CONTENT_TYPE)
                .expect("content type"),
            content_type
        );
        assert!(response.as_bytes().starts_with(expected));
    }

    Ok(())
}
