use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use async_trait::async_trait;
use axum::{Router, http::StatusCode};
use axum_test::TestServer;
use ferrex_core::{
    api::{
        routes::{self, utils as route_utils},
        types::intelligence::*,
    },
    application::intelligence_runtime::IntelligenceRunManagerConfig,
    database::repository_ports::intelligence::{
        IntelligenceRunCreate, IntelligenceRunKind,
    },
    domain::intelligence::{
        IntelligenceActionCompletion, IntelligenceActionCompletionRequest,
        IntelligenceChatCompletion, IntelligenceChatCompletionRequest,
        IntelligenceModelProvider, IntelligenceProviderError,
        IntelligenceProviderRequestOptions, IntelligenceProviderResult,
    },
};
use ferrex_model::{LibraryId, MediaID, MovieID};
use ferrex_server::infra::{app_state::AppState, startup::NoopStartupHooks};
use serde_json::{Value, json};
use sqlx::PgPool;
use tempfile::TempDir;
use uuid::Uuid;

mod common;
use common::{
    build_test_app_with_hooks,
    build_test_app_with_hooks_and_intelligence_provider,
};

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

#[derive(Debug, Default)]
struct RouteFakeProvider {
    models: Mutex<
        VecDeque<IntelligenceProviderResult<Vec<IntelligenceModelStatus>>>,
    >,
    actions: Mutex<
        VecDeque<IntelligenceProviderResult<IntelligenceActionCompletion>>,
    >,
    action_delay: Mutex<Option<Duration>>,
}

impl RouteFakeProvider {
    fn push_models(
        &self,
        result: IntelligenceProviderResult<Vec<IntelligenceModelStatus>>,
    ) {
        self.models.lock().unwrap().push_back(result);
    }

    fn push_action(
        &self,
        result: IntelligenceProviderResult<IntelligenceActionCompletion>,
    ) {
        self.actions.lock().unwrap().push_back(result);
    }

    fn delay_actions_by(&self, delay: Duration) {
        *self.action_delay.lock().unwrap() = Some(delay);
    }

    fn default_models(&self) -> Vec<IntelligenceModelStatus> {
        vec![route_model_status("fake-model", true)]
    }
}

fn route_model_status(
    name: impl Into<String>,
    supports_tools: bool,
) -> IntelligenceModelStatus {
    IntelligenceModelStatus {
        name: name.into(),
        selected: true,
        available: true,
        supports_tools,
        context_window_tokens: Some(8192),
    }
}

#[async_trait]
impl IntelligenceModelProvider for RouteFakeProvider {
    async fn discover_models(
        &self,
        _options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<Vec<IntelligenceModelStatus>> {
        Ok(self
            .models
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(self.default_models()))?)
    }

