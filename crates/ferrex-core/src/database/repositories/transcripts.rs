//! PostgreSQL-backed timed-text corpus repository.
//!
//! SQL uses compile-checked SQLx macros backed by checked-in offline metadata.
//! The migration owns the durable constraints and indexes.

use std::collections::HashSet;

use async_trait::async_trait;
use ferrex_model::{
    EpisodeID, LibraryId, MediaID, MovieID, SeasonID, SeriesID,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    api::types::intelligence::{
        IntelligenceCaps, IntelligenceMediaKind, IntelligenceMediaRef,
        IntelligencePageInfo, IntelligenceSummary, MAX_INTELLIGENCE_PAGE_LIMIT,
        TimedTextSnippet, TimedTextSnippetSearchRequest,
        TimedTextSnippetSearchResponse, TimedTextSourceKind,
        clamp_intelligence_summary_chars, clamp_timed_text_segment_limit,
        clamp_timed_text_snippet_chars, clamp_timed_text_snippet_limit,
    },
    database::repository_ports::transcripts::{
        TranscriptProcessingState, TranscriptProcessingStatusSummary,
        TranscriptProcessingStatusUpdate, TranscriptRepository,
        TranscriptSegmentUpsert, TranscriptSourceStatus,
        TranscriptSourceStatusFilter, TranscriptSourceStatusSummary,
        TranscriptSourceUpsert, TranscriptSourceUpsertResult,
        TranscriptStatusFilter,
    },
    error::{MediaError, Result},
};

#[derive(Clone, Debug)]
pub struct PostgresTranscriptRepository {
    pool: PgPool,
}

impl PostgresTranscriptRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn snippet_context_segments(
        &self,
        source_id: Uuid,
        cue_index: i32,
        segment_limit: usize,
    ) -> Result<Vec<SnippetContextSegment>> {
        let before = i32::try_from(segment_limit.saturating_sub(1) / 2)
            .unwrap_or_default();
        let start_cue_index = cue_index.saturating_sub(before).max(0);
        let rows = sqlx::query!(
            r#"
            SELECT id AS "segment_id!", start_ms, end_ms, cue_text
            FROM transcript_segments
            WHERE transcript_source_id = $1
              AND cue_index >= $2
              AND status = 'active'
              AND invalidated_at IS NULL
              AND purged_at IS NULL
            ORDER BY cue_index ASC
            LIMIT $3
            "#,
            source_id,
            start_cue_index,
            i64::try_from(segment_limit).unwrap_or(i64::MAX),
        )
        .fetch_all(self.pool())
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| SnippetContextSegment {
                segment_id: row.segment_id,
                start_ms: row.start_ms,
                end_ms: row.end_ms,
                cue_text: row.cue_text,
            })
            .collect())
    }
}

fn media_type_str(media_id: &MediaID) -> &'static str {
    match media_id {
        MediaID::Movie(_) => "movie",
        MediaID::Series(_) => "series",
        MediaID::Season(_) => "season",
        MediaID::Episode(_) => "episode",
    }
}

fn playable_media_type_str(media_id: &MediaID) -> Result<&'static str> {
    match media_id {
        MediaID::Movie(_) => Ok("movie"),
        MediaID::Episode(_) => Ok("episode"),
        MediaID::Series(_) | MediaID::Season(_) => Err(MediaError::InvalidMedia(
            "transcripts are keyed by playable movie or episode media files"
                .to_string(),
        )),
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

fn media_kind_to_db(kind: IntelligenceMediaKind) -> &'static str {
    match kind {
        IntelligenceMediaKind::Movie => "movie",
        IntelligenceMediaKind::Series => "series",
        IntelligenceMediaKind::Season => "season",
        IntelligenceMediaKind::Episode => "episode",
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

fn timed_text_cursor_offset(cursor: Option<&str>) -> Result<i64> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty())
    else {
        return Ok(0);
    };
    let cursor = cursor.strip_prefix("tt:").unwrap_or(cursor);
    let offset = cursor.parse::<i64>().map_err(|_| {
        MediaError::InvalidMedia(
            "timed-text pagination cursor is invalid".to_string(),
        )
    })?;
    if offset < 0 {
        return Err(MediaError::InvalidMedia(
            "timed-text pagination cursor must be non-negative".to_string(),
        ));
    }
    Ok(offset)
}

fn timed_text_cursor(offset: i64) -> String {
    format!("tt:{offset}")
}

fn effective_snippet_caps(caps: IntelligenceCaps) -> IntelligenceCaps {
    let summary_max_chars =
        clamp_intelligence_summary_chars(caps.summary_max_chars);
    let timed_text_snippet_max_chars =
        clamp_timed_text_snippet_chars(caps.timed_text_snippet_max_chars)
            .min(summary_max_chars);

    IntelligenceCaps {
        candidate_limit: caps.candidate_limit,
        artifact_limit: caps.artifact_limit,
        related_limit: caps.related_limit,
        facet_limit: caps.facet_limit,
        grounding_limit: caps.grounding_limit,
        tool_call_limit: caps.tool_call_limit,
        summary_max_chars,
        timed_text_snippet_limit: clamp_timed_text_snippet_limit(
            caps.timed_text_snippet_limit,
        ),
        timed_text_segment_limit: clamp_timed_text_segment_limit(
            caps.timed_text_segment_limit,
        ),
        timed_text_snippet_max_chars,
    }
}

#[derive(Debug, Clone)]
struct SnippetMatchRow {
    segment_id: Uuid,
    source_id: Uuid,
    library_id: LibraryId,
    media_id: MediaID,
    title: String,
    artifact_id: Option<Uuid>,
    source_kind: TimedTextSourceKind,
    language_code: String,
    cue_index: i32,
    score: f32,
}

