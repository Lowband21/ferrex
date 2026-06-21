use std::net::SocketAddr;

use anyhow::Result;
use axum::{Router, http::StatusCode};
use axum_test::TestServer;
use ferrex_core::api::routes::{self, utils as route_utils};
use ferrex_server::infra::{app_state::AppState, startup::NoopStartupHooks};
use serde_json::{Value, json};
use sqlx::PgPool;
use tempfile::TempDir;
use uuid::Uuid;

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

async fn register_user(
    server: &TestServer,
    username: &str,
) -> Result<(Uuid, String)> {
    let response = server
        .post(routes::v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": format!("{username} display"),
            "password": "Password#123"
        }))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    let user_id = Uuid::parse_str(
        body["data"]["user_id"]
            .as_str()
            .expect("register returns user_id"),
    )?;
    let access_token = body["data"]["access_token"]
        .as_str()
        .expect("register returns access token")
        .to_string();

    Ok((user_id, access_token))
}

async fn seed_library(pool: &PgPool, id: Uuid) {
    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, $2, 'movies', ARRAY['/tmp'])
        "#,
    )
    .bind(id)
    .bind(format!("intelligence-{id}"))
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
    genre_id: i64,
    genre: &str,
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
    .bind(format!("/tmp/raw-path-{file_id}.mkv"))
    .bind(format!("{file_id}.mkv"))
    .execute(pool)
    .await
    .expect("insert media file");

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
    .expect("insert movie reference");

    sqlx::query(
        r#"
        INSERT INTO movie_genres (movie_id, library_id, batch_id, genre_id, name)
        VALUES ($1, $2, 1, $3, $4)
        "#,
    )
    .bind(movie_id)
    .bind(library_id)
    .bind(genre_id)
    .bind(genre)
    .execute(pool)
    .await
    .expect("insert movie genre");
}

async fn seed_search_document(
    pool: &PgPool,
    library_id: Uuid,
    movie_id: Uuid,
    title: &str,
    summary: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO intelligence_search_documents (
            library_id, media_id, media_type, document_kind, title, summary,
            search_excerpt, search_text, content_hash
        ) VALUES ($1, $2, 'movie', 'combined', $3, $4, $5, $6, $7)
        "#,
    )
    .bind(library_id)
    .bind(movie_id)
    .bind(title)
    .bind(summary)
    .bind("cosmic signal compact excerpt")
    .bind(format!("{title} cosmic signal grounded bounded context"))
    .bind(hex_hash(1))
    .execute(pool)
    .await
    .expect("insert search document");
}

async fn seed_artifact(
    pool: &PgPool,
    artifact_id: Uuid,
    library_id: Uuid,
    user_id: Option<Uuid>,
    movie_id: Uuid,
    title: &str,
    hash_seed: u64,
) {
    let scope = if user_id.is_some() { "user" } else { "global" };
    let long_summary = "safe bounded summary ".repeat(80);
    sqlx::query(
        r#"
        INSERT INTO intelligence_artifacts (
            id, artifact_kind, scope, status, library_id, user_id, media_id,
            media_type, title, summary, content_hash, content, metadata,
            source_revision
        ) VALUES (
            $1, 'summary', $2, 'active', $3, $4, $5,
            'movie', $6, $7, $8, '{}'::jsonb, '{}'::jsonb, 1
        )
        "#,
    )
    .bind(artifact_id)
    .bind(scope)
    .bind(library_id)
    .bind(user_id)
    .bind(movie_id)
    .bind(title)
    .bind(long_summary)
    .bind(hex_hash(hash_seed))
    .execute(pool)
    .await
    .expect("insert artifact");

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind, source_library_id,
            source_media_id, source_media_type
        ) VALUES ($1, 0, 'media', $2, $3, 'movie')
        "#,
    )
    .bind(artifact_id)
    .bind(library_id)
    .bind(movie_id)
    .execute(pool)
    .await
    .expect("insert artifact source");
}

fn hex_hash(seed: u64) -> String {
    format!("{seed:064x}")
}

