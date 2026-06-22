//! SQLx-backed behavior tests for the Postgres transcript repository.
//!
//! These tests run against a migrated Postgres instance with the shared
//! fixtures. Run via:
//!
//! `DATABASE_URL=... cargo test -p ferrex-core --test transcript_repository`

use ferrex_core::api::types::intelligence::{
    IntelligenceArtifactKind, IntelligenceCaps, IntelligencePagination,
    TimedTextSnippetSearchRequest, TimedTextSourceKind,
};
use ferrex_core::database::repositories::intelligence::PostgresIntelligenceRepository;
use ferrex_core::database::repositories::transcripts::PostgresTranscriptRepository;
use ferrex_core::database::repository_ports::intelligence::IntelligenceRepository;
use ferrex_core::database::repository_ports::transcripts::{
    TranscriptProcessingState, TranscriptRepository, TranscriptSegmentUpsert,
    TranscriptSourceStatus, TranscriptSourceStatusFilter,
    TranscriptSourceUpsert, TranscriptStatusFilter,
};
use ferrex_core::types::{LibraryId, MediaID, MovieID};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const LIB_A: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
const LIB_C: &str = "cccccccc-cccc-cccc-cccc-cccccccccccc";
const MOVIE_ARRIVAL: &str = "22222222-0000-0000-0000-000000000001";
const MOVIE_TENET: &str = "22222222-0000-0000-0000-000000000002";
const FILE_TENET: &str = "33333333-0000-0000-0000-000000000002";
const FIXTURE_SOURCE: &str = "88888888-0000-0000-0000-000000000101";
const FIXTURE_ARTIFACT: &str = "88888888-0000-0000-0000-000000000001";

fn lib_a() -> LibraryId {
    LibraryId(Uuid::parse_str(LIB_A).unwrap())
}

fn lib_c() -> LibraryId {
    LibraryId(Uuid::parse_str(LIB_C).unwrap())
}

fn movie(id: &str) -> MediaID {
    MediaID::Movie(MovieID(Uuid::parse_str(id).unwrap()))
}

fn transcript_repo(pool: &PgPool) -> PostgresTranscriptRepository {
    PostgresTranscriptRepository::new(pool.clone())
}