#[derive(Debug, Clone)]
struct SnippetContextSegment {
    segment_id: Uuid,
    start_ms: i64,
    end_ms: i64,
    cue_text: String,
}

fn content_hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\x1f");
    }
    hex::encode(hasher.finalize())
}

fn bounded_excerpt(value: Option<String>) -> Option<String> {
    value.map(|message| message.chars().take(2048).collect())
}

fn validate_hash(name: &str, value: &str) -> Result<()> {
    if value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(MediaError::InvalidMedia(format!(
            "{name} must be a sha-256 hex digest"
        )))
    }
}

fn ensure_json_object(name: &str, value: &Value) -> Result<()> {
    if value.is_object() {
        Ok(())
    } else {
        Err(MediaError::InvalidMedia(format!(
            "{name} must be a JSON object"
        )))
    }
}

fn validate_source(source: &TranscriptSourceUpsert) -> Result<()> {
    playable_media_type_str(&source.media_id)?;
    if source.source_key.trim().is_empty() {
        return Err(MediaError::InvalidMedia(
            "transcript source_key must not be empty".to_string(),
        ));
    }
    validate_hash("source_content_hash", &source.source_content_hash)?;
    if let Some(hash) = source.normalized_content_hash.as_deref() {
        validate_hash("normalized_content_hash", hash)?;
    }
    if let Some(hash) = source.source_path_hash.as_deref() {
        validate_hash("source_path_hash", hash)?;
    }

    match source.source_kind {
        TimedTextSourceKind::Embedded if source.stream_index.is_none() => {
            Err(MediaError::InvalidMedia(
                "embedded transcript sources require stream_index".to_string(),
            ))
        }
        TimedTextSourceKind::Sidecar if source.source_path_hash.is_none() => {
            Err(MediaError::InvalidMedia(
                "sidecar transcript sources require source_path_hash"
                    .to_string(),
            ))
        }
        _ => Ok(()),
    }?;

    ensure_json_object("source_locator", &source.source_locator)?;
    ensure_json_object("metadata", &source.metadata)?;
    Ok(())
}

fn validate_segment(segment: &TranscriptSegmentUpsert) -> Result<()> {
    if segment.cue_index < 0 {
        return Err(MediaError::InvalidMedia(
            "transcript cue_index must be non-negative".to_string(),
        ));
    }
    if segment.start_ms < 0 || segment.end_ms <= segment.start_ms {
        return Err(MediaError::InvalidMedia(
            "transcript segment times must be non-negative and increasing"
                .to_string(),
        ));
    }
    let text_chars = segment.text.chars().count();
    if text_chars == 0 || text_chars > 4000 {
        return Err(MediaError::InvalidMedia(
            "transcript cue text must contain 1..=4000 characters".to_string(),
        ));
    }
    ensure_json_object("segment metadata", &segment.metadata)?;
    Ok(())
}

fn transcript_job_dedupe_key(
    library_id: LibraryId,
    media_file_id: Uuid,
) -> String {
    format!("transcript:{}:{}", library_id, media_file_id)
}

async fn media_file_ids_for_media(
    executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    library_id: LibraryId,
    media_uuid: Uuid,
    media_type: &str,
) -> Result<Vec<Uuid>> {
    let rows = sqlx::query!(
        r#"
        SELECT id
        FROM media_files
        WHERE library_id = $1
          AND media_id = $2
          AND media_type = ($3::text)::media_type
        "#,
        library_id.0,
        media_uuid,
        media_type,
    )
    .fetch_all(&mut **executor)
    .await?;

    Ok(rows.into_iter().map(|row| row.id).collect())
}

async fn delete_pending_transcript_jobs(
    executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    library_id: LibraryId,
    media_file_ids: &[Uuid],
) -> Result<u64> {
    if media_file_ids.is_empty() {
        return Ok(0);
    }

    let dedupe_keys: Vec<String> = media_file_ids
        .iter()
        .map(|media_file_id| {
            transcript_job_dedupe_key(library_id, *media_file_id)
        })
        .collect();

    let result = sqlx::query!(
        r#"
        DELETE FROM orchestrator_jobs
        WHERE kind = 7
          AND state IN ('ready', 'deferred')
          AND dedupe_key = ANY($1::text[])
        "#,
        &dedupe_keys,
    )
    .execute(&mut **executor)
    .await?;

    Ok(result.rows_affected())
}

async fn fetch_counts(
    executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    library_id: LibraryId,
    media_file_id: Uuid,
) -> Result<(i64, i64)> {
    let source_count = sqlx::query_scalar!(
        r#"
        SELECT count(*) AS "count!"
        FROM transcript_sources
        WHERE library_id = $1
          AND media_file_id = $2
          AND status = 'active'
          AND invalidated_at IS NULL
          AND purged_at IS NULL
        "#,
        library_id.0,
        media_file_id,
    )
    .fetch_one(&mut **executor)
    .await?;

    let segment_count = sqlx::query_scalar!(
        r#"
        SELECT count(*) AS "count!"
        FROM transcript_segments
        WHERE library_id = $1
          AND media_file_id = $2
          AND status = 'active'
          AND invalidated_at IS NULL
          AND purged_at IS NULL
        "#,
        library_id.0,
        media_file_id,
    )
    .fetch_one(&mut **executor)
    .await?;

    Ok((source_count, segment_count))
}

