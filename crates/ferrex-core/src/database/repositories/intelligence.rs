//! PostgreSQL-backed implementation of [`IntelligenceRepository`].
//!
//! Repository/business SQL uses SQLx compile-checked macros so schema drift is
//! caught by offline metadata and CI. Behavior is validated by the SQLx-backed
//! integration tests in `tests/intelligence_repository.rs`.

use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use ferrex_model::{
    EpisodeID, LibraryId, MediaID, MovieID, SeasonID, SeriesID,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

use crate::{
    api::types::intelligence::{
        self, DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT,
        DEFAULT_INTELLIGENCE_CANDIDATE_LIMIT, DEFAULT_INTELLIGENCE_FACET_LIMIT,
        DEFAULT_INTELLIGENCE_GROUNDING_LIMIT, DEFAULT_INTELLIGENCE_PAGE_LIMIT,
        DEFAULT_INTELLIGENCE_RELATED_LIMIT, DEFAULT_INTELLIGENCE_SUMMARY_CHARS,
        DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT, IntelligenceArtifactKind,
        IntelligenceArtifactSearchRequest, IntelligenceArtifactSearchResponse,
        IntelligenceArtifactSourceEdge, IntelligenceArtifactSourceKind,
        IntelligenceArtifactStatus, IntelligenceArtifactSummary,
        IntelligenceCandidate, IntelligenceCandidateSearchRequest,
        IntelligenceCandidateSearchResponse, IntelligenceContextItem,
        IntelligenceDraftArtifactPayload, IntelligenceError,
        IntelligenceErrorCode, IntelligenceFacetGroup, IntelligenceFacetKind,
        IntelligenceFacetValue, IntelligenceGroundingRef,
        IntelligenceGroundingSource, IntelligenceItemContextRequest,
        IntelligenceItemContextResponse, IntelligenceLibraryOverview,
        IntelligenceLibraryOverviewRequest,
        IntelligenceLibraryOverviewResponse, IntelligenceMediaCounts,
        IntelligenceMediaKind, IntelligenceMediaRef, IntelligenceProvenanceRef,
        IntelligenceRelatedContext, IntelligenceRelatedContextRequest,
        IntelligenceRelatedContextResponse, IntelligenceRelationshipKind,
        IntelligenceRunAudit, IntelligenceRunAuditRequest,
        IntelligenceRunAuditResponse, IntelligenceRunEvent,
        IntelligenceRunEventKind, IntelligenceRunPurpose,
        IntelligenceRunStatus, IntelligenceSummary, IntelligenceToolCallAudit,
        IntelligenceToolCallStatus, MAX_INTELLIGENCE_ARTIFACT_LIMIT,
        MAX_INTELLIGENCE_CANDIDATE_LIMIT, MAX_INTELLIGENCE_FACET_LIMIT,
        MAX_INTELLIGENCE_GROUNDING_LIMIT, MAX_INTELLIGENCE_PAGE_LIMIT,
        MAX_INTELLIGENCE_RELATED_LIMIT, MAX_INTELLIGENCE_TOOL_CALL_LIMIT,
    },
    database::repository_ports::intelligence::{
        IntelligenceArtifactScope, IntelligenceArtifactUpsert,
        IntelligenceDraftArtifactCreate, IntelligenceRepository,
        IntelligenceRunCreate, IntelligenceRunEventCreate,
        IntelligenceRunEventListFilter, IntelligenceRunKind,
        IntelligenceRunListFilter, IntelligenceRunStatus as RunStatusInternal,
        IntelligenceRunSummary, IntelligenceRunUpdate,
        IntelligenceToolCallCreate,
        IntelligenceToolCallStatus as ToolStatusInternal,
        IntelligenceToolCallSummary, IntelligenceToolCallUpdate,
        IntelligenceToolKind,
    },
    error::{MediaError, Result},
};

#[derive(Clone, Debug)]
pub struct PostgresIntelligenceRepository {
    pool: PgPool,
}

impl PostgresIntelligenceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn media_type_str(media_id: &MediaID) -> &'static str {
    match media_id {
        MediaID::Movie(_) => "movie",
        MediaID::Series(_) => "series",
        MediaID::Season(_) => "season",
        MediaID::Episode(_) => "episode",
    }
}

fn media_id_from_parts(value: &str, uuid: Uuid) -> MediaID {
    match value {
        "series" => MediaID::Series(SeriesID(uuid)),
        "season" => MediaID::Season(SeasonID(uuid)),
        "episode" => MediaID::Episode(EpisodeID(uuid)),
        _ => MediaID::Movie(MovieID(uuid)),
    }
}

fn clamp_limit(limit: u16, default: u16, max: u16) -> u16 {
    if limit == 0 {
        default
    } else if limit > max {
        max
    } else {
        limit
    }
}

/// Deterministic SHA-256 hex digest used for `content_hash` columns.
fn content_hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\x1f");
    }
    hex::encode(hasher.finalize())
}

/// Deterministic JSON string (serde_json sorts object keys by default).
fn canonical_json(value: &Value) -> String {
    value.to_string()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => text[..idx].to_string(),
        None => text.to_string(),
    }
}

fn bounded_summary(text: &str, max_chars: u16) -> IntelligenceSummary {
    IntelligenceSummary::with_max_chars(text, max_chars)
}

fn year_from_date(date: Option<NaiveDate>) -> Option<u16> {
    date.and_then(|d| {
        let y = d.year();
        u16::try_from(y).ok().filter(|_| (0..=9999).contains(&y))
    })
}

/// Map a DTO artifact kind to the database `artifact_kind` enum value.
///
/// `EmbeddingChunk` and `TranscriptSegment` are explicitly deferred by the
/// foundation schema (vector and transcript segment storage land in later
/// phases) and are rejected with a validation error rather than stored.
fn artifact_kind_to_db(kind: IntelligenceArtifactKind) -> Result<&'static str> {
    Ok(match kind {
        IntelligenceArtifactKind::Summary => "summary",
        IntelligenceArtifactKind::Recommendation => "recommendation",
        IntelligenceArtifactKind::GeneratedAnswer => "search_answer",
        IntelligenceArtifactKind::UserNote => "note",
        IntelligenceArtifactKind::AuditRecord => "analysis",
        IntelligenceArtifactKind::EmbeddingChunk => {
            return Err(MediaError::InvalidMedia(
                "embedding chunk artifacts are deferred to a later phase"
                    .to_string(),
            ));
        }
        IntelligenceArtifactKind::TranscriptSegment => {
            return Err(MediaError::InvalidMedia(
                "transcript segment artifacts are deferred to a later phase"
                    .to_string(),
            ));
        }
    })
}

fn artifact_kind_from_db(value: &str) -> IntelligenceArtifactKind {
    match value {
        "recommendation" => IntelligenceArtifactKind::Recommendation,
        "search_answer" => IntelligenceArtifactKind::GeneratedAnswer,
        "note" => IntelligenceArtifactKind::UserNote,
        "analysis" | "index_manifest" => IntelligenceArtifactKind::AuditRecord,
        "watch_plan" => IntelligenceArtifactKind::GeneratedAnswer,
        "collection" => IntelligenceArtifactKind::Recommendation,
        _ => IntelligenceArtifactKind::Summary,
    }
}

fn artifact_status_from_db(value: &str) -> IntelligenceArtifactStatus {
    match value {
        "active" => IntelligenceArtifactStatus::Active,
        "stale" => IntelligenceArtifactStatus::Stale,
        "superseded" => IntelligenceArtifactStatus::Superseded,
        "invalidated" => IntelligenceArtifactStatus::Invalidated,
        "deleted" => IntelligenceArtifactStatus::Deleted,
        "failed" => IntelligenceArtifactStatus::Failed,
        _ => IntelligenceArtifactStatus::Draft,
    }
}

fn artifact_source_kind_from_db(value: &str) -> IntelligenceArtifactSourceKind {
    match value {
        "media_context" => IntelligenceArtifactSourceKind::MediaContext,
        "search_document" => IntelligenceArtifactSourceKind::SearchDocument,
        "artifact" => IntelligenceArtifactSourceKind::Artifact,
        "run" => IntelligenceArtifactSourceKind::Run,
        "tool_call" => IntelligenceArtifactSourceKind::ToolCall,
        "manual" => IntelligenceArtifactSourceKind::Manual,
        _ => IntelligenceArtifactSourceKind::Media,
    }
}

fn api_run_status_from_db(value: &str) -> IntelligenceRunStatus {
    match value {
        "running" => IntelligenceRunStatus::Running,
        "succeeded" => IntelligenceRunStatus::Succeeded,
        "failed" => IntelligenceRunStatus::Failed,
        "cancelled" => IntelligenceRunStatus::Cancelled,
        _ => IntelligenceRunStatus::Queued,
    }
}

fn run_event_kind_from_db(value: &str) -> IntelligenceRunEventKind {
    match value {
        "started" => IntelligenceRunEventKind::Started,
        "status_changed" => IntelligenceRunEventKind::StatusChanged,
        "model_token" => IntelligenceRunEventKind::ModelToken,
        "tool_call_started" => IntelligenceRunEventKind::ToolCallStarted,
        "tool_call_finished" => IntelligenceRunEventKind::ToolCallFinished,
        "draft_artifact_created" => {
            IntelligenceRunEventKind::DraftArtifactCreated
        }
        "draft_artifact_updated" => {
            IntelligenceRunEventKind::DraftArtifactUpdated
        }
        "cancel_requested" => IntelligenceRunEventKind::CancelRequested,
        "cancelled" => IntelligenceRunEventKind::Cancelled,
        "completed" => IntelligenceRunEventKind::Completed,
        "failed" => IntelligenceRunEventKind::Failed,
        "heartbeat" => IntelligenceRunEventKind::Heartbeat,
        _ => IntelligenceRunEventKind::Queued,
    }
}

fn intelligence_error_code_from_db(value: &str) -> IntelligenceErrorCode {
    match value {
        "feature_disabled" => IntelligenceErrorCode::FeatureDisabled,
        "provider_not_configured" => {
            IntelligenceErrorCode::ProviderNotConfigured
        }
        "provider_unavailable" => IntelligenceErrorCode::ProviderUnavailable,
        "provider_unauthorized" => IntelligenceErrorCode::ProviderUnauthorized,
        "provider_rate_limited" => IntelligenceErrorCode::ProviderRateLimited,
        "provider_timeout" => IntelligenceErrorCode::ProviderTimeout,
        "provider_error" => IntelligenceErrorCode::ProviderError,
        "model_unavailable" => IntelligenceErrorCode::ModelUnavailable,
        "invalid_request" => IntelligenceErrorCode::InvalidRequest,
        "not_found" => IntelligenceErrorCode::NotFound,
        "conflict" => IntelligenceErrorCode::Conflict,
        "concurrency_limit" => IntelligenceErrorCode::ConcurrencyLimit,
        "run_cancelled" => IntelligenceErrorCode::RunCancelled,
        "run_timed_out" => IntelligenceErrorCode::RunTimedOut,
        "tool_timed_out" => IntelligenceErrorCode::ToolTimedOut,
        "storage_error" => IntelligenceErrorCode::StorageError,
        _ => IntelligenceErrorCode::Internal,
    }
}

fn run_kind_from_db(value: &str) -> IntelligenceRunKind {
    match value {
        "search" => IntelligenceRunKind::Search,
        "summarize" => IntelligenceRunKind::Summarize,
        "recommend" => IntelligenceRunKind::Recommend,
        "answer" => IntelligenceRunKind::Answer,
        "maintenance" => IntelligenceRunKind::Maintenance,
        _ => IntelligenceRunKind::Index,
    }
}

fn run_purpose_from_db(value: &str) -> IntelligenceRunPurpose {
    match value {
        "search" => IntelligenceRunPurpose::CandidateSearch,
        "recommend" => IntelligenceRunPurpose::Recommendation,
        "answer" => IntelligenceRunPurpose::Other,
        "summarize" | "maintenance" | "index" => {
            IntelligenceRunPurpose::ArtifactRefresh
        }
        _ => IntelligenceRunPurpose::Other,
    }
}

fn run_status_from_db(value: &str) -> IntelligenceRunStatus {
    match value {
        "running" => IntelligenceRunStatus::Running,
        "succeeded" => IntelligenceRunStatus::Succeeded,
        "failed" => IntelligenceRunStatus::Failed,
        "cancelled" => IntelligenceRunStatus::Cancelled,
        _ => IntelligenceRunStatus::Queued,
    }
}

fn tool_status_from_db(value: &str) -> IntelligenceToolCallStatus {
    match value {
        "running" => IntelligenceToolCallStatus::Running,
        "succeeded" => IntelligenceToolCallStatus::Succeeded,
        "failed" => IntelligenceToolCallStatus::Failed,
        "skipped" => IntelligenceToolCallStatus::Skipped,
        "cancelled" => IntelligenceToolCallStatus::Failed,
        _ => IntelligenceToolCallStatus::Pending,
    }
}

fn tool_kind_from_db(value: &str) -> IntelligenceToolKind {
    match value {
        "read_model" => IntelligenceToolKind::ReadModel,
        "artifact" => IntelligenceToolKind::Artifact,
        "external" => IntelligenceToolKind::External,
        "system" => IntelligenceToolKind::System,
        _ => IntelligenceToolKind::Search,
    }
}

fn internal_err(message: impl Into<String>) -> MediaError {
    MediaError::Internal(message.into())
}

/// Row payload used when building read models and media references.
#[derive(Debug, Clone)]
struct MediaRefRow {
    media_id: MediaID,
    library_id: LibraryId,
    title: String,
    year: Option<u16>,
    poster_iid: Option<Uuid>,
    overview: Option<String>,
    runtime_seconds: Option<i32>,
    release_date: Option<NaiveDate>,
}

/// Genres for a media item, ordered by name for stable output.
async fn fetch_genres(
    pool: &PgPool,
    media_id: &MediaID,
) -> Result<Vec<String>> {
    match media_id {
        MediaID::Movie(id) => sqlx::query_scalar!(
            r#"SELECT name AS "name!" FROM movie_genres WHERE movie_id = $1 ORDER BY name"#,
            id.0
        )
        .fetch_all(pool)
        .await,
        MediaID::Series(id) => sqlx::query_scalar!(
            r#"SELECT name AS "name!" FROM series_genres WHERE series_id = $1 ORDER BY name"#,
            id.0
        )
        .fetch_all(pool)
        .await,
        _ => return Ok(Vec::new()),
    }
    .map_err(|e| internal_err(format!("failed to load genres: {e}")))
}

/// Cast/crew names for a media item, ordered deterministically and bounded.
async fn fetch_people(
    pool: &PgPool,
    media_id: &MediaID,
) -> Result<Vec<String>> {
    let rows = match media_id {
        MediaID::Movie(id) => sqlx::query_scalar!(
            r#"
            SELECT p.name AS "name!"
            FROM (
                SELECT mc.person_id, COALESCE(mc.order_index, 32767) AS order_key
                FROM movie_cast mc
                WHERE mc.movie_id = $1
                UNION ALL
                SELECT mc.person_id, 32767 AS order_key
                FROM movie_crew mc
                WHERE mc.movie_id = $1
            ) credits
            JOIN persons p ON p.id = credits.person_id
            ORDER BY credits.order_key, p.name
            LIMIT 12
            "#,
            id.0
        )
        .fetch_all(pool)
        .await,
        MediaID::Series(id) => sqlx::query_scalar!(
            r#"
            SELECT p.name AS "name!"
            FROM (
                SELECT sc.person_id, COALESCE(sc.order_index, 32767) AS order_key
                FROM series_cast sc
                WHERE sc.series_id = $1
                UNION ALL
                SELECT sc.person_id, 32767 AS order_key
                FROM series_crew sc
                WHERE sc.series_id = $1
            ) credits
            JOIN persons p ON p.id = credits.person_id
            ORDER BY credits.order_key, p.name
            LIMIT 12
            "#,
            id.0
        )
        .fetch_all(pool)
        .await,
        MediaID::Episode(id) => sqlx::query_scalar!(
            r#"
            SELECT p.name AS "name!"
            FROM (
                SELECT ec.person_id, COALESCE(ec.order_index, 32767) AS order_key
                FROM episode_cast ec
                WHERE ec.episode_id = $1
                UNION ALL
                SELECT eg.person_id, COALESCE(eg.order_index, 32767) AS order_key
                FROM episode_guest_stars eg
                WHERE eg.episode_id = $1
                UNION ALL
                SELECT ec.person_id, 32767 AS order_key
                FROM episode_crew ec
                WHERE ec.episode_id = $1
            ) credits
            JOIN persons p ON p.id = credits.person_id
            ORDER BY credits.order_key, p.name
            LIMIT 12
            "#,
            id.0
        )
        .fetch_all(pool)
        .await,
        MediaID::Season(_) => return Ok(Vec::new()),
    }
    .map_err(|e| internal_err(format!("failed to load people: {e}")))?;

    let mut out = Vec::with_capacity(rows.len());
    for name in rows {
        if !out.iter().any(|existing| existing == &name) {
            out.push(name);
        }
    }
    Ok(out)
}

/// Fetch a bounded media reference row for a single media id.
async fn fetch_media_ref_row(
    pool: &PgPool,
    media_id: &MediaID,
    library_id: Option<LibraryId>,
) -> Result<Option<MediaRefRow>> {
    match media_id {
        MediaID::Movie(id) => fetch_movie_ref_row(pool, id, library_id).await,
        MediaID::Series(id) => fetch_series_ref_row(pool, id, library_id).await,
        MediaID::Season(id) => fetch_season_ref_row(pool, id, library_id).await,
        MediaID::Episode(id) => {
            fetch_episode_ref_row(pool, id, library_id).await
        }
    }
}