fn media_segment(movie_id: Uuid) -> String {
    format!("movie:{movie_id}")
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn intelligence_routes_are_authenticated_bounded_and_scoped(
    pool: PgPool,
) -> Result<()> {
    let library_id = Uuid::from_u128(0x100);
    let movie_id = Uuid::from_u128(0x200);
    let related_movie_id = Uuid::from_u128(0x201);
    let global_artifact_id = Uuid::from_u128(0x300);
    let user_artifact_id = Uuid::from_u128(0x301);

    seed_library(&pool, library_id).await;
    seed_movie(
        &pool,
        library_id,
        movie_id,
        Uuid::from_u128(0x210),
        1,
        "Cosmic Arrival",
        878,
        "Science Fiction",
    )
    .await;
    seed_movie(
        &pool,
        library_id,
        related_movie_id,
        Uuid::from_u128(0x211),
        2,
        "Cosmic Neighbor",
        878,
        "Science Fiction",
    )
    .await;
    seed_search_document(
        &pool,
        library_id,
        movie_id,
        "Cosmic Arrival",
        "A concise searchable library summary.",
    )
    .await;

    let (server, _state, _tempdir) = build_server(pool.clone()).await?;
    let (user_id, access_token) =
        register_user(&server, "intelligence_owner").await?;
    let (_other_user_id, other_access_token) =
        register_user(&server, "intelligence_other").await?;

    seed_artifact(
        &pool,
        global_artifact_id,
        library_id,
        None,
        movie_id,
        "Global compact summary",
        2,
    )
    .await;
    seed_artifact(
        &pool,
        user_artifact_id,
        library_id,
        Some(user_id),
        movie_id,
        "Owner private note",
        3,
    )
    .await;

    let unauthenticated = server
        .post(routes::v1::intelligence::LIBRARY_OVERVIEW)
        .json(&json!({ "library_ids": [library_id] }))
        .await;
    unauthenticated.assert_status(StatusCode::UNAUTHORIZED);

    let overview = server
        .post(routes::v1::intelligence::LIBRARY_OVERVIEW)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "library_ids": [library_id],
            "pagination": { "limit": 999 },
            "caps": {
                "artifact_limit": 999,
                "facet_limit": 999,
                "summary_max_chars": 30
            }
        }))
        .await;
    overview.assert_status_ok();
    let overview_body: Value = overview.json();
    assert_eq!(overview_body["data"]["page"]["limit"], 50);
    assert_eq!(overview_body["data"]["caps"]["artifact_limit"], 24);
    assert_eq!(overview_body["data"]["caps"]["facet_limit"], 32);
    assert_eq!(overview_body["data"]["libraries"][0]["counts"]["movies"], 2);
    assert_eq!(
        overview_body["data"]["libraries"][0]["artifact_ids"]
            .as_array()
            .expect("artifact ids array")
            .len(),
        2
    );

    let facets = server
        .post(routes::v1::intelligence::FACETS)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({ "library_ids": [library_id] }))
        .await;
    facets.assert_status_ok();
    let facets_body: Value = facets.json();
    assert!(
        facets_body["data"]["facets"]
            .as_array()
            .expect("facets array")
            .iter()
            .any(|group| group["kind"] == "media_kind")
    );

    let candidates = server
        .post(routes::v1::intelligence::CANDIDATE_SEARCH)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "query": "cosmic",
            "library_ids": [library_id],
            "include_artifacts": true,
            "pagination": { "limit": 999 },
            "caps": { "candidate_limit": 999, "artifact_limit": 999 }
        }))
        .await;
    candidates.assert_status_ok();
    let candidates_body: Value = candidates.json();
    assert_eq!(candidates_body["data"]["page"]["limit"], 50);
    assert_eq!(candidates_body["data"]["caps"]["candidate_limit"], 50);
    assert_eq!(
        candidates_body["data"]["candidates"][0]["media"]["title"],
        "Cosmic Arrival"
    );
    assert!(
        candidates_body["data"]["candidates"][0]["artifact_ids"]
            .as_array()
            .expect("candidate artifact ids")
            .len()
            >= 2
    );

    let artifact_search = server
        .post(routes::v1::intelligence::ARTIFACT_SEARCH)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "artifact_ids": [global_artifact_id, user_artifact_id],
            "pagination": { "limit": 999 },
            "caps": { "artifact_limit": 999, "summary_max_chars": 30 }
        }))
        .await;
    artifact_search.assert_status_ok();
    let artifact_body: Value = artifact_search.json();
    assert_eq!(artifact_body["data"]["page"]["limit"], 24);
    let artifacts = artifact_body["data"]["artifacts"]
        .as_array()
        .expect("artifact results");
    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0]["summary"]["max_chars"], 30);
    assert_eq!(artifacts[0]["summary"]["truncated"], true);
    assert!(artifacts[0]["provenance"].as_array().is_some());

    let detail_path = route_utils::replace_param(
        routes::v1::intelligence::ARTIFACT_DETAIL,
        "{artifact_id}",
        user_artifact_id.to_string(),
    );
    let detail = server
        .get(&detail_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    detail.assert_status_ok();
    let detail_body: Value = detail.json();
    assert_eq!(
        detail_body["data"]["artifact_id"],
        user_artifact_id.to_string()
    );
    assert_eq!(detail_body["data"]["summary"]["max_chars"], 400);

    let hidden_detail = server
        .get(&detail_path)
        .add_header("Authorization", bearer(&other_access_token))
        .await;
    hidden_detail.assert_status(StatusCode::NOT_FOUND);

    let hidden_search = server
        .post(routes::v1::intelligence::ARTIFACT_SEARCH)
        .add_header("Authorization", bearer(&other_access_token))
        .json(&json!({ "artifact_ids": [user_artifact_id] }))
        .await;
    hidden_search.assert_status_ok();
    let hidden_body: Value = hidden_search.json();
    assert!(
        hidden_body["data"]["artifacts"]
            .as_array()
            .expect("hidden artifact search results")
            .is_empty()
    );

    let item_path = route_utils::replace_param(
        routes::v1::intelligence::ITEM_CONTEXT,
        "{media_id}",
        media_segment(movie_id),
    );
    let item_context = server
        .post(&item_path)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "library_id": library_id,
            "caps": { "artifact_limit": 999, "related_limit": 999 }
        }))
        .await;
    item_context.assert_status_ok();
    let item_body: Value = item_context.json();
    assert_eq!(item_body["data"]["caps"]["related_limit"], 24);
    assert_eq!(
        item_body["data"]["item"]["media"]["title"],
        "Cosmic Arrival"
    );
    assert!(
        item_body["data"]["artifacts"]
            .as_array()
            .expect("item artifacts")
            .len()
            >= 2
    );

    let related_path = route_utils::replace_param(
        routes::v1::intelligence::RELATED_CONTEXT,
        "{media_id}",
        media_segment(movie_id),
    );
    let related = server
        .post(&related_path)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "relationship_kinds": ["similar_genre"],
            "pagination": { "limit": 999 },
            "caps": { "related_limit": 999 }
        }))
        .await;
    related.assert_status_ok();
    let related_body: Value = related.json();
    assert_eq!(related_body["data"]["page"]["limit"], 50);
    assert_eq!(
        related_body["data"]["related"][0]["media"]["title"],
        "Cosmic Neighbor"
    );

    let response_shape = serde_json::to_string(&json!({
        "overview": overview_body["data"],
        "candidate": candidates_body["data"],
        "artifact": artifact_body["data"],
        "item": item_body["data"],
        "related": related_body["data"],
    }))?;
    assert!(response_shape.contains(&global_artifact_id.to_string()));
    assert!(response_shape.contains("provenance"));
    assert!(!response_shape.contains("/tmp/raw-path"));
    assert!(!response_shape.contains("file_path"));
    assert!(!response_shape.contains("content_hash"));
    assert!(!response_shape.contains("technical_metadata"));

    Ok(())
}
