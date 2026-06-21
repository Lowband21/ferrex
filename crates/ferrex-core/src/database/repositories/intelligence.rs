//! PostgreSQL-backed implementation of [`IntelligenceRepository`].
//!
//! All queries use the runtime `sqlx::query` form (matching the watch-status
//! adapter) so the crate continues to compile under `SQLX_OFFLINE=true` without
//! per-query offline data. Behavior is validated by the SQLx-backed
//! integration tests in `tests/intelligence_repository.rs`.

use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use ferrex_model::{
    EpisodeID, LibraryId, MediaID, MovieID, SeasonID, SeriesID,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
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
        IntelligenceArtifactSummary, IntelligenceCandidate,
        IntelligenceCandidateSearchRequest,
        IntelligenceCandidateSearchResponse, IntelligenceContextItem,
        IntelligenceFacetGroup, IntelligenceFacetKind, IntelligenceFacetValue,
        IntelligenceGroundingRef, IntelligenceGroundingSource,
        IntelligenceItemContextRequest, IntelligenceItemContextResponse,
        IntelligenceLibraryOverview, IntelligenceLibraryOverviewRequest,
        IntelligenceLibraryOverviewResponse, IntelligenceMediaCounts,
        IntelligenceMediaKind, IntelligenceMediaRef, IntelligenceProvenanceRef,
        IntelligenceRelatedContext, IntelligenceRelatedContextRequest,
        IntelligenceRelatedContextResponse, IntelligenceRelationshipKind,
        IntelligenceRunAudit, IntelligenceRunAuditRequest,
        IntelligenceRunAuditResponse, IntelligenceRunPurpose,
        IntelligenceRunStatus, IntelligenceSummary, IntelligenceToolCallAudit,
        IntelligenceToolCallStatus, MAX_INTELLIGENCE_ARTIFACT_LIMIT,
        MAX_INTELLIGENCE_CANDIDATE_LIMIT, MAX_INTELLIGENCE_FACET_LIMIT,
        MAX_INTELLIGENCE_GROUNDING_LIMIT, MAX_INTELLIGENCE_PAGE_LIMIT,
        MAX_INTELLIGENCE_RELATED_LIMIT, MAX_INTELLIGENCE_TOOL_CALL_LIMIT,
    },
    database::repository_ports::intelligence::{
        IntelligenceArtifactScope, IntelligenceArtifactUpsert,
        IntelligenceRepository, IntelligenceRunCreate, IntelligenceRunKind,
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
    let rows = match media_id {
        MediaID::Movie(id) => sqlx::query(
            "SELECT name FROM movie_genres WHERE movie_id = $1 ORDER BY name",
        )
        .bind(id.0)
        .fetch_all(pool)
        .await,
        MediaID::Series(id) => sqlx::query(
            "SELECT name FROM series_genres WHERE series_id = $1 ORDER BY name",
        )
        .bind(id.0)
        .fetch_all(pool)
        .await,
        _ => return Ok(Vec::new()),
    }
    .map_err(|e| internal_err(format!("failed to load genres: {e}")))?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(row.try_get::<String, _>("name").map_err(|e| {
            internal_err(format!("failed to decode genre name: {e}"))
        })?);
    }
    Ok(out)
}

/// Cast/crew names for a media item, ordered deterministically and bounded.
async fn fetch_people(
    pool: &PgPool,
    media_id: &MediaID,
) -> Result<Vec<String>> {
    let rows = match media_id {
        MediaID::Movie(id) => sqlx::query(
            r#"
            SELECT p.name
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
        )
        .bind(id.0)
        .fetch_all(pool)
        .await,
        MediaID::Series(id) => sqlx::query(
            r#"
            SELECT p.name
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
        )
        .bind(id.0)
        .fetch_all(pool)
        .await,
        MediaID::Episode(id) => sqlx::query(
            r#"
            SELECT p.name
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
        )
        .bind(id.0)
        .fetch_all(pool)
        .await,
        MediaID::Season(_) => return Ok(Vec::new()),
    }
    .map_err(|e| internal_err(format!("failed to load people: {e}")))?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let name = row.try_get::<String, _>("name").map_err(|e| {
            internal_err(format!("failed to decode person name: {e}"))
        })?;
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
    let row = sqlx::query(
        r#"
        SELECT mr.id, mr.library_id, mr.title,
               mm.overview, mm.release_date, mm.runtime,
               mm.primary_poster_image_id
        FROM movie_references mr
        LEFT JOIN movie_metadata mm ON mm.movie_id = mr.id
        WHERE mr.id = $1
          AND ($2::uuid IS NULL OR mr.library_id = $2)
        "#,
    )
    .bind(id.0)
    .bind(library_id.map(|l| l.0))
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load movie ref: {e}")))?;

    let Some(row) = row else { return Ok(None) };
    let title: String = row.try_get("title").map_err(|e| {
        internal_err(format!("failed to decode movie title: {e}"))
    })?;
    Ok(Some(MediaRefRow {
        media_id: MediaID::Movie(*id),
        library_id: LibraryId(row.try_get::<Uuid, _>("library_id").map_err(
            |e| internal_err(format!("failed to decode library_id: {e}")),
        )?),
        title,
        year: year_from_date(
            row.try_get::<Option<NaiveDate>, _>("release_date")
                .ok()
                .flatten(),
        ),
        poster_iid: row
            .try_get::<Option<Uuid>, _>("primary_poster_image_id")
            .ok()
            .flatten(),
        overview: row.try_get::<Option<String>, _>("overview").ok().flatten(),
        runtime_seconds: row
            .try_get::<Option<i32>, _>("runtime")
            .ok()
            .flatten()
            .map(|m| m.saturating_mul(60)),
        release_date: row
            .try_get::<Option<NaiveDate>, _>("release_date")
            .ok()
            .flatten(),
    }))
}

async fn fetch_series_ref_row(
    pool: &PgPool,
    id: &SeriesID,
    library_id: Option<LibraryId>,
) -> Result<Option<MediaRefRow>> {
    let row = sqlx::query(
        r#"
        SELECT s.id, s.library_id, s.title,
               sm.overview, sm.first_air_date, sm.primary_poster_image_id
        FROM series s
        LEFT JOIN series_metadata sm ON sm.series_id = s.id
        WHERE s.id = $1
          AND ($2::uuid IS NULL OR s.library_id = $2)
        "#,
    )
    .bind(id.0)
    .bind(library_id.map(|l| l.0))
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load series ref: {e}")))?;

    let Some(row) = row else { return Ok(None) };
    let title: String = row.try_get("title").map_err(|e| {
        internal_err(format!("failed to decode series title: {e}"))
    })?;
    Ok(Some(MediaRefRow {
        media_id: MediaID::Series(*id),
        library_id: LibraryId(row.try_get::<Uuid, _>("library_id").map_err(
            |e| internal_err(format!("failed to decode library_id: {e}")),
        )?),
        title,
        year: year_from_date(
            row.try_get::<Option<NaiveDate>, _>("first_air_date")
                .ok()
                .flatten(),
        ),
        poster_iid: row
            .try_get::<Option<Uuid>, _>("primary_poster_image_id")
            .ok()
            .flatten(),
        overview: row.try_get::<Option<String>, _>("overview").ok().flatten(),
        runtime_seconds: None,
        release_date: row
            .try_get::<Option<NaiveDate>, _>("first_air_date")
            .ok()
            .flatten(),
    }))
}