async fn fetch_movie_ref_row(
    pool: &PgPool,
    id: &MovieID,
    library_id: Option<LibraryId>,
) -> Result<Option<MediaRefRow>> {
    let row = sqlx::query!(
        r#"
        SELECT mr.id AS "id!", mr.library_id AS "library_id!", mr.title AS "title!",
               mm.overview, mm.release_date, mm.runtime,
               mm.primary_poster_image_id AS "primary_poster_image_id?"
        FROM movie_references mr
        LEFT JOIN movie_metadata mm ON mm.movie_id = mr.id
        WHERE mr.id = $1
          AND ($2::uuid IS NULL OR mr.library_id = $2)
        "#,
        id.0,
        library_id.map(|l| l.0)
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load movie ref: {e}")))?;

    let Some(row) = row else { return Ok(None) };
    Ok(Some(MediaRefRow {
        media_id: MediaID::Movie(*id),
        library_id: LibraryId(row.library_id),
        title: row.title,
        year: year_from_date(row.release_date),
        poster_iid: row.primary_poster_image_id,
        overview: row.overview,
        runtime_seconds: row.runtime.map(|m| m.saturating_mul(60)),
        release_date: row.release_date,
    }))
}

async fn fetch_series_ref_row(
    pool: &PgPool,
    id: &SeriesID,
    library_id: Option<LibraryId>,
) -> Result<Option<MediaRefRow>> {
    let row = sqlx::query!(
        r#"
        SELECT s.id AS "id!", s.library_id AS "library_id!", s.title AS "title!",
               sm.overview, sm.first_air_date, sm.primary_poster_image_id AS "primary_poster_image_id?"
        FROM series s
        LEFT JOIN series_metadata sm ON sm.series_id = s.id
        WHERE s.id = $1
          AND ($2::uuid IS NULL OR s.library_id = $2)
        "#,
        id.0,
        library_id.map(|l| l.0)
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load series ref: {e}")))?;

    let Some(row) = row else { return Ok(None) };
    Ok(Some(MediaRefRow {
        media_id: MediaID::Series(*id),
        library_id: LibraryId(row.library_id),
        title: row.title,
        year: year_from_date(row.first_air_date),
        poster_iid: row.primary_poster_image_id,
        overview: row.overview,
        runtime_seconds: None,
        release_date: row.first_air_date,
    }))
}

async fn fetch_season_ref_row(
    pool: &PgPool,
    id: &SeasonID,
    library_id: Option<LibraryId>,
) -> Result<Option<MediaRefRow>> {
    let row = sqlx::query!(
        r#"
        SELECT sr.id AS "id!", sr.library_id AS "library_id!", sr.season_number AS "season_number!",
               sm.name, sm.overview, sm.air_date,
               sm.primary_poster_image_id AS "primary_poster_image_id?"
        FROM season_references sr
        LEFT JOIN season_metadata sm ON sm.season_id = sr.id
        WHERE sr.id = $1
          AND ($2::uuid IS NULL OR sr.library_id = $2)
        "#,
        id.0,
        library_id.map(|l| l.0)
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load season ref: {e}")))?;

    let Some(row) = row else { return Ok(None) };
    let title = row
        .name
        .unwrap_or_else(|| format!("Season {}", row.season_number));
    Ok(Some(MediaRefRow {
        media_id: MediaID::Season(*id),
        library_id: LibraryId(row.library_id),
        title,
        year: year_from_date(row.air_date),
        poster_iid: row.primary_poster_image_id,
        overview: row.overview,
        runtime_seconds: None,
        release_date: row.air_date,
    }))
}

async fn fetch_episode_ref_row(
    pool: &PgPool,
    id: &EpisodeID,
    library_id: Option<LibraryId>,
) -> Result<Option<MediaRefRow>> {
    let row = sqlx::query!(
        r#"
        SELECT er.id AS "id!", er.season_number AS "season_number!", er.episode_number AS "episode_number!",
               s.library_id AS "library_id!", em.name, em.overview, em.air_date,
               em.runtime, em.primary_thumbnail_image_id AS "primary_thumbnail_image_id?"
        FROM episode_references er
        JOIN series s ON er.series_id = s.id
        LEFT JOIN episode_metadata em ON em.episode_id = er.id
        WHERE er.id = $1
          AND ($2::uuid IS NULL OR s.library_id = $2)
        "#,
        id.0,
        library_id.map(|l| l.0)
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load episode ref: {e}")))?;

    let Some(row) = row else { return Ok(None) };
    let title = row.name.unwrap_or_else(|| {
        format!("S{:02}E{:02}", row.season_number, row.episode_number)
    });
    Ok(Some(MediaRefRow {
        media_id: MediaID::Episode(*id),
        library_id: LibraryId(row.library_id),
        title,
        year: year_from_date(row.air_date),
        poster_iid: row.primary_thumbnail_image_id,
        overview: row.overview,
        runtime_seconds: row.runtime.map(|m| m.saturating_mul(60)),
        release_date: row.air_date,
    }))
}

fn build_media_ref(
    row: &MediaRefRow,
    artifact_ids: Vec<Uuid>,
) -> IntelligenceMediaRef {
    IntelligenceMediaRef {
        media_id: row.media_id,
        media_kind: IntelligenceMediaKind::from(&row.media_id),
        library_id: Some(row.library_id),
        title: row.title.clone(),
        year: row.year,
        poster_iid: row.poster_iid,
        artifact_ids,
    }
}

/// Build the bounded `metadata` jsonb payload for a media context row.
fn context_metadata_json(
    overview: &Option<String>,
    genres: &[String],
    people: &[String],
    vote_average: Option<f32>,
    content_rating: Option<&str>,
) -> Value {
    let mut obj = serde_json::Map::new();
    if let Some(v) = overview {
        if !v.is_empty() {
            obj.insert("overview".to_string(), json!(truncate_chars(v, 2048)));
        }
    }
    if !genres.is_empty() {
        obj.insert("genres".to_string(), json!(genres));
    }
    if !people.is_empty() {
        obj.insert("people".to_string(), json!(people));
    }
    if let Some(v) = vote_average {
        obj.insert("vote_average".to_string(), json!(v));
    }
    if let Some(r) = content_rating {
        if !r.is_empty() {
            obj.insert("content_rating".to_string(), json!(r));
        }
    }
    Value::Object(obj)
}

/// Upsert a global or user-scoped media-context read-model row.
async fn upsert_context_row(
    pool: &PgPool,
    library_id: LibraryId,
    user_id: Option<Uuid>,
    row: &MediaRefRow,
    context_kind: &str,
    summary: &str,
    excerpt: &str,
    metadata: &Value,
    source_revision: i64,
) -> Result<bool> {
    let media_id = row.media_id.as_uuid();
    let media_type = media_type_str(&row.media_id);
    let title = truncate_chars(&row.title, 512);
    let sort_title = title.to_lowercase();
    let summary = truncate_chars(summary, 4000);
    let excerpt = truncate_chars(excerpt, 2048);
    // content_hash fingerprints only the bounded content fields and metadata so
    // identical content produces identical hashes across refresh batches.
    let hash = content_hash(&[
        &library_id.0.to_string(),
        media_type,
        &media_id.to_string(),
        context_kind,
        &title,
        &summary,
        &excerpt,
        &row.release_date.map(|d| d.to_string()).unwrap_or_default(),
        &row.runtime_seconds.unwrap_or(0).to_string(),
        &canonical_json(metadata),
    ]);

    let changed = if user_id.is_some() {
        sqlx::query!(
            r#"
            INSERT INTO intelligence_media_context (
                library_id, user_id, media_id, media_type, context_kind, status,
                title, sort_title, summary, excerpt, release_date, runtime_seconds,
                source_system, source_revision, source_updated_at, content_hash, metadata
            )
            VALUES ($1, $2, $3, ($4::text)::media_type, $5, 'active', $6, $7, $8, $9, $10, $11,
                    'ferrex', $12, now(), $13, $14::jsonb)
            ON CONFLICT (library_id, user_id, media_type, media_id, context_kind)
                WHERE user_id IS NOT NULL
            DO UPDATE SET
                status = 'active',
                title = EXCLUDED.title,
                sort_title = EXCLUDED.sort_title,
                summary = EXCLUDED.summary,
                excerpt = EXCLUDED.excerpt,
                release_date = EXCLUDED.release_date,
                runtime_seconds = EXCLUDED.runtime_seconds,
                source_revision = EXCLUDED.source_revision,
                source_updated_at = now(),
                content_hash = EXCLUDED.content_hash,
                metadata = EXCLUDED.metadata,
                invalidated_at = NULL,
                invalidation_reason = NULL,
                updated_at = now()
            WHERE intelligence_media_context.status <> 'active'
               OR intelligence_media_context.content_hash IS DISTINCT FROM EXCLUDED.content_hash
               OR intelligence_media_context.invalidated_at IS NOT NULL
               OR intelligence_media_context.invalidation_reason IS NOT NULL
            RETURNING id
            "#,
            library_id.0,
            user_id,
            media_id,
            media_type,
            context_kind,
            title,
            sort_title,
            summary,
            excerpt,
            row.release_date,
            row.runtime_seconds,
            source_revision,
            hash,
            metadata
        )
        .fetch_optional(pool)
        .await
        .map(|row| row.is_some())
    } else {
        sqlx::query!(
            r#"
            INSERT INTO intelligence_media_context (
                library_id, user_id, media_id, media_type, context_kind, status,
                title, sort_title, summary, excerpt, release_date, runtime_seconds,
                source_system, source_revision, source_updated_at, content_hash, metadata
            )
            VALUES ($1, $2, $3, ($4::text)::media_type, $5, 'active', $6, $7, $8, $9, $10, $11,
                    'ferrex', $12, now(), $13, $14::jsonb)
            ON CONFLICT (library_id, media_type, media_id, context_kind)
                WHERE user_id IS NULL
            DO UPDATE SET
                status = 'active',
                title = EXCLUDED.title,
                sort_title = EXCLUDED.sort_title,
                summary = EXCLUDED.summary,
                excerpt = EXCLUDED.excerpt,
                release_date = EXCLUDED.release_date,
                runtime_seconds = EXCLUDED.runtime_seconds,
                source_revision = EXCLUDED.source_revision,
                source_updated_at = now(),
                content_hash = EXCLUDED.content_hash,
                metadata = EXCLUDED.metadata,
                invalidated_at = NULL,
                invalidation_reason = NULL,
                updated_at = now()
            WHERE intelligence_media_context.status <> 'active'
               OR intelligence_media_context.content_hash IS DISTINCT FROM EXCLUDED.content_hash
               OR intelligence_media_context.invalidated_at IS NOT NULL
               OR intelligence_media_context.invalidation_reason IS NOT NULL
            RETURNING id
            "#,
            library_id.0,
            user_id,
            media_id,
            media_type,
            context_kind,
            title,
            sort_title,
            summary,
            excerpt,
            row.release_date,
            row.runtime_seconds,
            source_revision,
            hash,
            metadata
        )
        .fetch_optional(pool)
        .await
        .map(|row| row.is_some())
    }
    .map_err(|e| internal_err(format!("failed to upsert context: {e}")))?;
    Ok(changed)
}

/// Upsert a global or user-scoped search-document read-model row.
async fn upsert_search_row(
    pool: &PgPool,
    library_id: LibraryId,
    user_id: Option<Uuid>,
    row: &MediaRefRow,
    document_kind: &str,
    summary: &str,
    search_excerpt: &str,
    search_text: &str,
    metadata: &Value,
    source_revision: i64,
) -> Result<bool> {
    let media_id = row.media_id.as_uuid();
    let media_type = media_type_str(&row.media_id);
    let title = truncate_chars(&row.title, 512);
    let summary = truncate_chars(summary, 4000);
    let excerpt = truncate_chars(search_excerpt, 2048);
    let text = truncate_chars(search_text, 16000);
    let hash = content_hash(&[
        &library_id.0.to_string(),
        media_type,
        &media_id.to_string(),
        document_kind,
        &title,
        &summary,
        &excerpt,
        &text,
        &canonical_json(metadata),
    ]);

    let changed = if user_id.is_some() {
        sqlx::query!(
            r#"
            INSERT INTO intelligence_search_documents (
                library_id, user_id, media_id, media_type, document_kind, status,
                title, summary, search_excerpt, search_text, language,
                source_system, source_revision, source_updated_at, content_hash, metadata
            )
            VALUES ($1, $2, $3, ($4::text)::media_type, $5, 'active', $6, $7, $8, $9, 'simple',
                    'ferrex', $10, now(), $11, $12::jsonb)
            ON CONFLICT (library_id, user_id, media_type, media_id, document_kind)
                WHERE user_id IS NOT NULL
            DO UPDATE SET
                status = 'active',
                title = EXCLUDED.title,
                summary = EXCLUDED.summary,
                search_excerpt = EXCLUDED.search_excerpt,
                search_text = EXCLUDED.search_text,
                source_revision = EXCLUDED.source_revision,
                source_updated_at = now(),
                content_hash = EXCLUDED.content_hash,
                metadata = EXCLUDED.metadata,
                invalidated_at = NULL,
                invalidation_reason = NULL,
                updated_at = now()
            WHERE intelligence_search_documents.status <> 'active'
               OR intelligence_search_documents.content_hash IS DISTINCT FROM EXCLUDED.content_hash
               OR intelligence_search_documents.invalidated_at IS NOT NULL
               OR intelligence_search_documents.invalidation_reason IS NOT NULL
            RETURNING id
            "#,
            library_id.0,
            user_id,
            media_id,
            media_type,
            document_kind,
            title,
            summary,
            excerpt,
            text,
            source_revision,
            hash,
            metadata
        )
        .fetch_optional(pool)
        .await
        .map(|row| row.is_some())
    } else {
        sqlx::query!(
            r#"
            INSERT INTO intelligence_search_documents (
                library_id, user_id, media_id, media_type, document_kind, status,
                title, summary, search_excerpt, search_text, language,
                source_system, source_revision, source_updated_at, content_hash, metadata
            )
            VALUES ($1, $2, $3, ($4::text)::media_type, $5, 'active', $6, $7, $8, $9, 'simple',
                    'ferrex', $10, now(), $11, $12::jsonb)
            ON CONFLICT (library_id, media_type, media_id, document_kind)
                WHERE user_id IS NULL
            DO UPDATE SET
                status = 'active',
                title = EXCLUDED.title,
                summary = EXCLUDED.summary,
                search_excerpt = EXCLUDED.search_excerpt,
                search_text = EXCLUDED.search_text,
                source_revision = EXCLUDED.source_revision,
                source_updated_at = now(),
                content_hash = EXCLUDED.content_hash,
                metadata = EXCLUDED.metadata,
                invalidated_at = NULL,
                invalidation_reason = NULL,
                updated_at = now()
            WHERE intelligence_search_documents.status <> 'active'
               OR intelligence_search_documents.content_hash IS DISTINCT FROM EXCLUDED.content_hash
               OR intelligence_search_documents.invalidated_at IS NOT NULL
               OR intelligence_search_documents.invalidation_reason IS NOT NULL
            RETURNING id
            "#,
            library_id.0,
            user_id,
            media_id,
            media_type,
            document_kind,
            title,
            summary,
            excerpt,
            text,
            source_revision,
            hash,
            metadata
        )
        .fetch_optional(pool)
        .await
        .map(|row| row.is_some())
    }
    .map_err(|e| internal_err(format!("failed to upsert search doc: {e}")))?;
    Ok(changed)
}

/// Available-movie rows for global read-model refresh.
#[derive(Debug, Clone)]
struct MovieRefreshRow {
    media_id: Uuid,
    title: String,
    overview: Option<String>,
    release_date: Option<NaiveDate>,
    runtime: Option<i32>,
    vote_average: Option<f32>,
    primary_certification: Option<String>,
}

async fn load_available_movies(
    pool: &PgPool,
    library_id: LibraryId,
) -> Result<Vec<MovieRefreshRow>> {
    sqlx::query_as!(
        MovieRefreshRow,
        r#"
        SELECT mr.id AS "media_id!", mr.title AS "title!",
               mm.overview, mm.release_date, mm.runtime,
               mm.vote_average, mm.primary_certification
        FROM movie_references mr
        JOIN media_files mf ON mr.file_id = mf.id
            AND mf.is_available = TRUE
            AND mf.tombstoned_at IS NULL
        LEFT JOIN movie_metadata mm ON mm.movie_id = mr.id
        WHERE mr.library_id = $1
        ORDER BY mr.title, mr.id
        "#,
        library_id.0
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load movies: {e}")))
}

/// Available-episode rows for global read-model refresh.
#[derive(Debug, Clone)]
struct EpisodeRefreshRow {
    media_id: Uuid,
    series_id: Uuid,
    season_id: Uuid,
    season_number: i16,
    episode_number: i16,
    title: Option<String>,
    overview: Option<String>,
    air_date: Option<NaiveDate>,
    runtime: Option<i32>,
}

async fn load_available_episodes(
    pool: &PgPool,
    library_id: LibraryId,
) -> Result<Vec<EpisodeRefreshRow>> {
    sqlx::query_as!(
        EpisodeRefreshRow,
        r#"
        SELECT er.id AS "media_id!", er.series_id AS "series_id!", er.season_id AS "season_id!",
               er.season_number AS "season_number!", er.episode_number AS "episode_number!",
               em.name AS title, em.overview, em.air_date, em.runtime
        FROM episode_references er
        JOIN series s ON er.series_id = s.id AND s.library_id = $1
        JOIN media_files mf ON er.file_id = mf.id
            AND mf.is_available = TRUE
            AND mf.tombstoned_at IS NULL
        LEFT JOIN episode_metadata em ON em.episode_id = er.id
        ORDER BY er.series_id, er.season_number, er.episode_number, er.id
        "#,
        library_id.0
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load episodes: {e}")))
}

async fn upsert_movie_read_model(
    pool: &PgPool,
    library_id: LibraryId,
    user_id: Option<Uuid>,
    row: &MovieRefreshRow,
    source_revision: i64,
) -> Result<bool> {
    let media_id = MediaID::Movie(MovieID(row.media_id));
    let genres = fetch_genres(pool, &media_id).await?;
    let people = fetch_people(pool, &media_id).await?;
    let media_row = MediaRefRow {
        media_id: MediaID::Movie(MovieID(row.media_id)),
        library_id,
        title: row.title.clone(),
        year: year_from_date(row.release_date),
        poster_iid: None,
        overview: row.overview.clone(),
        runtime_seconds: row.runtime.map(|m| m.saturating_mul(60)),
        release_date: row.release_date,
    };
    let metadata = context_metadata_json(
        &row.overview,
        &genres,
        &people,
        row.vote_average,
        row.primary_certification.as_deref(),
    );

    let summary = row
        .overview
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&row.title);
    let search_text = build_search_text(
        &row.title,
        row.overview.as_deref(),
        &genres,
        &people,
    );

    let mut changed = false;
    if user_id.is_none() {
        changed |= upsert_context_row(
            pool,
            library_id,
            None,
            &media_row,
            "metadata",
            summary,
            summary,
            &metadata,
            source_revision,
        )
        .await?;
        changed |= upsert_search_row(
            pool,
            library_id,
            None,
            &media_row,
            "combined",
            summary,
            summary,
            &search_text,
            &metadata,
            source_revision,
        )
        .await?;
    }
    Ok(changed)
}

fn build_search_text(
    title: &str,
    overview: Option<&str>,
    genres: &[String],
    people: &[String],
) -> String {
    let mut text = String::new();
    text.push_str(title);
    if let Some(o) = overview {
        if !o.is_empty() {
            text.push_str(" | ");
            text.push_str(o);
        }
    }
    if !genres.is_empty() {
        text.push_str(" | genres: ");
        text.push_str(&genres.join(", "));
    }
    if !people.is_empty() {
        text.push_str(" | people: ");
        text.push_str(&people.join(", "));
    }
    text
}

async fn upsert_episode_read_model(
    pool: &PgPool,
    library_id: LibraryId,
    _user_id: Option<Uuid>,
    row: &EpisodeRefreshRow,
    source_revision: i64,
) -> Result<bool> {
    let title = row.title.clone().unwrap_or_else(|| {
        format!("S{:02}E{:02}", row.season_number, row.episode_number)
    });
    let media_row = MediaRefRow {
        media_id: MediaID::Episode(EpisodeID(row.media_id)),
        library_id,
        title: title.clone(),
        year: year_from_date(row.air_date),
        poster_iid: None,
        overview: row.overview.clone(),
        runtime_seconds: row.runtime.map(|m| m.saturating_mul(60)),
        release_date: row.air_date,
    };
    let media_id = MediaID::Episode(EpisodeID(row.media_id));
    let people = fetch_people(pool, &media_id).await?;
    let metadata =
        context_metadata_json(&row.overview, &[], &people, None, None);
    let summary = row
        .overview
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&title);
    let search_text =
        build_search_text(&title, row.overview.as_deref(), &[], &people);

    // Episodes always contribute a global row (they have a direct media file).
    let mut changed = upsert_context_row(
        pool,
        library_id,
        None,
        &media_row,
        "metadata",
        summary,
        summary,
        &metadata,
        source_revision,
    )
    .await?;
    changed |= upsert_search_row(
        pool,
        library_id,
        None,
        &media_row,
        "combined",
        summary,
        summary,
        &search_text,
        &metadata,
        source_revision,
    )
    .await?;
    Ok(changed)
}

async fn upsert_series_read_model(
    pool: &PgPool,
    library_id: LibraryId,
    series_id: Uuid,
    available_episode_ids: &[Uuid],
    source_revision: i64,
) -> Result<bool> {
    if available_episode_ids.is_empty() {
        return Ok(false);
    }
    let media_id = MediaID::Series(SeriesID(series_id));
    let genres = fetch_genres(pool, &media_id).await?;
    let people = fetch_people(pool, &media_id).await?;
    let row = sqlx::query!(
        r#"
        SELECT s.title AS "title!", sm.overview, sm.first_air_date,
               sm.primary_poster_image_id AS "primary_poster_image_id?", sm.primary_content_rating,
               sm.vote_average
        FROM series s
        LEFT JOIN series_metadata sm ON sm.series_id = s.id
        WHERE s.id = $1
        "#,
        series_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load series: {e}")))?;
    let Some(row) = row else { return Ok(false) };
    let title = row.title;
    let overview = row.overview;
    let first_air_date = row.first_air_date;
    let poster = row.primary_poster_image_id;
    let content_rating = row.primary_content_rating;
    let vote_average = row.vote_average;

    let media_row = MediaRefRow {
        media_id: MediaID::Series(SeriesID(series_id)),
        library_id,
        title: title.clone(),
        year: year_from_date(first_air_date),
        poster_iid: poster,
        overview: overview.clone(),
        runtime_seconds: None,
        release_date: first_air_date,
    };
    let metadata = context_metadata_json(
        &overview,
        &genres,
        &people,
        vote_average,
        content_rating.as_deref(),
    );
    let summary = overview
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&title);
    let search_text =
        build_search_text(&title, overview.as_deref(), &genres, &people);

    let mut changed = upsert_context_row(
        pool,
        library_id,
        None,
        &media_row,
        "metadata",
        summary,
        summary,
        &metadata,
        source_revision,
    )
    .await?;
    changed |= upsert_search_row(
        pool,
        library_id,
        None,
        &media_row,
        "combined",
        summary,
        summary,
        &search_text,
        &metadata,
        source_revision,
    )
    .await?;
    Ok(changed)
}

async fn upsert_season_read_model(
    pool: &PgPool,
    library_id: LibraryId,
    season_id: Uuid,
    source_revision: i64,
) -> Result<bool> {
    let row = sqlx::query!(
        r#"
        SELECT sr.season_number AS "season_number!", sm.name, sm.overview, sm.air_date,
               sm.primary_poster_image_id AS "primary_poster_image_id?", sm.runtime
        FROM season_references sr
        LEFT JOIN season_metadata sm ON sm.season_id = sr.id
        WHERE sr.id = $1
        "#,
        season_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load season: {e}")))?;
    let Some(row) = row else { return Ok(false) };
    let season_number = row.season_number;
    let name = row.name;
    let overview = row.overview;
    let air_date = row.air_date;
    let poster = row.primary_poster_image_id;
    let runtime = row.runtime;

    let title = name.unwrap_or_else(|| format!("Season {}", season_number));
    let media_row = MediaRefRow {
        media_id: MediaID::Season(SeasonID(season_id)),
        library_id,
        title: title.clone(),
        year: year_from_date(air_date),
        poster_iid: poster,
        overview: overview.clone(),
        runtime_seconds: runtime.map(|m| m.saturating_mul(60)),
        release_date: air_date,
    };
    let metadata = context_metadata_json(&overview, &[], &[], None, None);
    let summary = overview
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&title);
    let search_text = build_search_text(&title, overview.as_deref(), &[], &[]);

    let mut changed = upsert_context_row(
        pool,
        library_id,
        None,
        &media_row,
        "metadata",
        summary,
        summary,
        &metadata,
        source_revision,
    )
    .await?;
    changed |= upsert_search_row(
        pool,
        library_id,
        None,
        &media_row,
        "combined",
        summary,
        summary,
        &search_text,
        &metadata,
        source_revision,
    )
    .await?;
    Ok(changed)
}

/// User watch-progress row joined to its reference title.
#[derive(Debug, Clone)]
struct WatchProgressRow {
    media_uuid: Uuid,
    media_type: i16,
    position: f32,
    duration: f32,
    last_watched: i64,
    title: String,
    library_id: Uuid,
}

async fn load_user_watch_progress(
    pool: &PgPool,
    library_id: LibraryId,
    user_id: Uuid,
) -> Result<Vec<WatchProgressRow>> {
    sqlx::query_as!(
        WatchProgressRow,
        r#"
        SELECT uwp.media_uuid AS "media_uuid!", uwp.media_type AS "media_type!",
               uwp.position AS "position!", uwp.duration AS "duration!",
               uwp.last_watched AS "last_watched!", COALESCE(m.title, e.title, s.title, sm.name,
                                          uwp.media_uuid::text) AS "title!",
               COALESCE(m.library_id, s.library_id, sr.library_id, e.library_id) AS "library_id!"
        FROM user_watch_progress uwp
        LEFT JOIN movie_references m
            ON uwp.media_uuid = m.id AND uwp.media_type = 0
        LEFT JOIN series s
            ON uwp.media_uuid = s.id AND uwp.media_type = 1
        LEFT JOIN season_references sr
            ON uwp.media_uuid = sr.id AND uwp.media_type = 2
        LEFT JOIN season_metadata sm
            ON sm.season_id = sr.id AND uwp.media_type = 2
        LEFT JOIN (
            SELECT er.id AS id, s.library_id AS library_id, em.name AS title
            FROM episode_references er
            JOIN series s ON er.series_id = s.id
            LEFT JOIN episode_metadata em ON em.episode_id = er.id
        ) e ON uwp.media_uuid = e.id AND uwp.media_type = 3
        WHERE uwp.user_id = $1
          AND COALESCE(m.library_id, s.library_id, sr.library_id, e.library_id) = $2
          AND (
            (uwp.media_type = 0 AND EXISTS (
                SELECT 1 FROM movie_references am
                JOIN media_files mf ON am.file_id = mf.id
                    AND mf.is_available = TRUE
                    AND mf.tombstoned_at IS NULL
                WHERE am.id = uwp.media_uuid AND am.library_id = $2
            ))
            OR (uwp.media_type = 1 AND EXISTS (
                SELECT 1 FROM episode_references ae
                JOIN series avs ON ae.series_id = avs.id AND avs.library_id = $2
                JOIN media_files mf ON ae.file_id = mf.id
                    AND mf.is_available = TRUE
                    AND mf.tombstoned_at IS NULL
                WHERE ae.series_id = uwp.media_uuid
            ))
            OR (uwp.media_type = 2 AND EXISTS (
                SELECT 1 FROM episode_references ae
                JOIN series avs ON ae.series_id = avs.id AND avs.library_id = $2
                JOIN media_files mf ON ae.file_id = mf.id
                    AND mf.is_available = TRUE
                    AND mf.tombstoned_at IS NULL
                WHERE ae.season_id = uwp.media_uuid
            ))
            OR (uwp.media_type = 3 AND EXISTS (
                SELECT 1 FROM episode_references ae
                JOIN series avs ON ae.series_id = avs.id AND avs.library_id = $2
                JOIN media_files mf ON ae.file_id = mf.id
                    AND mf.is_available = TRUE
                    AND mf.tombstoned_at IS NULL
                WHERE ae.id = uwp.media_uuid
            ))
          )
        ORDER BY uwp.last_watched DESC, uwp.media_uuid
        "#,
        user_id,
        library_id.0
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load watch progress: {e}")))
}

fn media_id_from_watch(uuid: Uuid, media_type: i16) -> Option<MediaID> {
    match media_type {
        0 => Some(MediaID::Movie(MovieID(uuid))),
        1 => Some(MediaID::Series(SeriesID(uuid))),
        2 => Some(MediaID::Season(SeasonID(uuid))),
        3 => Some(MediaID::Episode(EpisodeID(uuid))),
        _ => None,
    }
}

fn watch_state_label(position: f32, duration: f32) -> &'static str {
    if duration <= 0.0 {
        return "started";
    }
    let pct = position / duration;
    if pct >= 0.95 {
        "completed"
    } else if pct > 0.0 {
        "in-progress"
    } else {
        "unwatched"
    }
}

/// Active artifact ids for a media item, bounded by `limit` and user scope.
async fn active_artifact_ids_for_media(
    pool: &PgPool,
    media_id: &MediaID,
    user_id: Option<Uuid>,
    limit: u16,
) -> Result<Vec<Uuid>> {
    sqlx::query_scalar!(
        r#"
        SELECT id AS "id!" FROM intelligence_artifacts
        WHERE media_id = $1
          AND status = 'active'
          AND invalidated_at IS NULL
          AND (user_id IS NULL OR user_id = $2)
        ORDER BY updated_at DESC, id
        LIMIT $3
        "#,
        media_id.as_uuid(),
        user_id,
        i64::from(limit)
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load artifact ids: {e}")))
}

/// Resolve a library name.
async fn library_name(pool: &PgPool, library_id: LibraryId) -> Result<String> {
    let row = sqlx::query_scalar!(
        r#"SELECT name AS "name!" FROM libraries WHERE id = $1"#,
        library_id.0
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load library name: {e}")))?;
    row.ok_or_else(|| MediaError::NotFound("library not found".to_string()))
}

#[derive(Debug, Clone, Copy)]
enum InvalidationScope {
    Global,
    User(Uuid),
    All,
}

impl InvalidationScope {
    fn bind_values(self) -> (&'static str, Option<Uuid>) {
        match self {
            InvalidationScope::Global => ("global", None),
            InvalidationScope::User(user_id) => ("user", Some(user_id)),
            InvalidationScope::All => ("all", None),
        }
    }
}

async fn invalidate_read_model_rows_for_media(
    pool: &PgPool,
    library_id: LibraryId,
    media_id: MediaID,
    scope: InvalidationScope,
    reason: &str,
    source_revision: i64,
) -> Result<()> {
    let (scope_mode, scope_user_id) = scope.bind_values();
    let media_type = media_type_str(&media_id);
    let reason = truncate_chars(reason, 512);

    sqlx::query!(
        r#"
        UPDATE intelligence_media_context
        SET status = 'invalidated',
            source_revision = $7,
            source_updated_at = now(),
            invalidated_at = now(),
            invalidation_reason = $1,
            updated_at = now()
        WHERE library_id = $2
          AND media_id = $3
          AND media_type = ($4::text)::media_type
          AND (
              $5::text = 'all'
              OR ($5::text = 'global' AND user_id IS NULL)
              OR ($5::text = 'user' AND user_id = $6)
          )
        "#,
        &reason,
        library_id.0,
        media_id.as_uuid(),
        media_type,
        scope_mode,
        scope_user_id,
        source_revision
    )
    .execute(pool)
    .await
    .map_err(|e| {
        internal_err(format!("failed to invalidate media context rows: {e}"))
    })?;

    sqlx::query!(
        r#"
        UPDATE intelligence_search_documents
        SET status = 'invalidated',
            source_revision = $7,
            source_updated_at = now(),
            invalidated_at = now(),
            invalidation_reason = $1,
            updated_at = now()
        WHERE library_id = $2
          AND media_id = $3
          AND media_type = ($4::text)::media_type
          AND (
              $5::text = 'all'
              OR ($5::text = 'global' AND user_id IS NULL)
              OR ($5::text = 'user' AND user_id = $6)
          )
        "#,
        &reason,
        library_id.0,
        media_id.as_uuid(),
        media_type,
        scope_mode,
        scope_user_id,
        source_revision
    )
    .execute(pool)
    .await
    .map_err(|e| {
        internal_err(format!("failed to invalidate search document rows: {e}"))
    })?;

    Ok(())
}

async fn dependent_artifact_ids_for_media(
    pool: &PgPool,
    library_id: LibraryId,
    media_id: MediaID,
    scope: InvalidationScope,
) -> Result<Vec<Uuid>> {
    let (scope_mode, scope_user_id) = scope.bind_values();
    let media_type = media_type_str(&media_id);

    let rows = sqlx::query_scalar!(
        r#"
        WITH RECURSIVE
        media_context_rows AS (
            SELECT id
            FROM intelligence_media_context
            WHERE library_id = $1
              AND media_id = $2
              AND media_type = ($3::text)::media_type
              AND (
                  $4::text = 'all'
                  OR ($4::text = 'global' AND user_id IS NULL)
                  OR ($4::text = 'user' AND user_id = $5)
              )
        ),
        search_document_rows AS (
            SELECT id
            FROM intelligence_search_documents
            WHERE library_id = $1
              AND media_id = $2
              AND media_type = ($3::text)::media_type
              AND (
                  $4::text = 'all'
                  OR ($4::text = 'global' AND user_id IS NULL)
                  OR ($4::text = 'user' AND user_id = $5)
              )
        ),
        dependent_artifacts(artifact_id) AS (
            SELECT ia.id
            FROM intelligence_artifacts ia
            WHERE ia.library_id = $1
              AND ia.media_id = $2
              AND ia.media_type = ($3::text)::media_type
              AND ia.status IN ('draft', 'active', 'stale')
              AND (
                  $4::text = 'all'
                  OR ($4::text = 'global' AND ia.user_id IS NULL)
                  OR ($4::text = 'user' AND ia.user_id = $5)
              )
            UNION
            SELECT src.artifact_id
            FROM intelligence_artifact_sources src
            JOIN intelligence_artifacts ia ON ia.id = src.artifact_id
            WHERE src.status <> 'invalidated'
              AND ia.status IN ('draft', 'active', 'stale')
              AND (
                  $4::text = 'all'
                  OR ($4::text = 'global' AND ia.user_id IS NULL)
                  OR ($4::text = 'user' AND ia.user_id = $5)
              )
              AND (
                  (
                      src.source_kind = 'media'
                      AND src.source_library_id = $1
                      AND src.source_media_id = $2
                      AND src.source_media_type = ($3::text)::media_type
                  )
                  OR src.source_media_context_id IN (SELECT id FROM media_context_rows)
                  OR src.source_search_document_id IN (SELECT id FROM search_document_rows)
              )
            UNION
            SELECT src.artifact_id
            FROM intelligence_artifact_sources src
            JOIN dependent_artifacts dep ON src.source_artifact_id = dep.artifact_id
            JOIN intelligence_artifacts ia ON ia.id = src.artifact_id
            WHERE src.status <> 'invalidated'
              AND ia.status IN ('draft', 'active', 'stale')
              AND (
                  $4::text = 'all'
                  OR ($4::text = 'global' AND ia.user_id IS NULL)
                  OR ($4::text = 'user' AND ia.user_id = $5)
              )
        )
        SELECT artifact_id AS "artifact_id!" FROM dependent_artifacts
        "#,
        library_id.0,
        media_id.as_uuid(),
        media_type,
        scope_mode,
        scope_user_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        internal_err(format!("failed to resolve dependent artifacts: {e}"))
    })?;

    Ok(rows)
}

async fn invalidate_artifacts_for_media(
    pool: &PgPool,
    library_id: LibraryId,
    media_id: MediaID,
    scope: InvalidationScope,
    reason: &str,
) -> Result<()> {
    let artifact_ids =
        dependent_artifact_ids_for_media(pool, library_id, media_id, scope)
            .await?;
    if artifact_ids.is_empty() {
        return Ok(());
    }

    let reason = truncate_chars(reason, 512);
    sqlx::query!(
        r#"
        UPDATE intelligence_artifacts
        SET status = 'invalidated',
            invalidated_at = now(),
            invalidation_reason = $1,
            updated_at = now()
        WHERE id = ANY($2)
          AND status IN ('draft', 'active', 'stale')
        "#,
        &reason,
        &artifact_ids
    )
    .execute(pool)
    .await
    .map_err(|e| {
        internal_err(format!("failed to invalidate artifacts: {e}"))
    })?;

    sqlx::query!(
        r#"
        UPDATE intelligence_artifact_sources
        SET status = 'invalidated',
            invalidated_at = now(),
            invalidation_reason = $1,
            updated_at = now()
        WHERE artifact_id = ANY($2)
          AND status <> 'invalidated'
        "#,
        &reason,
        &artifact_ids
    )
    .execute(pool)
    .await
    .map_err(|e| {
        internal_err(format!("failed to invalidate artifact sources: {e}"))
    })?;

    Ok(())
}

async fn invalidate_catalog_change_for_media(
    pool: &PgPool,
    library_id: LibraryId,
    media_id: MediaID,
    reason: &str,
) -> Result<()> {
    let source_revision = current_source_revision(pool).await?;
    invalidate_read_model_rows_for_media(
        pool,
        library_id,
        media_id,
        InvalidationScope::All,
        reason,
        source_revision,
    )
    .await?;
    invalidate_artifacts_for_media(
        pool,
        library_id,
        media_id,
        InvalidationScope::All,
        reason,
    )
    .await
}

// ---------------------------------------------------------------------------
// Trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl IntelligenceRepository for PostgresIntelligenceRepository {
    async fn refresh_library_read_models(
        &self,
        library_id: LibraryId,
        user_id: Option<Uuid>,
    ) -> Result<u64> {
        let pool = self.pool();
        let mut refreshed: u64 = 0;
        let source_revision = current_source_revision(pool).await?;

        if let Some(uid) = user_id {
            let progress =
                load_user_watch_progress(pool, library_id, uid).await?;
            for entry in progress {
                let Some(media_id) =
                    media_id_from_watch(entry.media_uuid, entry.media_type)
                else {
                    continue;
                };
                let state = watch_state_label(entry.position, entry.duration);
                let summary = format!(
                    "{}: {} watch state at {:.0}% ({}s of {}s).",
                    entry.title,
                    state,
                    if entry.duration > 0.0 {
                        (entry.position / entry.duration) * 100.0
                    } else {
                        0.0
                    },
                    entry.position as i64,
                    entry.duration as i64
                );
                let entry_library_id = LibraryId(entry.library_id);
                let media_row = MediaRefRow {
                    media_id,
                    library_id: entry_library_id,
                    title: entry.title.clone(),
                    year: None,
                    poster_iid: None,
                    overview: None,
                    runtime_seconds: None,
                    release_date: None,
                };
                let metadata = json!({
                    "state": state,
                    "position": entry.position,
                    "duration": entry.duration,
                    "last_watched": entry.last_watched,
                });
                let mut changed = upsert_context_row(
                    pool,
                    entry_library_id,
                    Some(uid),
                    &media_row,
                    "watch_state",
                    &summary,
                    &summary,
                    &metadata,
                    source_revision,
                )
                .await?;
                changed |= upsert_search_row(
                    pool,
                    entry_library_id,
                    Some(uid),
                    &media_row,
                    "watch_state",
                    &summary,
                    &summary,
                    &summary,
                    &metadata,
                    source_revision,
                )
                .await?;
                if changed {
                    invalidate_artifacts_for_media(
                        pool,
                        entry_library_id,
                        media_row.media_id,
                        InvalidationScope::User(uid),
                        "watch_state_changed",
                    )
                    .await?;
                }
                refreshed += 1;
            }
            return Ok(refreshed);
        }

        // Global refresh: movies.
        let movies = load_available_movies(pool, library_id).await?;
        for movie in &movies {
            let changed = upsert_movie_read_model(
                pool,
                library_id,
                None,
                movie,
                source_revision,
            )
            .await?;
            if changed {
                invalidate_artifacts_for_media(
                    pool,
                    library_id,
                    MediaID::Movie(MovieID(movie.media_id)),
                    InvalidationScope::All,
                    "media_metadata_changed",
                )
                .await?;
            }
            refreshed += 1;
        }

        // Episodes (and derived series/season rows).
        let episodes = load_available_episodes(pool, library_id).await?;
        let mut series_episodes: std::collections::BTreeMap<Uuid, Vec<Uuid>> =
            std::collections::BTreeMap::new();
        let mut seasons_seen: std::collections::BTreeSet<Uuid> =
            std::collections::BTreeSet::new();
        for ep in &episodes {
            let episode_changed = upsert_episode_read_model(
                pool,
                library_id,
                None,
                ep,
                source_revision,
            )
            .await?;
            if episode_changed {
                invalidate_artifacts_for_media(
                    pool,
                    library_id,
                    MediaID::Episode(EpisodeID(ep.media_id)),
                    InvalidationScope::All,
                    "media_metadata_changed",
                )
                .await?;
            }
            refreshed += 1;
            series_episodes
                .entry(ep.series_id)
                .or_default()
                .push(ep.media_id);
            if seasons_seen.insert(ep.season_id) {
                let season_changed = upsert_season_read_model(
                    pool,
                    library_id,
                    ep.season_id,
                    source_revision,
                )
                .await?;
                if season_changed {
                    invalidate_artifacts_for_media(
                        pool,
                        library_id,
                        MediaID::Season(SeasonID(ep.season_id)),
                        InvalidationScope::All,
                        "media_metadata_changed",
                    )
                    .await?;
                }
                refreshed += 1;
            }
        }
        for (series_id, eps) in &series_episodes {
            let series_changed = upsert_series_read_model(
                pool,
                library_id,
                *series_id,
                eps,
                source_revision,
            )
            .await?;
            if series_changed {
                invalidate_artifacts_for_media(
                    pool,
                    library_id,
                    MediaID::Series(SeriesID(*series_id)),
                    InvalidationScope::All,
                    "media_metadata_changed",
                )
                .await?;
            }
            refreshed += 1;
        }

        debug!(
            "refreshed {} intelligence read-model rows for library {}",
            refreshed, library_id
        );
        Ok(refreshed)
    }

    async fn refresh_media_read_model(
        &self,
        library_id: LibraryId,
        media_id: MediaID,
        user_id: Option<Uuid>,
    ) -> Result<()> {
        if user_id.is_some() {
            // User-scoped refresh delegates to the full library user refresh,
            // which is bounded by the user's actual watch progress.
            self.refresh_library_read_models(library_id, user_id)
                .await?;
            return Ok(());
        }
        let pool = self.pool();
        let source_revision = current_source_revision(pool).await?;
        match media_id {
            MediaID::Movie(id) => {
                let movie = sqlx::query_as!(
                    MovieRefreshRow,
                    r#"
                    SELECT mr.id AS "media_id!", mr.title AS "title!",
                           mm.overview, mm.release_date, mm.runtime,
                           mm.vote_average, mm.primary_certification
                    FROM movie_references mr
                    JOIN media_files mf ON mr.file_id = mf.id
                        AND mf.is_available = TRUE
                        AND mf.tombstoned_at IS NULL
                    LEFT JOIN movie_metadata mm ON mm.movie_id = mr.id
                    WHERE mr.id = $1 AND mr.library_id = $2
                    "#,
                    id.0,
                    library_id.0
                )
                .fetch_optional(pool)
                .await
                .map_err(|e| {
                    internal_err(format!("failed to load movie: {e}"))
                })?;
                let Some(movie) = movie else {
                    invalidate_catalog_change_for_media(
                        pool,
                        library_id,
                        MediaID::Movie(id),
                        "media_unavailable",
                    )
                    .await?;
                    return Ok(());
                };
                let changed = upsert_movie_read_model(
                    pool,
                    library_id,
                    None,
                    &movie,
                    source_revision,
                )
                .await?;
                if changed {
                    invalidate_artifacts_for_media(
                        pool,
                        library_id,
                        MediaID::Movie(id),
                        InvalidationScope::All,
                        "media_metadata_changed",
                    )
                    .await?;
                }
            }
            MediaID::Episode(id) => {
                let ep = sqlx::query_as!(
                    EpisodeRefreshRow,
                    r#"
                    SELECT er.id AS "media_id!", er.series_id AS "series_id!", er.season_id AS "season_id!",
                           er.season_number AS "season_number!", er.episode_number AS "episode_number!",
                           em.name AS title, em.overview, em.air_date, em.runtime
                    FROM episode_references er
                    JOIN series s ON er.series_id = s.id AND s.library_id = $2
                    JOIN media_files mf ON er.file_id = mf.id
                        AND mf.is_available = TRUE
                        AND mf.tombstoned_at IS NULL
                    LEFT JOIN episode_metadata em ON em.episode_id = er.id
                    WHERE er.id = $1
                    "#,
                    id.0,
                    library_id.0
                )
                .fetch_optional(pool)
                .await
                .map_err(|e| internal_err(format!("failed to load episode: {e}")))?;
                let Some(ep) = ep else {
                    invalidate_catalog_change_for_media(
                        pool,
                        library_id,
                        MediaID::Episode(id),
                        "media_unavailable",
                    )
                    .await?;
                    return Ok(());
                };
                let episode_changed = upsert_episode_read_model(
                    pool,
                    library_id,
                    None,
                    &ep,
                    source_revision,
                )
                .await?;
                if episode_changed {
                    invalidate_artifacts_for_media(
                        pool,
                        library_id,
                        MediaID::Episode(EpisodeID(ep.media_id)),
                        InvalidationScope::All,
                        "media_metadata_changed",
                    )
                    .await?;
                }
                let season_changed = upsert_season_read_model(
                    pool,
                    library_id,
                    ep.season_id,
                    source_revision,
                )
                .await?;
                if season_changed {
                    invalidate_artifacts_for_media(
                        pool,
                        library_id,
                        MediaID::Season(SeasonID(ep.season_id)),
                        InvalidationScope::All,
                        "media_metadata_changed",
                    )
                    .await?;
                }
                let series_changed = upsert_series_read_model(
                    pool,
                    library_id,
                    ep.series_id,
                    &[ep.media_id],
                    source_revision,
                )
                .await?;
                if series_changed {
                    invalidate_artifacts_for_media(
                        pool,
                        library_id,
                        MediaID::Series(SeriesID(ep.series_id)),
                        InvalidationScope::All,
                        "media_metadata_changed",
                    )
                    .await?;
                }
            }
            MediaID::Season(id) => {
                let has_available_episode = sqlx::query_scalar!(
                    r#"
                    SELECT EXISTS (
                        SELECT 1 FROM episode_references er
                        JOIN series s ON er.series_id = s.id AND s.library_id = $2
                        JOIN media_files mf ON er.file_id = mf.id
                            AND mf.is_available = TRUE
                            AND mf.tombstoned_at IS NULL
                        WHERE er.season_id = $1
                    ) AS "exists!"
                    "#,
                    id.0,
                    library_id.0
                )
                .fetch_one(pool)
                .await
                .map_err(|e| {
                    internal_err(format!("failed to check season availability: {e}"))
                })?;
                if has_available_episode {
                    let changed = upsert_season_read_model(
                        pool,
                        library_id,
                        id.0,
                        source_revision,
                    )
                    .await?;
                    if changed {
                        invalidate_artifacts_for_media(
                            pool,
                            library_id,
                            MediaID::Season(id),
                            InvalidationScope::All,
                            "media_metadata_changed",
                        )
                        .await?;
                    }
                } else {
                    invalidate_catalog_change_for_media(
                        pool,
                        library_id,
                        MediaID::Season(id),
                        "media_unavailable",
                    )
                    .await?;
                }
            }
            MediaID::Series(id) => {
                let ids = sqlx::query_scalar!(
                    r#"
                    SELECT er.id AS "id!" FROM episode_references er
                    JOIN series s ON er.series_id = s.id AND s.library_id = $2
                    JOIN media_files mf ON er.file_id = mf.id
                        AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
                    WHERE er.series_id = $1
                    "#,
                    id.0,
                    library_id.0
                )
                .fetch_all(pool)
                .await
                .map_err(|e| {
                    internal_err(format!("failed to load series episodes: {e}"))
                })?;
                if ids.is_empty() {
                    invalidate_catalog_change_for_media(
                        pool,
                        library_id,
                        MediaID::Series(id),
                        "media_unavailable",
                    )
                    .await?;
                } else {
                    let changed = upsert_series_read_model(
                        pool,
                        library_id,
                        id.0,
                        &ids,
                        source_revision,
                    )
                    .await?;
                    if changed {
                        invalidate_artifacts_for_media(
                            pool,
                            library_id,
                            MediaID::Series(id),
                            InvalidationScope::All,
                            "media_metadata_changed",
                        )
                        .await?;
                    }
                }
            }
        }
        Ok(())
    }

    async fn invalidate_media_read_model(
        &self,
        library_id: LibraryId,
        media_id: MediaID,
        user_id: Option<Uuid>,
        reason: &str,
    ) -> Result<()> {
        let pool = self.pool();
        let scope = user_id
            .map(InvalidationScope::User)
            .unwrap_or(InvalidationScope::Global);
        let source_revision = current_source_revision(pool).await?;
        invalidate_read_model_rows_for_media(
            pool,
            library_id,
            media_id,
            scope,
            reason,
            source_revision,
        )
        .await?;
        invalidate_artifacts_for_media(
            pool, library_id, media_id, scope, reason,
        )
        .await
    }

    async fn invalidate_media_catalog_change(
        &self,
        library_id: LibraryId,
        media_id: MediaID,
        reason: &str,
    ) -> Result<()> {
        invalidate_catalog_change_for_media(
            self.pool(),
            library_id,
            media_id,
            reason,
        )
        .await
    }

    async fn library_overview(
        &self,
        request: &IntelligenceLibraryOverviewRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceLibraryOverviewResponse> {
        let pool = self.pool();
        let caps = request.caps;
        let facet_limit = clamp_limit(
            caps.facet_limit,
            DEFAULT_INTELLIGENCE_FACET_LIMIT,
            MAX_INTELLIGENCE_FACET_LIMIT,
        ) as i64;
        let artifact_limit = clamp_limit(
            caps.artifact_limit,
            DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT,
            MAX_INTELLIGENCE_ARTIFACT_LIMIT,
        ) as i64;
        let page_limit = clamp_limit(
            request.pagination.limit,
            DEFAULT_INTELLIGENCE_PAGE_LIMIT,
            MAX_INTELLIGENCE_PAGE_LIMIT,
        ) as i64;

        // Resolve target library ids, ordered deterministically by id, with
        // cursor pagination.
        let library_ids: Vec<Uuid> = if request.library_ids.is_empty() {
            sqlx::query_scalar!(
                r#"SELECT id AS "id!" FROM libraries WHERE enabled = TRUE ORDER BY id"#
            )
            .fetch_all(pool)
            .await
            .map_err(|e| {
                internal_err(format!("failed to load libraries: {e}"))
            })?
        } else {
            let mut ids: Vec<Uuid> =
                request.library_ids.iter().map(|l| l.0).collect();
            ids.sort();
            ids.dedup();
            ids
        };

        let cursor_uuid = parse_cursor_uuid(&request.pagination.cursor)?;
        let after = cursor_uuid.unwrap_or(Uuid::nil());
        let selected: Vec<Uuid> = library_ids
            .into_iter()
            .filter(|id| *id > after)
            .take(page_limit as usize + 1)
            .collect();
        let has_more = selected.len() > page_limit as usize;
        let page: Vec<Uuid> =
            selected.into_iter().take(page_limit as usize).collect();
        let next_cursor = if has_more {
            page.last().map(|id| id.to_string())
        } else {
            None
        };

        let mut libraries: Vec<IntelligenceLibraryOverview> = Vec::new();
        let mut aggregate_facets: Vec<IntelligenceFacetGroup> = Vec::new();
        for lib_id in &page {
            let library_id = LibraryId(*lib_id);
            let name = library_name(pool, library_id).await?;
            let counts = library_counts(pool, library_id, user_id).await?;
            let mut facets = Vec::new();
            facets.push(media_kind_facet(&counts));
            if let Some(group) =
                genre_facet(pool, library_id, facet_limit as u16).await?
            {
                facets.push(group);
            }
            if let Some(group) =
                release_decade_facet(pool, library_id, facet_limit as u16)
                    .await?
            {
                facets.push(group);
            }
            if let Some(group) =
                content_rating_facet(pool, library_id, facet_limit as u16)
                    .await?
            {
                facets.push(group);
            }
            if let Some(group) =
                runtime_bucket_facet(pool, library_id, facet_limit as u16)
                    .await?
            {
                facets.push(group);
            }
            if let Some(uid) = user_id {
                if let Some(group) =
                    watch_state_facet(pool, library_id, uid, facet_limit as u16)
                        .await?
                {
                    facets.push(group);
                }
            }
            let artifact_ids = active_artifact_ids_for_library(
                pool,
                library_id,
                user_id,
                artifact_limit as u16,
            )
            .await?;

            for facet in &facets {
                merge_facet_group(&mut aggregate_facets, facet);
            }

            let summary_text = format!(
                "{}: {} movies, {} series, {} seasons, {} episodes available.",
                name,
                counts.movies,
                counts.series,
                counts.seasons,
                counts.episodes
            );
            libraries.push(IntelligenceLibraryOverview {
                library_id,
                name,
                summary: Some(bounded_summary(
                    &summary_text,
                    caps.summary_max_chars,
                )),
                counts,
                facets,
                artifact_ids,
            });
        }

        // Bound aggregate facets to the requested limit per group.
        for group in &mut aggregate_facets {
            if group.values.len() > facet_limit as usize {
                group.values.truncate(facet_limit as usize);
            }
        }

        Ok(IntelligenceLibraryOverviewResponse {
            libraries,
            facets: aggregate_facets,
            page: intelligence::IntelligencePageInfo {
                next_cursor,
                limit: page_limit as u16,
                has_more,
            },
            caps,
            generated_at_epoch_seconds: Some(Utc::now().timestamp()),
        })
    }

    async fn candidate_search(
        &self,
        request: &IntelligenceCandidateSearchRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceCandidateSearchResponse> {
        let pool = self.pool();
        let caps = request.caps;
        let candidate_limit = clamp_limit(
            caps.candidate_limit,
            DEFAULT_INTELLIGENCE_CANDIDATE_LIMIT,
            MAX_INTELLIGENCE_CANDIDATE_LIMIT,
        );
        let artifact_limit = clamp_limit(
            caps.artifact_limit,
            DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT,
            MAX_INTELLIGENCE_ARTIFACT_LIMIT,
        );
        let grounding_limit = clamp_limit(
            caps.grounding_limit,
            DEFAULT_INTELLIGENCE_GROUNDING_LIMIT,
            MAX_INTELLIGENCE_GROUNDING_LIMIT,
        );
        let page_limit = clamp_limit(
            request.pagination.limit,
            DEFAULT_INTELLIGENCE_PAGE_LIMIT,
            MAX_INTELLIGENCE_PAGE_LIMIT,
        );

        let limit = candidate_limit.min(page_limit) as i64;
        let fetch_limit = limit.saturating_add(1);
        let library_ids: Vec<Uuid> =
            request.library_ids.iter().map(|l| l.0).collect();
        let media_kinds: Vec<String> = request
            .media_kinds
            .iter()
            .copied()
            .map(media_kind_to_str)
            .map(str::to_string)
            .collect();
        let query = request.query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(empty_candidate_response(caps));
        }

        // Lexical FTS matches ordered by rank, then title trigram similarity for
        // fuzzy title hits not already captured. The union is deduped by media id.
        let rows = sqlx::query!(
            r#"
            WITH fts AS (
                SELECT sd.media_id, sd.media_type::text AS media_type,
                       sd.library_id, sd.title, sd.summary, sd.search_excerpt,
                       ts_rank(sd.search_vector, plainto_tsquery('simple', $1)) AS rank
                FROM intelligence_search_documents sd
                WHERE sd.status = 'active'
                  AND sd.invalidated_at IS NULL
                  AND (array_length($2::uuid[], 1) IS NULL OR sd.library_id = ANY($2::uuid[]))
                  AND (array_length($3::text[], 1) IS NULL OR sd.media_type::text = ANY($3::text[]))
                  AND (sd.user_id IS NULL OR sd.user_id = $4)
                  AND sd.search_vector @@ plainto_tsquery('simple', $1)
            ),
            trgm AS (
                SELECT sd.media_id, sd.media_type::text AS media_type,
                       sd.library_id, sd.title, sd.summary, sd.search_excerpt,
                       similarity(sd.title, $1) AS rank
                FROM intelligence_search_documents sd
                WHERE sd.status = 'active'
                  AND sd.invalidated_at IS NULL
                  AND (array_length($2::uuid[], 1) IS NULL OR sd.library_id = ANY($2::uuid[]))
                  AND (array_length($3::text[], 1) IS NULL OR sd.media_type::text = ANY($3::text[]))
                  AND (sd.user_id IS NULL OR sd.user_id = $4)
                  AND sd.title %> $1
            )
            SELECT media_id AS "media_id!", media_type AS "media_type!", library_id AS "library_id!",
                   title AS "title!", summary, search_excerpt, max(rank)::real AS "rank?"
            FROM (
                SELECT * FROM fts
                UNION ALL
                SELECT * FROM trgm
            ) combined
            GROUP BY media_id, media_type, library_id, title, summary, search_excerpt
            ORDER BY max(rank) DESC, title ASC, media_id ASC
            LIMIT $5
            "#,
            query,
            &library_ids,
            &media_kinds,
            user_id,
            fetch_limit
        )
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("candidate search failed: {e}")))?;

        let has_more = (rows.len() as i64) > limit;
        let mut candidates = Vec::with_capacity(rows.len().min(limit as usize));
        for row in rows.into_iter().take(limit as usize) {
            let media_uuid = row.media_id;
            let media_type = row.media_type;
            let library_id = row.library_id;
            let title = row.title;
            let summary = row.summary;
            let excerpt = row.search_excerpt;
            let rank = row.rank;

            let media_id = media_id_from_parts(&media_type, media_uuid);
            let media_ref_row = fetch_media_ref_row(
                pool,
                &media_id,
                Some(LibraryId(library_id)),
            )
            .await?
            .unwrap_or(MediaRefRow {
                media_id,
                library_id: LibraryId(library_id),
                title: title.clone(),
                year: None,
                poster_iid: None,
                overview: None,
                runtime_seconds: None,
                release_date: None,
            });
            let artifact_ids = if request.include_artifacts {
                active_artifact_ids_for_media(
                    pool,
                    &media_id,
                    user_id,
                    artifact_limit,
                )
                .await?
            } else {
                Vec::new()
            };
            let media = build_media_ref(&media_ref_row, artifact_ids);

            let summary_text = summary
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&title);
            let mut grounding = Vec::new();
            if grounding.len() < grounding_limit as usize {
                grounding.push(IntelligenceGroundingRef {
                    source: IntelligenceGroundingSource::SearchIndex,
                    media_id: Some(media_id),
                    artifact_id: None,
                    field: Some("search_vector".to_string()),
                    label: format!("Search index match for {:?}", query),
                    evidence: Some(bounded_summary(
                        excerpt.as_deref().unwrap_or(summary_text),
                        caps.summary_max_chars,
                    )),
                });
            }
            if grounding.len() < grounding_limit as usize {
                grounding.push(IntelligenceGroundingRef {
                    source: IntelligenceGroundingSource::MediaMetadata,
                    media_id: Some(media_id),
                    artifact_id: None,
                    field: Some("overview".to_string()),
                    label: "Library metadata".to_string(),
                    evidence: Some(bounded_summary(
                        media_ref_row
                            .overview
                            .as_deref()
                            .unwrap_or(summary_text),
                        caps.summary_max_chars,
                    )),
                });
            }

            let candidate_artifact_ids = media.artifact_ids.clone();
            candidates.push(IntelligenceCandidate {
                media,
                summary: Some(bounded_summary(
                    summary_text,
                    caps.summary_max_chars,
                )),
                match_reason: Some(bounded_summary(
                    &format!("Matched search index for {:?}", query),
                    caps.summary_max_chars,
                )),
                score: rank,
                artifact_ids: candidate_artifact_ids,
                grounding,
            });
        }

        Ok(IntelligenceCandidateSearchResponse {
            candidates,
            page: intelligence::IntelligencePageInfo {
                next_cursor: None,
                limit: page_limit,
                has_more,
            },
            caps,
        })
    }

    async fn item_context(
        &self,
        request: &IntelligenceItemContextRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceItemContextResponse> {
        let pool = self.pool();
        let caps = request.caps;
        let related_limit = clamp_limit(
            caps.related_limit,
            DEFAULT_INTELLIGENCE_RELATED_LIMIT,
            MAX_INTELLIGENCE_RELATED_LIMIT,
        );
        let artifact_limit = clamp_limit(
            caps.artifact_limit,
            DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT,
            MAX_INTELLIGENCE_ARTIFACT_LIMIT,
        );
        let grounding_limit = clamp_limit(
            caps.grounding_limit,
            DEFAULT_INTELLIGENCE_GROUNDING_LIMIT,
            MAX_INTELLIGENCE_GROUNDING_LIMIT,
        );

        let Some(media_row) =
            fetch_media_ref_row(pool, &request.media_id, request.library_id)
                .await?
        else {
            return Err(MediaError::NotFound(format!(
                "media item {:?} not found",
                request.media_id
            )));
        };
        let genres = fetch_genres(pool, &request.media_id).await?;
        let artifact_ids = active_artifact_ids_for_media(
            pool,
            &request.media_id,
            user_id,
            artifact_limit,
        )
        .await?;

        let mut facet_values: Vec<IntelligenceFacetValue> = genres
            .iter()
            .map(|g| IntelligenceFacetValue {
                key: g.to_lowercase(),
                label: g.clone(),
                count: 1,
                sample_media_ids: vec![request.media_id],
            })
            .take(grounding_limit as usize)
            .collect();

        let summary_text = media_row
            .overview
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&media_row.title);

        let mut grounding = Vec::new();
        if grounding.len() < grounding_limit as usize {
            grounding.push(IntelligenceGroundingRef {
                source: IntelligenceGroundingSource::MediaMetadata,
                media_id: Some(request.media_id),
                artifact_id: None,
                field: Some("title".to_string()),
                label: media_row.title.clone(),
                evidence: Some(bounded_summary(
                    &media_row.title,
                    caps.summary_max_chars,
                )),
            });
        }
        if let Some(overview) = &media_row.overview {
            if !overview.is_empty()
                && grounding.len() < grounding_limit as usize
            {
                grounding.push(IntelligenceGroundingRef {
                    source: IntelligenceGroundingSource::MediaMetadata,
                    media_id: Some(request.media_id),
                    artifact_id: None,
                    field: Some("overview".to_string()),
                    label: "Overview".to_string(),
                    evidence: Some(bounded_summary(
                        overview,
                        caps.summary_max_chars,
                    )),
                });
            }
        }

        let item = IntelligenceContextItem {
            media: build_media_ref(&media_row, artifact_ids.clone()),
            summary: Some(bounded_summary(
                summary_text,
                caps.summary_max_chars,
            )),
            facets: std::mem::take(&mut facet_values),
            artifact_ids: artifact_ids.clone(),
            provenance: Vec::new(),
        };

        // Related items: same-series for episodes/seasons, then same-library
        // similar-genre items, bounded by related_limit.
        let related = related_items(
            pool,
            &media_row,
            &genres,
            related_limit,
            &[],
            user_id,
        )
        .await?;

        // Artifacts for this media item.
        let artifacts = artifact_summaries_for_media(
            pool,
            &request.media_id,
            user_id,
            artifact_limit,
            caps.summary_max_chars,
        )
        .await?;

        Ok(IntelligenceItemContextResponse {
            item,
            related,
            artifacts,
            grounding,
            caps,
        })
    }

    async fn related_context(
        &self,
        request: &IntelligenceRelatedContextRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRelatedContextResponse> {
        let pool = self.pool();
        let caps = request.caps;
        let related_limit = clamp_limit(
            caps.related_limit,
            DEFAULT_INTELLIGENCE_RELATED_LIMIT,
            MAX_INTELLIGENCE_RELATED_LIMIT,
        );
        let page_limit = clamp_limit(
            request.pagination.limit,
            DEFAULT_INTELLIGENCE_PAGE_LIMIT,
            MAX_INTELLIGENCE_PAGE_LIMIT,
        );
        let limit = related_limit.min(page_limit) as i64;

        let Some(media_row) =
            fetch_media_ref_row(pool, &request.media_id, None).await?
        else {
            return Err(MediaError::NotFound(format!(
                "seed media {:?} not found",
                request.media_id
            )));
        };
        let genres = fetch_genres(pool, &request.media_id).await?;
        let mut related = related_items_bounded(
            pool,
            &media_row,
            &genres,
            limit.saturating_add(1),
            &request.relationship_kinds,
            user_id,
        )
        .await?;
        let seed = build_media_ref(&media_row, Vec::new());

        let has_more = (related.len() as i64) > limit;
        related.truncate(limit as usize);
        Ok(IntelligenceRelatedContextResponse {
            seed,
            related,
            page: intelligence::IntelligencePageInfo {
                next_cursor: None,
                limit: page_limit,
                has_more,
            },
            caps,
        })
    }

    async fn artifact_search(
        &self,
        request: &IntelligenceArtifactSearchRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceArtifactSearchResponse> {
        let pool = self.pool();
        let caps = request.caps;
        let page_limit = clamp_limit(
            request.pagination.limit,
            DEFAULT_INTELLIGENCE_PAGE_LIMIT,
            MAX_INTELLIGENCE_PAGE_LIMIT,
        );
        let artifact_limit = clamp_limit(
            caps.artifact_limit,
            DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT,
            MAX_INTELLIGENCE_ARTIFACT_LIMIT,
        );
        let limit = page_limit.min(artifact_limit) as i64;
        let fetch_limit = limit.saturating_add(1);
        let artifact_ids: Vec<Uuid> = request.artifact_ids.clone();
        let media_ids: Vec<Uuid> =
            request.media_ids.iter().map(|m| *m.as_uuid()).collect();
        let library_ids: Vec<Uuid> =
            request.library_ids.iter().map(|l| l.0).collect();
        let kinds: Vec<String> = request
            .kinds
            .iter()
            .map(|k| artifact_kind_to_db(*k).map(str::to_string))
            .collect::<Result<_>>()?;

        let rows = sqlx::query_as!(
            ArtifactSummaryRow,
            r#"
            SELECT id AS "id!", artifact_kind::text AS "artifact_kind!",
                   library_id, media_id, media_type::text AS media_type,
                   title AS "title!", summary, created_at AS "created_at!", updated_at AS "updated_at!"
            FROM intelligence_artifacts
            WHERE status = 'active'
              AND invalidated_at IS NULL
              AND (array_length($1::uuid[], 1) IS NULL OR id = ANY($1::uuid[]))
              AND (array_length($2::uuid[], 1) IS NULL OR media_id = ANY($2::uuid[]))
              AND (array_length($3::uuid[], 1) IS NULL OR library_id = ANY($3::uuid[]))
              AND (array_length($4::text[], 1) IS NULL OR artifact_kind::text = ANY($4::text[]))
              AND (user_id IS NULL OR user_id = $5)
            ORDER BY updated_at DESC, id
            LIMIT $6
            "#,
            &artifact_ids,
            &media_ids,
            &library_ids,
            &kinds,
            user_id,
            fetch_limit
        )
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("artifact search failed: {e}")))?;

        let has_more = (rows.len() as i64) > limit;
        let mut artifacts = Vec::with_capacity(rows.len().min(limit as usize));
        for row in rows.into_iter().take(limit as usize) {
            artifacts.push(
                artifact_summary_from_row(pool, &row, caps.summary_max_chars)
                    .await?,
            );
        }

        Ok(IntelligenceArtifactSearchResponse {
            artifacts,
            page: intelligence::IntelligencePageInfo {
                next_cursor: None,
                limit: limit as u16,
                has_more,
            },
            caps,
        })
    }

    async fn get_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<Option<IntelligenceArtifactSummary>> {
        let pool = self.pool();
        let row = sqlx::query_as!(
            ArtifactSummaryRow,
            r#"
            SELECT id AS "id!", artifact_kind::text AS "artifact_kind!",
                   library_id, media_id, media_type::text AS media_type,
                   title AS "title!", summary, created_at AS "created_at!", updated_at AS "updated_at!"
            FROM intelligence_artifacts
            WHERE id = $1
              AND status = 'active'
              AND invalidated_at IS NULL
              AND (user_id IS NULL OR user_id = $2)
            "#,
            artifact_id,
            user_id
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| internal_err(format!("get_artifact failed: {e}")))?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(
            artifact_summary_from_row(
                pool,
                &row,
                DEFAULT_INTELLIGENCE_SUMMARY_CHARS,
            )
            .await?,
        ))
    }

    async fn upsert_artifact(
        &self,
        upsert: IntelligenceArtifactUpsert,
    ) -> Result<Uuid> {
        let pool = self.pool();
        let kind_db = artifact_kind_to_db(upsert.kind)?;
        let scope_db = match upsert.scope {
            IntelligenceArtifactScope::Global => "global",
            IntelligenceArtifactScope::User(_) => "user",
        };
        let user_id = upsert.scope.user_id();
        if matches!(upsert.scope, IntelligenceArtifactScope::User(_))
            && user_id.is_none()
        {
            return Err(MediaError::InvalidMedia(
                "user-scoped artifact requires a user id".to_string(),
            ));
        }
        let (media_id, media_type) = match upsert.media_id {
            Some(id) => (Some(*id.as_uuid()), Some(media_type_str(&id))),
            None => (None, None),
        };
        let title = truncate_chars(&upsert.title, 512);
        let summary =
            upsert.summary.as_deref().map(|s| truncate_chars(s, 4000));
        let excerpt =
            upsert.excerpt.as_deref().map(|s| truncate_chars(s, 2048));
        let content_json = canonical_json(&upsert.content);
        let metadata_json = canonical_json(&upsert.metadata);
        let hash = content_hash(&[
            &title,
            summary.as_deref().unwrap_or(""),
            excerpt.as_deref().unwrap_or(""),
            &content_json,
            &metadata_json,
            &upsert.source_revision.to_string(),
        ]);

        let artifact_id = match upsert.artifact_id {
            Some(id) => {
                let result = sqlx::query!(
                    r#"
                    UPDATE intelligence_artifacts
                    SET artifact_kind = $2::varchar,
                        scope = $3::varchar,
                        library_id = $4,
                        user_id = $5,
                        media_id = $6,
                        media_type = ($7::text)::media_type,
                        run_id = $8,
                        title = $9,
                        summary = $10,
                        excerpt = $11,
                        content = $12::jsonb,
                        metadata = $13::jsonb,
                        source_revision = $14,
                        source_updated_at = now(),
                        content_hash = $15,
                        status = 'active',
                        invalidated_at = NULL,
                        invalidation_reason = NULL,
                        updated_at = now()
                    WHERE id = $1
                      AND (($5::uuid IS NULL AND user_id IS NULL) OR user_id = $5)
                    "#,
                    id,
                    kind_db,
                    scope_db,
                    upsert.library_id.map(|l| l.0),
                    user_id,
                    media_id,
                    media_type,
                    upsert.run_id,
                    &title,
                    summary.as_deref(),
                    excerpt.as_deref(),
                    &upsert.content,
                    &upsert.metadata,
                    upsert.source_revision,
                    &hash
                )
                .execute(pool)
                .await
                .map_err(|e| {
                    internal_err(format!("update artifact failed: {e}"))
                })?;
                if result.rows_affected() == 0 {
                    return Err(MediaError::InvalidMedia(
                        "artifact not visible for the requested scope"
                            .to_string(),
                    ));
                }
                id
            }
            None => {
                let new_id = Uuid::now_v7();
                sqlx::query!(
                    r#"
                    INSERT INTO intelligence_artifacts (
                        id, artifact_kind, scope, status, library_id, user_id,
                        media_id, media_type, run_id, supersedes_artifact_id,
                        title, summary, excerpt, content, metadata,
                        source_system, source_revision, source_updated_at, content_hash
                    )
                    VALUES ($1, $2::varchar, $3::varchar, 'active', $4, $5, $6, ($7::text)::media_type,
                            $8, $9, $10, $11, $12, $13::jsonb, $14::jsonb,
                            'ferrex', $15, now(), $16)
                    "#,
                    new_id,
                    kind_db,
                    scope_db,
                    upsert.library_id.map(|l| l.0),
                    user_id,
                    media_id,
                    media_type,
                    upsert.run_id,
                    upsert.supersedes_artifact_id,
                    &title,
                    summary.as_deref(),
                    excerpt.as_deref(),
                    &upsert.content,
                    &upsert.metadata,
                    upsert.source_revision,
                    &hash
                )
                .execute(pool)
                .await
                .map_err(|e| internal_err(format!("insert artifact failed: {e}")))?;
                if let Some(superseded) = upsert.supersedes_artifact_id {
                    let superseded_reason = truncate_chars(
                        &format!("superseded by artifact {new_id}"),
                        512,
                    );
                    let result = sqlx::query!(
                        r#"
                        UPDATE intelligence_artifacts
                        SET status = 'superseded',
                            invalidated_at = COALESCE(invalidated_at, now()),
                            invalidation_reason = $3,
                            updated_at = now()
                        WHERE id = $1
                          AND (($2::uuid IS NULL AND user_id IS NULL) OR user_id = $2)
                        "#,
                        superseded,
                        user_id,
                        &superseded_reason
                    )
                    .execute(pool)
                    .await
                    .map_err(|e| {
                        internal_err(format!("supersede artifact failed: {e}"))
                    })?;
                    if result.rows_affected() == 0 {
                        return Err(MediaError::InvalidMedia(
                            "superseded artifact not visible for the requested scope".to_string(),
                        ));
                    }
                }
                new_id
            }
        };
        Ok(artifact_id)
    }

    async fn create_draft_artifact(
        &self,
        create: IntelligenceDraftArtifactCreate,
    ) -> Result<Uuid> {
        let pool = self.pool();
        let kind_db = artifact_kind_to_db(create.kind)?;
        let scope_db = match create.scope {
            IntelligenceArtifactScope::Global => "global",
            IntelligenceArtifactScope::User(_) => "user",
        };
        let user_id = create.scope.user_id();
        if matches!(create.scope, IntelligenceArtifactScope::User(_))
            && user_id.is_none()
        {
            return Err(MediaError::InvalidMedia(
                "user-scoped draft artifact requires a user id".to_string(),
            ));
        }
        let (media_id, media_type) = match create.media_id {
            Some(id) => (Some(*id.as_uuid()), Some(media_type_str(&id))),
            None => (None, None),
        };
        let title = truncate_chars(&create.title, 512);
        let summary =
            create.summary.as_deref().map(|s| truncate_chars(s, 4000));
        let excerpt =
            create.excerpt.as_deref().map(|s| truncate_chars(s, 2048));
        let content_json = canonical_json(&create.content);
        let metadata_json = canonical_json(&create.metadata);
        let hash = content_hash(&[
            &title,
            summary.as_deref().unwrap_or(""),
            excerpt.as_deref().unwrap_or(""),
            &content_json,
            &metadata_json,
            &create.source_revision.to_string(),
        ]);
        let artifact_id = create.artifact_id.unwrap_or_else(Uuid::now_v7);

        sqlx::query!(
            r#"
            INSERT INTO intelligence_artifacts (
                id, artifact_kind, scope, status, library_id, user_id,
                media_id, media_type, run_id, title, summary, excerpt,
                content, metadata, source_system, source_revision,
                source_updated_at, content_hash
            )
            VALUES ($1, $2::varchar, $3::varchar, 'draft', $4, $5, $6,
                    ($7::text)::media_type, $8, $9, $10, $11, $12::jsonb,
                    $13::jsonb, 'ferrex', $14, now(), $15)
            "#,
            artifact_id,
            kind_db,
            scope_db,
            create.library_id.map(|l| l.0),
            user_id,
            media_id,
            media_type,
            create.run_id,
            &title,
            summary.as_deref(),
            excerpt.as_deref(),
            &create.content,
            &create.metadata,
            create.source_revision,
            &hash
        )
        .execute(pool)
        .await
        .map_err(|e| {
            internal_err(format!("insert draft artifact failed: {e}"))
        })?;

        Ok(artifact_id)
    }

    async fn get_draft_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<Option<IntelligenceDraftArtifactPayload>> {
        let pool = self.pool();
        let row = sqlx::query!(
            r#"
            SELECT id AS "id!", artifact_kind::text AS "artifact_kind!",
                   status::text AS "status!", library_id, user_id,
                   media_id, media_type::text AS media_type, run_id,
                   title AS "title!", summary, excerpt,
                   content AS "content!", metadata AS "metadata!",
                   created_at AS "created_at!", updated_at AS "updated_at!"
            FROM intelligence_artifacts
            WHERE id = $1
              AND status = 'draft'
              AND (user_id IS NULL OR user_id = $2)
            "#,
            artifact_id,
            user_id
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| internal_err(format!("get draft artifact failed: {e}")))?;

        let Some(row) = row else { return Ok(None) };
        let media = row
            .media_id
            .zip(row.media_type)
            .map(|(id, media_type)| media_id_from_parts(&media_type, id));
        let sources = load_artifact_source_edges(pool, artifact_id).await?;

        Ok(Some(IntelligenceDraftArtifactPayload {
            artifact_id: row.id,
            kind: artifact_kind_from_db(&row.artifact_kind),
            status: artifact_status_from_db(&row.status),
            library_id: row.library_id.map(LibraryId),
            owner_user_id: row.user_id,
            media_id: media,
            run_id: row.run_id,
            title: row.title,
            summary: row.summary.as_deref().map(|s| {
                bounded_summary(s, DEFAULT_INTELLIGENCE_SUMMARY_CHARS)
            }),
            excerpt: row.excerpt.as_deref().map(|s| {
                bounded_summary(s, DEFAULT_INTELLIGENCE_SUMMARY_CHARS)
            }),
            content: row.content,
            metadata: row.metadata,
            sources,
            created_at_epoch_seconds: Some(row.created_at.timestamp()),
            updated_at_epoch_seconds: Some(row.updated_at.timestamp()),
        }))
    }

    async fn set_artifact_status(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
        status: IntelligenceArtifactStatus,
        reason: Option<&str>,
    ) -> Result<()> {
        let pool = self.pool();
        let invalidates = matches!(
            status,
            IntelligenceArtifactStatus::Invalidated
                | IntelligenceArtifactStatus::Deleted
        );
        let clears_invalidation = matches!(
            status,
            IntelligenceArtifactStatus::Draft
                | IntelligenceArtifactStatus::Active
                | IntelligenceArtifactStatus::Stale
                | IntelligenceArtifactStatus::Failed
        );
        let reason = reason.map(|value| truncate_chars(value, 512));
        let result = sqlx::query!(
            r#"
            UPDATE intelligence_artifacts
            SET status = $2::varchar,
                invalidated_at = CASE
                    WHEN $3 THEN COALESCE(invalidated_at, now())
                    WHEN $4 THEN NULL
                    ELSE invalidated_at
                END,
                invalidation_reason = CASE
                    WHEN $3 THEN $5::varchar
                    WHEN $4 THEN NULL
                    ELSE invalidation_reason
                END,
                updated_at = now()
            WHERE id = $1
              AND (($6::uuid IS NULL AND user_id IS NULL) OR user_id = $6)
            "#,
            artifact_id,
            status.as_db_str(),
            invalidates,
            clears_invalidation,
            reason.as_deref(),
            user_id
        )
        .execute(pool)
        .await
        .map_err(|e| {
            internal_err(format!("set artifact status failed: {e}"))
        })?;

        if result.rows_affected() == 0 {
            return Err(MediaError::NotFound(
                "artifact not found for requested scope".to_string(),
            ));
        }
        Ok(())
    }

    async fn replace_artifact_sources(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
        sources: Vec<IntelligenceArtifactSourceEdge>,
    ) -> Result<()> {
        let pool = self.pool();
        let mut tx = pool.begin().await.map_err(|e| {
            internal_err(format!("begin source transaction failed: {e}"))
        })?;

        let visible: Option<Uuid> = sqlx::query_scalar!(
            r#"
            SELECT id AS "id!" FROM intelligence_artifacts
            WHERE id = $1
              AND (($2::uuid IS NULL AND user_id IS NULL) OR user_id = $2)
            "#,
            artifact_id,
            user_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            internal_err(format!("artifact source scope check failed: {e}"))
        })?;
        if visible.is_none() {
            return Err(MediaError::NotFound(
                "artifact not found for requested scope".to_string(),
            ));
        }

        sqlx::query!(
            "DELETE FROM intelligence_artifact_sources WHERE artifact_id = $1",
            artifact_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            internal_err(format!("delete artifact sources failed: {e}"))
        })?;

        for source in sources {
            let (source_media_id, source_media_type) = source
                .source_media_id
                .as_ref()
                .map(|media| {
                    (Some(*media.as_uuid()), Some(media_type_str(media)))
                })
                .unwrap_or((None, None));
            let source_locator = if source.source_locator.is_null() {
                json!({})
            } else {
                source.source_locator.clone()
            };
            let source_excerpt = source.source_excerpt.as_ref().map(|s| {
                truncate_chars(&s.text, usize::from(s.max_chars).min(2048))
            });

            sqlx::query!(
                r#"
                INSERT INTO intelligence_artifact_sources (
                    artifact_id, source_ordinal, source_kind,
                    source_media_context_id, source_search_document_id,
                    source_artifact_id, source_run_id, source_tool_call_id,
                    source_library_id, source_user_id, source_media_id,
                    source_media_type, source_revision, source_content_hash,
                    source_excerpt, source_locator
                )
                VALUES ($1, $2, $3::varchar, $4, $5, $6, $7, $8, $9, $10,
                        $11, ($12::text)::media_type, $13, $14, $15, $16::jsonb)
                "#,
                artifact_id,
                source.source_ordinal,
                source.source_kind.as_db_str(),
                source.source_media_context_id,
                source.source_search_document_id,
                source.source_artifact_id,
                source.source_run_id,
                source.source_tool_call_id,
                source.source_library_id.as_ref().map(|l| l.0),
                source.source_user_id,
                source_media_id,
                source_media_type,
                source.source_revision,
                source.source_content_hash.as_deref(),
                source_excerpt.as_deref(),
                &source_locator
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                internal_err(format!("insert artifact source failed: {e}"))
            })?;
        }

        tx.commit().await.map_err(|e| {
            internal_err(format!("commit source transaction failed: {e}"))
        })?;
        Ok(())
    }

    async fn invalidate_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
        reason: &str,
    ) -> Result<()> {
        let pool = self.pool();
        let reason = truncate_chars(reason, 512);
        let result = sqlx::query!(
            r#"
            UPDATE intelligence_artifacts
            SET status = 'invalidated',
                invalidated_at = now(),
                invalidation_reason = $1,
                updated_at = now()
            WHERE id = $2
              AND (($3::uuid IS NULL AND user_id IS NULL) OR user_id = $3)
            "#,
            &reason,
            artifact_id,
            user_id
        )
        .execute(pool)
        .await
        .map_err(|e| {
            internal_err(format!("invalidate artifact failed: {e}"))
        })?;
        if result.rows_affected() == 0 {
            let exists = sqlx::query_scalar!(
                r#"SELECT id AS "id!" FROM intelligence_artifacts WHERE id = $1"#,
                artifact_id
            )
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                internal_err(format!("artifact lookup failed: {e}"))
            })?;
            if exists.is_none() {
                return Err(MediaError::NotFound(
                    "artifact not found".to_string(),
                ));
            }
            return Err(MediaError::InvalidMedia(
                "artifact not visible to the requesting user".to_string(),
            ));
        }
        Ok(())
    }

    async fn create_run(&self, create: IntelligenceRunCreate) -> Result<Uuid> {
        let pool = self.pool();
        if let Some(key) = &create.idempotency_key {
            let existing = sqlx::query_scalar!(
                r#"
                SELECT id AS "id!" FROM intelligence_runs
                WHERE idempotency_key = $1
                "#,
                key
            )
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                internal_err(format!("idempotency lookup failed: {e}"))
            })?;
            if let Some(id) = existing {
                return Ok(id);
            }
        }
        let (media_id, media_type) = match create.media_id {
            Some(id) => (Some(*id.as_uuid()), Some(media_type_str(&id))),
            None => (None, None),
        };
        let run_id = create.run_id.unwrap_or_else(Uuid::now_v7);
        sqlx::query!(
            r#"
            INSERT INTO intelligence_runs (
                id, run_kind, status, library_id, user_id, media_id, media_type,
                idempotency_key, provider_name, model_name, request_hash,
                prompt_excerpt, metadata
            )
            VALUES ($1, $2::varchar, 'queued', $3, $4, $5, ($6::text)::media_type, $7, $8, $9, $10,
                    $11, $12::jsonb)
            "#,
            run_id,
            create.run_kind.as_db_str(),
            create.library_id.map(|l| l.0),
            create.user_id,
            media_id,
            media_type,
            create.idempotency_key.as_deref(),
            create.provider_name.as_deref(),
            create.model_name.as_deref(),
            create.request_hash.as_deref(),
            create.prompt_excerpt.as_deref(),
            &create.metadata
        )
        .execute(pool)
        .await
        .map_err(|e| internal_err(format!("create run failed: {e}")))?;
        Ok(run_id)
    }

    async fn update_run(
        &self,
        run_id: Uuid,
        update: IntelligenceRunUpdate,
    ) -> Result<()> {
        let pool = self.pool();
        let status = update.status.map(|s| s.as_db_str());
        // Satisfy the finished_at CHECK: terminal statuses require finished_at.
        let finished_at = match (status, update.finished_at) {
            (Some("succeeded" | "failed" | "cancelled"), None) => {
                Some(Utc::now())
            }
            (_, other) => other,
        };
        let started_at = match (status, update.started_at) {
            (Some("running"), None) => Some(Utc::now()),
            (_, other) => other,
        };

        sqlx::query!(
            r#"
            UPDATE intelligence_runs
            SET status = COALESCE($2::varchar, status),
                provider_name = COALESCE($3, provider_name),
                model_name = COALESCE($4, model_name),
                result_summary = COALESCE($5, result_summary),
                error_excerpt = COALESCE($6, error_excerpt),
                started_at = COALESCE($7, started_at),
                finished_at = COALESCE($8, finished_at),
                metadata = COALESCE($9::jsonb, metadata),
                updated_at = now()
            WHERE id = $1
            "#,
            run_id,
            status,
            update.provider_name.as_deref(),
            update.model_name.as_deref(),
            update.result_summary.as_deref(),
            update.error_excerpt.as_deref(),
            started_at,
            finished_at,
            update.metadata.as_ref()
        )
        .execute(pool)
        .await
        .map_err(|e| internal_err(format!("update run failed: {e}")))?;
        Ok(())
    }

    async fn list_runs(
        &self,
        filter: IntelligenceRunListFilter,
    ) -> Result<Vec<IntelligenceRunSummary>> {
        let pool = self.pool();
        let limit = clamp_limit(
            filter.limit,
            DEFAULT_INTELLIGENCE_PAGE_LIMIT,
            MAX_INTELLIGENCE_PAGE_LIMIT,
        ) as i64;
        let run_kind = filter.run_kind.map(|k| k.as_db_str().to_string());
        let status = filter.status.map(|s| s.as_db_str().to_string());

        let rows = sqlx::query!(
            r#"
            SELECT id AS "id!", run_kind::text AS "run_kind!", status::text AS "status!",
                   library_id, user_id, media_id, media_type::text AS media_type,
                   correlation_id AS "correlation_id!", idempotency_key, model_name,
                   started_at, finished_at, created_at AS "created_at!", updated_at AS "updated_at!"
            FROM intelligence_runs
            WHERE ($1::uuid IS NULL OR library_id = $1)
              AND ($2::uuid IS NULL OR user_id = $2)
              AND ($3::text IS NULL OR run_kind = $3::varchar)
              AND ($4::text IS NULL OR status = $4::varchar)
            ORDER BY created_at DESC, id
            LIMIT $5
            "#,
            filter.library_id.map(|l| l.0),
            filter.user_id,
            run_kind.as_deref(),
            status.as_deref(),
            limit
        )
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("list runs failed: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let media = row
                .media_id
                .zip(row.media_type)
                .map(|(id, t)| media_id_from_parts(&t, id));
            out.push(IntelligenceRunSummary {
                run_id: row.id,
                run_kind: run_kind_from_db(&row.run_kind),
                status: match row.status.as_str() {
                    "running" => RunStatusInternal::Running,
                    "succeeded" => RunStatusInternal::Succeeded,
                    "failed" => RunStatusInternal::Failed,
                    "cancelled" => RunStatusInternal::Cancelled,
                    _ => RunStatusInternal::Queued,
                },
                library_id: row.library_id.map(LibraryId),
                user_id: row.user_id,
                media_id: media,
                correlation_id: row.correlation_id,
                idempotency_key: row.idempotency_key,
                model_name: row.model_name,
                started_at: row.started_at,
                finished_at: row.finished_at,
                created_at: row.created_at,
                updated_at: row.updated_at,
            });
        }
        Ok(out)
    }

    async fn run_audit(
        &self,
        request: &IntelligenceRunAuditRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRunAuditResponse> {
        let pool = self.pool();
        let caps = request.caps;
        let tool_call_limit = clamp_limit(
            caps.tool_call_limit,
            DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT,
            MAX_INTELLIGENCE_TOOL_CALL_LIMIT,
        ) as i64;
        let tool_call_fetch_limit = tool_call_limit.saturating_add(1);
        let artifact_limit = clamp_limit(
            caps.artifact_limit,
            DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT,
            MAX_INTELLIGENCE_ARTIFACT_LIMIT,
        );

        let row = sqlx::query!(
            r#"
            SELECT id AS "id!", run_kind::text AS "run_kind!", status::text AS "status!",
                   user_id, model_name, provider_name, prompt_excerpt,
                   result_summary, error_excerpt, started_at, finished_at,
                   created_at AS "created_at!", updated_at AS "updated_at!"
            FROM intelligence_runs
            WHERE id = $1
              AND (user_id IS NULL OR user_id = $2)
            "#,
            request.run_id,
            user_id
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| internal_err(format!("run_audit failed: {e}")))?;
        let row = row.ok_or_else(|| {
            MediaError::NotFound("intelligence run not found".to_string())
        })?;

        let run_kind = row.run_kind.clone();
        let status = row.status.clone();

        // Tool calls for this run, bounded and ordered by sequence.
        let tc_rows = sqlx::query!(
            r#"
            SELECT tc.id AS "id!", tc.tool_name AS "tool_name!", tc.status::text AS "status!",
                   tc.started_at, tc.finished_at, tc.error_excerpt,
                   tc.arguments AS "arguments!", tc.result
            FROM intelligence_tool_calls tc
            WHERE tc.run_id = $1
            ORDER BY tc.sequence, tc.id
            LIMIT $2
            "#,
            request.run_id,
            tool_call_fetch_limit
        )
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("list tool calls failed: {e}")))?;

        let has_more = (tc_rows.len() as i64) > tool_call_limit;
        let mut tool_calls =
            Vec::with_capacity(tc_rows.len().min(tool_call_limit as usize));
        for tc in tc_rows.into_iter().take(tool_call_limit as usize) {
            let arguments = tc.arguments;
            let result = tc.result;
            let tool_name = tc.tool_name;
            let tc_status = tc.status;
            let started = tc.started_at;
            let finished = tc.finished_at;
            let error_excerpt = tc.error_excerpt;

            // Artifact ids produced by this tool call (via run linkage).
            let artifact_ids = sqlx::query_scalar!(
                r#"
                SELECT id AS "id!" FROM intelligence_artifacts
                WHERE run_id = $1
                  AND status = 'active'
                  AND invalidated_at IS NULL
                  AND (user_id IS NULL OR user_id = $2)
                ORDER BY updated_at DESC, id
                LIMIT $3
                "#,
                request.run_id,
                user_id,
                i64::from(artifact_limit)
            )
            .fetch_all(pool)
            .await
            .map_err(|e| internal_err(format!("load run artifacts: {e}")))?;

            tool_calls.push(IntelligenceToolCallAudit {
                tool_call_id: tc.id,
                name: tool_name,
                status: tool_status_from_db(&tc_status),
                started_at_epoch_seconds: started.map(|t| t.timestamp()),
                completed_at_epoch_seconds: finished.map(|t| t.timestamp()),
                input_summary: Some(bounded_summary(
                    &arguments.to_string(),
                    caps.summary_max_chars,
                )),
                output_summary: result.as_ref().map(|r| {
                    bounded_summary(&r.to_string(), caps.summary_max_chars)
                }),
                error_summary: error_excerpt
                    .as_deref()
                    .map(|e| bounded_summary(e, caps.summary_max_chars)),
                artifact_ids,
                grounding: Vec::new(),
            });
        }

        let run_artifact_ids = sqlx::query_scalar!(
            r#"
            SELECT id AS "id!" FROM intelligence_artifacts
            WHERE run_id = $1
              AND status = 'active'
              AND invalidated_at IS NULL
              AND (user_id IS NULL OR user_id = $2)
            ORDER BY updated_at DESC, id
            LIMIT $3
            "#,
            request.run_id,
            user_id,
            i64::from(artifact_limit)
        )
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("load run artifacts: {e}")))?;

        let started = row.started_at;
        let finished = row.finished_at;
        let prompt_excerpt = row.prompt_excerpt.clone();
        let result_summary = row.result_summary.clone();
        let user_id_row = row.user_id;
        let model_name = row.model_name.clone();

        let run = IntelligenceRunAudit {
            run_id: request.run_id,
            purpose: run_purpose_from_db(&run_kind),
            status: run_status_from_db(&status),
            requested_by_user_id: user_id_row,
            model: model_name,
            queued_at_epoch_seconds: Some(row.created_at.timestamp()),
            started_at_epoch_seconds: started.map(|t| t.timestamp()),
            completed_at_epoch_seconds: finished.map(|t| t.timestamp()),
            input_summary: prompt_excerpt
                .as_deref()
                .map(|p| bounded_summary(p, caps.summary_max_chars)),
            output_summary: result_summary
                .as_deref()
                .map(|r| bounded_summary(r, caps.summary_max_chars)),
            artifact_ids: run_artifact_ids,
            grounding: Vec::new(),
            tool_calls,
        };

        Ok(IntelligenceRunAuditResponse {
            run,
            page: intelligence::IntelligencePageInfo {
                next_cursor: None,
                limit: tool_call_limit as u16,
                has_more,
            },
            caps,
        })
    }

    async fn append_run_event(
        &self,
        create: IntelligenceRunEventCreate,
    ) -> Result<IntelligenceRunEvent> {
        if matches!(create.sequence, Some(sequence) if sequence < 0) {
            return Err(MediaError::InvalidMedia(
                "run event sequence must be non-negative".to_string(),
            ));
        }
        let pool = self.pool();
        let event_id = create.event_id.unwrap_or_else(Uuid::now_v7);
        let event_kind = create.event_kind;
        let status = create.status;
        let message = create.message.map(|m| truncate_chars(&m, 2048));
        let payload = if create.payload.is_null() {
            json!({})
        } else {
            create.payload
        };
        let error_code =
            create.error.as_ref().map(|e| e.code.as_str().to_string());
        let error_payload = create
            .error
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;

        let row = sqlx::query!(
            r#"
            WITH next_sequence AS (
                SELECT COALESCE(
                    $2::integer,
                    COALESCE(MAX(sequence) + 1, 0)
                ) AS sequence
                FROM intelligence_run_events
                WHERE run_id = $1
            )
            INSERT INTO intelligence_run_events (
                id, run_id, sequence, event_kind, status, tool_call_id,
                artifact_id, message, payload, error_code, error
            )
            SELECT $3, $1, next_sequence.sequence, $4::varchar, $5::varchar,
                   $6, $7, $8, $9::jsonb, $10, $11::jsonb
            FROM next_sequence
            RETURNING id AS "id!", sequence AS "sequence!",
                      created_at AS "created_at!"
            "#,
            create.run_id,
            create.sequence,
            event_id,
            event_kind.as_db_str(),
            status.map(|s| s.as_db_str()),
            create.tool_call_id,
            create.artifact_id,
            message.as_deref(),
            &payload,
            error_code.as_deref(),
            error_payload.as_ref()
        )
        .fetch_one(pool)
        .await
        .map_err(|e| internal_err(format!("append run event failed: {e}")))?;

        Ok(IntelligenceRunEvent {
            event_id: row.id,
            run_id: create.run_id,
            sequence: row.sequence,
            event_kind,
            status,
            tool_call_id: create.tool_call_id,
            artifact_id: create.artifact_id,
            message,
            payload,
            error: create.error,
            created_at_epoch_seconds: Some(row.created_at.timestamp()),
        })
    }

    async fn list_run_events(
        &self,
        filter: IntelligenceRunEventListFilter,
    ) -> Result<Vec<IntelligenceRunEvent>> {
        let pool = self.pool();
        let limit = clamp_limit(
            filter.limit,
            DEFAULT_INTELLIGENCE_PAGE_LIMIT,
            MAX_INTELLIGENCE_PAGE_LIMIT,
        ) as i64;

        let rows = sqlx::query!(
            r#"
            SELECT e.id AS "id!", e.run_id AS "run_id!",
                   e.sequence AS "sequence!",
                   e.event_kind::text AS "event_kind!",
                   e.status::text AS status, e.tool_call_id, e.artifact_id,
                   e.message, e.payload AS "payload!", e.error_code,
                   e.error, e.created_at AS "created_at!"
            FROM intelligence_run_events e
            JOIN intelligence_runs r ON r.id = e.run_id
            WHERE e.run_id = $1
              AND ($2::integer IS NULL OR e.sequence > $2)
              AND (r.user_id IS NULL OR r.user_id = $3)
            ORDER BY e.sequence, e.id
            LIMIT $4
            "#,
            filter.run_id,
            filter.after_sequence,
            filter.user_id,
            limit
        )
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("list run events failed: {e}")))?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let error = match row.error {
                Some(value) => Some(serde_json::from_value(value)?),
                None => row.error_code.map(|code| IntelligenceError {
                    code: intelligence_error_code_from_db(&code),
                    message: "runtime error".to_string(),
                    retryable: false,
                    details: Value::Null,
                }),
            };
            let status = row.status.as_deref().map(api_run_status_from_db);

            events.push(IntelligenceRunEvent {
                event_id: row.id,
                run_id: row.run_id,
                sequence: row.sequence,
                event_kind: run_event_kind_from_db(&row.event_kind),
                status,
                tool_call_id: row.tool_call_id,
                artifact_id: row.artifact_id,
                message: row.message,
                payload: row.payload,
                error,
                created_at_epoch_seconds: Some(row.created_at.timestamp()),
            });
        }

        Ok(events)
    }

    async fn create_tool_call(
        &self,
        create: IntelligenceToolCallCreate,
    ) -> Result<Uuid> {
        let pool = self.pool();
        let id = create.tool_call_id.unwrap_or_else(Uuid::now_v7);
        sqlx::query!(
            r#"
            INSERT INTO intelligence_tool_calls (
                id, run_id, sequence, tool_kind, tool_name, status,
                idempotency_key, input_hash, arguments
            )
            VALUES ($1, $2, $3, $4::varchar, $5, 'queued', $6, $7, $8::jsonb)
            "#,
            id,
            create.run_id,
            create.sequence,
            create.tool_kind.as_db_str(),
            &create.tool_name,
            create.idempotency_key.as_deref(),
            create.input_hash.as_deref(),
            &create.arguments
        )
        .execute(pool)
        .await
        .map_err(|e| internal_err(format!("create tool call failed: {e}")))?;
        Ok(id)
    }

    async fn update_tool_call(
        &self,
        tool_call_id: Uuid,
        update: IntelligenceToolCallUpdate,
    ) -> Result<()> {
        let pool = self.pool();
        let status = update.status.map(|s| s.as_db_str());
        let finished_at = match (status, update.finished_at) {
            (Some("succeeded" | "failed" | "skipped" | "cancelled"), None) => {
                Some(Utc::now())
            }
            (_, other) => other,
        };
        let started_at = match (status, update.started_at) {
            (Some("running"), None) => Some(Utc::now()),
            (_, other) => other,
        };

        sqlx::query!(
            r#"
            UPDATE intelligence_tool_calls
            SET status = COALESCE($2::varchar, status),
                output_hash = COALESCE($3, output_hash),
                result = COALESCE($4::jsonb, result),
                error_excerpt = COALESCE($5, error_excerpt),
                started_at = COALESCE($6, started_at),
                finished_at = COALESCE($7, finished_at),
                updated_at = now()
            WHERE id = $1
            "#,
            tool_call_id,
            status,
            update.output_hash.as_deref(),
            update.result.as_ref(),
            update.error_excerpt.as_deref(),
            started_at,
            finished_at
        )
        .execute(pool)
        .await
        .map_err(|e| internal_err(format!("update tool call failed: {e}")))?;
        Ok(())
    }

    async fn list_tool_calls(
        &self,
        run_id: Uuid,
    ) -> Result<Vec<IntelligenceToolCallSummary>> {
        let pool = self.pool();
        let rows = sqlx::query!(
            r#"
            SELECT id AS "id!", run_id AS "run_id!", sequence AS "sequence!", tool_kind::text AS "tool_kind!",
                   tool_name AS "tool_name!", status::text AS "status!", idempotency_key,
                   input_hash, output_hash, started_at, finished_at,
                   created_at AS "created_at!", updated_at AS "updated_at!"
            FROM intelligence_tool_calls
            WHERE run_id = $1
            ORDER BY sequence, id
            LIMIT $2
            "#,
            run_id,
            i64::from(DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT)
        )
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("list tool calls failed: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(IntelligenceToolCallSummary {
                tool_call_id: row.id,
                run_id: row.run_id,
                sequence: row.sequence,
                tool_kind: tool_kind_from_db(&row.tool_kind),
                tool_name: row.tool_name,
                status: match row.status.as_str() {
                    "running" => ToolStatusInternal::Running,
                    "succeeded" => ToolStatusInternal::Succeeded,
                    "failed" => ToolStatusInternal::Failed,
                    "skipped" => ToolStatusInternal::Skipped,
                    "cancelled" => ToolStatusInternal::Cancelled,
                    _ => ToolStatusInternal::Queued,
                },
                idempotency_key: row.idempotency_key,
                input_hash: row.input_hash,
                output_hash: row.output_hash,
                started_at: row.started_at,
                finished_at: row.finished_at,
                created_at: row.created_at,
                updated_at: row.updated_at,
            });
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Free helper functions for facets, counts, related items, and artifacts
// ---------------------------------------------------------------------------

