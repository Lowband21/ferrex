use anyhow::Result;
use axum::Router;
use axum::http::StatusCode;
use axum_test::TestServer;
use ferrex_core::api::routes::{utils as route_utils, v1};
use ferrex_server::infra::startup::NoopStartupHooks;
use serde_json::json;
use sqlx::PgPool;
use std::net::SocketAddr;
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

async fn register_user(
    server: &TestServer,
    username: &str,
) -> Result<(String, Uuid)> {
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
    let access_token = extract_token_field(&body, "access_token").to_string();
    let user_id = Uuid::parse_str(extract_token_field(&body, "user_id"))?;
    Ok((access_token, user_id))
}

async fn seed_library(pool: &PgPool, id: Uuid, library_type: &str) {
    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, $2, $3, ARRAY['/tmp'])
        "#,
    )
    .bind(id)
    .bind(format!("test-{library_type}-{id}"))
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
        INSERT INTO media_files (
            id, library_id, media_id, media_type, file_path, filename, file_size
        ) VALUES ($1, $2, $3, 'movie', $4, $5, 123)
        "#,
    )
    .bind(file_id)
    .bind(library_id)
    .bind(movie_id)
    .bind(format!("/tmp/{file_id}.mkv"))
    .bind(format!("{file_id}.mkv"))
    .execute(pool)
    .await
    .expect("insert movie media_file");

    sqlx::query(
        r#"
        INSERT INTO movie_references (
            id, library_id, file_id, tmdb_id, title, batch_id
        ) VALUES ($1, $2, $3, $4, $5, 1)
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
        INSERT INTO season_references (
            id, series_id, season_number, tmdb_series_id, library_id
        ) VALUES ($1, $2, $3, $4, $5)
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
        INSERT INTO media_files (
            id, library_id, media_id, media_type, file_path, filename, file_size
        ) VALUES ($1, $2, $3, 'episode', $4, $5, 456)
        "#,
    )
    .bind(file_id)
    .bind(library_id)
    .bind(episode_id)
    .bind(format!("/tmp/{file_id}.mkv"))
    .bind(format!("{file_id}.mkv"))
    .execute(pool)
    .await
    .expect("insert episode media_file");

    sqlx::query(
        r#"
        INSERT INTO episode_references (
            id, series_id, season_id, file_id,
            season_number, episode_number, tmdb_series_id
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)
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
        ) VALUES ($1, $2, $3, $4, $5, $6)
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