async fn fetch_season_ref_row(
    pool: &PgPool,
    id: &SeasonID,
    library_id: Option<LibraryId>,
) -> Result<Option<MediaRefRow>> {
    let row = sqlx::query(
        r#"
        SELECT sr.id, sr.library_id, sr.season_number,
               sm.name, sm.overview, sm.air_date,
               sm.primary_poster_image_id
        FROM season_references sr
        LEFT JOIN season_metadata sm ON sm.season_id = sr.id
        WHERE sr.id = $1
          AND ($2::uuid IS NULL OR sr.library_id = $2)
        "#,
    )
    .bind(id.0)
    .bind(library_id.map(|l| l.0))
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load season ref: {e}")))?;

    let Some(row) = row else { return Ok(None) };
    let season_number: i16 = row.try_get("season_number").map_err(|e| {
        internal_err(format!("failed to decode season_number: {e}"))
    })?;
    let name: Option<String> =
        row.try_get::<Option<String>, _>("name").ok().flatten();
    let title = name.unwrap_or_else(|| format!("Season {}", season_number));
    Ok(Some(MediaRefRow {
        media_id: MediaID::Season(*id),
        library_id: LibraryId(row.try_get::<Uuid, _>("library_id").map_err(
            |e| internal_err(format!("failed to decode library_id: {e}")),
        )?),
        title,
        year: year_from_date(
            row.try_get::<Option<NaiveDate>, _>("air_date")
                .ok()
                .flatten(),
        ),
        poster_iid: row
            .try_get::<Option<Uuid>, _>("primary_poster_image_id")
            .ok()
            .flatten(),
        overview: row.try_get::<Option<String>, _>("overview").ok().flatten(),
        runtime_seconds: None,
        release_date: row
            .try_get::<Option<NaiveDate>, _>("air_date")
            .ok()
            .flatten(),
    }))
}

async fn fetch_episode_ref_row(
    pool: &PgPool,
    id: &EpisodeID,
    library_id: Option<LibraryId>,
) -> Result<Option<MediaRefRow>> {
    let row = sqlx::query(
        r#"
        SELECT er.id, er.season_number, er.episode_number,
               s.library_id, em.name, em.overview, em.air_date,
               em.runtime, em.primary_thumbnail_image_id
        FROM episode_references er
        JOIN series s ON er.series_id = s.id
        LEFT JOIN episode_metadata em ON em.episode_id = er.id
        WHERE er.id = $1
          AND ($2::uuid IS NULL OR s.library_id = $2)
        "#,
    )
    .bind(id.0)
    .bind(library_id.map(|l| l.0))
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load episode ref: {e}")))?;

    let Some(row) = row else { return Ok(None) };
    let season_number: i16 = row.try_get("season_number").map_err(|e| {
        internal_err(format!("failed to decode season_number: {e}"))
    })?;
    let episode_number: i16 = row.try_get("episode_number").map_err(|e| {
        internal_err(format!("failed to decode episode_number: {e}"))
    })?;
    let name: Option<String> =
        row.try_get::<Option<String>, _>("name").ok().flatten();
    let title = name.unwrap_or_else(|| {
        format!("S{:02}E{:02}", season_number, episode_number)
    });
    Ok(Some(MediaRefRow {
        media_id: MediaID::Episode(*id),
        library_id: LibraryId(row.try_get::<Uuid, _>("library_id").map_err(
            |e| internal_err(format!("failed to decode library_id: {e}")),
        )?),
        title,
        year: year_from_date(
            row.try_get::<Option<NaiveDate>, _>("air_date")
                .ok()
                .flatten(),
        ),
        poster_iid: row
            .try_get::<Option<Uuid>, _>("primary_thumbnail_image_id")
            .ok()
            .flatten(),
        overview: row.try_get::<Option<String>, _>("overview").ok().flatten(),
        runtime_seconds: row
            .try_get::<Option<i32>, _>("runtime")
            .ok()
            .flatten()
            .map(|m| m.saturating_mul(60)),
        release_date: row
            .try_get::<Option<NaiveDate>, _>("air_date")
            .ok()
            .flatten(),
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
) -> Result<()> {
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

    let conflict_clause = match user_id {
        Some(_) =>
            "ON CONFLICT (library_id, user_id, media_type, media_id, context_kind)
                WHERE user_id IS NOT NULL",
        None =>
            "ON CONFLICT (library_id, media_type, media_id, context_kind)
                WHERE user_id IS NULL",
    };
    let sql = format!(
        r#"
        INSERT INTO intelligence_media_context (
            library_id, user_id, media_id, media_type, context_kind, status,
            title, sort_title, summary, excerpt, release_date, runtime_seconds,
            source_system, source_revision, content_hash, metadata
        )
        VALUES ($1, $2, $3, $4::media_type, $5, 'active', $6, $7, $8, $9, $10, $11,
                'ferrex', $12, $13, $14::jsonb)
        {conflict_clause}
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
        "#,
    );
    sqlx::query(&sql)
        .bind(library_id.0)
        .bind(user_id)
        .bind(media_id)
        .bind(media_type)
        .bind(context_kind)
        .bind(&title)
        .bind(&sort_title)
        .bind(&summary)
        .bind(&excerpt)
        .bind(row.release_date)
        .bind(row.runtime_seconds)
        .bind(source_revision)
        .bind(&hash)
        .bind(metadata)
        .execute(pool)
        .await
        .map_err(|e| internal_err(format!("failed to upsert context: {e}")))?;
    Ok(())
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
) -> Result<()> {
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

    let conflict_clause = match user_id {
        Some(_) =>
            "ON CONFLICT (library_id, user_id, media_type, media_id, document_kind)
                WHERE user_id IS NOT NULL",
        None =>
            "ON CONFLICT (library_id, media_type, media_id, document_kind)
                WHERE user_id IS NULL",
    };
    let sql = format!(
        r#"
        INSERT INTO intelligence_search_documents (
            library_id, user_id, media_id, media_type, document_kind, status,
            title, summary, search_excerpt, search_text, language,
            source_system, source_revision, content_hash, metadata
        )
        VALUES ($1, $2, $3, $4::media_type, $5, 'active', $6, $7, $8, $9, 'simple',
                'ferrex', $10, $11, $12::jsonb)
        {conflict_clause}
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
        "#,
    );
    sqlx::query(&sql)
        .bind(library_id.0)
        .bind(user_id)
        .bind(media_id)
        .bind(media_type)
        .bind(document_kind)
        .bind(&title)
        .bind(&summary)
        .bind(&excerpt)
        .bind(&text)
        .bind(source_revision)
        .bind(&hash)
        .bind(metadata)
        .execute(pool)
        .await
        .map_err(|e| {
            internal_err(format!("failed to upsert search doc: {e}"))
        })?;
    Ok(())
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
    let rows = sqlx::query(
        r#"
        SELECT mr.id AS media_id, mr.title,
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
    )
    .bind(library_id.0)
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load movies: {e}")))?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(MovieRefreshRow {
            media_id: row
                .try_get("media_id")
                .map_err(|e| internal_err(format!("decode media_id: {e}")))?,
            title: row
                .try_get("title")
                .map_err(|e| internal_err(format!("decode title: {e}")))?,
            overview: row
                .try_get::<Option<String>, _>("overview")
                .ok()
                .flatten(),
            release_date: row
                .try_get::<Option<NaiveDate>, _>("release_date")
                .ok()
                .flatten(),
            runtime: row.try_get::<Option<i32>, _>("runtime").ok().flatten(),
            vote_average: row
                .try_get::<Option<f32>, _>("vote_average")
                .ok()
                .flatten(),
            primary_certification: row
                .try_get::<Option<String>, _>("primary_certification")
                .ok()
                .flatten(),
        });
    }
    Ok(out)
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
    let rows = sqlx::query(
        r#"
        SELECT er.id AS media_id, er.series_id, er.season_id,
               er.season_number, er.episode_number,
               em.name AS title, em.overview, em.air_date, em.runtime
        FROM episode_references er
        JOIN series s ON er.series_id = s.id AND s.library_id = $1
        JOIN media_files mf ON er.file_id = mf.id
            AND mf.is_available = TRUE
            AND mf.tombstoned_at IS NULL
        LEFT JOIN episode_metadata em ON em.episode_id = er.id
        ORDER BY er.series_id, er.season_number, er.episode_number, er.id
        "#,
    )
    .bind(library_id.0)
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load episodes: {e}")))?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(EpisodeRefreshRow {
            media_id: row
                .try_get("media_id")
                .map_err(|e| internal_err(format!("decode media_id: {e}")))?,
            series_id: row
                .try_get("series_id")
                .map_err(|e| internal_err(format!("decode series_id: {e}")))?,
            season_id: row
                .try_get("season_id")
                .map_err(|e| internal_err(format!("decode season_id: {e}")))?,
            season_number: row.try_get("season_number").map_err(|e| {
                internal_err(format!("decode season_number: {e}"))
            })?,
            episode_number: row.try_get("episode_number").map_err(|e| {
                internal_err(format!("decode episode_number: {e}"))
            })?,
            title: row.try_get::<Option<String>, _>("title").ok().flatten(),
            overview: row
                .try_get::<Option<String>, _>("overview")
                .ok()
                .flatten(),
            air_date: row
                .try_get::<Option<NaiveDate>, _>("air_date")
                .ok()
                .flatten(),
            runtime: row.try_get::<Option<i32>, _>("runtime").ok().flatten(),
        });
    }
    Ok(out)
}