async fn load_artifact_source_edges(
    pool: &PgPool,
    artifact_id: Uuid,
) -> Result<Vec<IntelligenceArtifactSourceEdge>> {
    let rows = sqlx::query!(
        r#"
        SELECT source_ordinal AS "source_ordinal!",
               source_kind::text AS "source_kind!",
               source_media_context_id, source_search_document_id,
               source_artifact_id, source_run_id, source_tool_call_id,
               source_library_id, source_user_id, source_media_id,
               source_media_type::text AS source_media_type,
               source_revision AS "source_revision!",
               source_content_hash, source_excerpt,
               source_locator AS "source_locator!"
        FROM intelligence_artifact_sources
        WHERE artifact_id = $1
          AND status = 'active'
          AND invalidated_at IS NULL
        ORDER BY source_ordinal, created_at
        "#,
        artifact_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("load artifact sources failed: {e}")))?;

    let mut sources = Vec::with_capacity(rows.len());
    for row in rows {
        let source_media_id = row
            .source_media_id
            .zip(row.source_media_type)
            .map(|(id, media_type)| media_id_from_parts(&media_type, id));

        sources.push(IntelligenceArtifactSourceEdge {
            source_ordinal: row.source_ordinal,
            source_kind: artifact_source_kind_from_db(&row.source_kind),
            source_media_context_id: row.source_media_context_id,
            source_search_document_id: row.source_search_document_id,
            source_artifact_id: row.source_artifact_id,
            source_run_id: row.source_run_id,
            source_tool_call_id: row.source_tool_call_id,
            source_library_id: row.source_library_id.map(LibraryId),
            source_user_id: row.source_user_id,
            source_media_id,
            source_revision: row.source_revision,
            source_content_hash: row.source_content_hash,
            source_excerpt: row.source_excerpt.as_deref().map(|s| {
                bounded_summary(s, DEFAULT_INTELLIGENCE_SUMMARY_CHARS)
            }),
            source_locator: row.source_locator,
        });
    }

    Ok(sources)
}