async fn count_rows(
    pool: &PgPool,
    table: &str,
    user_id: Uuid,
    media_ids: Vec<Uuid>,
) -> Result<i64> {
    let sql = format!(
        r#"
        SELECT COUNT(*)
        FROM {table}
        WHERE user_id = $1 AND media_uuid = ANY($2::uuid[])
        "#
    );
    Ok(sqlx::query_scalar::<_, i64>(&sql)
        .bind(user_id)
        .bind(media_ids)
        .fetch_one(pool)
        .await?)
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn json_watch_routes_resolve_file_ids_and_synchronize_state(
    pool: PgPool,
) -> Result<()> {
    let (server, _tempdir) = build_server(pool.clone()).await?;
    let (access_token, user_id) =
        register_user(&server, "watch_recovery_user").await?;

    let movie_library_id = Uuid::new_v4();
    seed_library(&pool, movie_library_id, "movies").await;

    let movie_id = Uuid::new_v4();
    let movie_file_id = Uuid::new_v4();
    seed_movie(
        &pool,
        movie_library_id,
        movie_id,
        movie_file_id,
        101,
        "Recovered Movie",
    )
    .await;

    let progress_response = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "media_id": movie_file_id,
            "position": 42.0,
            "duration": 120.0
        }))
        .await;
    progress_response.assert_status(StatusCode::NO_CONTENT);

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
    assert_eq!(stored.0, movie_id);
    assert_eq!(stored.1, 0);
    assert_eq!(stored.2, 42.0);
    assert_eq!(stored.3, 120.0);

    let clear_path = route_utils::replace_param(
        v1::watch::CLEAR_PROGRESS,
        "{media_id}",
        movie_file_id.to_string(),
    );
    let clear_response = server
        .delete(&clear_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    clear_response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        count_rows(&pool, "user_watch_progress", user_id, vec![movie_id])
            .await?,
        0
    );

    let stream_progress_path = route_utils::replace_param(
        v1::stream::REPORT_PROGRESS,
        "{media_type}",
        "Movie",
    );
    let stream_progress_path = route_utils::replace_param(
        &stream_progress_path,
        "{id}",
        movie_file_id.to_string(),
    );
    let stream_progress_response = server
        .post(&stream_progress_path)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "position": 50.0,
            "duration": 200.0
        }))
        .await;
    stream_progress_response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        count_rows(&pool, "user_watch_progress", user_id, vec![movie_id])
            .await?,
        1,
        "stream progress handler should resolve file ids to logical movie ids"
    );

    let clear_response = server
        .delete(&clear_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    clear_response.assert_status(StatusCode::NO_CONTENT);

    let exact_response = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "media_id": movie_file_id,
            "position": 95.0,
            "duration": 100.0
        }))
        .await;
    exact_response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        count_rows(&pool, "user_completed_media", user_id, vec![movie_id])
            .await?,
        1,
        "exactly 95% should be completed"
    );
    assert_eq!(
        count_rows(&pool, "user_watch_progress", user_id, vec![movie_id])
            .await?,
        0,
        "completed media should not remain in progress"
    );

    let complete_movie_id = Uuid::new_v4();
    let complete_movie_file_id = Uuid::new_v4();
    seed_movie(
        &pool,
        movie_library_id,
        complete_movie_id,
        complete_movie_file_id,
        102,
        "Complete Route Movie",
    )
    .await;
    let complete_path = route_utils::replace_param(
        v1::media::item::COMPLETE,
        "{id}",
        complete_movie_file_id.to_string(),
    );
    let complete_response = server
        .post(&complete_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    complete_response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        count_rows(
            &pool,
            "user_completed_media",
            user_id,
            vec![complete_movie_id]
        )
        .await?,
        1,
        "complete handler should resolve file ids to logical movie ids"
    );

    let watched_path = route_utils::replace_param(
        v1::watch::MOVIE_WATCHED,
        "{media_id}",
        complete_movie_file_id.to_string(),
    );
    let watched_response = server
        .post(&watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    watched_response.assert_status(StatusCode::NO_CONTENT);
    let unwatched_response = server
        .delete(&watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    unwatched_response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        count_rows(
            &pool,
            "user_completed_media",
            user_id,
            vec![complete_movie_id]
        )
        .await?,
        0,
        "movie unwatched route should clear completed state"
    );

    let tv_library_id = Uuid::new_v4();
    let series_id = Uuid::new_v4();
    let season_id = Uuid::new_v4();
    let episode_one_id = Uuid::new_v4();
    let episode_one_file_id = Uuid::new_v4();
    let episode_two_id = Uuid::new_v4();
    let episode_two_file_id = Uuid::new_v4();
    seed_library(&pool, tv_library_id, "tvshows").await;
    seed_series(&pool, tv_library_id, series_id, 2020, "Recovery Series").await;
    seed_season(&pool, tv_library_id, series_id, season_id, 2020, 1).await;
    seed_episode(
        &pool,
        tv_library_id,
        series_id,
        season_id,
        episode_one_id,
        episode_one_file_id,
        2020,
        1,
        1,
        "Pilot",
    )
    .await;
    seed_episode(
        &pool,
        tv_library_id,
        series_id,
        season_id,
        episode_two_id,
        episode_two_file_id,
        2020,
        1,
        2,
        "Second",
    )
    .await;

    let episode_progress = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "media_id": episode_one_file_id,
            "position": 120.0,
            "duration": 300.0
        }))
        .await;
    episode_progress.assert_status(StatusCode::NO_CONTENT);

    let identity = sqlx::query_as::<_, (Uuid, bool)>(
        r#"
        SELECT last_media_uuid, is_completed
        FROM user_episode_state
        WHERE user_id = $1
          AND tmdb_series_id = $2
          AND season_number = $3
          AND episode_number = $4
        "#,
    )
    .bind(user_id)
    .bind(2020_i64)
    .bind(1_i16)
    .bind(1_i16)
    .fetch_one(&pool)
    .await?;
    assert_eq!(identity.0, episode_one_file_id);
    assert!(!identity.1);
    assert_eq!(
        count_rows(&pool, "user_watch_progress", user_id, vec![episode_one_id])
            .await?,
        1,
        "episode progress should be stored on the logical episode id"
    );

    let episode_watched_path = route_utils::replace_param(
        v1::watch::EPISODE_WATCHED,
        "{media_id}",
        episode_one_file_id.to_string(),
    );
    let episode_watched_response = server
        .post(&episode_watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    episode_watched_response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        count_rows(
            &pool,
            "user_completed_media",
            user_id,
            vec![episode_one_id]
        )
        .await?,
        1,
        "episode watched route should mark logical episode completed"
    );
    let identity_completed = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT is_completed
        FROM user_episode_state
        WHERE user_id = $1 AND tmdb_series_id = $2
          AND season_number = $3 AND episode_number = $4
        "#,
    )
    .bind(user_id)
    .bind(2020_i64)
    .bind(1_i16)
    .bind(1_i16)
    .fetch_one(&pool)
    .await?;
    assert!(identity_completed);

    let episode_unwatched_response = server
        .delete(&episode_watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    episode_unwatched_response.assert_status(StatusCode::NO_CONTENT);
    let identity_rows = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_episode_state
        WHERE user_id = $1 AND tmdb_series_id = $2
        "#,
    )
    .bind(user_id)
    .bind(2020_i64)
    .fetch_one(&pool)
    .await?;
    assert_eq!(identity_rows, 0);

    let series_watched_path = route_utils::replace_param(
        v1::watch::SERIES_WATCHED,
        "{tmdb_series_id}",
        "2020",
    );
    let series_watched_response = server
        .post(&series_watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    series_watched_response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        count_rows(
            &pool,
            "user_completed_media",
            user_id,
            vec![episode_one_id, episode_two_id]
        )
        .await?,
        2,
        "series watched should mark every known episode completed"
    );
    let series_identity_completed = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_episode_state
        WHERE user_id = $1 AND tmdb_series_id = $2 AND is_completed = true
        "#,
    )
    .bind(user_id)
    .bind(2020_i64)
    .fetch_one(&pool)
    .await?;
    assert_eq!(series_identity_completed, 2);

    let series_unwatched_response = server
        .delete(&series_watched_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    series_unwatched_response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        count_rows(
            &pool,
            "user_completed_media",
            user_id,
            vec![episode_one_id, episode_two_id]
        )
        .await?,
        0,
        "series unwatched should clear completed rows"
    );
    let series_identity_rows = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_episode_state
        WHERE user_id = $1 AND tmdb_series_id = $2
        "#,
    )
    .bind(user_id)
    .bind(2020_i64)
    .fetch_one(&pool)
    .await?;
    assert_eq!(series_identity_rows, 0);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn continue_watching_returns_display_action_ready_rows(
    pool: PgPool,
) -> Result<()> {
    let (server, _tempdir) = build_server(pool.clone()).await?;
    let (access_token, user_id) =
        register_user(&server, "continue_contract_user").await?;

    let movie_library_id = Uuid::new_v4();
    let movie_id = Uuid::new_v4();
    let movie_file_id = Uuid::new_v4();
    seed_library(&pool, movie_library_id, "movies").await;
    seed_movie(
        &pool,
        movie_library_id,
        movie_id,
        movie_file_id,
        3030,
        "Continue Movie",
    )
    .await;

    let tv_library_id = Uuid::new_v4();
    let series_id = Uuid::new_v4();
    let season_id = Uuid::new_v4();
    let episode_one_id = Uuid::new_v4();
    let episode_one_file_id = Uuid::new_v4();
    let episode_two_id = Uuid::new_v4();
    let episode_two_file_id = Uuid::new_v4();
    seed_library(&pool, tv_library_id, "tvshows").await;
    seed_series(&pool, tv_library_id, series_id, 4040, "Continue Series").await;
    seed_season(&pool, tv_library_id, series_id, season_id, 4040, 1).await;
    seed_episode(
        &pool,
        tv_library_id,
        series_id,
        season_id,
        episode_one_id,
        episode_one_file_id,
        4040,
        1,
        1,
        "Episode One",
    )
    .await;
    seed_episode(
        &pool,
        tv_library_id,
        series_id,
        season_id,
        episode_two_id,
        episode_two_file_id,
        4040,
        1,
        2,
        "Episode Two",
    )
    .await;

    let movie_progress = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "media_id": movie_file_id,
            "position": 60.0,
            "duration": 600.0
        }))
        .await;
    movie_progress.assert_status(StatusCode::NO_CONTENT);

    let episode_progress = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "media_id": episode_one_file_id,
            "position": 180.0,
            "duration": 600.0
        }))
        .await;
    episode_progress.assert_status(StatusCode::NO_CONTENT);

    sqlx::query(
        r#"
        UPDATE user_watch_progress
        SET last_watched = CASE WHEN media_uuid = $2 THEN 1000 ELSE 2000 END
        WHERE user_id = $1 AND media_uuid = ANY($3::uuid[])
        "#,
    )
    .bind(user_id)
    .bind(movie_id)
    .bind(vec![movie_id, episode_one_id])
    .execute(&pool)
    .await?;
    sqlx::query(
        r#"
        UPDATE user_episode_state
        SET last_watched = 2000
        WHERE user_id = $1 AND tmdb_series_id = $2
        "#,
    )
    .bind(user_id)
    .bind(4040_i64)
    .execute(&pool)
    .await?;

    let continue_json = server
        .get(v1::watch::CONTINUE)
        .add_header("Authorization", bearer(&access_token))
        .await;
    continue_json.assert_status_ok();
    let body: serde_json::Value = continue_json.json();
    let items = body["data"]
        .as_array()
        .expect("continue-watching data array");
    assert_eq!(items.len(), 2);

    let series_item = &items[0];
    assert_eq!(series_item["title"], json!("Continue Series"));
    assert_eq!(series_item["media_id"], json!(episode_one_id.to_string()));
    assert_eq!(series_item["media_type"], json!("Series"));
    assert_eq!(series_item["card_media_id"], json!(series_id.to_string()));
    assert_eq!(series_item["action_hint"], json!("resume"));
    assert_eq!(
        series_item["action_target"]["media_id"],
        json!(episode_one_id.to_string())
    );
    assert_eq!(series_item["action_target"]["media_type"], json!("Episode"));
    assert!(
        series_item["subtitle"]
            .as_str()
            .expect("series subtitle")
            .contains("S01E01")
    );
    assert!(series_item["poster_iid"].is_null());

    let movie_item = &items[1];
    assert_eq!(movie_item["title"], json!("Continue Movie"));
    assert_eq!(movie_item["media_id"], json!(movie_id.to_string()));
    assert_eq!(movie_item["media_type"], json!("Movie"));
    assert_eq!(movie_item["card_media_id"], json!(movie_id.to_string()));
    assert_eq!(movie_item["action_hint"], json!("resume"));
    assert_eq!(
        movie_item["action_target"],
        json!({"media_id": movie_id, "media_type": "Movie"})
    );
    assert!(movie_item["subtitle"].as_str().unwrap().contains("left"));

    let complete_episode = server
        .post(v1::watch::UPDATE_PROGRESS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "media_id": episode_one_file_id,
            "position": 570.0,
            "duration": 600.0
        }))
        .await;
    complete_episode.assert_status(StatusCode::NO_CONTENT);
    sqlx::query(
        r#"
        UPDATE user_episode_state
        SET last_watched = 3000
        WHERE user_id = $1 AND tmdb_series_id = $2
        "#,
    )
    .bind(user_id)
    .bind(4040_i64)
    .execute(&pool)
    .await?;

    let continue_json = server
        .get(v1::watch::CONTINUE)
        .add_header("Authorization", bearer(&access_token))
        .await;
    continue_json.assert_status_ok();
    let body: serde_json::Value = continue_json.json();
    let items = body["data"]
        .as_array()
        .expect("continue-watching data array after completion");
    let series_item = items
        .iter()
        .find(|item| item["media_type"] == json!("Series"))
        .expect("series continue row");
    assert_eq!(series_item["media_id"], json!(episode_two_id.to_string()));
    assert_eq!(series_item["action_hint"], json!("next_episode"));
    assert_eq!(
        series_item["action_target"]["media_id"],
        json!(episode_two_id.to_string())
    );
    assert_eq!(series_item["position"], json!(0.0));
    assert_eq!(series_item["duration"], json!(0.0));

    Ok(())
}