async fn upsert_movie_read_model(
    pool: &PgPool,
    library_id: LibraryId,
    user_id: Option<Uuid>,
    row: &MovieRefreshRow,
    source_revision: i64,
) -> Result<()> {
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

    if user_id.is_none() {
        upsert_context_row(
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
        upsert_search_row(
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
    Ok(())
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
) -> Result<()> {
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
    upsert_context_row(
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
    upsert_search_row(
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
    Ok(())
}

async fn upsert_series_read_model(
    pool: &PgPool,
    library_id: LibraryId,
    series_id: Uuid,
    available_episode_ids: &[Uuid],
    source_revision: i64,
) -> Result<()> {
    if available_episode_ids.is_empty() {
        return Ok(());
    }
    let media_id = MediaID::Series(SeriesID(series_id));
    let genres = fetch_genres(pool, &media_id).await?;
    let people = fetch_people(pool, &media_id).await?;
    let row = sqlx::query(
        r#"
        SELECT s.title, sm.overview, sm.first_air_date,
               sm.primary_poster_image_id, sm.primary_content_rating,
               sm.vote_average
        FROM series s
        LEFT JOIN series_metadata sm ON sm.series_id = s.id
        WHERE s.id = $1
        "#,
    )
    .bind(series_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load series: {e}")))?;
    let Some(row) = row else { return Ok(()) };
    let title: String = row
        .try_get("title")
        .map_err(|e| internal_err(format!("decode series title: {e}")))?;
    let overview: Option<String> =
        row.try_get::<Option<String>, _>("overview").ok().flatten();
    let first_air_date: Option<NaiveDate> = row
        .try_get::<Option<NaiveDate>, _>("first_air_date")
        .ok()
        .flatten();
    let poster: Option<Uuid> = row
        .try_get::<Option<Uuid>, _>("primary_poster_image_id")
        .ok()
        .flatten();
    let content_rating: Option<String> = row
        .try_get::<Option<String>, _>("primary_content_rating")
        .ok()
        .flatten();
    let vote_average: Option<f32> =
        row.try_get::<Option<f32>, _>("vote_average").ok().flatten();

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

    upsert_context_row(
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
    upsert_search_row(
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
    Ok(())
}

async fn upsert_season_read_model(
    pool: &PgPool,
    library_id: LibraryId,
    season_id: Uuid,
    source_revision: i64,
) -> Result<()> {
    let row = sqlx::query(
        r#"
        SELECT sr.season_number, sm.name, sm.overview, sm.air_date,
               sm.primary_poster_image_id, sm.runtime
        FROM season_references sr
        LEFT JOIN season_metadata sm ON sm.season_id = sr.id
        WHERE sr.id = $1
        "#,
    )
    .bind(season_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load season: {e}")))?;
    let Some(row) = row else { return Ok(()) };
    let season_number: i16 = row
        .try_get("season_number")
        .map_err(|e| internal_err(format!("decode season_number: {e}")))?;
    let name: Option<String> =
        row.try_get::<Option<String>, _>("name").ok().flatten();
    let overview: Option<String> =
        row.try_get::<Option<String>, _>("overview").ok().flatten();
    let air_date: Option<NaiveDate> = row
        .try_get::<Option<NaiveDate>, _>("air_date")
        .ok()
        .flatten();
    let poster: Option<Uuid> = row
        .try_get::<Option<Uuid>, _>("primary_poster_image_id")
        .ok()
        .flatten();
    let runtime: Option<i32> =
        row.try_get::<Option<i32>, _>("runtime").ok().flatten();

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

    upsert_context_row(
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
    upsert_search_row(
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
    Ok(())
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
    library_id: LibraryId,
}

async fn load_user_watch_progress(
    pool: &PgPool,
    library_id: LibraryId,
    user_id: Uuid,
) -> Result<Vec<WatchProgressRow>> {
    let rows = sqlx::query(
        r#"
        SELECT uwp.media_uuid, uwp.media_type, uwp.position, uwp.duration,
               uwp.last_watched, COALESCE(m.title, e.title, s.title, sm.name,
                                          uwp.media_uuid::text) AS title,
               COALESCE(m.library_id, s.library_id, sr.library_id, e.library_id) AS library_id
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
    )
    .bind(user_id)
    .bind(library_id.0)
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load watch progress: {e}")))?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let library_uuid: Option<Uuid> =
            row.try_get::<Option<Uuid>, _>("library_id").ok().flatten();
        let resolved_library_id =
            LibraryId(library_uuid.unwrap_or(library_id.0));
        out.push(WatchProgressRow {
            media_uuid: row
                .try_get("media_uuid")
                .map_err(|e| internal_err(format!("decode media_uuid: {e}")))?,
            media_type: row
                .try_get("media_type")
                .map_err(|e| internal_err(format!("decode media_type: {e}")))?,
            position: row
                .try_get("position")
                .map_err(|e| internal_err(format!("decode position: {e}")))?,
            duration: row
                .try_get("duration")
                .map_err(|e| internal_err(format!("decode duration: {e}")))?,
            last_watched: row.try_get("last_watched").map_err(|e| {
                internal_err(format!("decode last_watched: {e}"))
            })?,
            title: row
                .try_get("title")
                .map_err(|e| internal_err(format!("decode title: {e}")))?,
            library_id: resolved_library_id,
        });
    }
    Ok(out)
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
    let rows = sqlx::query(
        r#"
        SELECT id FROM intelligence_artifacts
        WHERE media_id = $1
          AND status = 'active'
          AND invalidated_at IS NULL
          AND (user_id IS NULL OR user_id = $2)
        ORDER BY updated_at DESC, id
        LIMIT $3
        "#,
    )
    .bind(media_id.as_uuid())
    .bind(user_id)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load artifact ids: {e}")))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(
            row.try_get::<Uuid, _>("id").map_err(|e| {
                internal_err(format!("decode artifact id: {e}"))
            })?,
        );
    }
    Ok(out)
}

/// Resolve a library name.
async fn library_name(pool: &PgPool, library_id: LibraryId) -> Result<String> {
    let row = sqlx::query_scalar::<_, String>(
        "SELECT name FROM libraries WHERE id = $1",
    )
    .bind(library_id.0)
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("failed to load library name: {e}")))?;
    row.ok_or_else(|| MediaError::NotFound("library not found".to_string()))
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
                let media_row = MediaRefRow {
                    media_id,
                    library_id: entry.library_id,
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
                upsert_context_row(
                    pool,
                    entry.library_id,
                    Some(uid),
                    &media_row,
                    "watch_state",
                    &summary,
                    &summary,
                    &metadata,
                    source_revision,
                )
                .await?;
                upsert_search_row(
                    pool,
                    entry.library_id,
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
                refreshed += 1;
            }
            return Ok(refreshed);
        }

        // Global refresh: movies.
        let movies = load_available_movies(pool, library_id).await?;
        for movie in &movies {
            upsert_movie_read_model(
                pool,
                library_id,
                None,
                movie,
                source_revision,
            )
            .await?;
            refreshed += 1;
        }

        // Episodes (and derived series/season rows).
        let episodes = load_available_episodes(pool, library_id).await?;
        let mut series_episodes: std::collections::BTreeMap<Uuid, Vec<Uuid>> =
            std::collections::BTreeMap::new();
        let mut seasons_seen: std::collections::BTreeSet<Uuid> =
            std::collections::BTreeSet::new();
        for ep in &episodes {
            upsert_episode_read_model(
                pool,
                library_id,
                None,
                ep,
                source_revision,
            )
            .await?;
            refreshed += 1;
            series_episodes
                .entry(ep.series_id)
                .or_default()
                .push(ep.media_id);
            if seasons_seen.insert(ep.season_id) {
                upsert_season_read_model(
                    pool,
                    library_id,
                    ep.season_id,
                    source_revision,
                )
                .await?;
                refreshed += 1;
            }
        }
        for (series_id, eps) in &series_episodes {
            upsert_series_read_model(
                pool,
                library_id,
                *series_id,
                eps,
                source_revision,
            )
            .await?;
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
                let row = sqlx::query(
                    r#"
                    SELECT mr.id AS media_id, mr.title,
                           mm.overview, mm.release_date, mm.runtime,
                           mm.vote_average, mm.primary_certification
                    FROM movie_references mr
                    JOIN media_files mf ON mr.file_id = mf.id
                        AND mf.is_available = TRUE
                        AND mf.tombstoned_at IS NULL
                    LEFT JOIN movie_metadata mm ON mm.movie_id = mr.id
                    WHERE mr.id = $1 AND mr.library_id = $2
                    "#,
                )
                .bind(id.0)
                .bind(library_id.0)
                .fetch_optional(pool)
                .await
                .map_err(|e| {
                    internal_err(format!("failed to load movie: {e}"))
                })?;
                let Some(row) = row else {
                    return Ok(());
                };
                let movie = MovieRefreshRow {
                    media_id: row.try_get("media_id").map_err(|e| {
                        internal_err(format!("decode media_id: {e}"))
                    })?,
                    title: row.try_get("title").map_err(|e| {
                        internal_err(format!("decode title: {e}"))
                    })?,
                    overview: row
                        .try_get::<Option<String>, _>("overview")
                        .ok()
                        .flatten(),
                    release_date: row
                        .try_get::<Option<NaiveDate>, _>("release_date")
                        .ok()
                        .flatten(),
                    runtime: row
                        .try_get::<Option<i32>, _>("runtime")
                        .ok()
                        .flatten(),
                    vote_average: row
                        .try_get::<Option<f32>, _>("vote_average")
                        .ok()
                        .flatten(),
                    primary_certification: row
                        .try_get::<Option<String>, _>("primary_certification")
                        .ok()
                        .flatten(),
                };
                upsert_movie_read_model(
                    pool,
                    library_id,
                    None,
                    &movie,
                    source_revision,
                )
                .await?;
            }
            MediaID::Episode(id) => {
                let row = sqlx::query(
                    r#"
                    SELECT er.id AS media_id, er.series_id, er.season_id,
                           er.season_number, er.episode_number,
                           em.name AS title, em.overview, em.air_date, em.runtime
                    FROM episode_references er
                    JOIN series s ON er.series_id = s.id AND s.library_id = $2
                    JOIN media_files mf ON er.file_id = mf.id
                        AND mf.is_available = TRUE
                        AND mf.tombstoned_at IS NULL
                    LEFT JOIN episode_metadata em ON em.episode_id = er.id
                    WHERE er.id = $1
                    "#,
                )
                .bind(id.0)
                .bind(library_id.0)
                .fetch_optional(pool)
                .await
                .map_err(|e| internal_err(format!("failed to load episode: {e}")))?;
                let Some(row) = row else {
                    return Ok(());
                };
                let ep = EpisodeRefreshRow {
                    media_id: row.try_get("media_id").map_err(|e| {
                        internal_err(format!("decode media_id: {e}"))
                    })?,
                    series_id: row.try_get("series_id").map_err(|e| {
                        internal_err(format!("decode series_id: {e}"))
                    })?,
                    season_id: row.try_get("season_id").map_err(|e| {
                        internal_err(format!("decode season_id: {e}"))
                    })?,
                    season_number: row.try_get("season_number").map_err(
                        |e| internal_err(format!("decode season_number: {e}")),
                    )?,
                    episode_number: row.try_get("episode_number").map_err(
                        |e| internal_err(format!("decode episode_number: {e}")),
                    )?,
                    title: row
                        .try_get::<Option<String>, _>("title")
                        .ok()
                        .flatten(),
                    overview: row
                        .try_get::<Option<String>, _>("overview")
                        .ok()
                        .flatten(),
                    air_date: row
                        .try_get::<Option<NaiveDate>, _>("air_date")
                        .ok()
                        .flatten(),
                    runtime: row
                        .try_get::<Option<i32>, _>("runtime")
                        .ok()
                        .flatten(),
                };
                upsert_episode_read_model(
                    pool,
                    library_id,
                    None,
                    &ep,
                    source_revision,
                )
                .await?;
                upsert_season_read_model(
                    pool,
                    library_id,
                    ep.season_id,
                    source_revision,
                )
                .await?;
                upsert_series_read_model(
                    pool,
                    library_id,
                    ep.series_id,
                    &[ep.media_id],
                    source_revision,
                )
                .await?;
            }
            MediaID::Season(id) => {
                let has_available_episode = sqlx::query_scalar::<_, bool>(
                    r#"
                    SELECT EXISTS (
                        SELECT 1 FROM episode_references er
                        JOIN series s ON er.series_id = s.id AND s.library_id = $2
                        JOIN media_files mf ON er.file_id = mf.id
                            AND mf.is_available = TRUE
                            AND mf.tombstoned_at IS NULL
                        WHERE er.season_id = $1
                    )
                    "#,
                )
                .bind(id.0)
                .bind(library_id.0)
                .fetch_one(pool)
                .await
                .map_err(|e| {
                    internal_err(format!("failed to check season availability: {e}"))
                })?;
                if has_available_episode {
                    upsert_season_read_model(
                        pool,
                        library_id,
                        id.0,
                        source_revision,
                    )
                    .await?;
                }
            }
            MediaID::Series(id) => {
                let ep_ids = sqlx::query(
                    r#"
                    SELECT er.id FROM episode_references er
                    JOIN series s ON er.series_id = s.id AND s.library_id = $2
                    JOIN media_files mf ON er.file_id = mf.id
                        AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
                    WHERE er.series_id = $1
                    "#,
                )
                .bind(id.0)
                .bind(library_id.0)
                .fetch_all(pool)
                .await
                .map_err(|e| {
                    internal_err(format!("failed to load series episodes: {e}"))
                })?;
                let mut ids = Vec::with_capacity(ep_ids.len());
                for row in ep_ids {
                    ids.push(row.try_get::<Uuid, _>("id").map_err(|e| {
                        internal_err(format!("decode episode id: {e}"))
                    })?);
                }
                upsert_series_read_model(
                    pool,
                    library_id,
                    id.0,
                    &ids,
                    source_revision,
                )
                .await?;
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
        let media_type = media_type_str(&media_id);
        let reason = truncate_chars(reason, 512);
        let result = sqlx::query(
            r#"
            UPDATE intelligence_media_context
            SET status = 'invalidated',
                invalidated_at = now(),
                invalidation_reason = $1,
                updated_at = now()
            WHERE library_id = $2
              AND media_id = $3
              AND media_type = $4::media_type
              AND (($5::uuid IS NULL AND user_id IS NULL) OR user_id = $5)
            "#,
        )
        .bind(&reason)
        .bind(library_id.0)
        .bind(media_id.as_uuid())
        .bind(media_type)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|e| {
            internal_err(format!("failed to invalidate context: {e}"))
        })?;
        let _ = result;

        let result2 = sqlx::query(
            r#"
            UPDATE intelligence_search_documents
            SET status = 'invalidated',
                invalidated_at = now(),
                invalidation_reason = $1,
                updated_at = now()
            WHERE library_id = $2
              AND media_id = $3
              AND media_type = $4::media_type
              AND (($5::uuid IS NULL AND user_id IS NULL) OR user_id = $5)
            "#,
        )
        .bind(&reason)
        .bind(library_id.0)
        .bind(media_id.as_uuid())
        .bind(media_type)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|e| {
            internal_err(format!("failed to invalidate search doc: {e}"))
        })?;
        let _ = result2;
        Ok(())
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
            sqlx::query(
                "SELECT id FROM libraries WHERE enabled = TRUE ORDER BY id",
            )
            .fetch_all(pool)
            .await
            .map_err(|e| {
                internal_err(format!("failed to load libraries: {e}"))
            })?
            .into_iter()
            .map(|r| {
                r.try_get::<Uuid, _>("id").map_err(|e| {
                    internal_err(format!("decode library id: {e}"))
                })
            })
            .collect::<Result<_>>()?
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
        let rows = sqlx::query(
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
            SELECT media_id, media_type, library_id, title, summary, search_excerpt, max(rank) AS rank
            FROM (
                SELECT * FROM fts
                UNION ALL
                SELECT * FROM trgm
            ) combined
            GROUP BY media_id, media_type, library_id, title, summary, search_excerpt
            ORDER BY rank DESC, title ASC, media_id ASC
            LIMIT $5
            "#,
        )
        .bind(query)
        .bind(&library_ids)
        .bind(&media_kinds)
        .bind(user_id)
        .bind(fetch_limit)
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("candidate search failed: {e}")))?;

        let has_more = (rows.len() as i64) > limit;
        let mut candidates = Vec::with_capacity(rows.len().min(limit as usize));
        for row in rows.into_iter().take(limit as usize) {
            let media_uuid: Uuid = row
                .try_get("media_id")
                .map_err(|e| internal_err(format!("decode media_id: {e}")))?;
            let media_type: String = row
                .try_get("media_type")
                .map_err(|e| internal_err(format!("decode media_type: {e}")))?;
            let library_id: Uuid = row
                .try_get("library_id")
                .map_err(|e| internal_err(format!("decode library_id: {e}")))?;
            let title: String = row
                .try_get("title")
                .map_err(|e| internal_err(format!("decode title: {e}")))?;
            let summary: Option<String> =
                row.try_get::<Option<String>, _>("summary").ok().flatten();
            let excerpt: Option<String> = row
                .try_get::<Option<String>, _>("search_excerpt")
                .ok()
                .flatten();
            let rank: Option<f32> =
                row.try_get::<Option<f32>, _>("rank").ok().flatten();

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

        let rows = sqlx::query(
            r#"
            SELECT id, artifact_kind::text AS artifact_kind, scope::text AS scope,
                   library_id, user_id, media_id, media_type::text AS media_type,
                   title, summary, created_at, updated_at
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
        )
        .bind(&artifact_ids)
        .bind(&media_ids)
        .bind(&library_ids)
        .bind(&kinds)
        .bind(user_id)
        .bind(fetch_limit)
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
        let row = sqlx::query(
            r#"
            SELECT id, artifact_kind::text AS artifact_kind, scope::text AS scope,
                   library_id, user_id, media_id, media_type::text AS media_type,
                   title, summary, created_at, updated_at
            FROM intelligence_artifacts
            WHERE id = $1
              AND status = 'active'
              AND invalidated_at IS NULL
              AND (user_id IS NULL OR user_id = $2)
            "#,
        )
        .bind(artifact_id)
        .bind(user_id)
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
                let result = sqlx::query(
                    r#"
                    UPDATE intelligence_artifacts
                    SET artifact_kind = $2::varchar,
                        scope = $3::varchar,
                        library_id = $4,
                        user_id = $5,
                        media_id = $6,
                        media_type = $7::media_type,
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
                )
                .bind(id)
                .bind(kind_db)
                .bind(scope_db)
                .bind(upsert.library_id.map(|l| l.0))
                .bind(user_id)
                .bind(media_id)
                .bind(media_type)
                .bind(upsert.run_id)
                .bind(&title)
                .bind(&summary)
                .bind(&excerpt)
                .bind(&upsert.content)
                .bind(&upsert.metadata)
                .bind(upsert.source_revision)
                .bind(&hash)
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
                sqlx::query(
                    r#"
                    INSERT INTO intelligence_artifacts (
                        id, artifact_kind, scope, status, library_id, user_id,
                        media_id, media_type, run_id, supersedes_artifact_id,
                        title, summary, excerpt, content, metadata,
                        source_system, source_revision, content_hash
                    )
                    VALUES ($1, $2::varchar, $3::varchar, 'active', $4, $5, $6, $7::media_type,
                            $8, $9, $10, $11, $12, $13::jsonb, $14::jsonb,
                            'ferrex', $15, $16)
                    "#,
                )
                .bind(new_id)
                .bind(kind_db)
                .bind(scope_db)
                .bind(upsert.library_id.map(|l| l.0))
                .bind(user_id)
                .bind(media_id)
                .bind(media_type)
                .bind(upsert.run_id)
                .bind(upsert.supersedes_artifact_id)
                .bind(&title)
                .bind(&summary)
                .bind(&excerpt)
                .bind(&upsert.content)
                .bind(&upsert.metadata)
                .bind(upsert.source_revision)
                .bind(&hash)
                .execute(pool)
                .await
                .map_err(|e| internal_err(format!("insert artifact failed: {e}")))?;
                if let Some(superseded) = upsert.supersedes_artifact_id {
                    let result = sqlx::query(
                        r#"
                        UPDATE intelligence_artifacts
                        SET status = 'superseded', updated_at = now()
                        WHERE id = $1
                          AND (($2::uuid IS NULL AND user_id IS NULL) OR user_id = $2)
                        "#,
                    )
                    .bind(superseded)
                    .bind(user_id)
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

    async fn invalidate_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
        reason: &str,
    ) -> Result<()> {
        let pool = self.pool();
        let reason = truncate_chars(reason, 512);
        let result = sqlx::query(
            r#"
            UPDATE intelligence_artifacts
            SET status = 'invalidated',
                invalidated_at = now(),
                invalidation_reason = $1,
                updated_at = now()
            WHERE id = $2
              AND (($3::uuid IS NULL AND user_id IS NULL) OR user_id = $3)
            "#,
        )
        .bind(&reason)
        .bind(artifact_id)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|e| {
            internal_err(format!("invalidate artifact failed: {e}"))
        })?;
        if result.rows_affected() == 0 {
            let exists: Option<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM intelligence_artifacts WHERE id = $1",
            )
            .bind(artifact_id)
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
            let existing = sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT id FROM intelligence_runs
                WHERE idempotency_key = $1
                "#,
            )
            .bind(key)
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
        sqlx::query(
            r#"
            INSERT INTO intelligence_runs (
                id, run_kind, status, library_id, user_id, media_id, media_type,
                idempotency_key, provider_name, model_name, request_hash,
                prompt_excerpt, metadata
            )
            VALUES ($1, $2::varchar, 'queued', $3, $4, $5, $6::media_type, $7, $8, $9, $10,
                    $11, $12::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(create.run_kind.as_db_str())
        .bind(create.library_id.map(|l| l.0))
        .bind(create.user_id)
        .bind(media_id)
        .bind(media_type)
        .bind(&create.idempotency_key)
        .bind(&create.provider_name)
        .bind(&create.model_name)
        .bind(&create.request_hash)
        .bind(&create.prompt_excerpt)
        .bind(&create.metadata)
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

        sqlx::query(
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
        )
        .bind(run_id)
        .bind(status)
        .bind(&update.provider_name)
        .bind(&update.model_name)
        .bind(&update.result_summary)
        .bind(&update.error_excerpt)
        .bind(started_at)
        .bind(finished_at)
        .bind(update.metadata.as_ref())
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

        let rows = sqlx::query(
            r#"
            SELECT id, run_kind::text AS run_kind, status::text AS status,
                   library_id, user_id, media_id, media_type::text AS media_type,
                   correlation_id, idempotency_key, model_name,
                   started_at, finished_at, created_at, updated_at
            FROM intelligence_runs
            WHERE ($1::uuid IS NULL OR library_id = $1)
              AND ($2::uuid IS NULL OR user_id = $2)
              AND ($3::text IS NULL OR run_kind = $3::varchar)
              AND ($4::text IS NULL OR status = $4::varchar)
            ORDER BY created_at DESC, id
            LIMIT $5
            "#,
        )
        .bind(filter.library_id.map(|l| l.0))
        .bind(filter.user_id)
        .bind(&run_kind)
        .bind(&status)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("list runs failed: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let media_id: Option<Uuid> = row
                .try_get("media_id")
                .map_err(|e| internal_err(format!("decode media_id: {e}")))?;
            let media_type: Option<String> = row
                .try_get::<Option<String>, _>("media_type")
                .map_err(|e| internal_err(format!("decode media_type: {e}")))?;
            let media = media_id
                .zip(media_type)
                .map(|(id, t)| media_id_from_parts(&t, id));
            out.push(IntelligenceRunSummary {
                run_id: row
                    .try_get("id")
                    .map_err(|e| internal_err(format!("decode run id: {e}")))?,
                run_kind: run_kind_from_db(
                    &row.try_get::<String, _>("run_kind").map_err(|e| {
                        internal_err(format!("decode run_kind: {e}"))
                    })?,
                ),
                status: match row
                    .try_get::<String, _>("status")
                    .map_err(|e| internal_err(format!("decode status: {e}")))?
                    .as_str()
                {
                    "running" => RunStatusInternal::Running,
                    "succeeded" => RunStatusInternal::Succeeded,
                    "failed" => RunStatusInternal::Failed,
                    "cancelled" => RunStatusInternal::Cancelled,
                    _ => RunStatusInternal::Queued,
                },
                library_id: row
                    .try_get::<Option<Uuid>, _>("library_id")
                    .ok()
                    .flatten()
                    .map(LibraryId),
                user_id: row
                    .try_get::<Option<Uuid>, _>("user_id")
                    .ok()
                    .flatten(),
                media_id: media,
                correlation_id: row.try_get("correlation_id").map_err(|e| {
                    internal_err(format!("decode correlation_id: {e}"))
                })?,
                idempotency_key: row
                    .try_get::<Option<String>, _>("idempotency_key")
                    .ok()
                    .flatten(),
                model_name: row
                    .try_get::<Option<String>, _>("model_name")
                    .ok()
                    .flatten(),
                started_at: row
                    .try_get::<Option<DateTime<Utc>>, _>("started_at")
                    .ok()
                    .flatten(),
                finished_at: row
                    .try_get::<Option<DateTime<Utc>>, _>("finished_at")
                    .ok()
                    .flatten(),
                created_at: row.try_get("created_at").map_err(|e| {
                    internal_err(format!("decode created_at: {e}"))
                })?,
                updated_at: row.try_get("updated_at").map_err(|e| {
                    internal_err(format!("decode updated_at: {e}"))
                })?,
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

        let row = sqlx::query(
            r#"
            SELECT id, run_kind::text AS run_kind, status::text AS status,
                   user_id, model_name, provider_name, prompt_excerpt,
                   result_summary, error_excerpt, started_at, finished_at,
                   created_at, updated_at
            FROM intelligence_runs
            WHERE id = $1
              AND (user_id IS NULL OR user_id = $2)
            "#,
        )
        .bind(request.run_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| internal_err(format!("run_audit failed: {e}")))?;
        let row = row.ok_or_else(|| {
            MediaError::NotFound("intelligence run not found".to_string())
        })?;

        let run_kind: String = row
            .try_get("run_kind")
            .map_err(|e| internal_err(format!("decode run_kind: {e}")))?;
        let status: String = row
            .try_get("status")
            .map_err(|e| internal_err(format!("decode status: {e}")))?;

        // Tool calls for this run, bounded and ordered by sequence.
        let tc_rows = sqlx::query(
            r#"
            SELECT tc.id, tc.tool_name, tc.status::text AS status,
                   tc.started_at, tc.finished_at, tc.error_excerpt,
                   tc.arguments, tc.result
            FROM intelligence_tool_calls tc
            WHERE tc.run_id = $1
            ORDER BY tc.sequence, tc.id
            LIMIT $2
            "#,
        )
        .bind(request.run_id)
        .bind(tool_call_fetch_limit)
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("list tool calls failed: {e}")))?;

        let has_more = (tc_rows.len() as i64) > tool_call_limit;
        let mut tool_calls =
            Vec::with_capacity(tc_rows.len().min(tool_call_limit as usize));
        for tc in tc_rows.into_iter().take(tool_call_limit as usize) {
            let arguments: Value = tc
                .try_get("arguments")
                .map_err(|e| internal_err(format!("decode arguments: {e}")))?;
            let result: Option<Value> =
                tc.try_get::<Option<Value>, _>("result").ok().flatten();
            let tool_name: String = tc
                .try_get("tool_name")
                .map_err(|e| internal_err(format!("decode tool_name: {e}")))?;
            let tc_status: String = tc.try_get("status").map_err(|e| {
                internal_err(format!("decode tool status: {e}"))
            })?;
            let started: Option<DateTime<Utc>> = tc
                .try_get::<Option<DateTime<Utc>>, _>("started_at")
                .ok()
                .flatten();
            let finished: Option<DateTime<Utc>> = tc
                .try_get::<Option<DateTime<Utc>>, _>("finished_at")
                .ok()
                .flatten();
            let error_excerpt: Option<String> = tc
                .try_get::<Option<String>, _>("error_excerpt")
                .ok()
                .flatten();

            // Artifact ids produced by this tool call (via run linkage).
            let artifact_ids = sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT id FROM intelligence_artifacts
                WHERE run_id = $1
                  AND status = 'active'
                  AND invalidated_at IS NULL
                  AND (user_id IS NULL OR user_id = $2)
                ORDER BY updated_at DESC, id
                LIMIT $3
                "#,
            )
            .bind(request.run_id)
            .bind(user_id)
            .bind(i64::from(artifact_limit))
            .fetch_all(pool)
            .await
            .map_err(|e| internal_err(format!("load run artifacts: {e}")))?;

            tool_calls.push(IntelligenceToolCallAudit {
                tool_call_id: tc.try_get("id").map_err(|e| {
                    internal_err(format!("decode tool_call id: {e}"))
                })?,
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

        let run_artifact_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id FROM intelligence_artifacts
            WHERE run_id = $1
              AND status = 'active'
              AND invalidated_at IS NULL
              AND (user_id IS NULL OR user_id = $2)
            ORDER BY updated_at DESC, id
            LIMIT $3
            "#,
        )
        .bind(request.run_id)
        .bind(user_id)
        .bind(i64::from(artifact_limit))
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("load run artifacts: {e}")))?;

        let started: Option<DateTime<Utc>> = row
            .try_get::<Option<DateTime<Utc>>, _>("started_at")
            .ok()
            .flatten();
        let finished: Option<DateTime<Utc>> = row
            .try_get::<Option<DateTime<Utc>>, _>("finished_at")
            .ok()
            .flatten();
        let prompt_excerpt: Option<String> = row
            .try_get::<Option<String>, _>("prompt_excerpt")
            .ok()
            .flatten();
        let result_summary: Option<String> = row
            .try_get::<Option<String>, _>("result_summary")
            .ok()
            .flatten();
        let user_id_row: Option<Uuid> =
            row.try_get::<Option<Uuid>, _>("user_id").ok().flatten();
        let model_name: Option<String> = row
            .try_get::<Option<String>, _>("model_name")
            .ok()
            .flatten();

        let run = IntelligenceRunAudit {
            run_id: request.run_id,
            purpose: run_purpose_from_db(&run_kind),
            status: run_status_from_db(&status),
            requested_by_user_id: user_id_row,
            model: model_name,
            queued_at_epoch_seconds: row
                .try_get::<DateTime<Utc>, _>("created_at")
                .ok()
                .map(|t| t.timestamp()),
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

    async fn create_tool_call(
        &self,
        create: IntelligenceToolCallCreate,
    ) -> Result<Uuid> {
        let pool = self.pool();
        let id = create.tool_call_id.unwrap_or_else(Uuid::now_v7);
        sqlx::query(
            r#"
            INSERT INTO intelligence_tool_calls (
                id, run_id, sequence, tool_kind, tool_name, status,
                idempotency_key, input_hash, arguments
            )
            VALUES ($1, $2, $3, $4::varchar, $5, 'queued', $6, $7, $8::jsonb)
            "#,
        )
        .bind(id)
        .bind(create.run_id)
        .bind(create.sequence)
        .bind(create.tool_kind.as_db_str())
        .bind(&create.tool_name)
        .bind(&create.idempotency_key)
        .bind(&create.input_hash)
        .bind(&create.arguments)
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

        sqlx::query(
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
        )
        .bind(tool_call_id)
        .bind(status)
        .bind(&update.output_hash)
        .bind(update.result.as_ref())
        .bind(&update.error_excerpt)
        .bind(started_at)
        .bind(finished_at)
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
        let rows = sqlx::query(
            r#"
            SELECT id, run_id, sequence, tool_kind::text AS tool_kind,
                   tool_name, status::text AS status, idempotency_key,
                   input_hash, output_hash, started_at, finished_at,
                   created_at, updated_at
            FROM intelligence_tool_calls
            WHERE run_id = $1
            ORDER BY sequence, id
            LIMIT $2
            "#,
        )
        .bind(run_id)
        .bind(i64::from(DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT))
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("list tool calls failed: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(IntelligenceToolCallSummary {
                tool_call_id: row.try_get("id").map_err(|e| {
                    internal_err(format!("decode tool_call id: {e}"))
                })?,
                run_id: row
                    .try_get("run_id")
                    .map_err(|e| internal_err(format!("decode run_id: {e}")))?,
                sequence: row.try_get("sequence").map_err(|e| {
                    internal_err(format!("decode sequence: {e}"))
                })?,
                tool_kind: tool_kind_from_db(
                    &row.try_get::<String, _>("tool_kind").map_err(|e| {
                        internal_err(format!("decode tool_kind: {e}"))
                    })?,
                ),
                tool_name: row.try_get("tool_name").map_err(|e| {
                    internal_err(format!("decode tool_name: {e}"))
                })?,
                status: match row
                    .try_get::<String, _>("status")
                    .map_err(|e| internal_err(format!("decode status: {e}")))?
                    .as_str()
                {
                    "running" => ToolStatusInternal::Running,
                    "succeeded" => ToolStatusInternal::Succeeded,
                    "failed" => ToolStatusInternal::Failed,
                    "skipped" => ToolStatusInternal::Skipped,
                    "cancelled" => ToolStatusInternal::Cancelled,
                    _ => ToolStatusInternal::Queued,
                },
                idempotency_key: row
                    .try_get::<Option<String>, _>("idempotency_key")
                    .ok()
                    .flatten(),
                input_hash: row
                    .try_get::<Option<String>, _>("input_hash")
                    .ok()
                    .flatten(),
                output_hash: row
                    .try_get::<Option<String>, _>("output_hash")
                    .ok()
                    .flatten(),
                started_at: row
                    .try_get::<Option<DateTime<Utc>>, _>("started_at")
                    .ok()
                    .flatten(),
                finished_at: row
                    .try_get::<Option<DateTime<Utc>>, _>("finished_at")
                    .ok()
                    .flatten(),
                created_at: row.try_get("created_at").map_err(|e| {
                    internal_err(format!("decode created_at: {e}"))
                })?,
                updated_at: row.try_get("updated_at").map_err(|e| {
                    internal_err(format!("decode updated_at: {e}"))
                })?,
            });
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Free helper functions for facets, counts, related items, and artifacts
// ---------------------------------------------------------------------------

async fn current_source_revision(pool: &PgPool) -> Result<i64> {
    let max_ctx: Option<i64> = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(source_revision), 0) FROM intelligence_media_context",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("source revision lookup failed: {e}")))?;
    Ok(max_ctx.unwrap_or(0) + 1)
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
    let movies: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT count(*)
        FROM movie_references mr
        JOIN media_files mf ON mr.file_id = mf.id
            AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
        WHERE mr.library_id = $1
        "#,
    )
    .bind(library_id.0)
    .fetch_one(pool)
    .await
    .map_err(|e| internal_err(format!("count movies: {e}")))?;

    let series_eps: (i64, i64, i64) = sqlx::query_as::<_, (i64, i64, i64)>(
        r#"
        SELECT
          count(DISTINCT er.series_id)::bigint AS series,
          count(DISTINCT er.season_id)::bigint AS seasons,
          count(*)::bigint AS episodes
        FROM episode_references er
        JOIN series s ON er.series_id = s.id AND s.library_id = $1
        JOIN media_files mf ON er.file_id = mf.id
            AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
        "#,
    )
    .bind(library_id.0)
    .fetch_one(pool)
    .await
    .map_err(|e| internal_err(format!("count tv: {e}")))?;

    let artifacts: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT count(*) FROM intelligence_artifacts
        WHERE library_id = $1
          AND status = 'active'
          AND invalidated_at IS NULL
          AND (user_id IS NULL OR user_id = $2)
        "#,
    )
    .bind(library_id.0)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(|e| internal_err(format!("count artifacts: {e}")))?;

    Ok(IntelligenceMediaCounts {
        movies: movies as u64,
        series: series_eps.0 as u64,
        seasons: series_eps.1 as u64,
        episodes: series_eps.2 as u64,
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
    let movie_rows = sqlx::query(
        r#"
        SELECT g.name AS label, count(DISTINCT mr.id)::bigint AS cnt
        FROM movie_references mr
        JOIN media_files mf ON mr.file_id = mf.id
            AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
        JOIN movie_genres g ON g.movie_id = mr.id
        WHERE mr.library_id = $1
        GROUP BY g.name
        "#,
    )
    .bind(library_id.0)
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("movie genre facet: {e}")))?;

    let series_rows = sqlx::query(
        r#"
        SELECT g.name AS label, count(DISTINCT sg.id)::bigint AS cnt
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
    )
    .bind(library_id.0)
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("series genre facet: {e}")))?;

    let mut counts: std::collections::BTreeMap<String, i64> =
        std::collections::BTreeMap::new();
    for row in movie_rows.iter().chain(series_rows.iter()) {
        let label: String = row
            .try_get("label")
            .map_err(|e| internal_err(format!("decode genre label: {e}")))?;
        let cnt: i64 = row
            .try_get("cnt")
            .map_err(|e| internal_err(format!("decode genre count: {e}")))?;
        *counts.entry(label).or_insert(0) += cnt;
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
    let rows = sqlx::query(
        r#"
        SELECT decade, count(*)::bigint AS cnt FROM (
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
    )
    .bind(library_id.0)
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
            let decade: i32 = row
                .try_get("decade")
                .map_err(|e| internal_err(format!("decode decade: {e}")))?;
            let cnt: i64 = row.try_get("cnt").map_err(|e| {
                internal_err(format!("decode decade count: {e}"))
            })?;
            Ok::<_, MediaError>(IntelligenceFacetValue {
                key: decade.to_string(),
                label: format!("{}s", decade),
                count: cnt as u64,
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
    let rows = sqlx::query(
        r#"
        SELECT rating, count(*)::bigint AS cnt FROM (
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
    )
    .bind(library_id.0)
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
            let rating: String = row
                .try_get("rating")
                .map_err(|e| internal_err(format!("decode rating: {e}")))?;
            let cnt: i64 = row.try_get("cnt").map_err(|e| {
                internal_err(format!("decode rating count: {e}"))
            })?;
            Ok::<_, MediaError>(IntelligenceFacetValue {
                key: rating.to_lowercase(),
                label: rating,
                count: cnt as u64,
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
    let rows = sqlx::query(
        r#"
        SELECT bucket, count(*)::bigint AS cnt FROM (
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
    )
    .bind(library_id.0)
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
            let bucket: String = row
                .try_get("bucket")
                .map_err(|e| internal_err(format!("decode bucket: {e}")))?;
            let cnt: i64 = row.try_get("cnt").map_err(|e| {
                internal_err(format!("decode bucket count: {e}"))
            })?;
            Ok::<_, MediaError>(IntelligenceFacetValue {
                key: bucket.clone(),
                label: bucket.to_title_case(),
                count: cnt as u64,
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
    let rows = sqlx::query(
        r#"
        SELECT
          count(*) FILTER (WHERE uwp.position / NULLIF(uwp.duration, 0) >= 0.95)::bigint AS completed,
          count(*) FILTER (WHERE uwp.position / NULLIF(uwp.duration, 0) > 0 AND uwp.position / NULLIF(uwp.duration, 0) < 0.95)::bigint AS in_progress,
          count(*) FILTER (WHERE uwp.position = 0)::bigint AS started
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
    )
    .bind(user_id)
    .bind(library_id.0)
    .fetch_optional(pool)
    .await
    .map_err(|e| internal_err(format!("watch state facet: {e}")))?;
    let Some(row) = rows else { return Ok(None) };
    let completed: i64 = row.try_get("completed").unwrap_or(0);
    let in_progress: i64 = row.try_get("in_progress").unwrap_or(0);
    let started: i64 = row.try_get("started").unwrap_or(0);
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
    let rows = sqlx::query(
        r#"
        SELECT id FROM intelligence_artifacts
        WHERE library_id = $1
          AND status = 'active'
          AND invalidated_at IS NULL
          AND (user_id IS NULL OR user_id = $2)
        ORDER BY updated_at DESC, id
        LIMIT $3
        "#,
    )
    .bind(library_id.0)
    .bind(user_id)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("library artifact ids: {e}")))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(
            row.try_get::<Uuid, _>("id").map_err(|e| {
                internal_err(format!("decode artifact id: {e}"))
            })?,
        );
    }
    Ok(out)
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
            let rows = sqlx::query(
                r#"
            SELECT er.id, er.season_number, er.episode_number, em.name AS title
            FROM episode_references er
            LEFT JOIN episode_metadata em ON em.episode_id = er.id
            WHERE er.series_id = $1 AND er.id <> $2
            ORDER BY er.season_number, er.episode_number, er.id
            LIMIT $3
            "#,
            )
            .bind(series_id)
            .bind(media_row.media_id.as_uuid())
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(|e| internal_err(format!("same-series relatives: {e}")))?;
            for row in rows {
                let id: Uuid = row.try_get("id").map_err(|e| {
                    internal_err(format!("decode relative id: {e}"))
                })?;
                if !seen.insert(id) {
                    continue;
                }
                let season_number: i16 =
                    row.try_get("season_number").map_err(|e| {
                        internal_err(format!("decode season_number: {e}"))
                    })?;
                let episode_number: i16 =
                    row.try_get("episode_number").map_err(|e| {
                        internal_err(format!("decode episode_number: {e}"))
                    })?;
                let title: Option<String> =
                    row.try_get::<Option<String>, _>("title").ok().flatten();
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
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT mr.id, mr.title, mm.release_date, mm.primary_poster_image_id
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
                AND mf.is_available = TRUE AND mf.tombstoned_at IS NULL
            LEFT JOIN movie_metadata mm ON mm.movie_id = mr.id
            JOIN movie_genres g ON g.movie_id = mr.id AND g.name = ANY($1)
            WHERE mr.library_id = $2 AND mr.id <> $3
            ORDER BY mr.title, mr.id
            LIMIT $4
            "#,
        )
        .bind(genres)
        .bind(media_row.library_id.0)
        .bind(media_row.media_id.as_uuid())
        .bind(limit - out.len() as i64)
        .fetch_all(pool)
        .await
        .map_err(|e| internal_err(format!("similar-genre relatives: {e}")))?;
        for row in rows {
            let id: Uuid = row.try_get("id").map_err(|e| {
                internal_err(format!("decode relative id: {e}"))
            })?;
            if !seen.insert(id) {
                continue;
            }
            let title: String = row
                .try_get("title")
                .map_err(|e| internal_err(format!("decode title: {e}")))?;
            let release_date: Option<NaiveDate> = row
                .try_get::<Option<NaiveDate>, _>("release_date")
                .ok()
                .flatten();
            let poster: Option<Uuid> = row
                .try_get::<Option<Uuid>, _>("primary_poster_image_id")
                .ok()
                .flatten();
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
        MediaID::Episode(id) => sqlx::query_scalar::<_, Uuid>(
            "SELECT series_id FROM episode_references WHERE id = $1",
        )
        .bind(id.0)
        .fetch_optional(pool)
        .await
        .map_err(|e| internal_err(format!("resolve episode series: {e}"))),
        MediaID::Season(id) => sqlx::query_scalar::<_, Uuid>(
            "SELECT series_id FROM season_references WHERE id = $1",
        )
        .bind(id.0)
        .fetch_optional(pool)
        .await
        .map_err(|e| internal_err(format!("resolve season series: {e}"))),
        _ => Ok(None),
    }
}

/// Build an artifact summary from a database row, fetching the media ref.
async fn artifact_summary_from_row(
    pool: &PgPool,
    row: &sqlx::postgres::PgRow,
    summary_max_chars: u16,
) -> Result<IntelligenceArtifactSummary> {
    let artifact_id: Uuid = row
        .try_get("id")
        .map_err(|e| internal_err(format!("decode artifact id: {e}")))?;
    let kind_str: String = row
        .try_get("artifact_kind")
        .map_err(|e| internal_err(format!("decode artifact_kind: {e}")))?;
    let media_id: Option<Uuid> =
        row.try_get::<Option<Uuid>, _>("media_id").ok().flatten();
    let media_type: Option<String> = row
        .try_get::<Option<String>, _>("media_type")
        .ok()
        .flatten();
    let title: String = row
        .try_get("title")
        .map_err(|e| internal_err(format!("decode artifact title: {e}")))?;
    let summary: Option<String> =
        row.try_get::<Option<String>, _>("summary").ok().flatten();
    let created_at: DateTime<Utc> = row
        .try_get("created_at")
        .map_err(|e| internal_err(format!("decode created_at: {e}")))?;
    let updated_at: DateTime<Utc> = row
        .try_get("updated_at")
        .map_err(|e| internal_err(format!("decode updated_at: {e}")))?;
    let library_id: Option<Uuid> =
        row.try_get::<Option<Uuid>, _>("library_id").ok().flatten();

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
    let source_rows = sqlx::query(
        r#"
        SELECT source_kind::text AS source_kind, source_run_id, source_tool_call_id
        FROM intelligence_artifact_sources
        WHERE artifact_id = $1 AND status = 'active'
        ORDER BY source_ordinal
        "#,
    )
    .bind(artifact_id)
    .fetch_all(pool)
    .await
    .map_err(|e| internal_err(format!("load artifact sources: {e}")))?;
    let mut provenance = Vec::new();
    for s in source_rows {
        let source_kind: String = s
            .try_get("source_kind")
            .map_err(|e| internal_err(format!("decode source_kind: {e}")))?;
        let source_run_id: Option<Uuid> =
            s.try_get::<Option<Uuid>, _>("source_run_id").ok().flatten();
        let source_tool_call_id: Option<Uuid> = s
            .try_get::<Option<Uuid>, _>("source_tool_call_id")
            .ok()
            .flatten();
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
    let rows = sqlx::query(
        r#"
        SELECT id, artifact_kind::text AS artifact_kind, scope::text AS scope,
               library_id, user_id, media_id, media_type::text AS media_type,
               title, summary, created_at, updated_at
        FROM intelligence_artifacts
        WHERE media_id = $1
          AND media_type = $2::media_type
          AND status = 'active'
          AND invalidated_at IS NULL
          AND (user_id IS NULL OR user_id = $3)
        ORDER BY updated_at DESC, id
        LIMIT $4
        "#,
    )
    .bind(media_id.as_uuid())
    .bind(media_type_str(media_id))
    .bind(user_id)
    .bind(i64::from(limit))
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