async fn current_source_revision(pool: &PgPool) -> Result<i64> {
    let max_revision = sqlx::query_scalar!(
        r#"
        SELECT GREATEST(
            (SELECT COALESCE(MAX(source_revision), 0) FROM intelligence_media_context),
            (SELECT COALESCE(MAX(source_revision), 0) FROM intelligence_search_documents),
            (SELECT COALESCE(MAX(source_revision), 0) FROM intelligence_artifacts),
            (SELECT COALESCE(MAX(source_revision), 0) FROM intelligence_artifact_sources)
        ) AS "max_revision!"
        "#
    )
    .fetch_one(pool)
    .await
    .map_err(|e| internal_err(format!("source revision lookup failed: {e}")))?;
    Ok(max_revision + 1)
}

fn parse_cursor_uuid(cursor: &Option<String>) -> Result<Option<Uuid>> {
    match cursor {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) => Uuid::parse_str(value).map(Some).map_err(|e| {
            internal_err(format!("invalid pagination cursor: {e}"))
        }),
    }
}

async fn library_counts(
    pool: &PgPool,
    library_id: LibraryId,
    user_id: Option<Uuid>,
) -> Result<IntelligenceMediaCounts> {
    let movies = sqlx::query_scalar!(
        r#"
        SELECT count(*) AS "count!"
        FROM movie_references mr
        JOIN media_files mf ON mr.file_id = mf.id
            AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
        WHERE mr.library_id = $1
        "#,
        library_id.0
    )
    .fetch_one(pool)
    .await
    .map_err(|e| internal_err(format!("count movies: {e}")))?;

    let series_eps = sqlx::query!(
        r#"
        SELECT
          count(DISTINCT er.series_id)::bigint AS "series!",
          count(DISTINCT er.season_id)::bigint AS "seasons!",
          count(*)::bigint AS "episodes!"
        FROM episode_references er
        JOIN series s ON er.series_id = s.id AND s.library_id = $1
        JOIN media_files mf ON er.file_id = mf.id
            AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
        "#,
        library_id.0
    )
    .fetch_one(pool)
    .await
    .map_err(|e| internal_err(format!("count tv: {e}")))?;

    let artifacts = sqlx::query_scalar!(
        r#"
        SELECT count(*) AS "count!" FROM intelligence_artifacts
        WHERE library_id = $1
          AND status = 'active'
          AND invalidated_at IS NULL
          AND (user_id IS NULL OR user_id = $2)
        "#,
        library_id.0,
        user_id
    )
    .fetch_one(pool)
    .await
    .map_err(|e| internal_err(format!("count artifacts: {e}")))?;

    Ok(IntelligenceMediaCounts {
        movies: movies as u64,
        series: series_eps.series as u64,
        seasons: series_eps.seasons as u64,
        episodes: series_eps.episodes as u64,
        artifacts: artifacts as u64,
    })
}

