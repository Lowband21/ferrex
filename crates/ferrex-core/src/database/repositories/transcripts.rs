//! PostgreSQL-backed timed-text corpus repository.
//!
//! SQL uses runtime-checked `sqlx::query` calls so this foundation can evolve
//! without requiring new offline SQLx metadata for every transcript query. The
//! migration owns the durable constraints and indexes.

use async_trait::async_trait;
use ferrex_model::{
    EpisodeID, LibraryId, MediaID, MovieID, SeasonID, SeriesID,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    api::types::intelligence::{
        IntelligenceMediaKind, IntelligenceMediaRef, IntelligencePageInfo,
        IntelligenceSummary, TimedTextSnippet, TimedTextSnippetSearchRequest,
        TimedTextSnippetSearchResponse, TimedTextSourceKind,
    },
    database::repository_ports::transcripts::{
        TranscriptProcessingState, TranscriptProcessingStatusSummary,
        TranscriptRepository, TranscriptSegmentUpsert, TranscriptSourceStatus,
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

fn content_hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\x1f");
    }
    hex::encode(hasher.finalize())
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

async fn fetch_counts(
    executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    library_id: LibraryId,
    media_file_id: Uuid,
) -> Result<(i64, i64)> {
    let source_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT count(*)
        FROM transcript_sources
        WHERE library_id = $1
          AND media_file_id = $2
          AND status = 'active'
          AND invalidated_at IS NULL
          AND purged_at IS NULL
        "#,
    )
    .bind(library_id.0)
    .bind(media_file_id)
    .fetch_one(&mut **executor)
    .await?;

    let segment_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT count(*)
        FROM transcript_segments
        WHERE library_id = $1
          AND media_file_id = $2
          AND status = 'active'
          AND invalidated_at IS NULL
          AND purged_at IS NULL
        "#,
    )
    .bind(library_id.0)
    .bind(media_file_id)
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

    sqlx::query(
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
    )
    .bind(library_id.0)
    .bind(*media_id.as_uuid())
    .bind(media_type)
    .bind(media_file_id)
    .bind(status_str)
    .bind(i32::try_from(source_count).unwrap_or(i32::MAX))
    .bind(i32::try_from(segment_count).unwrap_or(i32::MAX))
    .bind(is_invalidated)
    .bind(is_purged)
    .bind(reason)
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
    let row = sqlx::query(
        r#"
        SELECT library_id, media_id, media_type::text AS media_type,
               media_file_id, artifact_id
        FROM transcript_sources
        WHERE id = $1
        "#,
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Err(MediaError::NotFound(format!(
            "transcript source {source_id} not found"
        )));
    };

    let library_id = LibraryId(row.try_get("library_id")?);
    let media_uuid: Uuid = row.try_get("media_id")?;
    let media_type: String = row.try_get("media_type")?;
    let media_id = media_id_from_parts(&media_type, media_uuid);
    Ok(SourceIdentity {
        library_id,
        media_id,
        media_file_id: row.try_get("media_file_id")?,
        artifact_id: row.try_get("artifact_id")?,
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

        let row = sqlx::query(
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
            RETURNING id
            "#,
        )
        .bind(source.source_id)
        .bind(source.library_id.0)
        .bind(media_uuid)
        .bind(media_type)
        .bind(source.media_file_id)
        .bind(source_kind)
        .bind(&language_code)
        .bind(source.source_key.trim())
        .bind(source.source_name.as_deref())
        .bind(source.stream_index)
        .bind(source.source_path_hash.as_deref())
        .bind(&source.source_content_hash)
        .bind(source.normalized_content_hash.as_deref())
        .bind(source.artifact_id)
        .bind(source.duration_ms)
        .bind(&source.source_locator)
        .bind(&source.metadata)
        .fetch_one(&mut *tx)
        .await?;

        let source_id: Uuid = row.try_get("id")?;

        sqlx::query(
            "DELETE FROM transcript_segments WHERE transcript_source_id = $1",
        )
        .bind(source_id)
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

            sqlx::query(
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
            )
            .bind(source_id)
            .bind(source.library_id.0)
            .bind(media_uuid)
            .bind(media_type)
            .bind(source.media_file_id)
            .bind(&language_code)
            .bind(segment.cue_index)
            .bind(segment.start_ms)
            .bind(segment.end_ms)
            .bind(&segment.text)
            .bind(segment_hash)
            .bind(&segment.metadata)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            r#"
            UPDATE transcript_sources
            SET segment_count = $2,
                status = 'active',
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(source_id)
        .bind(i32::try_from(segments.len()).unwrap_or(i32::MAX))
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

        let rows = sqlx::query(
            r#"
            SELECT id,
                   library_id,
                   media_id,
                   media_type::text AS media_type,
                   media_file_id,
                   source_kind::text AS source_kind,
                   status::text AS status,
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
        )
        .bind(filter.library_id.map(|id| id.0))
        .bind(filter.media_file_id)
        .bind(media_uuid)
        .bind(media_type)
        .bind(status)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let media_uuid: Uuid = row.try_get("media_id")?;
            let media_type: String = row.try_get("media_type")?;
            let source_kind: String = row.try_get("source_kind")?;
            let status: String = row.try_get("status")?;
            out.push(TranscriptSourceStatusSummary {
                source_id: row.try_get("id")?,
                library_id: LibraryId(row.try_get("library_id")?),
                media_id: media_id_from_parts(&media_type, media_uuid),
                media_file_id: row.try_get("media_file_id")?,
                source_kind: TimedTextSourceKind::from_db_str(&source_kind),
                status: TranscriptSourceStatus::from_db_str(&status),
                language_code: row.try_get("language_code")?,
                source_name: row.try_get("source_name")?,
                artifact_id: row.try_get("artifact_id")?,
                segment_count: row.try_get("segment_count")?,
                duration_ms: row.try_get("duration_ms")?,
                invalidated_at: row.try_get("invalidated_at")?,
                purged_at: row.try_get("purged_at")?,
                updated_at: row.try_get("updated_at")?,
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

        let rows = sqlx::query(
            r#"
            SELECT id,
                   library_id,
                   media_id,
                   media_type::text AS media_type,
                   media_file_id,
                   status::text AS status,
                   source_count,
                   segment_count,
                   attempt_count,
                   last_error_excerpt,
                   next_retry_at,
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
        )
        .bind(filter.library_id.map(|id| id.0))
        .bind(filter.media_file_id)
        .bind(media_uuid)
        .bind(media_type)
        .bind(status)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let media_uuid: Uuid = row.try_get("media_id")?;
            let media_type: String = row.try_get("media_type")?;
            let status: String = row.try_get("status")?;
            out.push(TranscriptProcessingStatusSummary {
                status_id: row.try_get("id")?,
                library_id: LibraryId(row.try_get("library_id")?),
                media_id: media_id_from_parts(&media_type, media_uuid),
                media_file_id: row.try_get("media_file_id")?,
                status: TranscriptProcessingState::from_db_str(&status),
                source_count: row.try_get("source_count")?,
                segment_count: row.try_get("segment_count")?,
                attempt_count: row.try_get("attempt_count")?,
                last_error_excerpt: row.try_get("last_error_excerpt")?,
                next_retry_at: row.try_get("next_retry_at")?,
                invalidated_at: row.try_get("invalidated_at")?,
                purged_at: row.try_get("purged_at")?,
                updated_at: row.try_get("updated_at")?,
            });
        }
        Ok(out)
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

        let artifact_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT artifact_id
            FROM transcript_sources
            WHERE library_id = $1
              AND media_id = $2
              AND media_type = ($3::text)::media_type
              AND artifact_id IS NOT NULL
              AND status <> 'purged'
            "#,
        )
        .bind(library_id.0)
        .bind(media_uuid)
        .bind(media_type)
        .fetch_all(&mut *tx)
        .await?;

        sqlx::query(
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
        )
        .bind(library_id.0)
        .bind(media_uuid)
        .bind(media_type)
        .bind(reason)
        .execute(&mut *tx)
        .await?;

        let affected = sqlx::query(
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
        )
        .bind(library_id.0)
        .bind(media_uuid)
        .bind(media_type)
        .bind(reason)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        if !artifact_ids.is_empty() {
            sqlx::query(
                r#"
                UPDATE intelligence_artifacts
                SET status = 'invalidated',
                    invalidated_at = now(),
                    invalidation_reason = $2,
                    updated_at = now()
                WHERE id = ANY($1::uuid[])
                  AND status <> 'deleted'
                "#,
            )
            .bind(&artifact_ids)
            .bind(reason)
            .execute(&mut *tx)
            .await?;
        }

        let media_file_rows = sqlx::query(
            r#"
            SELECT DISTINCT media_file_id
            FROM transcript_sources
            WHERE library_id = $1
              AND media_id = $2
              AND media_type = ($3::text)::media_type
            "#,
        )
        .bind(library_id.0)
        .bind(media_uuid)
        .bind(media_type)
        .fetch_all(&mut *tx)
        .await?;

        for row in media_file_rows {
            let media_file_id: Uuid = row.try_get("media_file_id")?;
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

        let rows = sqlx::query(
            r#"
            SELECT id, media_file_id, artifact_id
            FROM transcript_sources
            WHERE library_id = $1
              AND media_id = $2
              AND media_type = ($3::text)::media_type
              AND status <> 'purged'
            "#,
        )
        .bind(library_id.0)
        .bind(media_uuid)
        .bind(media_type)
        .fetch_all(&mut *tx)
        .await?;

        let mut source_ids = Vec::with_capacity(rows.len());
        let mut media_file_ids = Vec::new();
        let mut artifact_ids = Vec::new();
        for row in rows {
            source_ids.push(row.try_get::<Uuid, _>("id")?);
            let media_file_id: Uuid = row.try_get("media_file_id")?;
            if !media_file_ids.contains(&media_file_id) {
                media_file_ids.push(media_file_id);
            }
            if let Some(artifact_id) =
                row.try_get::<Option<Uuid>, _>("artifact_id")?
            {
                artifact_ids.push(artifact_id);
            }
        }

        if source_ids.is_empty() {
            tx.commit().await?;
            return Ok(0);
        }

        sqlx::query(
            "DELETE FROM transcript_segments WHERE transcript_source_id = ANY($1::uuid[])",
        )
        .bind(&source_ids)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE transcript_sources
            SET status = 'purged',
                segment_count = 0,
                purged_at = now(),
                purge_reason = $2,
                updated_at = now()
            WHERE id = ANY($1::uuid[])
            "#,
        )
        .bind(&source_ids)
        .bind(reason)
        .execute(&mut *tx)
        .await?;

        if !artifact_ids.is_empty() {
            sqlx::query(
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
            )
            .bind(&artifact_ids)
            .bind(reason)
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

        sqlx::query(
            r#"
            UPDATE transcript_segments
            SET status = 'invalidated',
                invalidated_at = now(),
                invalidation_reason = $2,
                updated_at = now()
            WHERE transcript_source_id = $1
              AND purged_at IS NULL
            "#,
        )
        .bind(source_id)
        .bind(reason)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE transcript_sources
            SET status = 'invalidated',
                invalidated_at = now(),
                invalidation_reason = $2,
                updated_at = now()
            WHERE id = $1
              AND status <> 'purged'
            "#,
        )
        .bind(source_id)
        .bind(reason)
        .execute(&mut *tx)
        .await?;

        if let Some(artifact_id) = identity.artifact_id {
            sqlx::query(
                r#"
                UPDATE intelligence_artifacts
                SET status = 'invalidated',
                    invalidated_at = now(),
                    invalidation_reason = $2,
                    updated_at = now()
                WHERE id = $1
                  AND status <> 'deleted'
                "#,
            )
            .bind(artifact_id)
            .bind(reason)
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

        sqlx::query(
            "DELETE FROM transcript_segments WHERE transcript_source_id = $1",
        )
        .bind(source_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE transcript_sources
            SET status = 'purged',
                segment_count = 0,
                purged_at = now(),
                purge_reason = $2,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(source_id)
        .bind(reason)
        .execute(&mut *tx)
        .await?;

        if let Some(artifact_id) = identity.artifact_id {
            sqlx::query(
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
            )
            .bind(artifact_id)
            .bind(reason)
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
    ) -> Result<TimedTextSnippetSearchResponse> {
        let query = request.query.trim();
        let limit = clamp_limit(request.pagination.limit, 20, 50);
        let caps = request.caps;

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
            .map(|lang| lang.trim().to_string())
            .filter(|lang| !lang.is_empty())
            .collect();
        let source_kinds: Vec<String> = request
            .source_kinds
            .iter()
            .map(|kind| kind.as_db_str().to_string())
            .collect();
        let fetch_limit = i64::from(limit) + 1;

        let rows = sqlx::query(
            r#"
            WITH query AS (
                SELECT websearch_to_tsquery('simple'::regconfig, $1) AS tsq
            )
            SELECT seg.id AS segment_id,
                   seg.library_id,
                   seg.media_id,
                   seg.media_type::text AS media_type,
                   seg.start_ms,
                   seg.end_ms,
                   seg.cue_text,
                   src.id AS source_id,
                   src.artifact_id,
                   src.source_kind::text AS source_kind,
                   src.language_code,
                   COALESCE(
                       mr.title,
                       em.name,
                       CASE
                           WHEN er.id IS NOT NULL THEN format('S%s E%s', er.season_number, er.episode_number)
                           ELSE 'Untitled media'
                       END
                   ) AS title,
                   ts_rank_cd(seg.search_vector, query.tsq) AS fts_score,
                   similarity(seg.cue_text::text, $1) AS trigram_score
            FROM transcript_segments seg
            JOIN transcript_sources src ON src.id = seg.transcript_source_id
            CROSS JOIN query
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
              AND (cardinality($5::text[]) = 0 OR src.language_code = ANY($5::text[]))
              AND (cardinality($6::text[]) = 0 OR src.source_kind::text = ANY($6::text[]))
              AND (
                  seg.search_vector @@ query.tsq
                  OR seg.cue_text ILIKE ('%' || $1 || '%')
                  OR similarity(seg.cue_text::text, $1) > 0.1
              )
            ORDER BY
                (ts_rank_cd(seg.search_vector, query.tsq) + similarity(seg.cue_text::text, $1)) DESC,
                seg.library_id,
                seg.media_type,
                seg.media_id,
                seg.start_ms,
                seg.id
            LIMIT $7
            "#,
        )
        .bind(query)
        .bind(&library_ids)
        .bind(&media_ids)
        .bind(&media_kinds)
        .bind(&language_codes)
        .bind(&source_kinds)
        .bind(fetch_limit)
        .fetch_all(self.pool())
        .await?;

        let has_more = rows.len() > usize::from(limit);
        let mut snippets =
            Vec::with_capacity(rows.len().min(usize::from(limit)));
        for row in rows.into_iter().take(usize::from(limit)) {
            let media_uuid: Uuid = row.try_get("media_id")?;
            let media_type: String = row.try_get("media_type")?;
            let media_id = media_id_from_parts(&media_type, media_uuid);
            let mut media = IntelligenceMediaRef::new(
                media_id,
                row.try_get::<String, _>("title")?,
            );
            media.library_id = Some(LibraryId(row.try_get("library_id")?));

            let fts_score: f32 = row.try_get("fts_score")?;
            let trigram_score: f32 = row.try_get("trigram_score")?;
            let artifact_id = if request.include_artifacts {
                row.try_get("artifact_id")?
            } else {
                None
            };
            let source_kind: String = row.try_get("source_kind")?;

            snippets.push(TimedTextSnippet {
                media,
                source_id: row.try_get("source_id")?,
                segment_ids: vec![row.try_get("segment_id")?],
                artifact_id,
                source_kind: TimedTextSourceKind::from_db_str(&source_kind),
                language_code: row.try_get("language_code")?,
                start_ms: row.try_get("start_ms")?,
                end_ms: row.try_get("end_ms")?,
                snippet: IntelligenceSummary::with_max_chars(
                    row.try_get::<String, _>("cue_text")?,
                    caps.summary_max_chars,
                ),
                score: Some(fts_score + trigram_score),
            });
        }

        Ok(TimedTextSnippetSearchResponse {
            snippets,
            page: IntelligencePageInfo {
                next_cursor: None,
                limit,
                has_more,
            },
            caps,
        })
    }
}