fn snippet_request(query: &str) -> TimedTextSnippetSearchRequest {
    TimedTextSnippetSearchRequest {
        query: query.to_string(),
        library_ids: vec![lib_a()],
        media_ids: Vec::new(),
        media_kinds: Vec::new(),
        language_codes: Vec::new(),
        source_kinds: Vec::new(),
        pagination: IntelligencePagination::new(None, 10),
        caps: IntelligenceCaps {
            summary_max_chars: 160,
            timed_text_snippet_max_chars: 160,
            ..Default::default()
        },
        include_artifacts: true,
    }
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base", "transcript_base")
    )
)]
async fn snippet_search_uses_fixture_rows_and_filters_scope(pool: PgPool) {
    let repo = transcript_repo(&pool);

    let response = repo
        .search_snippets(&snippet_request("alien language"), None)
        .await
        .unwrap();
    assert!(
        !response.snippets.is_empty(),
        "fixture snippet should match"
    );
    let snippet = &response.snippets[0];
    assert_eq!(snippet.media.title, "Arrival");
    assert_eq!(snippet.media.library_id, Some(lib_a()));
    assert_eq!(snippet.source_kind, TimedTextSourceKind::Sidecar);
    assert_eq!(snippet.language_code, "en");
    assert_eq!(snippet.start_ms, 1000);
    assert_eq!(snippet.end_ms, 9000);
    assert_eq!(snippet.segment_ids.len(), 3);
    let artifact_id = Uuid::parse_str(FIXTURE_ARTIFACT).unwrap();
    assert_eq!(snippet.artifact_id, Some(artifact_id));
    assert!(snippet.snippet.text.contains("alien language"));
    assert!(snippet.score.unwrap_or_default() > 0.0);

    let mut wrong_library = snippet_request("alien language");
    wrong_library.library_ids = vec![lib_c()];
    assert!(
        repo.search_snippets(&wrong_library, None)
            .await
            .unwrap()
            .snippets
            .is_empty(),
        "library scope must filter transcript rows"
    );

    let mut wrong_language = snippet_request("alien language");
    wrong_language.language_codes = vec!["fr".to_string()];
    assert!(
        repo.search_snippets(&wrong_language, None)
            .await
            .unwrap()
            .snippets
            .is_empty(),
        "language filter must be enforced"
    );

    let source_statuses = repo
        .list_source_status(TranscriptSourceStatusFilter {
            library_id: Some(lib_a()),
            media_id: Some(movie(MOVIE_ARRIVAL)),
            media_file_id: None,
            status: Some(TranscriptSourceStatus::Active),
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(source_statuses.len(), 1);
    assert_eq!(source_statuses[0].source_kind, TimedTextSourceKind::Sidecar);
    assert_eq!(source_statuses[0].segment_count, 3);
    assert_eq!(source_statuses[0].artifact_id, Some(artifact_id));

    let statuses = repo
        .list_processing_status(TranscriptStatusFilter {
            library_id: Some(lib_a()),
            media_id: Some(movie(MOVIE_ARRIVAL)),
            media_file_id: None,
            status: Some(TranscriptProcessingState::Succeeded),
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].source_count, 1);
    assert_eq!(statuses[0].segment_count, 3);
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base", "transcript_base")
    )
)]
async fn transcript_artifact_summaries_do_not_return_raw_content(pool: PgPool) {
    let intelligence = PostgresIntelligenceRepository::new(pool.clone());
    let artifact_id = Uuid::parse_str(FIXTURE_ARTIFACT).unwrap();
    let summary = intelligence
        .get_artifact(artifact_id, None)
        .await
        .unwrap()
        .expect("fixture transcript artifact should be visible");

    assert_eq!(summary.kind, IntelligenceArtifactKind::TranscriptSource);
    let json = serde_json::to_string(&summary).unwrap();
    assert!(json.contains("English sidecar transcript source"));
    assert!(!json.contains("raw_body"));
    assert!(!json.contains("Louise translates the alien language"));
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base")
    )
)]
async fn source_and_segment_upserts_are_idempotent_and_hashed(pool: PgPool) {
    let repo = transcript_repo(&pool);
    let source = TranscriptSourceUpsert {
        source_id: None,
        library_id: lib_a(),
        media_id: movie(MOVIE_TENET),
        media_file_id: Uuid::parse_str(FILE_TENET).unwrap(),
        source_kind: TimedTextSourceKind::Sidecar,
        language_code: "en".to_string(),
        source_key: format!("sidecar:{}", "1".repeat(64)),
        source_name: Some("Tenet English".to_string()),
        stream_index: None,
        source_path_hash: Some("1".repeat(64)),
        source_content_hash: "2".repeat(64),
        normalized_content_hash: Some("3".repeat(64)),
        artifact_id: None,
        duration_ms: Some(9000),
        source_locator: json!({"kind": "sidecar_hash"}),
        metadata: json!({"fixture": "upsert"}),
    };
    let segments = vec![
        TranscriptSegmentUpsert {
            cue_index: 0,
            start_ms: 0,
            end_ms: 2000,
            text: "Inverted objects move through the room.".to_string(),
            metadata: json!({}),
        },
        TranscriptSegmentUpsert {
            cue_index: 1,
            start_ms: 2000,
            end_ms: 4000,
            text: "The protagonist tracks a temporal signal.".to_string(),
            metadata: json!({}),
        },
    ];

    let first = repo
        .upsert_source_with_segments(source.clone(), segments.clone())
        .await
        .unwrap();
    let second = repo
        .upsert_source_with_segments(source.clone(), segments.clone())
        .await
        .unwrap();
    assert_eq!(first.source_id, second.source_id);
    assert_eq!(first.segment_count, 2);
    assert_eq!(first.source_content_hash, "2".repeat(64));

    let (source_hash, source_segment_count): (String, i32) = sqlx::query_as(
        "SELECT source_content_hash, segment_count FROM transcript_sources WHERE id = $1",
    )
    .bind(first.source_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(source_hash, "2".repeat(64));
    assert_eq!(source_segment_count, 2);

    let segment_hashes: Vec<String> = sqlx::query_scalar(
        "SELECT segment_hash FROM transcript_segments WHERE transcript_source_id = $1 ORDER BY cue_index",
    )
    .bind(first.source_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(segment_hashes.len(), 2);
    assert!(segment_hashes.iter().all(|hash| hash.len() == 64));

    let replace = repo
        .upsert_source_with_segments(
            TranscriptSourceUpsert {
                source_content_hash: "4".repeat(64),
                ..source
            },
            vec![TranscriptSegmentUpsert {
                cue_index: 0,
                start_ms: 0,
                end_ms: 3000,
                text: "Replacement transcript cue.".to_string(),
                metadata: json!({}),
            }],
        )
        .await
        .unwrap();
    assert_eq!(replace.source_id, first.source_id);
    assert_eq!(replace.segment_count, 1);

    let segment_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM transcript_segments WHERE transcript_source_id = $1",
    )
    .bind(first.source_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(segment_count, 1);
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base", "transcript_base")
    )
)]
async fn schema_rejects_overlapping_or_unbounded_segments(pool: PgPool) {
    let source_id = Uuid::parse_str(FIXTURE_SOURCE).unwrap();
    let overlap = sqlx::query(
        r#"
        INSERT INTO transcript_segments (
            transcript_source_id,
            library_id,
            media_id,
            media_type,
            media_file_id,
            language_code,
            cue_index,
            start_ms,
            end_ms,
            cue_text,
            segment_hash
        ) VALUES ($1, $2, $3, 'movie', $4, 'en', 99, 2000, 4000, 'overlap', repeat('1', 64))
        "#,
    )
    .bind(source_id)
    .bind(lib_a().0)
    .bind(Uuid::parse_str(MOVIE_ARRIVAL).unwrap())
    .bind(Uuid::parse_str("33333333-0000-0000-0000-000000000001").unwrap())
    .execute(&pool)
    .await;
    assert!(overlap.is_err(), "overlapping active cues must fail");

    let oversized = TranscriptSegmentUpsert {
        cue_index: 99,
        start_ms: 9000,
        end_ms: 10000,
        text: "x".repeat(4001),
        metadata: json!({}),
    };
    let repo = transcript_repo(&pool);
    let source = TranscriptSourceUpsert {
        source_id: None,
        library_id: lib_a(),
        media_id: movie(MOVIE_TENET),
        media_file_id: Uuid::parse_str(FILE_TENET).unwrap(),
        source_kind: TimedTextSourceKind::Sidecar,
        language_code: "en".to_string(),
        source_key: format!("sidecar:{}", "5".repeat(64)),
        source_name: None,
        stream_index: None,
        source_path_hash: Some("5".repeat(64)),
        source_content_hash: "6".repeat(64),
        normalized_content_hash: None,
        artifact_id: None,
        duration_ms: None,
        source_locator: json!({}),
        metadata: json!({}),
    };
    assert!(
        repo.upsert_source_with_segments(source, vec![oversized])
            .await
            .is_err(),
        "repository must reject oversized cue text before persistence"
    );
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "intelligence_base", "transcript_base")
    )
)]
async fn invalidation_and_purge_remove_snippets_and_status(pool: PgPool) {
    let repo = transcript_repo(&pool);

    let invalidated = repo
        .invalidate_media(lib_a(), movie(MOVIE_ARRIVAL), "media changed")
        .await
        .unwrap();
    assert_eq!(invalidated, 1);
    assert!(
        repo.search_snippets(&snippet_request("alien language"), None)
            .await
            .unwrap()
            .snippets
            .is_empty(),
        "invalidated sources must not be searchable"
    );
    let invalidated_status = repo
        .list_processing_status(TranscriptStatusFilter {
            library_id: Some(lib_a()),
            media_id: Some(movie(MOVIE_ARRIVAL)),
            media_file_id: None,
            status: Some(TranscriptProcessingState::Invalidated),
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(invalidated_status.len(), 1);

    let purged = repo
        .purge_media(lib_a(), movie(MOVIE_ARRIVAL), "operator purge")
        .await
        .unwrap();
    assert_eq!(purged, 1);

    let segment_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM transcript_segments WHERE transcript_source_id = $1",
    )
    .bind(Uuid::parse_str(FIXTURE_SOURCE).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(segment_count, 0, "purge removes stored cue text rows");

    let (source_status, source_segment_count): (String, i32) = sqlx::query_as(
        "SELECT status::text, segment_count FROM transcript_sources WHERE id = $1",
    )
    .bind(Uuid::parse_str(FIXTURE_SOURCE).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(source_status, "purged");
    assert_eq!(source_segment_count, 0);

    let purged_sources = repo
        .list_source_status(TranscriptSourceStatusFilter {
            library_id: Some(lib_a()),
            media_id: Some(movie(MOVIE_ARRIVAL)),
            media_file_id: None,
            status: Some(TranscriptSourceStatus::Purged),
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(purged_sources.len(), 1);
    assert_eq!(purged_sources[0].status, TranscriptSourceStatus::Purged);
    assert_eq!(purged_sources[0].segment_count, 0);

    let purged_status = repo
        .list_processing_status(TranscriptStatusFilter {
            library_id: Some(lib_a()),
            media_id: Some(movie(MOVIE_ARRIVAL)),
            media_file_id: None,
            status: Some(TranscriptProcessingState::Purged),
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(purged_status.len(), 1);

    let (artifact_status, content): (String, serde_json::Value) =
        sqlx::query_as("SELECT status::text, content FROM intelligence_artifacts WHERE id = $1")
            .bind(Uuid::parse_str(FIXTURE_ARTIFACT).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(artifact_status, "deleted");
    assert_eq!(content, json!({}));
}