fn media_kind_facet(
    counts: &IntelligenceMediaCounts,
) -> IntelligenceFacetGroup {
    IntelligenceFacetGroup {
        kind: IntelligenceFacetKind::MediaKind,
        label: "Media kind".to_string(),
        values: vec![
            IntelligenceFacetValue {
                key: "movie".to_string(),
                label: "Movies".to_string(),
                count: counts.movies,
                sample_media_ids: Vec::new(),
            },
            IntelligenceFacetValue {
                key: "series".to_string(),
                label: "Series".to_string(),
                count: counts.series,
                sample_media_ids: Vec::new(),
            },
            IntelligenceFacetValue {
                key: "season".to_string(),
                label: "Seasons".to_string(),
                count: counts.seasons,
                sample_media_ids: Vec::new(),
            },
            IntelligenceFacetValue {
                key: "episode".to_string(),
                label: "Episodes".to_string(),
                count: counts.episodes,
                sample_media_ids: Vec::new(),
            },
        ],
    }
}

async fn genre_facet(
    pool: &PgPool,
    library_id: LibraryId,
    limit: u16,
) -> Result<Option<IntelligenceFacetGroup>> {
    let movie_rows = sqlx::query!(
        r#"
        SELECT g.name AS "label!", count(DISTINCT mr.id)::bigint AS "cnt!"
        FROM movie_references mr
        JOIN media_files mf ON mr.file_id = mf.id
            AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
        JOIN movie_genres g ON g.movie_id = mr.id
        WHERE mr.library_id = $1
        GROUP BY g.name
        "#,
        library_id.0
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("movie genre facet: {e}")))?;

    let series_rows = sqlx::query!(
        r#"
        SELECT g.name AS "label!", count(DISTINCT sg.id)::bigint AS "cnt!"
        FROM series_genres g
        JOIN series sg ON sg.id = g.series_id AND sg.library_id = $1
        WHERE EXISTS (
            SELECT 1 FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
                AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
            WHERE er.series_id = sg.id
        )
        GROUP BY g.name
        "#,
        library_id.0
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("series genre facet: {e}")))?;

    let mut counts: std::collections::BTreeMap<String, i64> =
        std::collections::BTreeMap::new();
    for row in &movie_rows {
        *counts.entry(row.label.clone()).or_insert(0) += row.cnt;
    }
    for row in &series_rows {
        *counts.entry(row.label.clone()).or_insert(0) += row.cnt;
    }
    if counts.is_empty() {
        return Ok(None);
    }
    let mut entries: Vec<(String, i64)> = counts.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let values = entries
        .into_iter()
        .take(limit as usize)
        .map(|(label, cnt)| IntelligenceFacetValue {
            key: label.to_lowercase(),
            label,
            count: cnt as u64,
            sample_media_ids: Vec::new(),
        })
        .collect();
    Ok(Some(IntelligenceFacetGroup {
        kind: IntelligenceFacetKind::Genre,
        label: "Genre".to_string(),
        values,
    }))
}

