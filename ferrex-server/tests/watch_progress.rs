use anyhow::Result;
use axum::Router;
use axum::http::{StatusCode, header::ACCEPT};
use axum_test::TestServer;
use ferrex_core::api::routes::utils as route_utils;
use ferrex_core::api::routes::v1;
use ferrex_flatbuffers::{
    fb,
    uuid_helpers::{fb_to_uuid, uuid_to_fb},
};
use ferrex_server::infra::startup::NoopStartupHooks;
use flatbuffers::FlatBufferBuilder;
use serde_json::json;
use sqlx::PgPool;
use std::net::SocketAddr;
use uuid::Uuid;

mod common;
use common::build_test_app_with_hooks;

fn bearer(token: &str) -> String {
    format!("Bearer {}", token)
}

fn extract_token_field<'a>(body: &'a serde_json::Value, key: &str) -> &'a str {
    body["data"][key]
        .as_str()
        .unwrap_or_else(|| panic!("{} missing", key))
}

fn build_watch_progress_buffer(
    media_id: Uuid,
    position: f64,
    duration: f64,
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let media_id = uuid_to_fb(&media_id);
    let timestamp = fb::common::Timestamp::new(1_735_689_600_000);
    let request = fb::watch::WatchProgressUpdate::create(
        &mut builder,
        &fb::watch::WatchProgressUpdateArgs {
            media_id: Some(&media_id),
            position,
            duration,
            timestamp: Some(&timestamp),
        },
    );
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

async fn seed_library(pool: &PgPool, id: Uuid, library_type: &str) {
    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, $2, $3, ARRAY['/tmp'])
        "#,
    )
    .bind(id)
    .bind(format!("test-{library_type}"))
    .bind(library_type)
    .execute(pool)
    .await
    .expect("insert library");
}

async fn seed_movie(
    pool: &PgPool,
    library_id: Uuid,
    movie_id: Uuid,
    file_id: Uuid,
    tmdb_id: i64,
    title: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO media_files (id, library_id, media_id, media_type, file_path, filename, file_size)
        VALUES ($1, $2, $3, 'movie', $4, $5, 123)
        "#,
    )
    .bind(file_id)
    .bind(library_id)
    .bind(movie_id)
    .bind(format!("/tmp/{movie_id}.mkv"))
    .bind(format!("{movie_id}.mkv"))
    .execute(pool)
    .await
    .expect("insert media_file");

    sqlx::query(
        r#"
        INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title, batch_id)
        VALUES ($1, $2, $3, $4, $5, 1)
        "#,
    )
    .bind(movie_id)
    .bind(library_id)
    .bind(file_id)
    .bind(tmdb_id)
    .bind(title)
    .execute(pool)
    .await
    .expect("insert movie_reference");
}

async fn seed_series(
    pool: &PgPool,
    library_id: Uuid,
    series_id: Uuid,
    tmdb_id: i64,
    title: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO series (id, library_id, tmdb_id, title)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(series_id)
    .bind(library_id)
    .bind(tmdb_id)
    .bind(title)
    .execute(pool)
    .await
    .expect("insert series");
}

