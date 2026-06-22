//! SQLx-backed behavior tests for the Postgres intelligence repository.
//!
//! These tests run against a migrated Postgres instance. They use the shared
//! `#[sqlx::test]` harness (matching `database_postgres_behaviour.rs`) with the
//! `test_libraries` and `intelligence_base` fixtures, so they require a
//! reachable `DATABASE_URL` at runtime. Run via:
//!
//! `DATABASE_URL=... cargo test -p ferrex-core --test intelligence_repository`

use ferrex_core::api::types::intelligence::{
    IntelligenceArtifactKind, IntelligenceArtifactSearchRequest,
    IntelligenceCandidateSearchRequest, IntelligenceCaps,
    IntelligenceItemContextRequest, IntelligenceLibraryOverviewRequest,
    IntelligenceMediaKind, IntelligencePagination,
    IntelligenceRelatedContextRequest, IntelligenceRelationshipKind,
    IntelligenceRunAuditRequest, MAX_INTELLIGENCE_ARTIFACT_LIMIT,
};
use ferrex_core::database::repositories::intelligence::PostgresIntelligenceRepository;
use ferrex_core::database::repository_ports::intelligence::{
    IntelligenceArtifactScope, IntelligenceArtifactUpsert,
    IntelligenceRepository, IntelligenceRunCreate, IntelligenceRunKind,
    IntelligenceRunListFilter, IntelligenceRunStatus as RunStatus,
    IntelligenceRunUpdate, IntelligenceToolCallCreate,
    IntelligenceToolCallStatus as ToolStatus, IntelligenceToolCallUpdate,
    IntelligenceToolKind,
};
use ferrex_core::types::{EpisodeID, LibraryId, MediaID, MovieID};

use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const LIB_A: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
const LIB_C: &str = "cccccccc-cccc-cccc-cccc-cccccccccccc";
const ALICE: &str = "11111111-0000-0000-0000-000000000001";
const BOB: &str = "11111111-0000-0000-0000-000000000002";
const MOVIE_ARRIVAL: &str = "22222222-0000-0000-0000-000000000001";
const MOVIE_TENET: &str = "22222222-0000-0000-0000-000000000002";
const MOVIE_GONE: &str = "22222222-0000-0000-0000-000000000003";
const EPISODE_1: &str = "66666666-0000-0000-0000-000000000001";
const EPISODE_2: &str = "66666666-0000-0000-0000-000000000002";

fn lib_a() -> LibraryId {
    LibraryId(Uuid::parse_str(LIB_A).unwrap())
}

fn lib_c() -> LibraryId {
    LibraryId(Uuid::parse_str(LIB_C).unwrap())
}

fn movie(id: &str) -> MediaID {
    MediaID::Movie(MovieID(Uuid::parse_str(id).unwrap()))
}

fn repo(pool: &PgPool) -> PostgresIntelligenceRepository {
    PostgresIntelligenceRepository::new(pool.clone())
}