async fn release_decade_facet(
    pool: &PgPool,
    library_id: LibraryId,
    limit: u16,
) -> Result<Option<IntelligenceFacetGroup>> {
    let rows = sqlx::query!(
        r#"
        SELECT decade AS "decade!", count(*)::bigint AS "cnt!" FROM (
            SELECT (EXTRACT(year FROM mm.release_date)::int / 10 * 10) AS decade
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
                AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
            JOIN movie_metadata mm ON mm.movie_id = mr.id
            WHERE mr.library_id = $1 AND mm.release_date IS NOT NULL
            UNION ALL
            SELECT (EXTRACT(year FROM sm.first_air_date)::int / 10 * 10) AS decade
            FROM series s
            JOIN series_metadata sm ON sm.series_id = s.id
            WHERE s.library_id = $1 AND sm.first_air_date IS NOT NULL
              AND EXISTS (
                SELECT 1 FROM episode_references er
                JOIN media_files mf ON er.file_id = mf.id
                    AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
                WHERE er.series_id = s.id
              )
        ) decades
        WHERE decade IS NOT NULL
        GROUP BY decade
        ORDER BY decade
        "#,
        library_id.0
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("decade facet: {e}")))?;
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows
        .into_iter()
        .take(limit as usize)
        .map(|row| {
            Ok::<_, MediaError>(IntelligenceFacetValue {
                key: row.decade.to_string(),
                label: format!("{}s", row.decade),
                count: row.cnt as u64,
                sample_media_ids: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(IntelligenceFacetGroup {
        kind: IntelligenceFacetKind::ReleaseDecade,
        label: "Release decade".to_string(),
        values,
    }))
}

async fn content_rating_facet(
    pool: &PgPool,
    library_id: LibraryId,
    limit: u16,
) -> Result<Option<IntelligenceFacetGroup>> {
    let rows = sqlx::query!(
        r#"
        SELECT rating AS "rating!", count(*)::bigint AS "cnt!" FROM (
            SELECT mm.primary_certification AS rating
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
                AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
            JOIN movie_metadata mm ON mm.movie_id = mr.id
            WHERE mr.library_id = $1 AND mm.primary_certification IS NOT NULL
            UNION ALL
            SELECT sm.primary_content_rating AS rating
            FROM series s
            JOIN series_metadata sm ON sm.series_id = s.id
            WHERE s.library_id = $1 AND sm.primary_content_rating IS NOT NULL
              AND EXISTS (
                SELECT 1 FROM episode_references er
                JOIN media_files mf ON er.file_id = mf.id
                    AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
                WHERE er.series_id = s.id
              )
        ) ratings
        WHERE rating IS NOT NULL AND rating <> ''
        GROUP BY rating
        ORDER BY count(*) DESC, rating
        "#,
        library_id.0
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("content rating facet: {e}")))?;
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows
        .into_iter()
        .take(limit as usize)
        .map(|row| {
            Ok::<_, MediaError>(IntelligenceFacetValue {
                key: row.rating.to_lowercase(),
                label: row.rating,
                count: row.cnt as u64,
                sample_media_ids: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(IntelligenceFacetGroup {
        kind: IntelligenceFacetKind::ContentRating,
        label: "Content rating".to_string(),
        values,
    }))
}

async fn runtime_bucket_facet(
    pool: &PgPool,
    library_id: LibraryId,
    limit: u16,
) -> Result<Option<IntelligenceFacetGroup>> {
    let rows = sqlx::query!(
        r#"
        SELECT bucket AS "bucket!", count(*)::bigint AS "cnt!" FROM (
            SELECT CASE
                WHEN mm.runtime < 60 THEN 'short'
                WHEN mm.runtime < 120 THEN 'standard'
                WHEN mm.runtime < 180 THEN 'long'
                ELSE 'extended'
            END AS bucket
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
                AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
            JOIN movie_metadata mm ON mm.movie_id = mr.id
            WHERE mr.library_id = $1 AND mm.runtime IS NOT NULL
        ) buckets
        WHERE bucket IS NOT NULL
        GROUP BY bucket
        ORDER BY bucket
        "#,
        library_id.0
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("runtime facet: {e}")))?;
    if rows.is_empty() {
        return Ok(None);
    }
    let order = ["short", "standard", "long", "extended"];
    let mut values: Vec<IntelligenceFacetValue> = rows
        .into_iter()
        .map(|row| {
            Ok::<_, MediaError>(IntelligenceFacetValue {
                key: row.bucket.clone(),
                label: row.bucket.to_title_case(),
                count: row.cnt as u64,
                sample_media_ids: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    values.sort_by_key(|v| {
        order.iter().position(|k| k == &v.key).unwrap_or(usize::MAX)
    });
    values.truncate(limit as usize);
    Ok(Some(IntelligenceFacetGroup {
        kind: IntelligenceFacetKind::RuntimeBucket,
        label: "Runtime bucket".to_string(),
        values,
    }))
}

async fn watch_state_facet(
    pool: &PgPool,
    library_id: LibraryId,
    user_id: Uuid,
    limit: u16,
) -> Result<Option<IntelligenceFacetGroup>> {
    let rows = sqlx::query!(
        r#"
        SELECT
          count(*) FILTER (WHERE uwp.position / NULLIF(uwp.duration, 0) >= 0.95)::bigint AS "completed!",
          count(*) FILTER (WHERE uwp.position / NULLIF(uwp.duration, 0) > 0 AND uwp.position / NULLIF(uwp.duration, 0) < 0.95)::bigint AS "in_progress!",
          count(*) FILTER (WHERE uwp.position = 0)::bigint AS "started!"
        FROM user_watch_progress uwp
        WHERE uwp.user_id = $1
          AND (
            (uwp.media_type = 0 AND EXISTS (
                SELECT 1 FROM movie_references mr
                JOIN media_files mf ON mr.file_id = mf.id
                    AND mf.is_available = TRUE
                    AND mf.tombstoned_at IS NULL
                WHERE mr.id = uwp.media_uuid AND mr.library_id = $2
            ))
            OR (uwp.media_type = 1 AND EXISTS (
                SELECT 1 FROM episode_references er
                JOIN series s ON er.series_id = s.id AND s.library_id = $2
                JOIN media_files mf ON er.file_id = mf.id
                    AND mf.is_available = TRUE
                    AND mf.tombstoned_at IS NULL
                WHERE er.series_id = uwp.media_uuid
            ))
            OR (uwp.media_type = 2 AND EXISTS (
                SELECT 1 FROM episode_references er
                JOIN series s ON er.series_id = s.id AND s.library_id = $2
                JOIN media_files mf ON er.file_id = mf.id
                    AND mf.is_available = TRUE
                    AND mf.tombstoned_at IS NULL
                WHERE er.season_id = uwp.media_uuid
            ))
            OR (uwp.media_type = 3 AND EXISTS (
                SELECT 1 FROM episode_references er
                JOIN series s ON er.series_id = s.id AND s.library_id = $2
                JOIN media_files mf ON er.file_id = mf.id
                    AND mf.is_available = TRUE
                    AND mf.tombstoned_at IS NULL
                WHERE er.id = uwp.media_uuid
            ))
          )
        "#,
        user_id,
        library_id.0
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("watch state facet: {e}")))?;
    let Some(row) = rows else { return Ok(None) };
    let completed = row.completed;
    let in_progress = row.in_progress;
    let started = row.started;
    if completed + in_progress + started == 0 {
        return Ok(None);
    }
    let mut values = vec![
        IntelligenceFacetValue {
            key: "completed".to_string(),
            label: "Completed".to_string(),
            count: completed as u64,
            sample_media_ids: Vec::new(),
        },
        IntelligenceFacetValue {
            key: "in_progress".to_string(),
            label: "In progress".to_string(),
            count: in_progress as u64,
            sample_media_ids: Vec::new(),
        },
        IntelligenceFacetValue {
            key: "started".to_string(),
            label: "Started".to_string(),
            count: started as u64,
            sample_media_ids: Vec::new(),
        },
    ];
    values.retain(|v| v.count > 0);
    values.truncate(limit as usize);
    Ok(Some(IntelligenceFacetGroup {
        kind: IntelligenceFacetKind::WatchState,
        label: "Watch state".to_string(),
        values,
    }))
}

async fn active_artifact_ids_for_library(
    pool: &PgPool,
    library_id: LibraryId,
    user_id: Option<Uuid>,
    limit: u16,
) -> Result<Vec<Uuid>> {
    sqlx::query_scalar!(
        r#"
        SELECT id AS "id!" FROM intelligence_artifacts
        WHERE library_id = $1
          AND status = 'active'
          AND invalidated_at IS NULL
          AND (user_id IS NULL OR user_id = $2)
        ORDER BY updated_at DESC, id
        LIMIT $3
        "#,
        library_id.0,
        user_id,
        i64::from(limit)
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("library artifact ids: {e}")))
}

fn merge_facet_group(
    aggregate: &mut Vec<IntelligenceFacetGroup>,
    group: &IntelligenceFacetGroup,
) {
    if let Some(existing) = aggregate.iter_mut().find(|g| g.kind == group.kind)
    {
        for value in &group.values {
            if let Some(ev) =
                existing.values.iter_mut().find(|v| v.key == value.key)
            {
                ev.count += value.count;
            } else {
                existing.values.push(value.clone());
            }
        }
    } else {
        aggregate.push(group.clone());
    }
}

fn media_kind_to_str(kind: IntelligenceMediaKind) -> &'static str {
    match kind {
        IntelligenceMediaKind::Movie => "movie",
        IntelligenceMediaKind::Series => "series",
        IntelligenceMediaKind::Season => "season",
        IntelligenceMediaKind::Episode => "episode",
    }
}

fn empty_candidate_response(
    caps: intelligence::IntelligenceCaps,
) -> IntelligenceCandidateSearchResponse {
    IntelligenceCandidateSearchResponse {
        candidates: Vec::new(),
        page: intelligence::IntelligencePageInfo::default(),
        caps,
    }
}

trait TitleCase {
    fn to_title_case(&self) -> String;
}
impl TitleCase for str {
    fn to_title_case(&self) -> String {
        let mut chars = self.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().chain(chars).collect(),
        }
    }
}

async fn related_items(
    pool: &PgPool,
    media_row: &MediaRefRow,
    genres: &[String],
    limit: u16,
    relationship_kinds: &[IntelligenceRelationshipKind],
    user_id: Option<Uuid>,
) -> Result<Vec<IntelligenceRelatedContext>> {
    related_items_bounded(
        pool,
        media_row,
        genres,
        limit as i64,
        relationship_kinds,
        user_id,
    )
    .await
}

fn relationship_allowed(
    filter: &[IntelligenceRelationshipKind],
    kind: IntelligenceRelationshipKind,
) -> bool {
    filter.is_empty() || filter.contains(&kind)
}

async fn related_items_bounded(
    pool: &PgPool,
    media_row: &MediaRefRow,
    genres: &[String],
    limit: i64,
    relationship_kinds: &[IntelligenceRelationshipKind],
    user_id: Option<Uuid>,
) -> Result<Vec<IntelligenceRelatedContext>> {
    if limit <= 0 {
        return Ok(Vec::new());
    }
    let mut out: Vec<IntelligenceRelatedContext> = Vec::new();
    let mut seen: std::collections::BTreeSet<Uuid> =
        std::collections::BTreeSet::new();
    seen.insert(*media_row.media_id.as_uuid());

    // Same-series relatives (episodes/seasons sharing a series).
    if relationship_allowed(
        relationship_kinds,
        IntelligenceRelationshipKind::SameSeries,
    ) {
        if let Some(series_id) =
            resolve_series_id(pool, &media_row.media_id).await?
        {
            let rows = sqlx::query!(
                r#"
            SELECT er.id AS "id!", er.season_number AS "season_number!", er.episode_number AS "episode_number!", em.name AS title
            FROM episode_references er
            LEFT JOIN episode_metadata em ON em.episode_id = er.id
            WHERE er.series_id = $1 AND er.id <> $2
            ORDER BY er.season_number, er.episode_number, er.id
            LIMIT $3
            "#,
                series_id,
                media_row.media_id.as_uuid(),
                limit
            )
            .fetch_all(pool)
            .await
            .map_err(|e| internal_err(format!("same-series relatives: {e}")))?;
            for row in rows {
                let id = row.id;
                if !seen.insert(id) {
                    continue;
                }
                let season_number = row.season_number;
                let episode_number = row.episode_number;
                let title = row.title;
                let media_id = MediaID::Episode(EpisodeID(id));
                let artifact_ids =
                    active_artifact_ids_for_media(pool, &media_id, user_id, 4)
                        .await?;
                let label = title.unwrap_or_else(|| {
                    format!("S{:02}E{:02}", season_number, episode_number)
                });
                out.push(IntelligenceRelatedContext {
                    media: IntelligenceMediaRef {
                        media_id,
                        media_kind: IntelligenceMediaKind::Episode,
                        library_id: Some(media_row.library_id),
                        title: label.clone(),
                        year: None,
                        poster_iid: None,
                        artifact_ids: artifact_ids.clone(),
                    },
                    relationship: IntelligenceRelationshipKind::SameSeries,
                    strength: Some(0.9),
                    reason: Some(bounded_summary(
                        "Shares the same series.",
                        DEFAULT_INTELLIGENCE_SUMMARY_CHARS,
                    )),
                    artifact_ids,
                    grounding: vec![IntelligenceGroundingRef {
                        source: IntelligenceGroundingSource::FerrexLibrary,
                        media_id: Some(media_id),
                        artifact_id: None,
                        field: Some("series_id".to_string()),
                        label,
                        evidence: None,
                    }],
                });
                if (out.len() as i64) >= limit {
                    break;
                }
            }
        }
    }

    // Same-library similar-genre movies.
    if relationship_allowed(
        relationship_kinds,
        IntelligenceRelationshipKind::SimilarGenre,
    ) && (out.len() as i64) < limit
        && !genres.is_empty()
    {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT mr.id AS "id!", mr.title AS "title!", mm.release_date, mm.primary_poster_image_id AS "primary_poster_image_id?"
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
                AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
            LEFT JOIN movie_metadata mm ON mm.movie_id = mr.id
            JOIN movie_genres g ON g.movie_id = mr.id AND g.name = ANY($1)
            WHERE mr.library_id = $2 AND mr.id <> $3
            ORDER BY mr.title, mr.id
            LIMIT $4
            "#,
            genres,
            media_row.library_id.0,
            media_row.media_id.as_uuid(),
            limit - out.len() as i64
        )
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("similar-genre relatives: {e}")))?;
        for row in rows {
            let id = row.id;
            if !seen.insert(id) {
                continue;
            }
            let title = row.title;
            let release_date = row.release_date;
            let poster = row.primary_poster_image_id;
            let media_id = MediaID::Movie(MovieID(id));
            let artifact_ids =
                active_artifact_ids_for_media(pool, &media_id, user_id, 4)
                    .await?;
            out.push(IntelligenceRelatedContext {
                media: IntelligenceMediaRef {
                    media_id,
                    media_kind: IntelligenceMediaKind::Movie,
                    library_id: Some(media_row.library_id),
                    title: title.clone(),
                    year: year_from_date(release_date),
                    poster_iid: poster,
                    artifact_ids: artifact_ids.clone(),
                },
                relationship: IntelligenceRelationshipKind::SimilarGenre,
                strength: Some(0.6),
                reason: Some(bounded_summary(
                    &format!("Shares genres: {}.", genres.join(", ")),
                    DEFAULT_INTELLIGENCE_SUMMARY_CHARS,
                )),
                artifact_ids,
                grounding: vec![IntelligenceGroundingRef {
                    source: IntelligenceGroundingSource::MediaMetadata,
                    media_id: Some(media_id),
                    artifact_id: None,
                    field: Some("genres".to_string()),
                    label: title,
                    evidence: None,
                }],
            });
            if (out.len() as i64) >= limit {
                break;
            }
        }
    }

    Ok(out)
}