async fn mark_status(
    executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    library_id: LibraryId,
    media_id: MediaID,
    media_file_id: Uuid,
    status: TranscriptProcessingState,
    source_count: i64,
    segment_count: i64,
    reason: Option<&str>,
) -> Result<()> {
    let media_type = playable_media_type_str(&media_id)?;
    let status_str = status.as_db_str();
    let is_invalidated = status == TranscriptProcessingState::Invalidated;
    let is_purged = status == TranscriptProcessingState::Purged;

    sqlx::query!(
        r#"
        INSERT INTO transcript_processing_status (
            library_id,
            media_id,
            media_type,
            media_file_id,
            status,
            source_count,
            segment_count,
            finished_at,
            invalidated_at,
            invalidation_reason,
            purged_at,
            purge_reason
        ) VALUES (
            $1,
            $2,
            ($3::text)::media_type,
            $4,
            $5,
            $6,
            $7,
            now(),
            CASE WHEN $8 THEN now() ELSE NULL END,
            CASE WHEN $8 THEN $10 ELSE NULL END,
            CASE WHEN $9 THEN now() ELSE NULL END,
            CASE WHEN $9 THEN $10 ELSE NULL END
        )
        ON CONFLICT (library_id, media_file_id) DO UPDATE SET
            media_id = EXCLUDED.media_id,
            media_type = EXCLUDED.media_type,
            status = EXCLUDED.status,
            source_count = EXCLUDED.source_count,
            segment_count = EXCLUDED.segment_count,
            last_error_excerpt = NULL,
            finished_at = now(),
            invalidated_at = EXCLUDED.invalidated_at,
            invalidation_reason = EXCLUDED.invalidation_reason,
            purged_at = EXCLUDED.purged_at,
            purge_reason = EXCLUDED.purge_reason,
            updated_at = now()
        "#,
        library_id.0,
        *media_id.as_uuid(),
        media_type,
        media_file_id,
        status_str,
        i32::try_from(source_count).unwrap_or(i32::MAX),
        i32::try_from(segment_count).unwrap_or(i32::MAX),
        is_invalidated,
        is_purged,
        reason,
    )
    .execute(&mut **executor)
    .await?;

    Ok(())
}

struct SourceIdentity {
    library_id: LibraryId,
    media_id: MediaID,
    media_file_id: Uuid,
    artifact_id: Option<Uuid>,
}

async fn fetch_source_identity(
    pool: &PgPool,
    source_id: Uuid,
) -> Result<SourceIdentity> {
    let row = sqlx::query!(
        r#"
        SELECT library_id, media_id, media_type::text AS "media_type!",
               media_file_id, artifact_id
        FROM transcript_sources
        WHERE id = $1
        "#,
        source_id,
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Err(MediaError::NotFound(format!(
            "transcript source {source_id} not found"
        )));
    };

    let library_id = LibraryId(row.library_id);
    let media_id = media_id_from_parts(&row.media_type, row.media_id);
    Ok(SourceIdentity {
        library_id,
        media_id,
        media_file_id: row.media_file_id,
        artifact_id: row.artifact_id,
    })
}

#[async_trait]
impl TranscriptRepository for PostgresTranscriptRepository {
    async fn upsert_source_with_segments(
        &self,
        source: TranscriptSourceUpsert,
        segments: Vec<TranscriptSegmentUpsert>,
    ) -> Result<TranscriptSourceUpsertResult> {
        validate_source(&source)?;
        for segment in &segments {
            validate_segment(segment)?;
        }

        let media_type = playable_media_type_str(&source.media_id)?;
        let media_uuid = *source.media_id.as_uuid();
        let source_kind = source.source_kind.as_db_str();
        let language_code = source.language_code.trim().to_string();
        let mut tx = self.pool().begin().await?;

        let row = sqlx::query!(
            r#"
            INSERT INTO transcript_sources (
                id,
                library_id,
                media_id,
                media_type,
                media_file_id,
                source_kind,
                status,
                language_code,
                source_key,
                source_name,
                stream_index,
                source_path_hash,
                source_content_hash,
                normalized_content_hash,
                artifact_id,
                duration_ms,
                extracted_at,
                source_locator,
                metadata
            ) VALUES (
                COALESCE($1::uuid, uuidv7()),
                $2,
                $3,
                ($4::text)::media_type,
                $5,
                $6,
                'active',
                $7,
                $8,
                $9,
                $10,
                $11,
                $12,
                $13,
                $14,
                $15,
                now(),
                $16,
                $17
            )
            ON CONFLICT (library_id, media_file_id, source_kind, language_code, source_key)
            DO UPDATE SET
                media_id = EXCLUDED.media_id,
                media_type = EXCLUDED.media_type,
                source_name = EXCLUDED.source_name,
                stream_index = EXCLUDED.stream_index,
                source_path_hash = EXCLUDED.source_path_hash,
                source_content_hash = EXCLUDED.source_content_hash,
                normalized_content_hash = EXCLUDED.normalized_content_hash,
                artifact_id = EXCLUDED.artifact_id,
                duration_ms = EXCLUDED.duration_ms,
                status = 'active',
                segment_count = 0,
                extracted_at = now(),
                invalidated_at = NULL,
                invalidation_reason = NULL,
                purged_at = NULL,
                purge_reason = NULL,
                source_locator = EXCLUDED.source_locator,
                metadata = EXCLUDED.metadata,
                updated_at = now()
            RETURNING id AS "id!"
            "#,
            source.source_id,
            source.library_id.0,
            media_uuid,
            media_type,
            source.media_file_id,
            source_kind,
            &language_code,
            source.source_key.trim(),
            source.source_name.as_deref(),
            source.stream_index,
            source.source_path_hash.as_deref(),
            &source.source_content_hash,
            source.normalized_content_hash.as_deref(),
            source.artifact_id,
            source.duration_ms,
            &source.source_locator,
            &source.metadata,
        )
        .fetch_one(&mut *tx)
        .await?;

        let source_id: Uuid = row.id;

        sqlx::query!(
            "DELETE FROM transcript_segments WHERE transcript_source_id = $1",
            source_id,
        )
        .execute(&mut *tx)
        .await?;

        for segment in &segments {
            let cue_index = segment.cue_index.to_string();
            let start_ms = segment.start_ms.to_string();
            let end_ms = segment.end_ms.to_string();
            let segment_hash = content_hash(&[
                &cue_index,
                &start_ms,
                &end_ms,
                segment.text.as_str(),
            ]);

            sqlx::query!(
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
                    segment_hash,
                    status,
                    metadata
                ) VALUES (
                    $1,
                    $2,
                    $3,
                    ($4::text)::media_type,
                    $5,
                    $6,
                    $7,
                    $8,
                    $9,
                    $10,
                    $11,
                    'active',
                    $12
                )
                "#,
                source_id,
                source.library_id.0,
                media_uuid,
                media_type,
                source.media_file_id,
                &language_code,
                segment.cue_index,
                segment.start_ms,
                segment.end_ms,
                &segment.text,
                segment_hash,
                &segment.metadata,
            )
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query!(
            r#"
            UPDATE transcript_sources
            SET segment_count = $2,
                status = 'active',
                updated_at = now()
            WHERE id = $1
            "#,
            source_id,
            i32::try_from(segments.len()).unwrap_or(i32::MAX),
        )
        .execute(&mut *tx)
        .await?;

        let (source_count, segment_count) =
            fetch_counts(&mut tx, source.library_id, source.media_file_id)
                .await?;
        mark_status(
            &mut tx,
            source.library_id,
            source.media_id,
            source.media_file_id,
            TranscriptProcessingState::Succeeded,
            source_count,
            segment_count,
            None,
        )
        .await?;

        tx.commit().await?;

        Ok(TranscriptSourceUpsertResult {
            source_id,
            segment_count: segments.len() as u64,
            source_content_hash: source.source_content_hash,
        })
    }