async fn context_count(
    pool: &PgPool,
    media_uuid: Uuid,
    user_id: Option<Uuid>,
) -> i64 {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT count(*) FROM intelligence_media_context
        WHERE media_id = $1
          AND ($2::uuid IS NULL AND user_id IS NULL OR user_id = $2)
        "#,
    )
    .bind(media_uuid)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn context_status(pool: &PgPool, media_uuid: Uuid) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT status::text FROM intelligence_media_context
        WHERE media_id = $1 AND user_id IS NULL
        "#,
    )
    .bind(media_uuid)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn search_doc_count(pool: &PgPool, media_uuid: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_search_documents \
         WHERE media_id = $1 AND user_id IS NULL",
    )
    .bind(media_uuid)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn refresh_upserts_available_media_rows(pool: PgPool) {
    let repo = repo(&pool);

    // Global refresh of library A (movies): two available, one tombstoned.
    let refreshed = repo
        .refresh_library_read_models(lib_a(), None)
        .await
        .unwrap();
    // 2 available movies → 2 context + 2 search docs each.
    assert!(refreshed >= 2, "refreshed={refreshed}");
    assert_eq!(
        context_count(&pool, Uuid::parse_str(MOVIE_ARRIVAL).unwrap(), None)
            .await,
        1
    );
    assert_eq!(
        context_count(&pool, Uuid::parse_str(MOVIE_TENET).unwrap(), None).await,
        1
    );
    assert_eq!(
        context_count(&pool, Uuid::parse_str(MOVIE_GONE).unwrap(), None).await,
        0,
        "unavailable movie must not get a read-model row"
    );
    assert_eq!(
        search_doc_count(&pool, Uuid::parse_str(MOVIE_ARRIVAL).unwrap()).await,
        1
    );

    repo.refresh_media_read_model(lib_a(), movie_from_str(MOVIE_GONE), None)
        .await
        .unwrap();
    assert_eq!(
        context_count(&pool, Uuid::parse_str(MOVIE_GONE).unwrap(), None).await,
        0,
        "direct refresh must also skip unavailable movies"
    );

    // Global refresh of library C (tvshows): one available episode derives a
    // series, season, and episode context row; the unavailable episode is skipped.
    repo.refresh_library_read_models(lib_c(), None)
        .await
        .unwrap();
    assert_eq!(
        context_count(&pool, Uuid::parse_str(EPISODE_1).unwrap(), None).await,
        1
    );
    assert_eq!(
        context_count(&pool, Uuid::parse_str(EPISODE_2).unwrap(), None).await,
        0,
        "unavailable episode must not get a read-model row"
    );
    repo.refresh_media_read_model(lib_c(), episode_from_str(EPISODE_2), None)
        .await
        .unwrap();
    assert_eq!(
        context_count(&pool, Uuid::parse_str(EPISODE_2).unwrap(), None).await,
        0,
        "direct refresh must also skip unavailable episodes"
    );
    // Derived series + season context rows exist.
    let series_rows: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_media_context \
         WHERE media_type = 'series' AND library_id = $1 AND user_id IS NULL",
    )
    .bind(lib_c().0)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(series_rows, 1, "series context row should be derived");
    let season_rows: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_media_context \
         WHERE media_type = 'season' AND library_id = $1 AND user_id IS NULL",
    )
    .bind(lib_c().0)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(season_rows, 1, "season context row should be derived");
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn metadata_refresh_updates_revision_rows_and_dependent_artifacts(
    pool: PgPool,
) {
    let repo = repo(&pool);
    let arrival = Uuid::parse_str(MOVIE_ARRIVAL).unwrap();

    repo.refresh_media_read_model(lib_a(), movie_from_str(MOVIE_ARRIVAL), None)
        .await
        .unwrap();
    let (revision_before, summary_before): (i64, Option<String>) =
        sqlx::query_as(
            "SELECT source_revision, summary FROM intelligence_media_context \
             WHERE media_id = $1 AND user_id IS NULL AND context_kind = 'metadata'",
        )
        .bind(arrival)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(summary_before.unwrap().contains("linguist"));

    let artifact_id = repo
        .upsert_artifact(IntelligenceArtifactUpsert {
            artifact_id: None,
            kind: IntelligenceArtifactKind::Summary,
            scope: IntelligenceArtifactScope::Global,
            library_id: Some(lib_a()),
            media_id: Some(movie_from_str(MOVIE_ARRIVAL)),
            run_id: None,
            supersedes_artifact_id: None,
            title: "Arrival generated summary".to_string(),
            summary: Some("Old generated context".to_string()),
            excerpt: None,
            content: json!({"body": "old"}),
            metadata: json!({}),
            source_revision: revision_before,
        })
        .await
        .unwrap();

    sqlx::query("UPDATE movie_metadata SET overview = $2 WHERE movie_id = $1")
        .bind(arrival)
        .bind("A revised linguistic first-contact summary for refresh testing.")
        .execute(&pool)
        .await
        .unwrap();

    repo.refresh_media_read_model(lib_a(), movie_from_str(MOVIE_ARRIVAL), None)
        .await
        .unwrap();

    let (revision_after, summary_after): (i64, Option<String>) =
        sqlx::query_as(
            "SELECT source_revision, summary FROM intelligence_media_context \
             WHERE media_id = $1 AND user_id IS NULL AND context_kind = 'metadata'",
        )
        .bind(arrival)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(revision_after > revision_before);
    assert!(summary_after.unwrap().contains("revised linguistic"));

    let search_text: String = sqlx::query_scalar(
        "SELECT search_text FROM intelligence_search_documents \
         WHERE media_id = $1 AND user_id IS NULL AND document_kind = 'combined'",
    )
    .bind(arrival)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(search_text.contains("revised linguistic"));

    let (status, invalidated, reason): (String, bool, Option<String>) =
        sqlx::query_as(
            "SELECT status::text, invalidated_at IS NOT NULL, invalidation_reason \
             FROM intelligence_artifacts WHERE id = $1",
        )
        .bind(artifact_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "invalidated");
    assert!(invalidated);
    assert_eq!(reason.as_deref(), Some("media_metadata_changed"));
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn refresh_is_deterministic_across_runs(pool: PgPool) {
    let repo = repo(&pool);
    repo.refresh_library_read_models(lib_a(), None)
        .await
        .unwrap();
    let hash_before: String = sqlx::query_scalar::<_, String>(
        "SELECT content_hash FROM intelligence_media_context \
         WHERE media_id = $1 AND user_id IS NULL AND context_kind = 'metadata'",
    )
    .bind(Uuid::parse_str(MOVIE_ARRIVAL).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();

    repo.refresh_library_read_models(lib_a(), None)
        .await
        .unwrap();
    let hash_after: String = sqlx::query_scalar::<_, String>(
        "SELECT content_hash FROM intelligence_media_context \
         WHERE media_id = $1 AND user_id IS NULL AND context_kind = 'metadata'",
    )
    .bind(Uuid::parse_str(MOVIE_ARRIVAL).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        hash_before, hash_after,
        "content_hash must be deterministic"
    );
    assert_eq!(
        hash_before.len(),
        64,
        "content_hash is a sha-256 hex digest"
    );
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn candidate_search_is_bounded_and_ordered(pool: PgPool) {
    let repo = repo(&pool);
    repo.refresh_library_read_models(lib_a(), None)
        .await
        .unwrap();

    let request = IntelligenceCandidateSearchRequest {
        query: "arrival alien".to_string(),
        library_ids: vec![lib_a()],
        media_kinds: vec![IntelligenceMediaKind::Movie],
        pagination: IntelligencePagination::default(),
        caps: IntelligenceCaps {
            candidate_limit: 1,
            ..Default::default()
        },
        include_artifacts: false,
    };
    let response = repo.candidate_search(&request, None).await.unwrap();
    assert!(!response.candidates.is_empty(), "arrival should match");
    assert!(
        response.candidates.len() <= 1,
        "candidate_limit=1 must bound results"
    );
    let candidate = &response.candidates[0];
    assert_eq!(candidate.media.title, "Arrival");
    assert!(candidate.score.is_some(), "FTS rank should be present");
    assert!(
        !candidate.grounding.is_empty(),
        "grounding refs should be attached"
    );

    // People credits are included in deterministic search documents.
    let people_request = IntelligenceCandidateSearchRequest {
        query: "amy adams".to_string(),
        caps: IntelligenceCaps::default(),
        ..request.clone()
    };
    let people_response =
        repo.candidate_search(&people_request, None).await.unwrap();
    assert!(
        people_response
            .candidates
            .iter()
            .any(|candidate| candidate.media.title == "Arrival"),
        "Arrival should be searchable by seeded cast member"
    );

    // A query that matches nothing returns an empty, bounded response.
    let miss = IntelligenceCandidateSearchRequest {
        query: "zzzznomatch".to_string(),
        ..request.clone()
    };
    let miss_response = repo.candidate_search(&miss, None).await.unwrap();
    assert!(miss_response.candidates.is_empty());
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn library_overview_counts_facets_and_caps(pool: PgPool) {
    let repo = repo(&pool);
    repo.refresh_library_read_models(lib_a(), None)
        .await
        .unwrap();

    let request = IntelligenceLibraryOverviewRequest {
        library_ids: vec![lib_a()],
        pagination: IntelligencePagination::default(),
        caps: IntelligenceCaps {
            facet_limit: 1,
            ..Default::default()
        },
    };
    let response = repo.library_overview(&request, None).await.unwrap();
    assert_eq!(response.libraries.len(), 1);
    let overview = &response.libraries[0];
    assert_eq!(overview.counts.movies, 2, "two available movies");
    assert_eq!(overview.counts.episodes, 0);
    assert!(overview.summary.is_some());

    // The media_kind facet group is always present; genre facet should exist.
    let has_media_kind = overview
        .facets
        .iter()
        .any(|g| matches!(g.kind, ferrex_core::api::types::intelligence::IntelligenceFacetKind::MediaKind));
    assert!(has_media_kind);
    let genre_group = overview.facets.iter().find(|g| {
        matches!(
            g.kind,
            ferrex_core::api::types::intelligence::IntelligenceFacetKind::Genre
        )
    });
    assert!(genre_group.is_some(), "genre facet should be derived");
    let genre_group = genre_group.unwrap();
    assert!(
        genre_group.values.len() <= 1,
        "facet_limit=1 must bound facet values"
    );
    // Aggregate facets across libraries are also bounded.
    for group in &response.facets {
        assert!(group.values.len() <= 1, "aggregate facets respect caps");
    }
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn item_context_and_related_context(pool: PgPool) {
    let repo = repo(&pool);
    repo.refresh_library_read_models(lib_c(), None)
        .await
        .unwrap();
    repo.refresh_library_read_models(lib_a(), None)
        .await
        .unwrap();

    // Episode item context should carry a summary and same-series relatives.
    let request = IntelligenceItemContextRequest {
        media_id: episode_from_str(EPISODE_1),
        library_id: Some(lib_c()),
        caps: IntelligenceCaps::default(),
    };
    let response = repo.item_context(&request, None).await.unwrap();
    assert!(response.item.summary.is_some());
    assert!(
        !response.related.is_empty(),
        "episode should have same-series relatives"
    );
    assert!(response.related.iter().any(|r| matches!(
        r.relationship,
        IntelligenceRelationshipKind::SameSeries
    )));

    // A movie seed should produce similar-genre relatives in the same library.
    let related_request = IntelligenceRelatedContextRequest {
        media_id: movie_from_str(MOVIE_ARRIVAL),
        relationship_kinds: vec![IntelligenceRelationshipKind::SimilarGenre],
        pagination: IntelligencePagination::default(),
        caps: IntelligenceCaps::default(),
    };
    let related = repo.related_context(&related_request, None).await.unwrap();
    assert_eq!(related.seed.title, "Arrival");
    assert!(
        related.related.iter().any(|r| r.media.title == "Tenet"),
        "Tenet shares the Science Fiction genre with Arrival"
    );
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn artifact_upsert_list_get_invalidate_global_and_user(pool: PgPool) {
    let repo = repo(&pool);
    let alice = Uuid::parse_str(ALICE).unwrap();
    let bob = Uuid::parse_str(BOB).unwrap();

    // Global artifact.
    let global_id = repo
        .upsert_artifact(IntelligenceArtifactUpsert {
            artifact_id: None,
            kind: IntelligenceArtifactKind::Summary,
            scope: IntelligenceArtifactScope::Global,
            library_id: Some(lib_a()),
            media_id: Some(movie_from_str(MOVIE_ARRIVAL)),
            run_id: None,
            supersedes_artifact_id: None,
            title: "Arrival summary".to_string(),
            summary: Some("A bounded arrival summary.".to_string()),
            excerpt: None,
            content: json!({"body": "short"}),
            metadata: json!({}),
            source_revision: 1,
        })
        .await
        .unwrap();

    let fetched = repo.get_artifact(global_id, None).await.unwrap();
    assert!(
        fetched.is_some(),
        "global artifact visible to anonymous caller"
    );
    assert_eq!(fetched.unwrap().title, "Arrival summary");

    // User-scoped artifact for Alice.
    let alice_id = repo
        .upsert_artifact(IntelligenceArtifactUpsert {
            artifact_id: None,
            kind: IntelligenceArtifactKind::UserNote,
            scope: IntelligenceArtifactScope::User(alice),
            library_id: Some(lib_a()),
            media_id: Some(movie_from_str(MOVIE_ARRIVAL)),
            run_id: None,
            supersedes_artifact_id: None,
            title: "Alice note".to_string(),
            summary: Some("Private note.".to_string()),
            excerpt: None,
            content: json!({}),
            metadata: json!({}),
            source_revision: 1,
        })
        .await
        .unwrap();

    // Alice can see it; Bob cannot.
    assert!(
        repo.get_artifact(alice_id, Some(alice))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        repo.get_artifact(alice_id, Some(bob))
            .await
            .unwrap()
            .is_none(),
        "user artifacts must be isolated from other users"
    );

    // Artifact search respects scope: Bob only sees the global artifact.
    let search = IntelligenceArtifactSearchRequest {
        artifact_ids: Vec::new(),
        media_ids: Vec::new(),
        library_ids: vec![lib_a()],
        kinds: Vec::new(),
        pagination: IntelligencePagination::default(),
        caps: IntelligenceCaps::default(),
    };
    let bob_results = repo.artifact_search(&search, Some(bob)).await.unwrap();
    assert!(
        bob_results
            .artifacts
            .iter()
            .any(|a| a.artifact_id == global_id)
    );
    assert!(
        !bob_results
            .artifacts
            .iter()
            .any(|a| a.artifact_id == alice_id),
        "Bob must not see Alice's artifacts in search"
    );
    let alice_results =
        repo.artifact_search(&search, Some(alice)).await.unwrap();
    assert!(
        alice_results
            .artifacts
            .iter()
            .any(|a| a.artifact_id == alice_id)
    );

    // Deferred artifact kinds are rejected instead of widening the filter.
    let deferred = repo
        .artifact_search(
            &IntelligenceArtifactSearchRequest {
                kinds: vec![IntelligenceArtifactKind::EmbeddingChunk],
                ..search.clone()
            },
            Some(alice),
        )
        .await;
    assert!(deferred.is_err(), "deferred artifact kinds must error");

    // Bob cannot update Alice's user-scoped artifact.
    let bob_update = repo
        .upsert_artifact(IntelligenceArtifactUpsert {
            artifact_id: Some(alice_id),
            kind: IntelligenceArtifactKind::UserNote,
            scope: IntelligenceArtifactScope::User(bob),
            library_id: Some(lib_a()),
            media_id: Some(movie_from_str(MOVIE_ARRIVAL)),
            run_id: None,
            supersedes_artifact_id: None,
            title: "Bob rewrite".to_string(),
            summary: Some("Should not apply.".to_string()),
            excerpt: None,
            content: json!({}),
            metadata: json!({}),
            source_revision: 2,
        })
        .await;
    assert!(bob_update.is_err(), "Bob must not update Alice's artifact");

    // Invalidation: global artifact invalidated by global scope; user artifact
    // only by its owner.
    repo.invalidate_artifact(global_id, None, "superseded")
        .await
        .unwrap();
    assert!(repo.get_artifact(global_id, None).await.unwrap().is_none());

    // Bob cannot invalidate Alice's artifact.
    let bob_invalidate = repo
        .invalidate_artifact(alice_id, Some(bob), "malicious")
        .await;
    assert!(
        bob_invalidate.is_err(),
        "Bob must not invalidate Alice's artifact"
    );
    // Alice can.
    repo.invalidate_artifact(alice_id, Some(alice), "owner removed")
        .await
        .unwrap();
    assert!(
        repo.get_artifact(alice_id, Some(alice))
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn artifact_search_clamps_pagination_and_omits_raw_content(pool: PgPool) {
    let repo = repo(&pool);
    let raw_payload = "RAW_PROVIDER_PAYLOAD_SHOULD_NOT_LEAK".repeat(256);

    for index in 0..(MAX_INTELLIGENCE_ARTIFACT_LIMIT + 2) {
        repo.upsert_artifact(IntelligenceArtifactUpsert {
            artifact_id: None,
            kind: IntelligenceArtifactKind::Summary,
            scope: IntelligenceArtifactScope::Global,
            library_id: Some(lib_a()),
            media_id: Some(movie_from_str(MOVIE_ARRIVAL)),
            run_id: None,
            supersedes_artifact_id: None,
            title: format!("Bounded Arrival artifact {index}"),
            summary: Some(format!(
                "Artifact {index} summary intentionally exceeds the tiny cap."
            )),
            excerpt: Some("Fixture excerpt remains bounded.".to_string()),
            content: json!({
                "raw_provider_dump": raw_payload.clone(),
                "messages": ["unbounded payload must stay out of DTOs"]
            }),
            metadata: json!({"fixture_index": index}),
            source_revision: i64::from(index),
        })
        .await
        .unwrap();
    }

    let response = repo
        .artifact_search(
            &IntelligenceArtifactSearchRequest {
                artifact_ids: Vec::new(),
                media_ids: Vec::new(),
                library_ids: vec![lib_a()],
                kinds: vec![IntelligenceArtifactKind::Summary],
                pagination: IntelligencePagination {
                    cursor: None,
                    limit: u16::MAX,
                },
                caps: IntelligenceCaps {
                    artifact_limit: u16::MAX,
                    summary_max_chars: 16,
                    ..Default::default()
                },
            },
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        response.artifacts.len(),
        usize::from(MAX_INTELLIGENCE_ARTIFACT_LIMIT),
        "artifact_limit cap should bound returned summaries"
    );
    assert_eq!(
        response.page.limit, MAX_INTELLIGENCE_ARTIFACT_LIMIT,
        "oversized pagination and artifact caps clamp to the public maximum"
    );
    assert!(response.page.has_more, "extra rows should set has_more");
    for artifact in &response.artifacts {
        let summary = artifact
            .summary
            .as_ref()
            .expect("fixture artifacts carry summaries");
        assert!(summary.text.chars().count() <= 16);
        assert!(summary.truncated);
    }

    let serialized = serde_json::to_string(&response).unwrap();
    assert!(
        !serialized.contains("RAW_PROVIDER_PAYLOAD_SHOULD_NOT_LEAK"),
        "raw artifact content must not leak through summary DTOs"
    );
    assert!(
        !serialized.contains("raw_provider_dump"),
        "raw provider field names must stay out of public DTOs"
    );
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn superseded_artifact_records_invalidation_metadata_and_keeps_sources(
    pool: PgPool,
) {
    let repo = repo(&pool);

    let old_id = repo
        .upsert_artifact(IntelligenceArtifactUpsert {
            artifact_id: None,
            kind: IntelligenceArtifactKind::Summary,
            scope: IntelligenceArtifactScope::Global,
            library_id: Some(lib_a()),
            media_id: Some(movie_from_str(MOVIE_ARRIVAL)),
            run_id: None,
            supersedes_artifact_id: None,
            title: "Old Arrival summary".to_string(),
            summary: Some("Old provenance-bearing summary".to_string()),
            excerpt: None,
            content: json!({"body": "old"}),
            metadata: json!({}),
            source_revision: 7,
        })
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO intelligence_artifact_sources \
         (artifact_id, source_ordinal, source_kind, source_revision, source_excerpt) \
         VALUES ($1, 0, 'manual', 7, 'operator supplied source')",
    )
    .bind(old_id)
    .execute(&pool)
    .await
    .unwrap();

    let new_id = repo
        .upsert_artifact(IntelligenceArtifactUpsert {
            artifact_id: None,
            kind: IntelligenceArtifactKind::Summary,
            scope: IntelligenceArtifactScope::Global,
            library_id: Some(lib_a()),
            media_id: Some(movie_from_str(MOVIE_ARRIVAL)),
            run_id: None,
            supersedes_artifact_id: Some(old_id),
            title: "New Arrival summary".to_string(),
            summary: Some("Replacement summary".to_string()),
            excerpt: None,
            content: json!({"body": "new"}),
            metadata: json!({}),
            source_revision: 8,
        })
        .await
        .unwrap();

    let (status, invalidated, reason): (String, bool, Option<String>) =
        sqlx::query_as(
            "SELECT status::text, invalidated_at IS NOT NULL, invalidation_reason \
             FROM intelligence_artifacts WHERE id = $1",
        )
        .bind(old_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "superseded");
    assert!(invalidated);
    assert!(
        reason.unwrap().contains(&new_id.to_string()),
        "supersede reason should identify replacement artifact"
    );

    let source_excerpt: String = sqlx::query_scalar(
        "SELECT source_excerpt FROM intelligence_artifact_sources \
         WHERE artifact_id = $1 AND source_ordinal = 0",
    )
    .bind(old_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(source_excerpt, "operator supplied source");
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn run_and_tool_call_audit_lifecycle(pool: PgPool) {
    let repo = repo(&pool);
    let alice = Uuid::parse_str(ALICE).unwrap();

    // Idempotent run creation.
    let idem_key = "audit-run-1".to_string();
    let run_id = repo
        .create_run(IntelligenceRunCreate {
            run_id: None,
            run_kind: IntelligenceRunKind::Search,
            library_id: Some(lib_a()),
            user_id: Some(alice),
            media_id: Some(movie_from_str(MOVIE_ARRIVAL)),
            idempotency_key: Some(idem_key.clone()),
            provider_name: Some("ferrex-local".to_string()),
            model_name: Some("phase-one".to_string()),
            request_hash: None,
            prompt_excerpt: Some("Find arrival context.".to_string()),
            metadata: json!({}),
        })
        .await
        .unwrap();
    let dup_id = repo
        .create_run(IntelligenceRunCreate {
            run_id: None,
            run_kind: IntelligenceRunKind::Search,
            library_id: Some(lib_a()),
            user_id: Some(alice),
            media_id: None,
            idempotency_key: Some(idem_key),
            provider_name: None,
            model_name: None,
            request_hash: None,
            prompt_excerpt: None,
            metadata: json!({}),
        })
        .await
        .unwrap();
    assert_eq!(run_id, dup_id, "idempotency key must deduplicate runs");

    // Transition queued -> running -> succeeded.
    repo.update_run(
        run_id,
        IntelligenceRunUpdate {
            status: Some(RunStatus::Running),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    repo.update_run(
        run_id,
        IntelligenceRunUpdate {
            status: Some(RunStatus::Succeeded),
            result_summary: Some("Found one context.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Tool call create + update.
    let tool_id = repo
        .create_tool_call(IntelligenceToolCallCreate {
            tool_call_id: None,
            run_id,
            sequence: 0,
            tool_kind: IntelligenceToolKind::ReadModel,
            tool_name: "library_lookup".to_string(),
            idempotency_key: None,
            input_hash: None,
            arguments: json!({"media_id": MOVIE_ARRIVAL}),
        })
        .await
        .unwrap();
    repo.update_tool_call(
        tool_id,
        IntelligenceToolCallUpdate {
            status: Some(ToolStatus::Succeeded),
            result: Some(json!({"ok": true})),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // list_runs filters by library.
    let listed = repo
        .list_runs(IntelligenceRunListFilter {
            library_id: Some(lib_a()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(listed.iter().any(|r| r.run_id == run_id));

    // list_tool_calls returns the call ordered by sequence.
    let tools = repo.list_tool_calls(run_id).await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_name, "library_lookup");

    // run_audit maps DB values to DTO enums and includes tool calls.
    let audit = repo
        .run_audit(
            &IntelligenceRunAuditRequest {
                run_id,
                pagination: IntelligencePagination::default(),
                caps: IntelligenceCaps::default(),
            },
            Some(alice),
        )
        .await
        .unwrap();
    assert_eq!(audit.run.run_id, run_id);
    assert!(matches!(
        audit.run.status,
        ferrex_core::api::types::intelligence::IntelligenceRunStatus::Succeeded
    ));
    assert_eq!(audit.run.tool_calls.len(), 1);
    assert!(
        audit
            .run
            .tool_calls
            .iter()
            .any(|t| t.name == "library_lookup")
    );

    // Bob cannot read Alice's run audit.
    let bob_audit = repo
        .run_audit(
            &IntelligenceRunAuditRequest {
                run_id,
                pagination: IntelligencePagination::default(),
                caps: IntelligenceCaps::default(),
            },
            Some(bob_uuid()),
        )
        .await;
    assert!(bob_audit.is_err(), "Bob must not audit Alice's run");
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn invalidate_media_read_model_marks_rows_invalidated(pool: PgPool) {
    let repo = repo(&pool);
    repo.refresh_library_read_models(lib_a(), None)
        .await
        .unwrap();
    let arrival = Uuid::parse_str(MOVIE_ARRIVAL).unwrap();

    assert_eq!(
        context_status(&pool, arrival).await,
        vec!["active".to_string()]
    );

    repo.invalidate_media_read_model(
        lib_a(),
        movie_from_str(MOVIE_ARRIVAL),
        None,
        "file removed",
    )
    .await
    .unwrap();

    let statuses = context_status(&pool, arrival).await;
    assert!(
        statuses.iter().all(|s| s == "invalidated"),
        "context rows should be invalidated, got {statuses:?}"
    );

    // Invalidated rows are excluded from candidate search.
    let response = repo
        .candidate_search(
            &IntelligenceCandidateSearchRequest {
                query: "arrival".to_string(),
                library_ids: vec![lib_a()],
                media_kinds: vec![IntelligenceMediaKind::Movie],
                pagination: IntelligencePagination::default(),
                caps: IntelligenceCaps::default(),
                include_artifacts: false,
            },
            None,
        )
        .await
        .unwrap();
    assert!(
        response.candidates.is_empty(),
        "invalidated read models must not appear in search"
    );
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn catalog_unavailability_invalidates_all_scopes_and_artifacts(
    pool: PgPool,
) {
    let repo = repo(&pool);
    let alice = Uuid::parse_str(ALICE).unwrap();
    let arrival = Uuid::parse_str(MOVIE_ARRIVAL).unwrap();

    repo.refresh_library_read_models(lib_a(), None)
        .await
        .unwrap();
    repo.refresh_library_read_models(lib_a(), Some(alice))
        .await
        .unwrap();

    let global_id = repo
        .upsert_artifact(IntelligenceArtifactUpsert {
            artifact_id: None,
            kind: IntelligenceArtifactKind::Summary,
            scope: IntelligenceArtifactScope::Global,
            library_id: Some(lib_a()),
            media_id: Some(movie_from_str(MOVIE_ARRIVAL)),
            run_id: None,
            supersedes_artifact_id: None,
            title: "Global Arrival note".to_string(),
            summary: Some("Global generated note".to_string()),
            excerpt: None,
            content: json!({}),
            metadata: json!({}),
            source_revision: 1,
        })
        .await
        .unwrap();
    let user_id = repo
        .upsert_artifact(IntelligenceArtifactUpsert {
            artifact_id: None,
            kind: IntelligenceArtifactKind::UserNote,
            scope: IntelligenceArtifactScope::User(alice),
            library_id: Some(lib_a()),
            media_id: Some(movie_from_str(MOVIE_ARRIVAL)),
            run_id: None,
            supersedes_artifact_id: None,
            title: "Alice Arrival note".to_string(),
            summary: Some("Private generated note".to_string()),
            excerpt: None,
            content: json!({}),
            metadata: json!({}),
            source_revision: 1,
        })
        .await
        .unwrap();

    repo.invalidate_media_catalog_change(
        lib_a(),
        movie_from_str(MOVIE_ARRIVAL),
        "file tombstoned",
    )
    .await
    .unwrap();

    let active_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM intelligence_media_context \
         WHERE media_id = $1 AND status = 'active'",
    )
    .bind(arrival)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_rows, 0);

    let invalidated_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM intelligence_media_context \
         WHERE media_id = $1 AND status = 'invalidated'",
    )
    .bind(arrival)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        invalidated_rows >= 2,
        "global and user rows should be invalidated"
    );

    let artifact_states: Vec<(Uuid, String, bool, Option<String>)> =
        sqlx::query_as(
            "SELECT id, status::text, invalidated_at IS NOT NULL, invalidation_reason \
             FROM intelligence_artifacts WHERE id = ANY($1)",
        )
        .bind(vec![global_id, user_id])
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(artifact_states.len(), 2);
    for (_, status, invalidated, reason) in artifact_states {
        assert_eq!(status, "invalidated");
        assert!(invalidated);
        assert_eq!(reason.as_deref(), Some("file tombstoned"));
    }
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn user_refresh_upserts_watch_state_rows(pool: PgPool) {
    let repo = repo(&pool);
    let alice = Uuid::parse_str(ALICE).unwrap();
    let arrival = Uuid::parse_str(MOVIE_ARRIVAL).unwrap();

    let refreshed = repo
        .refresh_library_read_models(lib_a(), Some(alice))
        .await
        .unwrap();
    assert!(refreshed >= 2, "Alice has watch progress on two movies");

    // User-scoped watch_state context row exists for Alice.
    let user_rows: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_media_context \
         WHERE media_id = $1 AND user_id = $2 AND context_kind = 'watch_state'",
    )
    .bind(arrival)
    .bind(alice)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        user_rows, 1,
        "watch_state context row should be upserted for Alice"
    );

    // Bob has only unavailable watch progress, so no user-scoped rows for him.
    sqlx::query(
        "INSERT INTO user_watch_progress \
         (user_id, position, duration, last_watched, updated_at, media_uuid, media_type) \
         VALUES ($1, 60.0, 600.0, 1700000100, 1700000100, $2, 0) \
         ON CONFLICT (user_id, media_uuid) DO NOTHING",
    )
    .bind(bob_uuid())
    .bind(Uuid::parse_str(MOVIE_GONE).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    let bob_refreshed = repo
        .refresh_library_read_models(lib_a(), Some(bob_uuid()))
        .await
        .unwrap();
    assert_eq!(
        bob_refreshed, 0,
        "unavailable watch progress must not build user context rows"
    );
    let bob_rows: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_media_context WHERE user_id = $1",
    )
    .bind(bob_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bob_rows, 0, "Bob has no watch-state rows");

    let watch_search = IntelligenceCandidateSearchRequest {
        query: "watch state".to_string(),
        library_ids: vec![lib_a()],
        media_kinds: vec![IntelligenceMediaKind::Movie],
        pagination: IntelligencePagination::default(),
        caps: IntelligenceCaps::default(),
        include_artifacts: false,
    };
    let alice_results = repo
        .candidate_search(&watch_search, Some(alice))
        .await
        .unwrap();
    assert!(
        !alice_results.candidates.is_empty(),
        "Alice should see her watch-state search documents"
    );
    let bob_results = repo
        .candidate_search(&watch_search, Some(bob_uuid()))
        .await
        .unwrap();
    assert!(
        bob_results.candidates.is_empty(),
        "Bob must not see Alice's watch-state search documents"
    );
}

fn movie_from_str(id: &str) -> MediaID {
    movie(id)
}

fn episode_from_str(id: &str) -> MediaID {
    MediaID::Episode(EpisodeID(Uuid::parse_str(id).unwrap()))
}

fn bob_uuid() -> Uuid {
    Uuid::parse_str(BOB).unwrap()
}