    async fn status(
        &self,
        options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<IntelligenceProviderStatus> {
        let models = self.discover_models(options).await?;
        Ok(IntelligenceProviderStatus {
            enabled: true,
            provider_name: "fake-intelligence".to_string(),
            base_url: "fake://local".to_string(),
            api_key_configured: false,
            default_model: Some("fake-model".to_string()),
            state: IntelligenceProviderState::Ready,
            models,
            checked_at_epoch_seconds: None,
            error: None,
        })
    }

    async fn complete_chat(
        &self,
        _request: IntelligenceChatCompletionRequest,
        _options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<IntelligenceChatCompletion> {
        Err(IntelligenceProviderError::InvalidRequest {
            message: "fake chat completions are not used by route tests"
                .to_string(),
        })
    }

    async fn complete_action(
        &self,
        _request: IntelligenceActionCompletionRequest,
        _options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<IntelligenceActionCompletion> {
        let delay = *self.action_delay.lock().unwrap();
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        self.actions.lock().unwrap().pop_front().unwrap_or_else(|| {
            Err(IntelligenceProviderError::InvalidRequest {
                message: "fake action queue is empty".to_string(),
            })
        })
    }
}

fn enabled_runtime_config() -> IntelligenceRunManagerConfig {
    IntelligenceRunManagerConfig {
        enabled: true,
        provider_name: "fake-intelligence".to_string(),
        default_model: Some("fake-model".to_string()),
        model_timeout: Duration::from_secs(2),
        tool_timeout: Duration::from_secs(2),
        total_timeout: Duration::from_secs(5),
        max_steps: 4,
        max_tool_calls: 4,
        per_user_concurrency: 1,
        max_malformed_retries: 0,
        max_output_bytes: 64 * 1024,
        max_tool_result_bytes: 64 * 1024,
    }
}

async fn build_server(pool: PgPool) -> Result<(TestServer, AppState, TempDir)> {
    let app = build_test_app_with_hooks(pool, &NoopStartupHooks).await?;
    server_from_app(app)
}

async fn build_server_with_provider(
    pool: PgPool,
    provider: Arc<dyn IntelligenceModelProvider>,
) -> Result<(TestServer, AppState, TempDir)> {
    let app = build_test_app_with_hooks_and_intelligence_provider(
        pool,
        &NoopStartupHooks,
        enabled_runtime_config(),
        provider,
    )
    .await?;
    server_from_app(app)
}

fn server_from_app(
    app: common::TestApp,
) -> Result<(TestServer, AppState, TempDir)> {
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

async fn seed_transcript_source(
    pool: &PgPool,
    library_id: Uuid,
    movie_id: Uuid,
    file_id: Uuid,
    source_id: Uuid,
    artifact_id: Uuid,
    owner_id: Uuid,
) {
    sqlx::query(
        r#"
        INSERT INTO intelligence_artifacts (
            id, artifact_kind, scope, status, library_id, user_id, media_id,
            media_type, title, summary, content_hash, content, metadata,
            source_revision
        ) VALUES (
            $1, 'transcript_source', 'user', 'active', $2, $3, $4,
            'movie', 'Cosmic Arrival transcript',
            'Owner-scoped transcript source summary.', $5,
            jsonb_build_object('raw_body', 'this full body must stay hidden'),
            '{}'::jsonb, 1
        )
        "#,
    )
    .bind(artifact_id)
    .bind(library_id)
    .bind(owner_id)
    .bind(movie_id)
    .bind(hex_hash(40))
    .execute(pool)
    .await
    .expect("insert transcript artifact");

    sqlx::query(
        r#"
        INSERT INTO transcript_sources (
            id, library_id, media_id, media_type, media_file_id,
            source_kind, status, language_code, source_key, source_name,
            source_path_hash, source_content_hash, normalized_content_hash,
            artifact_id, duration_ms, segment_count, extracted_at,
            source_locator, metadata
        ) VALUES (
            $1, $2, $3, 'movie', $4,
            'sidecar', 'active', 'en', $5, 'English sidecar',
            $6, $7, $8,
            $9, 120000, 4, now(),
            jsonb_build_object('private_locator', '/tmp/cosmic-arrival.srt'),
            '{}'::jsonb
        )
        "#,
    )
    .bind(source_id)
    .bind(library_id)
    .bind(movie_id)
    .bind(file_id)
    .bind(format!("sidecar:{}", hex_hash(41)))
    .bind(hex_hash(41))
    .bind(hex_hash(42))
    .bind(hex_hash(43))
    .bind(artifact_id)
    .execute(pool)
    .await
    .expect("insert transcript source");

    let segments = [
        (
            0_i32,
            1000_i64,
            2000_i64,
            "The crew listens in silence.",
            50_u64,
        ),
        (
            1,
            2000,
            3500,
            "A cosmic signal arrives with a bounded clue.",
            51,
        ),
        (2, 3500, 5000, "Grounded transcript evidence follows.", 52),
        (3, 9000, 11000, "Another cosmic signal appears later.", 53),
    ];

    for (cue_index, start_ms, end_ms, text, hash_seed) in segments {
        sqlx::query(
            r#"
            INSERT INTO transcript_segments (
                transcript_source_id, library_id, media_id, media_type,
                media_file_id, language_code, cue_index, start_ms, end_ms,
                cue_text, segment_hash, metadata
            ) VALUES ($1, $2, $3, 'movie', $4, 'en', $5, $6, $7, $8, $9, '{}'::jsonb)
            "#,
        )
        .bind(source_id)
        .bind(library_id)
        .bind(movie_id)
        .bind(file_id)
        .bind(cue_index)
        .bind(start_ms)
        .bind(end_ms)
        .bind(text)
        .bind(hex_hash(hash_seed))
        .execute(pool)
        .await
        .expect("insert transcript segment");
    }
}

fn hex_hash(seed: u64) -> String {
    format!("{seed:064x}")
}

fn media_segment(movie_id: Uuid) -> String {
    format!("movie:{movie_id}")
}

fn candidate_search_action(
    query: &str,
    library_id: Uuid,
) -> IntelligenceActionCompletion {
    IntelligenceActionCompletion {
        model: "fake-model".to_string(),
        action_name: "candidate_search".to_string(),
        arguments: json!({
            "query": query,
            "library_ids": [library_id]
        }),
        attempts: 1,
    }
}

fn create_grounded_draft_action(
    draft_id: Uuid,
    library_id: Uuid,
    movie_id: Uuid,
) -> IntelligenceActionCompletion {
    let library_id = LibraryId(library_id);
    let media_id = MediaID::Movie(MovieID(movie_id));
    IntelligenceActionCompletion {
        model: "fake-model".to_string(),
        action_name: "create_draft".to_string(),
        arguments: json!({
            "artifact_id": draft_id,
            "kind": "generated_answer",
            "library_id": library_id,
            "media_id": media_id,
            "title": "Grounded route draft",
            "summary": "A private grounded route draft",
            "content": {"answer": "Cosmic Arrival is a grounded pick"},
            "sources": [{
                "source_ordinal": 0,
                "source_kind": "media",
                "source_library_id": library_id,
                "source_media_id": media_id
            }]
        }),
        attempts: 1,
    }
}

fn final_response_with_draft(
    summary: &str,
    draft_id: Uuid,
) -> IntelligenceActionCompletion {
    IntelligenceActionCompletion {
        model: "fake-model".to_string(),
        action_name: "final_response".to_string(),
        arguments: json!({
            "summary": summary,
            "selected_media_ids": [],
            "artifact_citations": [],
            "draft_artifact_ids": [draft_id]
        }),
        attempts: 1,
    }
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn intelligence_routes_are_authenticated_bounded_and_scoped(
    pool: PgPool,
) -> Result<()> {
    let library_id = Uuid::from_u128(0x100);
    let movie_id = Uuid::from_u128(0x200);
    let related_movie_id = Uuid::from_u128(0x201);
    let movie_file_id = Uuid::from_u128(0x210);
    let global_artifact_id = Uuid::from_u128(0x300);
    let user_artifact_id = Uuid::from_u128(0x301);
    let transcript_artifact_id = Uuid::from_u128(0x302);
    let transcript_source_id = Uuid::from_u128(0x400);

    seed_library(&pool, library_id).await;
    seed_movie(
        &pool,
        library_id,
        movie_id,
        movie_file_id,
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
    seed_transcript_source(
        &pool,
        library_id,
        movie_id,
        movie_file_id,
        transcript_source_id,
        transcript_artifact_id,
        user_id,
    )
    .await;

    let unauthenticated = server
        .post(routes::v1::intelligence::LIBRARY_OVERVIEW)
        .json(&json!({ "library_ids": [library_id] }))
        .await;
    unauthenticated.assert_status(StatusCode::UNAUTHORIZED);

    let unauthenticated_transcripts = server
        .post(routes::v1::intelligence::TIMED_TEXT_SEARCH)
        .json(&json!({ "query": "cosmic", "library_ids": [library_id] }))
        .await;
    unauthenticated_transcripts.assert_status(StatusCode::UNAUTHORIZED);

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
    assert!(
        overview_body["data"]["libraries"][0]["artifact_ids"]
            .as_array()
            .expect("artifact ids array")
            .len()
            >= 2
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
    assert!(
        candidates_body["data"]["candidates"][0]
            .get("transcript_grounding")
            .is_none(),
        "candidate search must not include transcript snippets by default"
    );

    let grounded_candidates = server
        .post(routes::v1::intelligence::CANDIDATE_SEARCH)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "query": "cosmic signal",
            "library_ids": [library_id],
            "include_artifacts": true,
            "include_transcript_grounding": true,
            "caps": {
                "grounding_limit": 4,
                "timed_text_snippet_limit": 1,
                "timed_text_segment_limit": 2,
                "timed_text_snippet_max_chars": 30
            }
        }))
        .await;
    grounded_candidates.assert_status_ok();
    let grounded_candidates_body: Value = grounded_candidates.json();
    let transcript_grounding = grounded_candidates_body["data"]["candidates"]
        [0]["transcript_grounding"]
        .as_array()
        .expect("candidate transcript grounding");
    assert_eq!(transcript_grounding.len(), 1);
    assert_eq!(transcript_grounding[0]["start_ms"], 2000);
    assert_eq!(transcript_grounding[0]["end_ms"], 5000);
    assert_eq!(
        transcript_grounding[0]["artifact_id"],
        transcript_artifact_id.to_string()
    );

    let timed_text = server
        .post(routes::v1::intelligence::TIMED_TEXT_SEARCH)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "query": "cosmic signal",
            "library_ids": [library_id],
            "media_kinds": ["movie"],
            "language_codes": ["EN"],
            "source_kinds": ["sidecar"],
            "include_artifacts": true,
            "pagination": { "limit": 999 },
            "caps": {
                "timed_text_snippet_limit": 1,
                "timed_text_segment_limit": 2,
                "timed_text_snippet_max_chars": 30,
                "summary_max_chars": 30
            }
        }))
        .await;
    timed_text.assert_status_ok();
    let timed_text_body: Value = timed_text.json();
    assert_eq!(timed_text_body["data"]["page"]["limit"], 1);
    assert_eq!(timed_text_body["data"]["page"]["has_more"], true);
    assert!(timed_text_body["data"]["page"]["next_cursor"].is_string());
    assert_eq!(
        timed_text_body["data"]["caps"]["timed_text_segment_limit"],
        2
    );
    let snippets = timed_text_body["data"]["snippets"]
        .as_array()
        .expect("timed-text snippets");
    assert_eq!(snippets.len(), 1);
    assert_eq!(snippets[0]["media"]["title"], "Cosmic Arrival");
    assert_eq!(snippets[0]["source_id"], transcript_source_id.to_string());
    assert_eq!(
        snippets[0]["artifact_id"],
        transcript_artifact_id.to_string()
    );
    assert_eq!(snippets[0]["source_kind"], "sidecar");
    assert_eq!(snippets[0]["language_code"], "en");
    assert_eq!(snippets[0]["start_ms"], 2000);
    assert_eq!(snippets[0]["end_ms"], 5000);
    assert_eq!(
        snippets[0]["segment_ids"]
            .as_array()
            .expect("snippet segment ids")
            .len(),
        2
    );
    assert_eq!(snippets[0]["snippet"]["max_chars"], 30);
    assert_eq!(snippets[0]["snippet"]["truncated"], true);

    let next_cursor = timed_text_body["data"]["page"]["next_cursor"]
        .as_str()
        .expect("next cursor")
        .to_string();
    let second_page = server
        .post(routes::v1::intelligence::TIMED_TEXT_SEARCH)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "query": "cosmic signal",
            "library_ids": [library_id],
            "pagination": { "limit": 1, "cursor": next_cursor },
            "caps": {
                "timed_text_snippet_limit": 1,
                "timed_text_segment_limit": 2,
                "timed_text_snippet_max_chars": 80
            }
        }))
        .await;
    second_page.assert_status_ok();
    let second_page_body: Value = second_page.json();
    assert_eq!(second_page_body["data"]["page"]["has_more"], false);
    assert_eq!(second_page_body["data"]["snippets"][0]["start_ms"], 9000);

    let other_user_transcripts = server
        .post(routes::v1::intelligence::TIMED_TEXT_SEARCH)
        .add_header("Authorization", bearer(&other_access_token))
        .json(&json!({
            "query": "cosmic signal",
            "library_ids": [library_id],
            "include_artifacts": true,
            "caps": { "timed_text_snippet_limit": 1 }
        }))
        .await;
    other_user_transcripts.assert_status_ok();
    let other_user_transcripts_body: Value = other_user_transcripts.json();
    assert!(
        other_user_transcripts_body["data"]["snippets"][0]
            .get("artifact_id")
            .is_none(),
        "user-scoped transcript artifact ids must not leak to another user"
    );

    let transcript_miss = server
        .post(routes::v1::intelligence::TIMED_TEXT_SEARCH)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({ "query": "zzzznomatch", "library_ids": [library_id] }))
        .await;
    transcript_miss.assert_status_ok();
    let transcript_miss_body: Value = transcript_miss.json();
    assert!(
        transcript_miss_body["data"]["snippets"]
            .as_array()
            .expect("miss snippets")
            .is_empty()
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
        "grounded_candidate": grounded_candidates_body["data"],
        "timed_text": timed_text_body["data"],
        "artifact": artifact_body["data"],
        "item": item_body["data"],
        "related": related_body["data"],
    }))?;
    assert!(response_shape.contains(&global_artifact_id.to_string()));
    assert!(response_shape.contains("provenance"));
    assert!(!response_shape.contains("/tmp/raw-path"));
    assert!(!response_shape.contains("file_path"));
    assert!(!response_shape.contains("private_locator"));
    assert!(!response_shape.contains("raw_body"));
    assert!(!response_shape.contains("content_hash"));
    assert!(!response_shape.contains("technical_metadata"));

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn intelligence_run_start_enforces_per_user_concurrency(
    pool: PgPool,
) -> Result<()> {
    let provider = Arc::new(RouteFakeProvider::default());
    provider.delay_actions_by(Duration::from_secs(5));
    let (server, _state, _tempdir) =
        build_server_with_provider(pool.clone(), provider).await?;
    let (_user_id, access_token) =
        register_user(&server, "runtime_concurrency_owner").await?;
    let (_other_user_id, other_access_token) =
        register_user(&server, "runtime_concurrency_other").await?;
    let request = json!({
        "purpose": "recommendation",
        "prompt": "recommend something while another run is active",
        "model": "fake-model"
    });

    let first = server
        .post(routes::v1::intelligence::RUN_START)
        .add_header("Authorization", bearer(&access_token))
        .json(&request)
        .await;
    first.assert_status_ok();

    let same_user = server
        .post(routes::v1::intelligence::RUN_START)
        .add_header("Authorization", bearer(&access_token))
        .json(&request)
        .await;
    same_user.assert_status(StatusCode::TOO_MANY_REQUESTS);
    let same_user_body: Value = same_user.json();
    assert_eq!(same_user_body["error"]["code"], "concurrency_limit");

    let other_user = server
        .post(routes::v1::intelligence::RUN_START)
        .add_header("Authorization", bearer(&other_access_token))
        .json(&request)
        .await;
    other_user.assert_status_ok();

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn intelligence_runtime_routes_cover_auth_runs_sse_cancel_and_drafts(
    pool: PgPool,
) -> Result<()> {
    let runtime_library_id = Uuid::from_u128(0x6260);
    let runtime_movie_id = Uuid::from_u128(0x6261);
    let runtime_draft_id = Uuid::from_u128(0x6262);
    seed_library(&pool, runtime_library_id).await;
    seed_movie(
        &pool,
        runtime_library_id,
        runtime_movie_id,
        Uuid::from_u128(0x6263),
        626,
        "Cosmic Arrival",
        878,
        "Science Fiction",
    )
    .await;
    seed_search_document(
        &pool,
        runtime_library_id,
        runtime_movie_id,
        "Cosmic Arrival",
        "A grounded runtime route candidate.",
    )
    .await;

    let (disabled_server, _disabled_state, _disabled_tempdir) =
        build_server(pool.clone()).await?;
    let (_disabled_user_id, disabled_token) =
        register_user(&disabled_server, "runtime_disabled").await?;

    let unauthenticated = disabled_server
        .post(routes::v1::intelligence::RUN_START)
        .json(&json!({
            "purpose": "recommendation",
            "prompt": "recommend something grounded"
        }))
        .await;
    unauthenticated.assert_status(StatusCode::UNAUTHORIZED);

    let disabled = disabled_server
        .post(routes::v1::intelligence::RUN_START)
        .add_header("Authorization", bearer(&disabled_token))
        .json(&json!({
            "purpose": "recommendation",
            "prompt": "recommend something grounded"
        }))
        .await;
    disabled.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    let disabled_body: Value = disabled.json();
    assert_eq!(disabled_body["error"]["code"], "feature_disabled");

    let unavailable_provider = Arc::new(RouteFakeProvider::default());
    unavailable_provider.push_models(Err(
        IntelligenceProviderError::Unavailable {
            message: "connection refused".to_string(),
        },
    ));
    let (unavailable_server, _unavailable_state, _unavailable_tempdir) =
        build_server_with_provider(pool.clone(), unavailable_provider).await?;
    let (_unavailable_user_id, unavailable_token) =
        register_user(&unavailable_server, "runtime_unavailable").await?;
    let unavailable = unavailable_server
        .post(routes::v1::intelligence::RUN_START)
        .add_header("Authorization", bearer(&unavailable_token))
        .json(&json!({
            "purpose": "recommendation",
            "prompt": "recommend something grounded"
        }))
        .await;
    unavailable.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    let unavailable_body: Value = unavailable.json();
    assert_eq!(unavailable_body["error"]["code"], "provider_unavailable");

    let provider = Arc::new(RouteFakeProvider::default());
    provider.push_models(Ok(vec![route_model_status("fake-model", false)]));
    provider
        .push_action(Ok(candidate_search_action("cosmic", runtime_library_id)));
    provider.push_action(Ok(create_grounded_draft_action(
        runtime_draft_id,
        runtime_library_id,
        runtime_movie_id,
    )));
    provider.push_action(Ok(final_response_with_draft(
        "fake grounded draft ready",
        runtime_draft_id,
    )));
    let (server, state, _tempdir) =
        build_server_with_provider(pool.clone(), provider).await?;
    let (user_id, access_token) =
        register_user(&server, "runtime_owner").await?;
    let (_other_user_id, other_access_token) =
        register_user(&server, "runtime_other").await?;

    let provider_status = server
        .get(routes::v1::intelligence::PROVIDER_STATUS)
        .add_header("Authorization", bearer(&access_token))
        .await;
    provider_status.assert_status_ok();
    let provider_status_body: Value = provider_status.json();
    assert_eq!(provider_status_body["data"]["state"], "ready");
    assert_eq!(
        provider_status_body["data"]["models"][0]["supports_tools"]
            .as_bool()
            .unwrap_or(false),
        false
    );

    let bad_request = server
        .post(routes::v1::intelligence::RUN_START)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "purpose": "recommendation",
            "prompt": "   "
        }))
        .await;
    bad_request.assert_status(StatusCode::BAD_REQUEST);
    let bad_request_body: Value = bad_request.json();
    assert_eq!(bad_request_body["error"]["code"], "invalid_request");

    let start = server
        .post(routes::v1::intelligence::RUN_START)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "purpose": "recommendation",
            "library_id": runtime_library_id,
            "prompt": "recommend something grounded",
            "model": "fake-model",
            "metadata": {"refresh_token": "route-secret"}
        }))
        .await;
    start.assert_status_ok();
    let start_body: Value = start.json();
    let run_id = Uuid::parse_str(
        start_body["data"]["run_id"].as_str().expect("run id"),
    )?;

    let status_path = route_utils::replace_param(
        routes::v1::intelligence::RUN_STATUS,
        "{run_id}",
        run_id.to_string(),
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut status_body = Value::Null;
    for _ in 0..120 {
        let status = server
            .get(&status_path)
            .add_header("Authorization", bearer(&access_token))
            .await;
        status.assert_status_ok();
        status_body = status.json();
        if status_body["data"]["terminal"].as_bool() == Some(true) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        status_body["data"]["status"], "succeeded",
        "status body after polling: {status_body}"
    );
    assert_eq!(status_body["data"]["terminal"], true);
    assert_eq!(
        status_body["data"]["output_summary"]["text"],
        "fake grounded draft ready"
    );
    assert_eq!(
        status_body["data"]["draft_artifact_ids"][0],
        runtime_draft_id.to_string()
    );
    assert!(status_body["data"]["provider"].as_str().is_some());
    assert!(status_body["data"]["model"].as_str().is_some());
    assert!(status_body["data"]["current_phase"].as_str().is_some());

    let hidden_status = server
        .get(&status_path)
        .add_header("Authorization", bearer(&other_access_token))
        .await;
    hidden_status.assert_status(StatusCode::NOT_FOUND);

    let events_path = route_utils::replace_param(
        routes::v1::intelligence::RUN_EVENTS,
        "{run_id}",
        run_id.to_string(),
    );
    let events = server
        .get(&events_path)
        .add_header("Authorization", bearer(&access_token))
        .add_header("Last-Event-ID", "0")
        .await;
    events.assert_status_ok();
    let events_text = events.text();
    assert!(
        events_text.contains("event: started")
            || events_text.contains("event: completed")
    );
    assert!(events_text.contains("event: draft_artifact_created"));
    assert!(!events_text.contains("id: 0\n"));

    let cancel_run_id = state
        .unit_of_work()
        .intelligence
        .create_run(IntelligenceRunCreate {
            run_id: Some(Uuid::from_u128(0x6250)),
            run_kind: IntelligenceRunKind::Recommend,
            library_id: None,
            user_id: Some(user_id),
            media_id: None,
            idempotency_key: None,
            provider_name: Some("fake-intelligence".to_string()),
            model_name: Some("fake-model".to_string()),
            request_hash: Some(hex_hash(0x6250)),
            prompt_excerpt: Some("cancel me".to_string()),
            metadata: json!({}),
        })
        .await?;
    let cancel_path = route_utils::replace_param(
        routes::v1::intelligence::RUN_CANCEL,
        "{run_id}",
        cancel_run_id.to_string(),
    );
    let cancel = server
        .post(&cancel_path)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({"reason": "test cancellation"}))
        .await;
    cancel.assert_status_ok();
    let cancel_body: Value = cancel.json();
    assert_eq!(cancel_body["data"]["status"], "cancelled");
    assert_eq!(cancel_body["data"]["cancellation_requested"], true);

    let cancel_again = server
        .post(&cancel_path)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({}))
        .await;
    cancel_again.assert_status(StatusCode::CONFLICT);
    let cancel_again_body: Value = cancel_again.json();
    assert_eq!(cancel_again_body["error"]["code"], "conflict");

    let draft_path = route_utils::replace_param(
        routes::v1::intelligence::DRAFT_ARTIFACT_DETAIL,
        "{artifact_id}",
        runtime_draft_id.to_string(),
    );
    let owner_draft = server
        .get(&draft_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    owner_draft.assert_status_ok();
    let owner_draft_body: Value = owner_draft.json();
    assert_eq!(
        owner_draft_body["data"]["artifact_id"],
        runtime_draft_id.to_string()
    );
    assert_eq!(
        owner_draft_body["data"]["content"]["answer"],
        "Cosmic Arrival is a grounded pick"
    );
    assert_eq!(owner_draft_body["data"]["status"], "draft");
    assert_eq!(
        owner_draft_body["data"]["sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let hidden_draft = server
        .get(&draft_path)
        .add_header("Authorization", bearer(&other_access_token))
        .await;
    hidden_draft.assert_status(StatusCode::NOT_FOUND);

    let draft_list = server
        .get(routes::v1::intelligence::DRAFT_ARTIFACT_LIST)
        .add_query_param("run_id", &run_id.to_string())
        .add_header("Authorization", bearer(&access_token))
        .await;
    draft_list.assert_status_ok();
    let draft_list_body: Value = draft_list.json();
    assert_eq!(
        draft_list_body["data"]["drafts"][0]["artifact_id"],
        runtime_draft_id.to_string()
    );

    let hidden_draft_list = server
        .get(routes::v1::intelligence::DRAFT_ARTIFACT_LIST)
        .add_query_param("run_id", &run_id.to_string())
        .add_header("Authorization", bearer(&other_access_token))
        .await;
    hidden_draft_list.assert_status_ok();
    let hidden_draft_list_body: Value = hidden_draft_list.json();
    assert!(
        hidden_draft_list_body["data"]["drafts"]
            .as_array()
            .map(|drafts| drafts.is_empty())
            .unwrap_or(true)
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn transcript_purge_and_rebuild_routes_remove_searchable_segments(
    pool: PgPool,
) -> Result<()> {
    let library_id = Uuid::from_u128(0x1700);
    let movie_id = Uuid::from_u128(0x2700);
    let movie_file_id = Uuid::from_u128(0x2710);
    let transcript_artifact_id = Uuid::from_u128(0x3700);
    let transcript_source_id = Uuid::from_u128(0x4700);

    seed_library(&pool, library_id).await;
    seed_movie(
        &pool,
        library_id,
        movie_id,
        movie_file_id,
        77,
        "Purgeable Arrival",
        878,
        "Science Fiction",
    )
    .await;
    let (server, _state, _tempdir) = build_server(pool.clone()).await?;
    let (user_id, access_token) = register_user(&server, "purge_owner").await?;
    seed_transcript_source(
        &pool,
        library_id,
        movie_id,
        movie_file_id,
        transcript_source_id,
        transcript_artifact_id,
        user_id,
    )
    .await;

    let found = server
        .post(routes::v1::intelligence::TIMED_TEXT_SEARCH)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "query": "cosmic signal",
            "library_ids": [library_id],
            "caps": { "timed_text_snippet_limit": 1 }
        }))
        .await;
    found.assert_status_ok();
    let found_body: Value = found.json();
    assert_eq!(
        found_body["data"]["snippets"]
            .as_array()
            .expect("snippets before purge")
            .len(),
        1
    );

    let purge_path = route_utils::replace_params(
        routes::v1::transcripts::PURGE,
        &[
            ("{library_id}", library_id.to_string()),
            ("{type}", "movie".to_string()),
            ("{id}", movie_id.to_string()),
        ],
    );
    let purge = server
        .post(&purge_path)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({"reason": "test purge"}))
        .await;
    purge.assert_status_ok();
    let purge_body: Value = purge.json();
    assert_eq!(purge_body["data"]["purged_sources"], 1);
    assert_eq!(purge_body["data"]["rebuild_queued"], false);

    let after_purge = server
        .post(routes::v1::intelligence::TIMED_TEXT_SEARCH)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "query": "cosmic signal",
            "library_ids": [library_id]
        }))
        .await;
    after_purge.assert_status_ok();
    let after_purge_body: Value = after_purge.json();
    assert!(
        after_purge_body["data"]["snippets"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "purged transcripts must not remain searchable"
    );

    let rebuild_path = route_utils::replace_params(
        routes::v1::transcripts::REBUILD,
        &[
            ("{library_id}", library_id.to_string()),
            ("{type}", "movie".to_string()),
            ("{id}", movie_id.to_string()),
        ],
    );
    let rebuild = server
        .post(&rebuild_path)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({"reason": "test rebuild"}))
        .await;
    rebuild.assert_status_ok();
    let rebuild_body: Value = rebuild.json();
    assert_eq!(rebuild_body["data"]["purged_sources"], 0);
    assert_eq!(rebuild_body["data"]["rebuild_queued"], false);
    assert_eq!(
        rebuild_body["data"]["reason"],
        "transcript_indexing_disabled"
    );

    Ok(())
}