/// Resolve the parent series id for a season/episode media item.
async fn resolve_series_id(
    pool: &PgPool,
    media_id: &MediaID,
) -> Result<Option<Uuid>> {
    match media_id {
        MediaID::Episode(id) => sqlx::query_scalar!(
            r#"SELECT series_id AS "series_id!" FROM episode_references WHERE id = $1"#,
            id.0
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| internal_err(format!("resolve episode series: {e}"))),
        MediaID::Season(id) => sqlx::query_scalar!(
            r#"SELECT series_id AS "series_id!" FROM season_references WHERE id = $1"#,
            id.0
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| internal_err(format!("resolve season series: {e}"))),
        _ => Ok(None),
    }
}

#[derive(Debug, Clone)]
struct ArtifactSummaryRow {
    id: Uuid,
    artifact_kind: String,
    library_id: Option<Uuid>,
    media_id: Option<Uuid>,
    media_type: Option<String>,
    title: String,
    summary: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Build an artifact summary from a database row, fetching the media ref.
async fn artifact_summary_from_row(
    pool: &PgPool,
    row: &ArtifactSummaryRow,
    summary_max_chars: u16,
) -> Result<IntelligenceArtifactSummary> {
    let artifact_id = row.id;
    let kind_str = row.artifact_kind.clone();
    let media_id = row.media_id;
    let media_type = row.media_type.clone();
    let title = row.title.clone();
    let summary = row.summary.clone();
    let created_at = row.created_at;
    let updated_at = row.updated_at;
    let library_id = row.library_id;

    let media = match media_id.zip(media_type) {
        Some((id, t)) => {
            let media_id = media_id_from_parts(&t, id);
            let media_ref_row =
                fetch_media_ref_row(pool, &media_id, library_id.map(LibraryId))
                    .await?;
            media_ref_row.map(|r| build_media_ref(&r, Vec::new()))
        }
        None => None,
    };

    // Provenance sources for this artifact.
    let source_rows = sqlx::query!(
        r#"
        SELECT source_kind::text AS "source_kind!", source_run_id, source_tool_call_id
        FROM intelligence_artifact_sources
        WHERE artifact_id = $1 AND status = 'active'
        ORDER BY source_ordinal
        "#,
        artifact_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("load artifact sources: {e}")))?;
    let mut provenance = Vec::new();
    for s in source_rows {
        let source_kind = s.source_kind;
        let source_run_id = s.source_run_id;
        let source_tool_call_id = s.source_tool_call_id;
        let source = match source_kind.as_str() {
            "tool_call" => IntelligenceGroundingSource::ToolCall,
            "artifact" | "manual" => {
                IntelligenceGroundingSource::IntelligenceArtifact
            }
            "search_document" => IntelligenceGroundingSource::SearchIndex,
            _ => IntelligenceGroundingSource::FerrexLibrary,
        };
        provenance.push(IntelligenceProvenanceRef {
            source,
            run_id: source_run_id,
            tool_call_id: source_tool_call_id,
            grounding: Vec::new(),
        });
    }