async fn seed_season(
    pool: &PgPool,
    library_id: Uuid,
    series_id: Uuid,
    season_id: Uuid,
    tmdb_series_id: i64,
    season_number: i16,
) {
    sqlx::query(
        r#"
        INSERT INTO season_references (id, series_id, season_number, tmdb_series_id, library_id)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(season_id)
    .bind(series_id)
    .bind(season_number)
    .bind(tmdb_series_id)
    .bind(library_id)
    .execute(pool)
    .await
    .expect("insert season_reference");
}

async fn seed_episode(
    pool: &PgPool,
    library_id: Uuid,
    series_id: Uuid,
    season_id: Uuid,
    episode_id: Uuid,
    file_id: Uuid,
    tmdb_series_id: i64,
    season_number: i16,
    episode_number: i16,
    title: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO media_files (id, library_id, media_id, media_type, file_path, filename, file_size)
        VALUES ($1, $2, $3, 'episode', $4, $5, 456)
        "#,
    )
    .bind(file_id)
    .bind(library_id)
    .bind(episode_id)
    .bind(format!("/tmp/{episode_id}.mkv"))
    .bind(format!("{episode_id}.mkv"))
    .execute(pool)
    .await
    .expect("insert episode media_file");

    sqlx::query(
        r#"
        INSERT INTO episode_references (
            id, series_id, season_id, file_id,
            season_number, episode_number, tmdb_series_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(episode_id)
    .bind(series_id)
    .bind(season_id)
    .bind(file_id)
    .bind(season_number)
    .bind(episode_number)
    .bind(tmdb_series_id)
    .execute(pool)
    .await
    .expect("insert episode_reference");

    sqlx::query(
        r#"
        INSERT INTO episode_metadata (
            episode_id, tmdb_id, series_tmdb_id,
            season_number, episode_number, name
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(episode_id)
    .bind(
        tmdb_series_id * 10_000
            + i64::from(season_number) * 100
            + i64::from(episode_number),
    )
    .bind(tmdb_series_id)
    .bind(i32::from(season_number))
    .bind(i32::from(episode_number))
    .bind(title)
    .execute(pool)
    .await
    .expect("insert episode_metadata");
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn progress_update_rejects_negative_completion_sentinel(
    pool: PgPool,
) -> Result<()> {
    let app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (router, state, _tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state);
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": "negative_progress_user",
            "display_name": "Negative Progress",
            "password": "Password#123"
        }))
        .await;
    register.assert_status_ok();
    let body: serde_json::Value = register.json();
    let access_token = extract_token_field(&body, "access_token").to_string();

    let response = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "media_id": Uuid::new_v4(),
            "media_type": "Movie",
            "position": -1.0,
            "duration": -1.0
        }))
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn flatbuffers_watch_progress_resolves_file_ids_but_preserves_flatbuffer_compat(
    pool: PgPool,
) -> Result<()> {
    let app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (router, state, _tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state);
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": "watch_progress_user",
            "display_name": "Watch Progress",
            "password": "Password#123"
        }))
        .await;
    register.assert_status_ok();
    let body: serde_json::Value = register.json();
    let access_token = extract_token_field(&body, "access_token").to_string();
    let user_id = Uuid::parse_str(extract_token_field(&body, "user_id"))?;

    let library_id = Uuid::new_v4();
    let movie_id = Uuid::new_v4();
    let file_id = Uuid::new_v4();
    seed_library(&pool, library_id, "movies").await;
    seed_movie(&pool, library_id, movie_id, file_id, 101, "Compat Movie").await;

    let request_bytes = build_watch_progress_buffer(file_id, 42.0, 120.0);
    let response = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .add_header(ACCEPT, "application/x-flatbuffers")
        .content_type("application/x-flatbuffers")
        .bytes(request_bytes.into())
        .await;
    response.assert_status(StatusCode::NO_CONTENT);

    let stored = sqlx::query_as::<_, (Uuid, i16, f32, f32)>(
        r#"
        SELECT media_uuid, media_type, position, duration
        FROM user_watch_progress
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        stored.0, movie_id,
        "watch progress should be stored against the logical movie id"
    );
    assert_eq!(stored.1, 0, "movie media_type should be persisted");
    assert_eq!(stored.2, 42.0);
    assert_eq!(stored.3, 120.0);

    let state_response = server
        .get(v1::watch::STATE)
        .add_header("Authorization", bearer(&access_token))
        .add_header(ACCEPT, "application/x-flatbuffers")
        .await;
    state_response.assert_status_ok();
    let state_bytes = state_response.into_bytes();
    let watch_state = fb::watch::root_as_watch_state(state_bytes.as_ref())?;
    let state_items = watch_state.items().expect("watch state items");
    assert_eq!(state_items.len(), 1);
    let state_entry = state_items.get(0);
    assert_eq!(
        fb_to_uuid(state_entry.media_id()),
        file_id,
        "FlatBuffers watch-state response should stay file-id keyed for current Android clients"
    );

    let continue_response = server
        .get(v1::watch::CONTINUE)
        .add_header("Authorization", bearer(&access_token))
        .add_header(ACCEPT, "application/x-flatbuffers")
        .await;
    continue_response.assert_status_ok();
    let continue_bytes = continue_response.into_bytes();
    let continue_list = flatbuffers::root::<fb::watch::ContinueWatchingList<'_>>(
        continue_bytes.as_ref(),
    )?;
    let continue_items =
        continue_list.items().expect("continue watching items");
    assert_eq!(continue_items.len(), 1);
    let continue_entry = continue_items.get(0);
    assert_eq!(
        fb_to_uuid(continue_entry.media_id()),
        file_id,
        "FlatBuffers continue-watching response should keep playback file ids during the compatibility bridge"
    );
    assert_eq!(
        continue_entry.media_type(),
        fb::common::VideoMediaType::Movie,
        "continue-watching entries should carry the resolved media type"
    );
    assert_eq!(
        continue_entry.title(),
        Some("Compat Movie"),
        "continue-watching entries should include display titles for Android"
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn mark_completed_route_resolves_media_type_without_path_type_param(
    pool: PgPool,
) -> Result<()> {
    let app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (router, state, _tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state);
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": "mark_complete_user",
            "display_name": "Mark Complete",
            "password": "Password#123"
        }))
        .await;
    register.assert_status_ok();
    let body: serde_json::Value = register.json();
    let access_token = extract_token_field(&body, "access_token").to_string();
    let user_id = Uuid::parse_str(extract_token_field(&body, "user_id"))?;

    let library_id = Uuid::new_v4();
    let movie_id = Uuid::new_v4();
    let file_id = Uuid::new_v4();
    seed_library(&pool, library_id, "movies").await;
    seed_movie(&pool, library_id, movie_id, file_id, 202, "Completed Movie")
        .await;

    let path = route_utils::replace_param(
        v1::media::item::COMPLETE,
        "{id}",
        file_id.to_string(),
    );
    let response = server
        .post(&path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    response.assert_status(StatusCode::NO_CONTENT);

    let completed = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM user_completed_media
            WHERE user_id = $1 AND media_uuid = $2
        )
        "#,
    )
    .bind(user_id)
    .bind(movie_id)
    .fetch_one(&pool)
    .await?;
    assert!(
        completed,
        "mark complete should resolve the file id back to the logical movie id"
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn explicit_movie_watched_routes_resolve_file_ids_and_clear_state(
    pool: PgPool,
) -> Result<()> {
    let app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (router, state, _tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state);
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": "movie_toggle_user",
            "display_name": "Movie Toggle",
            "password": "Password#123"
        }))
        .await;
    register.assert_status_ok();
    let body: serde_json::Value = register.json();
    let access_token = extract_token_field(&body, "access_token").to_string();
    let user_id = Uuid::parse_str(extract_token_field(&body, "user_id"))?;

    let library_id = Uuid::new_v4();
    let movie_id = Uuid::new_v4();
    let file_id = Uuid::new_v4();
    seed_library(&pool, library_id, "movies").await;
    seed_movie(&pool, library_id, movie_id, file_id, 303, "Toggle Movie").await;

    let watched_path = route_utils::replace_param(
        v1::watch::MOVIE_WATCHED,
        "{media_id}",
        file_id.to_string(),
    );
    let watched_response = server
        .post(&watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    watched_response.assert_status(StatusCode::NO_CONTENT);

    let completed = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM user_completed_media
            WHERE user_id = $1 AND media_uuid = $2
        )
        "#,
    )
    .bind(user_id)
    .bind(movie_id)
    .fetch_one(&pool)
    .await?;
    assert!(
        completed,
        "movie watched route should persist completed state"
    );

    let progress_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_watch_progress
        WHERE user_id = $1 AND media_uuid = $2
        "#,
    )
    .bind(user_id)
    .bind(movie_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        progress_count, 0,
        "movie watched route should clear progress rows"
    );

    let unwatched_response = server
        .delete(&watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    unwatched_response.assert_status(StatusCode::NO_CONTENT);

    let completed = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM user_completed_media
            WHERE user_id = $1 AND media_uuid = $2
        )
        "#,
    )
    .bind(user_id)
    .bind(movie_id)
    .fetch_one(&pool)
    .await?;
    assert!(
        !completed,
        "movie unwatched route should remove completed state"
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn explicit_episode_watched_routes_keep_identity_state_in_sync(
    pool: PgPool,
) -> Result<()> {
    let app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (router, state, _tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state);
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": "episode_toggle_user",
            "display_name": "Episode Toggle",
            "password": "Password#123"
        }))
        .await;
    register.assert_status_ok();
    let body: serde_json::Value = register.json();
    let access_token = extract_token_field(&body, "access_token").to_string();
    let user_id = Uuid::parse_str(extract_token_field(&body, "user_id"))?;

    let library_id = Uuid::new_v4();
    let series_id = Uuid::new_v4();
    let season_id = Uuid::new_v4();
    let episode_id = Uuid::new_v4();
    let file_id = Uuid::new_v4();
    seed_library(&pool, library_id, "tvshows").await;
    seed_series(&pool, library_id, series_id, 404, "Toggle Series").await;
    seed_season(&pool, library_id, series_id, season_id, 404, 1).await;
    seed_episode(
        &pool, library_id, series_id, season_id, episode_id, file_id, 404, 1,
        1, "Pilot",
    )
    .await;

    let watched_path = route_utils::replace_param(
        v1::watch::EPISODE_WATCHED,
        "{media_id}",
        file_id.to_string(),
    );
    let watched_response = server
        .post(&watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    watched_response.assert_status(StatusCode::NO_CONTENT);

    let completed = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM user_completed_media
            WHERE user_id = $1 AND media_uuid = $2
        )
        "#,
    )
    .bind(user_id)
    .bind(episode_id)
    .fetch_one(&pool)
    .await?;
    assert!(
        completed,
        "episode watched route should mark logical episode completed"
    );

    let identity_completed = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM user_episode_state
            WHERE user_id = $1
              AND tmdb_series_id = $2
              AND season_number = $3
              AND episode_number = $4
              AND is_completed = true
        )
        "#,
    )
    .bind(user_id)
    .bind(404_i64)
    .bind(1_i16)
    .bind(1_i16)
    .fetch_one(&pool)
    .await?;
    assert!(
        identity_completed,
        "episode watched route should keep identity aggregate state in sync"
    );

    let unwatched_response = server
        .delete(&watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    unwatched_response.assert_status(StatusCode::NO_CONTENT);

    let identity_rows = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_episode_state
        WHERE user_id = $1 AND tmdb_series_id = $2
        "#,
    )
    .bind(user_id)
    .bind(404_i64)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        identity_rows, 0,
        "episode unwatched route should clear identity aggregate state"
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn explicit_series_watched_routes_mark_and_clear_all_known_episodes(
    pool: PgPool,
) -> Result<()> {
    let app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (router, state, _tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state);
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": "series_toggle_user",
            "display_name": "Series Toggle",
            "password": "Password#123"
        }))
        .await;
    register.assert_status_ok();
    let body: serde_json::Value = register.json();
    let access_token = extract_token_field(&body, "access_token").to_string();
    let user_id = Uuid::parse_str(extract_token_field(&body, "user_id"))?;

    let library_id = Uuid::new_v4();
    let series_id = Uuid::new_v4();
    let season_id = Uuid::new_v4();
    let episode_one_id = Uuid::new_v4();
    let episode_one_file_id = Uuid::new_v4();
    let episode_two_id = Uuid::new_v4();
    let episode_two_file_id = Uuid::new_v4();
    seed_library(&pool, library_id, "tvshows").await;
    seed_series(&pool, library_id, series_id, 505, "Series Toggle").await;
    seed_season(&pool, library_id, series_id, season_id, 505, 1).await;
    seed_episode(
        &pool,
        library_id,
        series_id,
        season_id,
        episode_one_id,
        episode_one_file_id,
        505,
        1,
        1,
        "Episode One",
    )
    .await;
    seed_episode(
        &pool,
        library_id,
        series_id,
        season_id,
        episode_two_id,
        episode_two_file_id,
        505,
        1,
        2,
        "Episode Two",
    )
    .await;

    let progress_response = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "media_id": episode_one_file_id,
            "position": 120.0,
            "duration": 360.0
        }))
        .await;
    progress_response.assert_status(StatusCode::NO_CONTENT);

    let watched_path = route_utils::replace_param(
        v1::watch::SERIES_WATCHED,
        "{tmdb_series_id}",
        505_u64.to_string(),
    );
    let watched_response = server
        .post(&watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    watched_response.assert_status(StatusCode::NO_CONTENT);

    let completed_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_completed_media
        WHERE user_id = $1
          AND media_uuid = ANY($2::uuid[])
        "#,
    )
    .bind(user_id)
    .bind(vec![episode_one_id, episode_two_id])
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        completed_count, 2,
        "series watched route should mark every known episode completed"
    );

    let progress_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_watch_progress
        WHERE user_id = $1
          AND media_uuid = ANY($2::uuid[])
        "#,
    )
    .bind(user_id)
    .bind(vec![episode_one_id, episode_two_id])
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        progress_count, 0,
        "series watched route should clear in-progress episode rows"
    );

    let identity_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_episode_state
        WHERE user_id = $1 AND tmdb_series_id = $2 AND is_completed = true
        "#,
    )
    .bind(user_id)
    .bind(505_i64)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        identity_count, 2,
        "series watched route should mark aggregate identity rows completed"
    );

    let unwatched_response = server
        .delete(&watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    unwatched_response.assert_status(StatusCode::NO_CONTENT);

    let completed_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_completed_media
        WHERE user_id = $1
          AND media_uuid = ANY($2::uuid[])
        "#,
    )
    .bind(user_id)
    .bind(vec![episode_one_id, episode_two_id])
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        completed_count, 0,
        "series unwatched should clear completed rows"
    );

    let identity_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_episode_state
        WHERE user_id = $1 AND tmdb_series_id = $2
        "#,
    )
    .bind(user_id)
    .bind(505_i64)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        identity_count, 0,
        "series unwatched should clear aggregate rows"
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn continue_watching_collapses_tv_progress_into_series_cards(
    pool: PgPool,
) -> Result<()> {
    let app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (router, state, _tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state);
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": "series_continue_user",
            "display_name": "Series Continue",
            "password": "Password#123"
        }))
        .await;
    register.assert_status_ok();
    let body: serde_json::Value = register.json();
    let access_token = extract_token_field(&body, "access_token").to_string();

    let library_id = Uuid::new_v4();
    let series_id = Uuid::new_v4();
    let season_id = Uuid::new_v4();
    let episode_one_id = Uuid::new_v4();
    let episode_one_file_id = Uuid::new_v4();
    let episode_two_id = Uuid::new_v4();
    let episode_two_file_id = Uuid::new_v4();
    seed_library(&pool, library_id, "tvshows").await;
    seed_series(&pool, library_id, series_id, 606, "Continue Series").await;
    seed_season(&pool, library_id, series_id, season_id, 606, 1).await;
    seed_episode(
        &pool,
        library_id,
        series_id,
        season_id,
        episode_one_id,
        episode_one_file_id,
        606,
        1,
        1,
        "Episode One",
    )
    .await;
    seed_episode(
        &pool,
        library_id,
        series_id,
        season_id,
        episode_two_id,
        episode_two_file_id,
        606,
        1,
        2,
        "Episode Two",
    )
    .await;

    let progress_response = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "media_id": episode_one_file_id,
            "position": 180.0,
            "duration": 360.0
        }))
        .await;
    progress_response.assert_status(StatusCode::NO_CONTENT);

    let continue_json = server
        .get(v1::watch::CONTINUE)
        .add_header("Authorization", bearer(&access_token))
        .await;
    continue_json.assert_status_ok();
    let continue_body: serde_json::Value = continue_json.json();
    let items = continue_body["data"]
        .as_array()
        .expect("continue watching json items");
    assert_eq!(
        items.len(),
        1,
        "tv continue watching should dedupe by series"
    );
    let item = &items[0];
    assert_eq!(item["media_id"], json!(episode_one_id.to_string()));
    assert_eq!(item["card_media_id"], json!(series_id.to_string()));
    assert_eq!(item["media_type"], json!("Series"));
    assert_eq!(item["action_hint"], json!("resume"));
    assert_eq!(item["title"], json!("Continue Series"));

    let continue_fb = server
        .get(v1::watch::CONTINUE)
        .add_header("Authorization", bearer(&access_token))
        .add_header(ACCEPT, "application/x-flatbuffers")
        .await;
    continue_fb.assert_status_ok();
    let continue_bytes = continue_fb.into_bytes();
    let continue_list = flatbuffers::root::<fb::watch::ContinueWatchingList<'_>>(
        continue_bytes.as_ref(),
    )?;
    let continue_items =
        continue_list.items().expect("continue watching items");
    assert_eq!(continue_items.len(), 1);
    let continue_entry = continue_items.get(0);
    assert_eq!(
        fb_to_uuid(continue_entry.media_id()),
        episode_one_file_id,
        "series resume card should still point flatbuffers clients at the resumable episode file"
    );
    assert_eq!(
        continue_entry.media_type(),
        fb::common::VideoMediaType::Series,
        "tv continue watching should surface series cards"
    );
    assert_eq!(continue_entry.title(), Some("Continue Series"));

    let complete_response = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "media_id": episode_one_file_id,
            "position": 350.0,
            "duration": 360.0
        }))
        .await;
    complete_response.assert_status(StatusCode::NO_CONTENT);

    let continue_json = server
        .get(v1::watch::CONTINUE)
        .add_header("Authorization", bearer(&access_token))
        .await;
    continue_json.assert_status_ok();
    let continue_body: serde_json::Value = continue_json.json();
    let items = continue_body["data"]
        .as_array()
        .expect("continue watching json items after completion");
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item["media_id"], json!(episode_two_id.to_string()));
    assert_eq!(item["card_media_id"], json!(series_id.to_string()));
    assert_eq!(item["media_type"], json!("Series"));
    assert_eq!(item["action_hint"], json!("next_episode"));
    assert_eq!(item["title"], json!("Continue Series"));

    let continue_fb = server
        .get(v1::watch::CONTINUE)
        .add_header("Authorization", bearer(&access_token))
        .add_header(ACCEPT, "application/x-flatbuffers")
        .await;
    continue_fb.assert_status_ok();
    let continue_bytes = continue_fb.into_bytes();
    let continue_list = flatbuffers::root::<fb::watch::ContinueWatchingList<'_>>(
        continue_bytes.as_ref(),
    )?;
    let continue_items =
        continue_list.items().expect("continue watching items");
    assert_eq!(continue_items.len(), 1);
    let continue_entry = continue_items.get(0);
    assert_eq!(
        fb_to_uuid(continue_entry.media_id()),
        episode_two_file_id,
        "after completion the series card should point flatbuffers clients at the next episode file"
    );
    assert_eq!(
        continue_entry.media_type(),
        fb::common::VideoMediaType::Series
    );
    assert_eq!(continue_entry.title(), Some("Continue Series"));

    Ok(())
}