    async fn list_source_status(
        &self,
        filter: TranscriptSourceStatusFilter,
    ) -> Result<Vec<TranscriptSourceStatusSummary>> {
        let (media_uuid, media_type) = match filter.media_id {
            Some(media_id) => {
                (Some(*media_id.as_uuid()), Some(media_type_str(&media_id)))
            }
            None => (None, None),
        };
        let status = filter.status.map(TranscriptSourceStatus::as_db_str);
        let limit = i64::from(clamp_limit(filter.limit, 50, 200));

        let rows = sqlx::query!(
            r#"
            SELECT id,
                   library_id,
                   media_id,
                   media_type::text AS "media_type!",
                   media_file_id,
                   source_kind::text AS "source_kind!",
                   status::text AS "status!",
                   language_code,
                   source_name,
                   artifact_id,
                   segment_count,
                   duration_ms,
                   invalidated_at,
                   purged_at,
                   updated_at
            FROM transcript_sources
            WHERE ($1::uuid IS NULL OR library_id = $1)
              AND ($2::uuid IS NULL OR media_file_id = $2)
              AND ($3::uuid IS NULL OR media_id = $3)
              AND ($4::text IS NULL OR media_type::text = $4)
              AND ($5::text IS NULL OR status::text = $5)
            ORDER BY updated_at DESC, id
            LIMIT $6
            "#,
            filter.library_id.map(|id| id.0),
            filter.media_file_id,
            media_uuid,
            media_type,
            status,
            limit,
        )
        .fetch_all(self.pool())
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(TranscriptSourceStatusSummary {
                source_id: row.id,
                library_id: LibraryId(row.library_id),
                media_id: media_id_from_parts(&row.media_type, row.media_id),
                media_file_id: row.media_file_id,
                source_kind: TimedTextSourceKind::from_db_str(&row.source_kind),
                status: TranscriptSourceStatus::from_db_str(&row.status),
                language_code: row.language_code,
                source_name: row.source_name,
                artifact_id: row.artifact_id,
                segment_count: row.segment_count,
                duration_ms: row.duration_ms,
                invalidated_at: row.invalidated_at,
                purged_at: row.purged_at,
                updated_at: row.updated_at,
            });
        }
        Ok(out)
    }

    async fn list_processing_status(
        &self,
        filter: TranscriptStatusFilter,
    ) -> Result<Vec<TranscriptProcessingStatusSummary>> {
        let (media_uuid, media_type) = match filter.media_id {
            Some(media_id) => {
                (Some(*media_id.as_uuid()), Some(media_type_str(&media_id)))
            }
            None => (None, None),
        };
        let status = filter.status.map(TranscriptProcessingState::as_db_str);
        let limit = i64::from(clamp_limit(filter.limit, 50, 200));

        let rows = sqlx::query!(
            r#"
            SELECT id,
                   library_id,
                   media_id,
                   media_type::text AS "media_type!",
                   media_file_id,
                   status::text AS "status!",
                   source_count,
                   segment_count,
                   attempt_count,
                   last_error_excerpt,
                   next_retry_at,
                   last_run_correlation_id,
                   invalidated_at,
                   purged_at,
                   updated_at
            FROM transcript_processing_status
            WHERE ($1::uuid IS NULL OR library_id = $1)
              AND ($2::uuid IS NULL OR media_file_id = $2)
              AND ($3::uuid IS NULL OR media_id = $3)
              AND ($4::text IS NULL OR media_type::text = $4)
              AND ($5::text IS NULL OR status::text = $5)
            ORDER BY updated_at DESC, id
            LIMIT $6
            "#,
            filter.library_id.map(|id| id.0),
            filter.media_file_id,
            media_uuid,
            media_type,
            status,
            limit,
        )
        .fetch_all(self.pool())
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(TranscriptProcessingStatusSummary {
                status_id: row.id,
                library_id: LibraryId(row.library_id),
                media_id: media_id_from_parts(&row.media_type, row.media_id),
                media_file_id: row.media_file_id,
                status: TranscriptProcessingState::from_db_str(&row.status),
                source_count: row.source_count,
                segment_count: row.segment_count,
                attempt_count: row.attempt_count,
                last_error_excerpt: row.last_error_excerpt,
                next_retry_at: row.next_retry_at,
                last_run_correlation_id: row.last_run_correlation_id,
                invalidated_at: row.invalidated_at,
                purged_at: row.purged_at,
                updated_at: row.updated_at,
            });
        }
        Ok(out)
    }

    async fn update_processing_status(
        &self,
        update: TranscriptProcessingStatusUpdate,
    ) -> Result<()> {
        let media_type = playable_media_type_str(&update.media_id)?;
        let status = update.status.as_db_str();
        let terminal = matches!(
            update.status,
            TranscriptProcessingState::Succeeded
                | TranscriptProcessingState::Failed
                | TranscriptProcessingState::Skipped
                | TranscriptProcessingState::Cancelled
                | TranscriptProcessingState::Invalidated
                | TranscriptProcessingState::Purged
        );
        let running = update.status == TranscriptProcessingState::Running;
        let invalidated =
            update.status == TranscriptProcessingState::Invalidated;
        let purged = update.status == TranscriptProcessingState::Purged;
        let error_excerpt = bounded_excerpt(update.last_error_excerpt);
        let max_attempts = update.max_attempts.unwrap_or(3).max(0);

        sqlx::query!(
            r#"
            INSERT INTO transcript_processing_status (
                library_id,
                media_id,
                media_type,
                media_file_id,
                status,
                source_count,
                segment_count,
                attempt_count,
                max_attempts,
                last_error_excerpt,
                last_run_correlation_id,
                next_retry_at,
                started_at,
                finished_at,
                invalidated_at,
                purged_at
            ) VALUES (
                $1,
                $2,
                ($3::text)::media_type,
                $4,
                $5,
                $6,
                $7,
                $8,
                $9,
                $10,
                $11,
                $12,
                CASE WHEN $13 THEN now() ELSE NULL END,
                CASE WHEN $14 THEN now() ELSE NULL END,
                CASE WHEN $15 THEN now() ELSE NULL END,
                CASE WHEN $16 THEN now() ELSE NULL END
            )
            ON CONFLICT (library_id, media_file_id) DO UPDATE SET
                media_id = EXCLUDED.media_id,
                media_type = EXCLUDED.media_type,
                status = EXCLUDED.status,
                source_count = EXCLUDED.source_count,
                segment_count = EXCLUDED.segment_count,
                attempt_count = EXCLUDED.attempt_count,
                max_attempts = EXCLUDED.max_attempts,
                last_error_excerpt = EXCLUDED.last_error_excerpt,
                last_run_correlation_id = COALESCE(
                    EXCLUDED.last_run_correlation_id,
                    transcript_processing_status.last_run_correlation_id
                ),
                next_retry_at = EXCLUDED.next_retry_at,
                started_at = CASE
                    WHEN $13 THEN COALESCE(transcript_processing_status.started_at, now())
                    ELSE NULL
                END,
                finished_at = CASE WHEN $14 THEN now() ELSE NULL END,
                invalidated_at = CASE WHEN $15 THEN now() ELSE NULL END,
                purged_at = CASE WHEN $16 THEN now() ELSE NULL END,
                updated_at = now()
            "#,
            update.library_id.0,
            *update.media_id.as_uuid(),
            media_type,
            update.media_file_id,
            status,
            update.source_count.max(0),
            update.segment_count.max(0),
            update.attempt_count.max(0),
            max_attempts,
            error_excerpt,
            update.last_run_correlation_id,
            update.next_retry_at,
            running,
            terminal,
            invalidated,
            purged,
        )
        .execute(self.pool())
        .await?;

        Ok(())
    }

    async fn invalidate_media(
        &self,
        library_id: LibraryId,
        media_id: MediaID,
        reason: &str,
    ) -> Result<u64> {
        let media_type = playable_media_type_str(&media_id)?;
        let media_uuid = *media_id.as_uuid();
        let mut tx = self.pool().begin().await?;
        let mut media_file_ids = media_file_ids_for_media(
            &mut tx, library_id, media_uuid, media_type,
        )
        .await?;
        let _ = delete_pending_transcript_jobs(
            &mut tx,
            library_id,
            &media_file_ids,
        )
        .await?;

        let artifact_ids = sqlx::query_scalar!(
            r#"
            SELECT artifact_id AS "artifact_id!"
            FROM transcript_sources
            WHERE library_id = $1
              AND media_id = $2
              AND media_type = ($3::text)::media_type
              AND artifact_id IS NOT NULL
              AND status <> 'purged'
            "#,
            library_id.0,
            media_uuid,
            media_type,
        )
        .fetch_all(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE transcript_segments
            SET status = 'invalidated',
                invalidated_at = now(),
                invalidation_reason = $4,
                updated_at = now()
            WHERE library_id = $1
              AND media_id = $2
              AND media_type = ($3::text)::media_type
              AND purged_at IS NULL
            "#,
            library_id.0,
            media_uuid,
            media_type,
            reason,
        )
        .execute(&mut *tx)
        .await?;

        let affected = sqlx::query!(
            r#"
            UPDATE transcript_sources
            SET status = 'invalidated',
                invalidated_at = now(),
                invalidation_reason = $4,
                updated_at = now()
            WHERE library_id = $1
              AND media_id = $2
              AND media_type = ($3::text)::media_type
              AND status <> 'purged'
            "#,
            library_id.0,
            media_uuid,
            media_type,
            reason,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();

        if !artifact_ids.is_empty() {
            sqlx::query!(
                r#"
                UPDATE intelligence_artifacts
                SET status = 'invalidated',
                    invalidated_at = now(),
                    invalidation_reason = $2,
                    updated_at = now()
                WHERE id = ANY($1::uuid[])
                  AND status <> 'deleted'
                "#,
                &artifact_ids,
                reason,
            )
            .execute(&mut *tx)
            .await?;
        }

        let media_file_rows = sqlx::query!(
            r#"
            SELECT DISTINCT media_file_id AS "media_file_id!"
            FROM transcript_sources
            WHERE library_id = $1
              AND media_id = $2
              AND media_type = ($3::text)::media_type
            "#,
            library_id.0,
            media_uuid,
            media_type,
        )
        .fetch_all(&mut *tx)
        .await?;

        for row in media_file_rows {
            if !media_file_ids.contains(&row.media_file_id) {
                media_file_ids.push(row.media_file_id);
            }
        }

        for media_file_id in media_file_ids {
            let (source_count, segment_count) =
                fetch_counts(&mut tx, library_id, media_file_id).await?;
            mark_status(
                &mut tx,
                library_id,
                media_id,
                media_file_id,
                TranscriptProcessingState::Invalidated,
                source_count,
                segment_count,
                Some(reason),
            )
            .await?;
        }

        tx.commit().await?;
        Ok(affected)
    }

    async fn purge_media(
        &self,
        library_id: LibraryId,
        media_id: MediaID,
        reason: &str,
    ) -> Result<u64> {
        let media_type = playable_media_type_str(&media_id)?;
        let media_uuid = *media_id.as_uuid();
        let mut tx = self.pool().begin().await?;
        let mut media_file_ids = media_file_ids_for_media(
            &mut tx, library_id, media_uuid, media_type,
        )
        .await?;
        let _ = delete_pending_transcript_jobs(
            &mut tx,
            library_id,
            &media_file_ids,
        )
        .await?;

        let rows = sqlx::query!(
            r#"
            SELECT id AS "id!", media_file_id, artifact_id
            FROM transcript_sources
            WHERE library_id = $1
              AND media_id = $2
              AND media_type = ($3::text)::media_type
              AND status <> 'purged'
            "#,
            library_id.0,
            media_uuid,
            media_type,
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut source_ids = Vec::with_capacity(rows.len());
        let mut artifact_ids = Vec::new();
        for row in rows {
            source_ids.push(row.id);
            if !media_file_ids.contains(&row.media_file_id) {
                media_file_ids.push(row.media_file_id);
            }
            if let Some(artifact_id) = row.artifact_id {
                artifact_ids.push(artifact_id);
            }
        }

        if source_ids.is_empty() {
            for media_file_id in media_file_ids {
                mark_status(
                    &mut tx,
                    library_id,
                    media_id,
                    media_file_id,
                    TranscriptProcessingState::Purged,
                    0,
                    0,
                    Some(reason),
                )
                .await?;
            }
            tx.commit().await?;
            return Ok(0);
        }

        sqlx::query!(
            "DELETE FROM transcript_segments WHERE transcript_source_id = ANY($1::uuid[])",
            &source_ids,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE transcript_sources
            SET status = 'purged',
                segment_count = 0,
                purged_at = now(),
                purge_reason = $2,
                updated_at = now()
            WHERE id = ANY($1::uuid[])
            "#,
            &source_ids,
            reason,
        )
        .execute(&mut *tx)
        .await?;

        if !artifact_ids.is_empty() {
            sqlx::query!(
                r#"
                UPDATE intelligence_artifacts
                SET status = 'deleted',
                    summary = NULL,
                    excerpt = NULL,
                    content = '{}'::jsonb,
                    invalidated_at = now(),
                    invalidation_reason = $2,
                    updated_at = now()
                WHERE id = ANY($1::uuid[])
                "#,
                &artifact_ids,
                reason,
            )
            .execute(&mut *tx)
            .await?;
        }

        for media_file_id in media_file_ids {
            mark_status(
                &mut tx,
                library_id,
                media_id,
                media_file_id,
                TranscriptProcessingState::Purged,
                0,
                0,
                Some(reason),
            )
            .await?;
        }

        tx.commit().await?;
        Ok(source_ids.len() as u64)
    }

    async fn invalidate_source(
        &self,
        source_id: Uuid,
        reason: &str,
    ) -> Result<()> {
        let identity = fetch_source_identity(self.pool(), source_id).await?;
        let mut tx = self.pool().begin().await?;

        sqlx::query!(
            r#"
            UPDATE transcript_segments
            SET status = 'invalidated',
                invalidated_at = now(),
                invalidation_reason = $2,
                updated_at = now()
            WHERE transcript_source_id = $1
              AND purged_at IS NULL
            "#,
            source_id,
            reason,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE transcript_sources
            SET status = 'invalidated',
                invalidated_at = now(),
                invalidation_reason = $2,
                updated_at = now()
            WHERE id = $1
              AND status <> 'purged'
            "#,
            source_id,
            reason,
        )
        .execute(&mut *tx)
        .await?;

        if let Some(artifact_id) = identity.artifact_id {
            sqlx::query!(
                r#"
                UPDATE intelligence_artifacts
                SET status = 'invalidated',
                    invalidated_at = now(),
                    invalidation_reason = $2,
                    updated_at = now()
                WHERE id = $1
                  AND status <> 'deleted'
                "#,
                artifact_id,
                reason,
            )
            .execute(&mut *tx)
            .await?;
        }

        let (source_count, segment_count) =
            fetch_counts(&mut tx, identity.library_id, identity.media_file_id)
                .await?;
        mark_status(
            &mut tx,
            identity.library_id,
            identity.media_id,
            identity.media_file_id,
            TranscriptProcessingState::Invalidated,
            source_count,
            segment_count,
            Some(reason),
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn purge_source(&self, source_id: Uuid, reason: &str) -> Result<()> {
        let identity = fetch_source_identity(self.pool(), source_id).await?;
        let mut tx = self.pool().begin().await?;

        sqlx::query!(
            "DELETE FROM transcript_segments WHERE transcript_source_id = $1",
            source_id,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE transcript_sources
            SET status = 'purged',
                segment_count = 0,
                purged_at = now(),
                purge_reason = $2,
                updated_at = now()
            WHERE id = $1
            "#,
            source_id,
            reason,
        )
        .execute(&mut *tx)
        .await?;

        if let Some(artifact_id) = identity.artifact_id {
            sqlx::query!(
                r#"
                UPDATE intelligence_artifacts
                SET status = 'deleted',
                    summary = NULL,
                    excerpt = NULL,
                    content = '{}'::jsonb,
                    invalidated_at = now(),
                    invalidation_reason = $2,
                    updated_at = now()
                WHERE id = $1
                "#,
                artifact_id,
                reason,
            )
            .execute(&mut *tx)
            .await?;
        }

        let (source_count, segment_count) =
            fetch_counts(&mut tx, identity.library_id, identity.media_file_id)
                .await?;
        mark_status(
            &mut tx,
            identity.library_id,
            identity.media_id,
            identity.media_file_id,
            TranscriptProcessingState::Purged,
            source_count,
            segment_count,
            Some(reason),
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn search_snippets(
        &self,
        request: &TimedTextSnippetSearchRequest,
        user_id: Option<Uuid>,
    ) -> Result<TimedTextSnippetSearchResponse> {
        let query = request.query.trim();
        let caps = effective_snippet_caps(request.caps);
        let page_limit = clamp_limit(
            request.pagination.limit,
            caps.timed_text_snippet_limit,
            MAX_INTELLIGENCE_PAGE_LIMIT,
        );
        let limit = page_limit.min(caps.timed_text_snippet_limit);
        let segment_limit = usize::from(caps.timed_text_segment_limit);
        let offset =
            timed_text_cursor_offset(request.pagination.cursor.as_deref())?;

        if query.is_empty() {
            return Ok(TimedTextSnippetSearchResponse {
                snippets: Vec::new(),
                page: IntelligencePageInfo {
                    next_cursor: None,
                    limit,
                    has_more: false,
                },
                caps,
            });
        }

        let library_ids: Vec<Uuid> =
            request.library_ids.iter().map(|id| id.0).collect();
        let media_ids: Vec<Uuid> =
            request.media_ids.iter().map(|id| *id.as_uuid()).collect();
        let media_kinds: Vec<String> = request
            .media_kinds
            .iter()
            .map(|kind| media_kind_to_db(*kind).to_string())
            .collect();
        let language_codes: Vec<String> = request
            .language_codes
            .iter()
            .map(|lang| lang.trim().to_ascii_lowercase())
            .filter(|lang| !lang.is_empty())
            .collect();
        let source_kinds: Vec<String> = request
            .source_kinds
            .iter()
            .map(|kind| kind.as_db_str().to_string())
            .collect();
        let match_window =
            (i64::from(limit) * i64::from(caps.timed_text_segment_limit) * 4)
                .max(i64::from(limit));
        let fetch_limit = match_window.saturating_add(1);

        let rows = sqlx::query!(
            r#"
            WITH query AS (
                SELECT websearch_to_tsquery('simple'::regconfig, $1) AS tsq
            )
            SELECT seg.id AS "segment_id!",
                   seg.library_id,
                   seg.media_id,
                   seg.media_type::text AS "media_type!",
                   seg.cue_index,
                   src.id AS "source_id!",
                   CASE WHEN $7::bool AND ia.id IS NOT NULL THEN ia.id ELSE NULL END AS artifact_id,
                   src.source_kind::text AS "source_kind!",
                   src.language_code,
                   COALESCE(
                       mr.title,
                       em.name,
                       CASE
                           WHEN er.id IS NOT NULL THEN format('S%s E%s', er.season_number, er.episode_number)
                           ELSE 'Untitled media'
                       END
                   ) AS "title!",
                   ts_rank_cd(seg.search_vector, query.tsq) AS "fts_score!",
                   similarity(seg.cue_text::text, $1) AS "trigram_score!"
            FROM transcript_segments seg
            JOIN transcript_sources src ON src.id = seg.transcript_source_id
            CROSS JOIN query
            LEFT JOIN intelligence_artifacts ia
              ON ia.id = src.artifact_id
             AND ia.status = 'active'
             AND ia.invalidated_at IS NULL
             AND (ia.user_id IS NULL OR ia.user_id = $8)
            LEFT JOIN movie_references mr
              ON seg.media_type = 'movie'
             AND mr.id = seg.media_id
             AND mr.library_id = seg.library_id
            LEFT JOIN episode_references er
              ON seg.media_type = 'episode'
             AND er.id = seg.media_id
            LEFT JOIN episode_metadata em
              ON em.episode_id = er.id
            WHERE seg.status = 'active'
              AND seg.invalidated_at IS NULL
              AND seg.purged_at IS NULL
              AND src.status = 'active'
              AND src.invalidated_at IS NULL
              AND src.purged_at IS NULL
              AND (cardinality($2::uuid[]) = 0 OR seg.library_id = ANY($2::uuid[]))
              AND (cardinality($3::uuid[]) = 0 OR seg.media_id = ANY($3::uuid[]))
              AND (cardinality($4::text[]) = 0 OR seg.media_type::text = ANY($4::text[]))
              AND (cardinality($5::text[]) = 0 OR lower(src.language_code) = ANY($5::text[]))
              AND (cardinality($6::text[]) = 0 OR src.source_kind::text = ANY($6::text[]))
              AND (
                  seg.search_vector @@ query.tsq
                  OR seg.cue_text ILIKE ('%' || $1 || '%')
                  OR similarity(seg.cue_text::text, $1) > 0.1
              )
            ORDER BY
                (seg.cue_text ILIKE ('%' || $1 || '%')) DESC,
                ts_rank_cd(seg.search_vector, query.tsq) DESC,
                CASE
                    WHEN seg.search_vector @@ query.tsq
                      OR seg.cue_text ILIKE ('%' || $1 || '%')
                    THEN seg.start_ms
                    ELSE NULL
                END ASC NULLS LAST,
                similarity(seg.cue_text::text, $1) DESC,
                seg.library_id,
                seg.media_type,
                seg.media_id,
                seg.start_ms,
                seg.id
            LIMIT $9 OFFSET $10
            "#,
            query,
            &library_ids,
            &media_ids,
            &media_kinds,
            &language_codes,
            &source_kinds,
            request.include_artifacts,
            user_id,
            fetch_limit,
            offset,
        )
        .fetch_all(self.pool())
        .await?;

        let fetched_has_extra = rows.len() as i64 > match_window;
        let mut matches = Vec::with_capacity(
            rows.len()
                .min(usize::try_from(match_window).unwrap_or(usize::MAX)),
        );
        for row in rows.into_iter().take(match_window as usize) {
            matches.push(SnippetMatchRow {
                segment_id: row.segment_id,
                source_id: row.source_id,
                library_id: LibraryId(row.library_id),
                media_id: media_id_from_parts(&row.media_type, row.media_id),
                title: row.title,
                artifact_id: row.artifact_id,
                source_kind: TimedTextSourceKind::from_db_str(&row.source_kind),
                language_code: row.language_code,
                cue_index: row.cue_index,
                score: row.fts_score + row.trigram_score,
            });
        }

        let mut snippets = Vec::with_capacity(usize::from(limit));
        let mut used_segment_ids = HashSet::new();
        let mut consumed_matches: i64 = 0;
        let mut stopped_on_limit = false;
        for matched in &matches {
            consumed_matches += 1;
            if used_segment_ids.contains(&matched.segment_id) {
                continue;
            }

            let context = self
                .snippet_context_segments(
                    matched.source_id,
                    matched.cue_index,
                    segment_limit,
                )
                .await?;
            if context.is_empty()
                || context.iter().any(|segment| {
                    used_segment_ids.contains(&segment.segment_id)
                })
            {
                continue;
            }

            let text = context
                .iter()
                .map(|segment| segment.cue_text.trim())
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if text.is_empty() {
                continue;
            }

            let mut media = IntelligenceMediaRef::new(
                matched.media_id,
                matched.title.clone(),
            );
            media.library_id = Some(matched.library_id);
            let start_ms = context
                .iter()
                .map(|segment| segment.start_ms)
                .min()
                .unwrap_or_default();
            let end_ms = context
                .iter()
                .map(|segment| segment.end_ms)
                .max()
                .unwrap_or_default();
            let segment_ids = context
                .iter()
                .map(|segment| segment.segment_id)
                .collect::<Vec<_>>();
            for segment_id in &segment_ids {
                used_segment_ids.insert(*segment_id);
            }

            snippets.push(TimedTextSnippet {
                media,
                source_id: matched.source_id,
                segment_ids,
                artifact_id: matched.artifact_id,
                source_kind: matched.source_kind,
                language_code: matched.language_code.clone(),
                start_ms,
                end_ms,
                snippet: IntelligenceSummary::with_max_chars(
                    text,
                    caps.timed_text_snippet_max_chars,
                ),
                score: Some(matched.score),
            });

            if snippets.len() >= usize::from(limit) {
                stopped_on_limit = true;
                break;
            }
        }

        let has_more = stopped_on_limit
            && (consumed_matches < matches.len() as i64 || fetched_has_extra)
            || (!stopped_on_limit && fetched_has_extra);
        let next_cursor = if has_more {
            Some(timed_text_cursor(offset.saturating_add(consumed_matches)))
        } else {
            None
        };

        Ok(TimedTextSnippetSearchResponse {
            snippets,
            page: IntelligencePageInfo {
                next_cursor,
                limit,
                has_more,
            },
            caps,
        })
    }
}