    Ok(IntelligenceArtifactSummary {
        artifact_id,
        kind: artifact_kind_from_db(&kind_str),
        media,
        title,
        summary: summary
            .as_deref()
            .map(|s| bounded_summary(s, summary_max_chars)),
        provenance,
        grounding: Vec::new(),
        created_at_epoch_seconds: Some(created_at.timestamp()),
        updated_at_epoch_seconds: Some(updated_at.timestamp()),
    })
}

async fn artifact_summaries_for_media(
    pool: &PgPool,
    media_id: &MediaID,
    user_id: Option<Uuid>,
    limit: u16,
    summary_max_chars: u16,
) -> Result<Vec<IntelligenceArtifactSummary>> {
    let rows = sqlx::query_as!(
        ArtifactSummaryRow,
        r#"
        SELECT id AS "id!", artifact_kind::text AS "artifact_kind!",
               library_id, media_id, media_type::text AS media_type,
               title AS "title!", summary, created_at AS "created_at!", updated_at AS "updated_at!"
        FROM intelligence_artifacts
        WHERE media_id = $1
          AND media_type = ($2::text)::media_type
          AND status = 'active'
          AND invalidated_at IS NULL
          AND (user_id IS NULL OR user_id = $3)
        ORDER BY updated_at DESC, id
        LIMIT $4
        "#,
        media_id.as_uuid(),
        media_type_str(media_id),
        user_id,
        i64::from(limit)
    )
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("artifact summaries for media: {e}")))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(
            artifact_summary_from_row(pool, row, summary_max_chars).await?,
        );
    }
    Ok(out)
}
