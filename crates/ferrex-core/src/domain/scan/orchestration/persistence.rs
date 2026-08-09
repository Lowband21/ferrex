//! Postgres-backed persistence for the scan orchestrator queue and cursors.

use crate::database::repository_ports::transcripts::TranscriptProcessingState;
use crate::error::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::from_value;
use sqlx::{Executor, PgPool};
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use tracing::{debug, info, trace, warn};

use crate::domain::scan::orchestration::{
    config::RetryConfig,
    events::stable_path_key,
    job::{
        DependencyKey, EnqueueRequest, JobHandle, JobId, JobKind, JobPayload,
        JobPriority, JobState, ScanReason, TranscriptExtractJob,
    },
    lease::{DequeueRequest, JobLease, LeaseId, LeaseRenewal},
    queue::{
        DurableJobState, FailOutcome, LeaseExpiryScanner, QueueInstrumentation,
        QueueService, QueueSnapshot, QueueTransitionOutcome, ReadyQueueCount,
    },
    scan_cursor::{ScanCursor, ScanCursorId, ScanCursorRepository},
};
use crate::{error::MediaError, types::LibraryId};

/// Durable queue backed by Postgres. All methods are stubs for now.
#[derive(Clone)]
pub struct PostgresQueueService {
    pool: PgPool,
    retry_config: RetryConfig,
}

fn transcript_media_type(job: &TranscriptExtractJob) -> Option<&'static str> {
    match &job.media_id {
        ferrex_model::MediaID::Movie(_) => Some("movie"),
        ferrex_model::MediaID::Episode(_) => Some("episode"),
        ferrex_model::MediaID::Series(_) | ferrex_model::MediaID::Season(_) => {
            None
        }
    }
}

fn bounded_queue_error(error: Option<String>) -> Option<String> {
    error.map(|message| message.chars().take(2048).collect())
}

async fn mark_transcript_queue_status(
    pool: &PgPool,
    job: &TranscriptExtractJob,
    status: TranscriptProcessingState,
    attempt_count: i32,
    max_attempts: u16,
    error: Option<String>,
    next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    correlation_id: Option<uuid::Uuid>,
) -> Result<()> {
    let Some(media_type) = transcript_media_type(job) else {
        return Ok(());
    };
    let status_str = status.as_db_str();
    let terminal = matches!(
        status,
        TranscriptProcessingState::Succeeded
            | TranscriptProcessingState::Failed
            | TranscriptProcessingState::Skipped
            | TranscriptProcessingState::Cancelled
            | TranscriptProcessingState::Invalidated
            | TranscriptProcessingState::Purged
    );
    let running = status == TranscriptProcessingState::Running;
    let error = bounded_queue_error(error);

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
            finished_at
        ) VALUES (
            $1,
            $2,
            ($3::text)::media_type,
            $4,
            $5,
            0,
            0,
            $6,
            $7,
            $8,
            $9,
            $10,
            CASE WHEN $11 THEN now() ELSE NULL END,
            CASE WHEN $12 THEN now() ELSE NULL END
        )
        ON CONFLICT (library_id, media_file_id) DO UPDATE SET
            media_id = EXCLUDED.media_id,
            media_type = EXCLUDED.media_type,
            status = EXCLUDED.status,
            source_count = CASE
                WHEN $5 IN ('queued', 'running', 'cancelled') THEN EXCLUDED.source_count
                ELSE transcript_processing_status.source_count
            END,
            segment_count = CASE
                WHEN $5 IN ('queued', 'running', 'cancelled') THEN EXCLUDED.segment_count
                ELSE transcript_processing_status.segment_count
            END,
            attempt_count = EXCLUDED.attempt_count,
            max_attempts = EXCLUDED.max_attempts,
            last_error_excerpt = EXCLUDED.last_error_excerpt,
            last_run_correlation_id = COALESCE(
                EXCLUDED.last_run_correlation_id,
                transcript_processing_status.last_run_correlation_id
            ),
            next_retry_at = EXCLUDED.next_retry_at,
            started_at = CASE
                WHEN $11 THEN COALESCE(transcript_processing_status.started_at, now())
                ELSE NULL
            END,
            finished_at = CASE WHEN $12 THEN now() ELSE NULL END,
            updated_at = now()
        "#,
        job.library_id.0,
        *job.media_id.as_uuid(),
        media_type,
        job.media_file_id,
        status_str,
        attempt_count.max(0),
        i32::from(max_attempts),
        error,
        correlation_id,
        next_retry_at,
        running,
        terminal,
    )
    .execute(pool)
    .await?;

    Ok(())
}

fn transcript_payload(payload: &JobPayload) -> Option<TranscriptExtractJob> {
    match payload {
        JobPayload::TranscriptExtract(job) => Some(job.clone()),
        _ => None,
    }
}

async fn replace_pending_transcript_payload<'e, E>(
    executor: E,
    job_id: uuid::Uuid,
    payload_json: &serde_json::Value,
) -> Result<()>
where
    E: Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query!(
        r#"
        UPDATE orchestrator_jobs
        SET payload = $1,
            updated_at = NOW()
        WHERE id = $2
          AND state IN ('ready','deferred')
        "#,
        payload_json,
        job_id,
    )
    .execute(executor)
    .await?;

    Ok(())
}

async fn enroll_job<'e, E>(
    executor: E,
    correlation_id: Option<uuid::Uuid>,
    job_id: JobId,
) -> Result<()>
where
    E: Executor<'e, Database = sqlx::Postgres>,
{
    let Some(correlation_id) = correlation_id else {
        return Ok(());
    };

    sqlx::query!(
        r#"
        INSERT INTO orchestrator_job_enrollments (correlation_id, job_id)
        VALUES ($1, $2)
        ON CONFLICT (correlation_id, job_id) DO NOTHING
        "#,
        correlation_id,
        job_id.0,
    )
    .execute(executor)
    .await
    .map_err(|err| {
        MediaError::Internal(format!(
            "orchestrator job enrollment failed: {err}"
        ))
    })?;

    Ok(())
}

/// Atomically linearize a merge against an active durable job and enroll the
/// requesting correlation. A terminal transition racing the earlier lookup
/// wins this check and forces the caller to enqueue a new job generation.
async fn enroll_active_job<'e, E>(
    executor: E,
    correlation_id: Option<uuid::Uuid>,
    job_id: JobId,
) -> Result<bool>
where
    E: Executor<'e, Database = sqlx::Postgres>,
{
    let row = sqlx::query!(
        r#"
        WITH mergeable_job AS MATERIALIZED (
            SELECT id
            FROM orchestrator_jobs
            WHERE id = $2
              AND state IN ('ready','deferred','leased')
            FOR UPDATE
        ), enrollment AS (
            INSERT INTO orchestrator_job_enrollments (correlation_id, job_id)
            SELECT $1::uuid, id
            FROM mergeable_job
            WHERE $1::uuid IS NOT NULL
            ON CONFLICT (correlation_id, job_id) DO NOTHING
            RETURNING job_id
        )
        SELECT
            EXISTS(SELECT 1 FROM mergeable_job) AS "mergeable!",
            (SELECT COUNT(*)::bigint FROM enrollment) AS "enrollment_count!"
        "#,
        correlation_id,
        job_id.0,
    )
    .fetch_one(executor)
    .await
    .map_err(|err| {
        MediaError::Internal(format!(
            "active orchestrator job enrollment failed: {err}"
        ))
    })?;

    let _ = row.enrollment_count;
    Ok(row.mergeable)
}

impl fmt::Debug for PostgresQueueService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresQueueService")
            .field("pool_size", &self.pool.size())
            .field("idle_connections", &self.pool.num_idle())
            .field("retry_config", &self.retry_config)
            .finish()
    }
}

impl PostgresQueueService {
    /// Create a new Postgres-backed queue service and verify DB health + schema.
    pub async fn new(pool: PgPool) -> Result<Self> {
        Self::new_with_retry(pool, RetryConfig::default()).await
    }

    /// Create a new service with an explicit retry policy.
    pub async fn new_with_retry(
        pool: PgPool,
        retry_config: RetryConfig,
    ) -> Result<Self> {
        // Health check
        sqlx::query_scalar!("SELECT 1")
            .fetch_one(&pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Queue service failed Postgres health check: {e}"
                ))
            })?;
        info!("Queue service connected to Postgres");

        // Schema validation: ensure critical dequeue index exists
        // The baseline migration now moves app objects into `ferrex`.
        // Accept either schema to support upgraded databases without forcing a rewrite.
        let idx_exists = sqlx::query_scalar!(
            r#"
            SELECT 1
            FROM pg_indexes
            WHERE indexname = $1
              AND schemaname IN ('ferrex','public')
            LIMIT 1
            "#,
            "idx_jobs_ready_dequeue"
        )
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Queue service schema validation failed: {e}"
            ))
        })?
        .is_some();

        if !idx_exists {
            return Err(MediaError::Internal(
                "Required index idx_jobs_ready_dequeue is missing; run migrations".into(),
            ));
        }

        Ok(Self { pool, retry_config })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Housekeeping: scan for expired leases and resurrect them according to backoff policy.
    /// Returns the number of jobs transitioned back to Ready.
    pub async fn scan_expired_leases(&self) -> Result<u64> {
        let expired = sqlx::query!(
            r#"
            SELECT id, attempts, library_id, payload
            FROM orchestrator_jobs
            WHERE state = 'leased'
              AND lease_expires_at IS NOT NULL
              AND lease_expires_at < NOW()
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("lease expiry scan failed: {e}"))
        })?;

        let mut resurrected = 0u64;
        let max_attempts = i32::from(self.retry_config.max_attempts);

        for row in expired {
            let attempts_before = row.attempts;
            if attempts_before < max_attempts {
                let attempt_next = attempts_before.saturating_add(1) as u16;
                let job_id = JobId(row.id);
                let library_id = LibraryId(row.library_id);
                let payload: JobPayload = from_value(row.payload).map_err(|e| {
                    MediaError::Internal(format!(
                        "lease resurrection payload decode failed for job {}: {e}",
                        row.id
                    ))
                })?;

                let mut library_under_pressure = false;
                if self.retry_config.heavy_library_attempt_threshold > 0 {
                    let threshold = i32::from(
                        self.retry_config.heavy_library_attempt_threshold,
                    );
                    if attempt_next as i32 >= threshold {
                        library_under_pressure = true;
                    } else {
                        let pressure_count: Option<i64> = sqlx::query_scalar!(
                            r#"
                            SELECT COUNT(*)::bigint
                            FROM orchestrator_jobs
                            WHERE library_id = $1
                              AND id <> $2
                              AND attempts >= $3
                              AND state IN ('ready','leased')
                            "#,
                            library_id.0,
                            job_id.0,
                            threshold
                        )
                        .fetch_one(&self.pool)
                        .await
                        .map_err(|e| {
                            MediaError::Internal(format!(
                                "lease resurrection pressure lookup failed: {e}"
                            ))
                        })?;
                        library_under_pressure =
                            pressure_count.unwrap_or(0) > 0;
                    }
                }

                let delay_ms = self.compute_delay_ms(
                    attempt_next,
                    &payload,
                    library_under_pressure,
                    job_id,
                );
                sqlx::query!(
                    r#"
                    UPDATE orchestrator_jobs
                    SET attempts = attempts + 1,
                        state = 'ready',
                        lease_owner = NULL,
                        lease_id = NULL,
                        lease_expires_at = NULL,
                        available_at = NOW() + ($2::bigint) * INTERVAL '1 millisecond',
                        last_error = COALESCE(last_error, 'lease expired'),
                        updated_at = NOW()
                    WHERE id = $1 AND state = 'leased'
                    "#,
                    row.id,
                    delay_ms as i64
                )
                    .execute(&self.pool)
                    .await
                    .map_err(|e| MediaError::Internal(format!("lease resurrection failed: {e}")))?;
                resurrected += 1;
            } else {
                sqlx::query!(
                    r#"
                    UPDATE orchestrator_jobs
                    SET
                        state = 'dead_letter',
                        lease_owner = NULL,
                        lease_id = NULL,
                        lease_expires_at = NULL,
                        updated_at = NOW(),
                        last_error = COALESCE(last_error, 'lease expired (max attempts)')
                    WHERE id = $1 AND state = 'leased'
                    "#,
                    row.id
                )
                    .execute(&self.pool)
                    .await
                    .map_err(|e| MediaError::Internal(format!("lease DLQ update failed: {e}")))?;
            }
        }

        Ok(resurrected)
    }

    /// Optional: fetch a queue metrics snapshot for observability.
    pub async fn metrics_snapshot(&self) -> Result<QueueSnapshot> {
        self.queue_snapshot().await
    }

    fn base_delay_ms(&self, attempt: u16) -> u64 {
        if attempt == 0 {
            return 0;
        }

        let exp = (attempt.saturating_sub(1)) as i32;
        let scaled =
            (self.retry_config.backoff_base_ms as f64) * 2f64.powi(exp);
        let capped = scaled.min(self.retry_config.backoff_max_ms as f64);
        capped.max(0.0) as u64
    }

    fn compute_delay_ms(
        &self,
        attempt: u16,
        payload: &JobPayload,
        library_under_pressure: bool,
        job_id: JobId,
    ) -> u64 {
        let anchor =
            self.anchor_delay_ms(attempt, payload, library_under_pressure);
        self.jittered_delay_for_anchor(anchor, job_id, attempt)
    }

    fn anchor_delay_ms(
        &self,
        attempt: u16,
        payload: &JobPayload,
        library_under_pressure: bool,
    ) -> u64 {
        if attempt == 0 {
            return 0;
        }

        let base = self.base_delay_ms(attempt);
        if base == 0 {
            return 0;
        }

        let fast_multiplier = self.fast_retry_multiplier(attempt, payload);
        let mut scaled = (base as f32 * fast_multiplier).round() as u64;
        if library_under_pressure {
            scaled = ((scaled as f32)
                * self.retry_config.heavy_library_slowdown_factor)
                .round() as u64;
        }

        scaled.clamp(0, self.retry_config.backoff_max_ms)
    }

    fn fast_retry_multiplier(&self, attempt: u16, payload: &JobPayload) -> f32 {
        if attempt == 0 || attempt > self.retry_config.fast_retry_attempts {
            return 1.0;
        }

        let fast_reason = |reason: &ScanReason| {
            matches!(reason, ScanReason::UserRequested | ScanReason::HotChange)
        };

        let is_fast_path = match payload {
            JobPayload::FolderScan(job) => fast_reason(&job.scan_reason),
            JobPayload::MediaAnalyze(job) => fast_reason(&job.scan_reason),
            _ => false,
        };

        if is_fast_path {
            self.retry_config.fast_retry_factor.clamp(0.05, 1.0)
        } else {
            1.0
        }
    }

    fn jittered_delay_for_anchor(
        &self,
        anchor_ms: u64,
        job_id: JobId,
        attempt: u16,
    ) -> u64 {
        if anchor_ms == 0 {
            return 0;
        }

        let jitter_ratio = f64::from(self.retry_config.jitter_ratio.max(0.0));
        let jitter_span = ((anchor_ms as f64) * jitter_ratio)
            .max(self.retry_config.jitter_min_ms as f64)
            .min(self.retry_config.backoff_max_ms as f64);

        let lower = 0f64.max(anchor_ms as f64 - jitter_span);
        let upper = (anchor_ms as f64 + jitter_span)
            .min(self.retry_config.backoff_max_ms as f64);
        if upper <= lower {
            return lower.round() as u64;
        }

        let unit = self.deterministic_unit(job_id, attempt);
        let jittered = lower + (upper - lower) * unit;
        jittered.round() as u64
    }

    fn deterministic_unit(&self, job_id: JobId, attempt: u16) -> f64 {
        let mut hasher = DefaultHasher::default();
        job_id.hash(&mut hasher);
        attempt.hash(&mut hasher);
        let bits = hasher.finish();
        (bits as f64) / (u64::MAX as f64)
    }

    fn parse_priority(priority: i16) -> Result<JobPriority> {
        match priority {
            0 => Ok(JobPriority::P0),
            1 => Ok(JobPriority::P1),
            2 => Ok(JobPriority::P2),
            3 => Ok(JobPriority::P3),
            other => Err(MediaError::Internal(format!(
                "queue returned unknown priority value {other}"
            ))),
        }
    }

    fn parse_state(state: &str) -> Result<JobState> {
        match state {
            "ready" => Ok(JobState::Ready),
            "deferred" => Ok(JobState::Deferred),
            "leased" => Ok(JobState::Leased),
            "completed" => Ok(JobState::Completed),
            "failed" => Ok(JobState::Failed),
            "dead_letter" => Ok(JobState::DeadLetter),
            other => Err(MediaError::Internal(format!(
                "queue returned unknown job state {other}"
            ))),
        }
    }

    /// Fetch grouped schedulable counts directly from persistence. Used to
    /// prime and repair the in-memory scheduler.
    pub async fn ready_counts_grouped(&self) -> Result<Vec<ReadyQueueCount>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                kind,
                library_id,
                priority,
                COUNT(*) FILTER (
                    WHERE state = 'ready' AND available_at <= NOW()
                )::bigint AS "ready!",
                COUNT(*) FILTER (
                    WHERE state = 'leased' AND lease_expires_at > NOW()
                )::bigint AS "leased!"
            FROM orchestrator_jobs
            WHERE (state = 'ready' AND available_at <= NOW())
               OR (state = 'leased' AND lease_expires_at > NOW())
            GROUP BY kind, library_id, priority
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("ready count aggregation failed: {e}"))
        })?;

        let mut counts = Vec::with_capacity(rows.len());

        for row in rows {
            let kind = JobKind::from_i16(row.kind)?;

            let priority = Self::parse_priority(row.priority)?;
            let ready = row.ready.max(0i64) as usize;
            let leased = row.leased.max(0i64) as usize;
            counts.push(ReadyQueueCount {
                kind,
                library_id: LibraryId(row.library_id),
                priority,
                ready,
                leased,
            });
        }

        Ok(counts)
    }
}

#[async_trait]
impl LeaseExpiryScanner for PostgresQueueService {
    async fn scan_expired_leases(&self) -> Result<u64> {
        // Delegate to the inherent method; actual SQL to be provided later
        Self::scan_expired_leases(self).await
    }
}

#[async_trait]
impl QueueInstrumentation for PostgresQueueService {
    async fn queue_snapshot(&self) -> Result<QueueSnapshot> {
        let rows = sqlx::query!(
            r#"
            SELECT kind, state, COUNT(*)::bigint AS cnt
            FROM orchestrator_jobs
            GROUP BY kind, state
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("metrics snapshot failed: {e}"))
        })?;

        let mut snapshot = QueueSnapshot::new(Utc::now());

        for kind in JobKind::all_kinds() {
            snapshot.entry_mut(*kind);
        }

        for row in rows {
            let kind = JobKind::from_i16(row.kind)?;
            let cnt = row.cnt.unwrap_or(0) as usize;
            let entry = snapshot.entry_mut(kind);
            match row.state.as_str() {
                "ready" => entry.ready = cnt,
                "leased" => entry.leased = cnt,
                "deferred" => entry.deferred = cnt,
                "failed" => entry.failed = cnt,
                "dead_letter" => entry.dead_letter = cnt,
                _ => {}
            }
        }

        Ok(snapshot)
    }
}

#[async_trait]
impl QueueService for PostgresQueueService {
    async fn ready_counts_grouped(&self) -> Result<Vec<ReadyQueueCount>> {
        Self::ready_counts_grouped(self).await
    }

    async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle> {
        request.validate()?;
        let job_id = crate::domain::scan::orchestration::job::JobId::new();
        let payload_json =
            serde_json::to_value(&request.payload).map_err(|e| {
                MediaError::Internal(format!(
                    "failed to serialize job payload: {e}"
                ))
            })?;
        let library_id = request.payload.library_id().to_uuid();
        let kind = request.payload.kind() as i16;
        let dedupe_key = request.dedupe_key().to_string();
        let priority_val: i16 = request.priority as i16;
        let correlation_id = request.correlation_id;
        let dependency_key = request
            .dependency_key
            .as_ref()
            .map(|key| key.as_str().to_string());
        let state = if dependency_key.is_some() {
            "deferred"
        } else {
            "ready"
        };

        // A merge target may become terminal at any point between discovery
        // and enrollment. Retry the complete enqueue decision so that race
        // creates a new generation rather than returning a terminal handle.
        for _enqueue_attempt in 0..4 {
            // Fast path: if an active job with the same dedupe_key exists, merge without
            // causing a unique violation. This avoids noisy ERROR logs in Postgres.
            if let Some(existing) = sqlx::query!(
                r#"
            SELECT id, priority, state, attempts
            FROM orchestrator_jobs
            WHERE dedupe_key = $1
              AND state IN ('ready','deferred','leased')
            ORDER BY created_at ASC
            LIMIT 1
            "#,
                &dedupe_key,
            )
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("enqueue precheck failed: {e}"))
            })? {
                let existing_uuid = existing.id;
                let existing_id =
                    crate::domain::scan::orchestration::job::JobId(
                        existing_uuid,
                    );
                let existing_priority = existing.priority;
                let existing_state = existing.state;
                let existing_attempts = existing.attempts;
                if !enroll_active_job(&self.pool, correlation_id, existing_id)
                    .await?
                {
                    continue;
                }
                // Try to elevate priority if incoming is higher and the job is not leased
                if priority_val < existing_priority {
                    let _ = sqlx::query!(
                        r#"
                    UPDATE orchestrator_jobs
                    SET priority = $1,
                        available_at = LEAST(available_at, NOW()),
                        updated_at = NOW()
                    WHERE id = $2 AND state IN ('ready','deferred')
                    "#,
                        priority_val,
                        existing_uuid
                    )
                    .execute(&self.pool)
                    .await;
                }
                if correlation_id.is_some() {
                    let _ = sqlx::query!(
                        r#"
                    UPDATE orchestrator_jobs
                    SET correlation_id = COALESCE(correlation_id, $1),
                        updated_at = NOW()
                    WHERE id = $2
                      AND correlation_id IS NULL
                    "#,
                        correlation_id,
                        existing_uuid
                    )
                    .execute(&self.pool)
                    .await;
                }
                if existing_state.as_str() != "leased"
                    && transcript_payload(&request.payload).is_some()
                {
                    replace_pending_transcript_payload(
                        &self.pool,
                        existing_uuid,
                        &payload_json,
                    )
                    .await
                    .map_err(|err| {
                        MediaError::Internal(format!(
                            "transcript merge payload update failed: {err}"
                        ))
                    })?;
                }
                if existing_state.as_str() != "leased"
                    && let Some(job) = transcript_payload(&request.payload)
                    && let Err(err) = mark_transcript_queue_status(
                        &self.pool,
                        &job,
                        TranscriptProcessingState::Queued,
                        existing_attempts,
                        self.retry_config.max_attempts,
                        None,
                        None,
                        correlation_id,
                    )
                    .await
                {
                    warn!(error = %err, job = %existing_uuid, "failed to mark transcript job queued");
                }
                return Ok(JobHandle::merged(
                    existing_id,
                    &request.payload,
                    request.priority,
                ));
            }

            // Attempt insert; rely on partial unique index uq_jobs_dedupe_active.
            // We cannot reference a partial unique index in ON CONFLICT directly, so we
            // perform a plain INSERT and treat unique violations as merge events.
            let insert_res = sqlx::query!(
            r#"
            WITH inserted_job AS (
                INSERT INTO orchestrator_jobs (
                    id, library_id, kind, payload, priority, state,
                    attempts, available_at, lease_owner, lease_id, lease_expires_at,
                    dedupe_key, dependency_key, correlation_id, last_error,
                    created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, 0, NOW(), NULL, NULL, NULL, $7, $8, $9, NULL, NOW(), NOW())
                RETURNING id
            )
            INSERT INTO orchestrator_job_enrollments (correlation_id, job_id)
            SELECT $9, id
            FROM inserted_job
            WHERE $9 IS NOT NULL
            ON CONFLICT (correlation_id, job_id) DO NOTHING
            "#,
            job_id.0,
            library_id,
            kind,
            payload_json,
            priority_val,
            state,
            dedupe_key,
            dependency_key,
            correlation_id
        )
            .execute(&self.pool)
            .await;

            match insert_res {
                Ok(_) => {
                    trace!("enqueue accepted new job {}", job_id.0);
                    if let Some(job) = transcript_payload(&request.payload)
                        && let Err(err) = mark_transcript_queue_status(
                            &self.pool,
                            &job,
                            TranscriptProcessingState::Queued,
                            0,
                            self.retry_config.max_attempts,
                            None,
                            None,
                            correlation_id,
                        )
                        .await
                    {
                        warn!(error = %err, job = %job_id.0, "failed to mark transcript job queued");
                    }
                    return Ok(JobHandle::accepted(
                        job_id,
                        &request.payload,
                        request.priority,
                    ));
                }
                Err(sqlx::Error::Database(db_err)) => {
                    // Unique violation => merge
                    let code = db_err.code().map(|c| c.to_string());
                    if code.as_deref() == Some("23505") {
                        let existing = sqlx::query!(
                            r#"
                        SELECT id, priority, available_at, state, attempts
                        FROM orchestrator_jobs
                        WHERE dedupe_key = $1
                          AND state IN ('ready','deferred','leased')
                        ORDER BY created_at ASC
                        LIMIT 1
                        "#,
                            &dedupe_key,
                        )
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| {
                            MediaError::Internal(format!(
                                "enqueue conflict lookup failed: {e}"
                            ))
                        })?;

                        if let Some(row) = existing
                            && enroll_active_job(
                                &self.pool,
                                correlation_id,
                                JobId(row.id),
                            )
                            .await?
                        {
                            let row_id = row.id;
                            let row_state = row.state;
                            let row_attempts = row.attempts;
                            // Elevate priority if incoming is higher (lower numeric value)
                            let existing_pri = row.priority;
                            if priority_val < existing_pri {
                                let update = sqlx::query!(
                                    r#"
                                UPDATE orchestrator_jobs
                                SET priority = $1,
                                    available_at = LEAST(available_at, NOW()),
                                    updated_at = NOW()
                                WHERE id = $2
                                  AND state IN ('ready','deferred')
                                "#,
                                    priority_val,
                                    row_id
                                )
                                .execute(&self.pool)
                                .await
                                .map_err(|e| {
                                    MediaError::Internal(format!(
                                        "enqueue merge elevation failed: {e}"
                                    ))
                                })?;

                                if update.rows_affected() > 0 {
                                    info!(
                                        "enqueue merged and elevated priority for job {} to {}",
                                        row_id, priority_val
                                    );
                                } else {
                                    // Likely leased or moved terminal concurrently; best-effort merge only
                                    info!(
                                        "enqueue merge: elevation skipped due to state transition for job {}",
                                        row_id
                                    );
                                }
                            } else {
                                info!(
                                    "enqueue merged into existing job {} without priority change",
                                    row_id
                                );
                            }
                            if correlation_id.is_some() {
                                let _ = sqlx::query!(
                                r#"
                                UPDATE orchestrator_jobs
                                SET correlation_id = COALESCE(correlation_id, $1),
                                    updated_at = NOW()
                                WHERE id = $2
                                  AND correlation_id IS NULL
                                "#,
                                correlation_id,
                                row_id
                            )
                            .execute(&self.pool)
                            .await;
                            }
                            if row_state.as_str() != "leased"
                                && transcript_payload(&request.payload)
                                    .is_some()
                            {
                                replace_pending_transcript_payload(
                                &self.pool,
                                row_id,
                                &payload_json,
                            )
                            .await
                            .map_err(|err| {
                                MediaError::Internal(format!(
                                    "transcript merge payload update failed: {err}"
                                ))
                            })?;
                            }
                            if row_state.as_str() != "leased"
                                && let Some(job) =
                                    transcript_payload(&request.payload)
                                && let Err(err) = mark_transcript_queue_status(
                                    &self.pool,
                                    &job,
                                    TranscriptProcessingState::Queued,
                                    row_attempts,
                                    self.retry_config.max_attempts,
                                    None,
                                    None,
                                    correlation_id,
                                )
                                .await
                            {
                                warn!(error = %err, job = %row_id, "failed to mark transcript job queued");
                            }
                            return Ok(JobHandle::merged(
                                crate::domain::scan::orchestration::job::JobId(
                                    row_id,
                                ),
                                &request.payload,
                                request.priority,
                            ));
                        } else {
                            // No active row found; try a fresh insert once and, on conflict again, return the found ID
                            let job_id2 =
                            crate::domain::scan::orchestration::job::JobId::new(
                            );
                            let retry = sqlx::query!(
                            r#"
                            WITH inserted_job AS (
                                INSERT INTO orchestrator_jobs (
                                    id, library_id, kind, payload, priority, state,
                                    attempts, available_at, lease_owner, lease_id, lease_expires_at,
                                    dedupe_key, dependency_key, correlation_id,
                                    last_error, created_at, updated_at
                                )
                                VALUES ($1, $2, $3, $4, $5, $6, 0, NOW(), NULL, NULL, NULL, $7, $8, $9, NULL, NOW(), NOW())
                                RETURNING id
                            )
                            INSERT INTO orchestrator_job_enrollments (correlation_id, job_id)
                            SELECT $9, id
                            FROM inserted_job
                            WHERE $9 IS NOT NULL
                            ON CONFLICT (correlation_id, job_id) DO NOTHING
                            "#,
                            job_id2.0,
                            library_id,
                            kind,
                            payload_json,
                            priority_val,
                            state,
                            dedupe_key,
                            dependency_key,
                            correlation_id
                        )
                            .execute(&self.pool)
                            .await;

                            match retry {
                                Ok(_) => {
                                    info!(
                                        "enqueue accepted new job {} on retry",
                                        job_id2.0
                                    );
                                    if let Some(job) =
                                    transcript_payload(&request.payload)
                                    && let Err(err) =
                                        mark_transcript_queue_status(
                                            &self.pool,
                                            &job,
                                            TranscriptProcessingState::Queued,
                                            0,
                                            self.retry_config.max_attempts,
                                            None,
                                            None,
                                            correlation_id,
                                        )
                                        .await
                                {
                                    warn!(error = %err, job = %job_id2.0, "failed to mark transcript job queued");
                                }
                                    return Ok(JobHandle::accepted(
                                        job_id2,
                                        &request.payload,
                                        request.priority,
                                    ));
                                }
                                Err(sqlx::Error::Database(db_err2))
                                    if db_err2
                                        .code()
                                        .map(|c| c.to_string())
                                        .as_deref()
                                        == Some("23505") =>
                                {
                                    // Another concurrent inserter won; fetch and return the winner
                                    let winner = sqlx::query!(
                                    r#"
                                    SELECT id, state, attempts, correlation_id
                                    FROM orchestrator_jobs
                                    WHERE dedupe_key = $1
                                      AND state IN ('ready','deferred','leased')
                                    ORDER BY created_at ASC
                                    LIMIT 1
                                    "#,
                                    &dedupe_key,
                                )
                                .fetch_optional(&self.pool)
                                .await
                                .map_err(|e| {
                                    MediaError::Internal(format!(
                                        "enqueue conflict lookup (retry) failed: {e}"
                                    ))
                                })?;

                                    if let Some(w) = winner
                                        && enroll_active_job(
                                            &self.pool,
                                            correlation_id,
                                            JobId(w.id),
                                        )
                                        .await?
                                    {
                                        let winner_id = w.id;
                                        let winner_state = w.state;
                                        let winner_attempts = w.attempts;
                                        let winner_correlation_id =
                                            w.correlation_id;
                                        if winner_state.as_str() != "leased"
                                            && transcript_payload(
                                                &request.payload,
                                            )
                                            .is_some()
                                        {
                                            replace_pending_transcript_payload(
                                            &self.pool,
                                            winner_id,
                                            &payload_json,
                                        )
                                        .await
                                        .map_err(|err| {
                                            MediaError::Internal(format!(
                                                "transcript merge payload update failed: {err}"
                                            ))
                                        })?;
                                        }
                                        if winner_state.as_str() != "leased"
                                        && let Some(job) =
                                            transcript_payload(&request.payload)
                                        && let Err(err) = mark_transcript_queue_status(
                                            &self.pool,
                                            &job,
                                            TranscriptProcessingState::Queued,
                                            winner_attempts,
                                            self.retry_config.max_attempts,
                                            None,
                                            None,
                                            correlation_id.or(winner_correlation_id),
                                        )
                                        .await
                                    {
                                        warn!(error = %err, job = %winner_id, "failed to mark transcript job queued");
                                    }
                                        return Ok(JobHandle::merged(
                                        crate::domain::scan::orchestration::job::JobId(
                                            winner_id,
                                        ),
                                        &request.payload,
                                        request.priority,
                                    ));
                                    }

                                    continue;
                                }
                                Err(e) => {
                                    return Err(MediaError::Internal(format!(
                                        "enqueue retry insert failed: {e}"
                                    )));
                                }
                            }
                        }
                    } else {
                        return Err(MediaError::Internal(format!(
                            "enqueue insert failed: {}",
                            db_err
                        )));
                    }
                }
                Err(e) => {
                    return Err(MediaError::Internal(format!(
                        "enqueue insert failed: {e}"
                    )));
                }
            }
        }

        Err(MediaError::Internal(
            "enqueue could not linearize after repeated concurrent terminal transitions"
                .into(),
        ))
    }

    async fn enqueue_many(
        &self,
        requests: Vec<EnqueueRequest>,
    ) -> Result<Vec<JobHandle>> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }

        let mut tx = self.pool.begin().await.map_err(|e| {
            MediaError::Internal(format!("begin enqueue_many tx failed: {e}"))
        })?;

        let mut out: Vec<JobHandle> = Vec::with_capacity(requests.len());
        let mut transcript_status_updates: Vec<(
            TranscriptExtractJob,
            i32,
            Option<uuid::Uuid>,
        )> = Vec::new();

        'requests: for request in requests {
            for _enqueue_attempt in 0..4 {
                request.validate()?;
                let job_id =
                    crate::domain::scan::orchestration::job::JobId::new();
                let payload_json = serde_json::to_value(&request.payload)
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "failed to serialize job payload: {e}"
                        ))
                    })?;
                let library_id = request.payload.library_id().to_uuid();
                let kind = request.payload.kind();
                let dedupe_key = request.dedupe_key().to_string();
                let priority_val: i16 = request.priority as u8 as i16;
                let correlation_id = request.correlation_id;
                let dependency_key = request
                    .dependency_key
                    .as_ref()
                    .map(|key| key.as_str().to_string());
                let state = if dependency_key.is_some() {
                    "deferred"
                } else {
                    "ready"
                };

                // Fast-path merge check inside transaction
                if let Some(existing) = sqlx::query!(
                    r#"
                SELECT id, priority, state, attempts, correlation_id
                FROM orchestrator_jobs
                WHERE dedupe_key = $1
                  AND state IN ('ready','deferred','leased')
                ORDER BY created_at ASC
                LIMIT 1
                FOR UPDATE
                "#,
                    &dedupe_key,
                )
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "enqueue_many precheck failed: {e}"
                    ))
                })? {
                    let existing_uuid = existing.id;
                    let existing_id =
                        crate::domain::scan::orchestration::job::JobId(
                            existing_uuid,
                        );
                    let existing_priority = existing.priority;
                    let existing_state = existing.state;
                    let existing_attempts = existing.attempts;
                    let existing_correlation_id = existing.correlation_id;
                    if !enroll_active_job(&mut *tx, correlation_id, existing_id)
                        .await?
                    {
                        continue;
                    }
                    if priority_val < existing_priority {
                        let _ = sqlx::query!(
                            r#"
                        UPDATE orchestrator_jobs
                        SET priority = $1,
                            available_at = LEAST(available_at, NOW()),
                            updated_at = NOW()
                        WHERE id = $2 AND state IN ('ready','deferred')
                        "#,
                            priority_val,
                            existing_uuid
                        )
                        .execute(&mut *tx)
                        .await;
                    }
                    if correlation_id.is_some() {
                        let _ = sqlx::query!(
                            r#"
                        UPDATE orchestrator_jobs
                        SET correlation_id = COALESCE(correlation_id, $1),
                            updated_at = NOW()
                        WHERE id = $2
                          AND correlation_id IS NULL
                        "#,
                            correlation_id,
                            existing_uuid
                        )
                        .execute(&mut *tx)
                        .await;
                    }
                    if existing_state.as_str() != "leased"
                        && transcript_payload(&request.payload).is_some()
                    {
                        replace_pending_transcript_payload(
                        &mut *tx,
                        existing_uuid,
                        &payload_json,
                    )
                    .await
                    .map_err(|err| {
                        MediaError::Internal(format!(
                            "enqueue_many transcript merge payload update failed: {err}"
                        ))
                    })?;
                    }
                    if existing_state.as_str() != "leased"
                        && let Some(job) = transcript_payload(&request.payload)
                    {
                        transcript_status_updates.push((
                            job,
                            existing_attempts,
                            correlation_id.or(existing_correlation_id),
                        ));
                    }
                    out.push(JobHandle::merged(
                        existing_id,
                        &request.payload,
                        request.priority,
                    ));
                    continue 'requests;
                }

                // Try insert; merge on unique violation
                let insert_res = sqlx::query!(
                r#"
                INSERT INTO orchestrator_jobs (
                    id, library_id, kind, payload, priority, state,
                    attempts, available_at, lease_owner, lease_id, lease_expires_at,
                    dedupe_key, dependency_key, correlation_id, last_error,
                    created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, 0, NOW(), NULL, NULL, NULL, $7, $8, $9, NULL, NOW(), NOW())
                ON CONFLICT DO NOTHING
                "#,
                job_id.0,
                library_id,
                kind as i16,
                payload_json,
                priority_val,
                state,
                dedupe_key,
                dependency_key,
                correlation_id
            )
                .execute(&mut *tx)
                .await;

                match insert_res {
                    Ok(result) if result.rows_affected() > 0 => {
                        info!("enqueue_many accepted new job {}", job_id.0);
                        enroll_job(&mut *tx, correlation_id, job_id).await?;
                        if let Some(job) = transcript_payload(&request.payload)
                        {
                            transcript_status_updates.push((
                                job,
                                0,
                                correlation_id,
                            ));
                        }
                        out.push(JobHandle::accepted(
                            job_id,
                            &request.payload,
                            request.priority,
                        ));
                        continue 'requests;
                    }
                    Ok(_) => {
                        let existing = sqlx::query!(
                            r#"
                            SELECT id, priority, available_at, state, attempts, correlation_id
                            FROM orchestrator_jobs
                            WHERE dedupe_key = $1
                              AND state IN ('ready','deferred','leased')
                            ORDER BY created_at ASC
                            LIMIT 1
                            FOR UPDATE
                            "#,
                            request.dedupe_key().to_string(),
                        )
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(|e| {
                            MediaError::Internal(format!(
                                "enqueue_many conflict lookup failed: {e}"
                            ))
                        })?;

                        if let Some(row) = existing
                            && enroll_active_job(
                                &mut *tx,
                                correlation_id,
                                JobId(row.id),
                            )
                            .await?
                        {
                            let row_id = row.id;
                            let existing_pri = row.priority;
                            let row_state = row.state;
                            let row_attempts = row.attempts;
                            let row_correlation_id = row.correlation_id;
                            if priority_val < existing_pri {
                                let _ = sqlx::query!(
                                    r#"
                                    UPDATE orchestrator_jobs
                                    SET priority = $1,
                                        available_at = LEAST(available_at, NOW()),
                                        updated_at = NOW()
                                    WHERE id = $2 AND state IN ('ready','deferred')
                                    "#,
                                    priority_val,
                                    row_id
                                )
                                    .execute(&mut *tx)
                                    .await
                                    .map_err(|e| {
                                        MediaError::Internal(format!(
                                            "enqueue_many merge elevation failed: {e}"
                                        ))
                                    })?;
                            }
                            if correlation_id.is_some() {
                                let _ = sqlx::query!(
                                    r#"
                                    UPDATE orchestrator_jobs
                                    SET correlation_id = COALESCE(correlation_id, $1),
                                        updated_at = NOW()
                                    WHERE id = $2
                                      AND correlation_id IS NULL
                                    "#,
                                    correlation_id,
                                    row_id
                                )
                                .execute(&mut *tx)
                                .await;
                            }
                            if row_state.as_str() != "leased"
                                && transcript_payload(&request.payload)
                                    .is_some()
                            {
                                replace_pending_transcript_payload(
                                    &mut *tx,
                                    row_id,
                                    &payload_json,
                                )
                                .await
                                .map_err(|err| {
                                    MediaError::Internal(format!(
                                        "enqueue_many transcript merge payload update failed: {err}"
                                    ))
                                })?;
                            }
                            if row_state.as_str() != "leased"
                                && let Some(job) =
                                    transcript_payload(&request.payload)
                            {
                                transcript_status_updates.push((
                                    job,
                                    row_attempts,
                                    correlation_id.or(row_correlation_id),
                                ));
                            }
                            out.push(JobHandle::merged(
                                crate::domain::scan::orchestration::job::JobId(
                                    row_id,
                                ),
                                &request.payload,
                                request.priority,
                            ));
                            continue 'requests;
                        } else {
                            // The conflict winner became terminal before it
                            // could be enrolled. Retry this request so the next
                            // iteration inserts a new active generation.
                            continue;
                        }
                    }
                    Err(e) => {
                        drop(tx.rollback().await);
                        return Err(MediaError::Internal(format!(
                            "enqueue_many insert failed: {e}"
                        )));
                    }
                }
            }

            drop(tx.rollback().await);
            return Err(MediaError::Internal(
                "enqueue_many could not linearize after repeated concurrent terminal transitions"
                    .into(),
            ));
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("enqueue_many tx commit failed: {e}"))
        })?;

        for (job, attempts, correlation_id) in transcript_status_updates {
            if let Err(err) = mark_transcript_queue_status(
                &self.pool,
                &job,
                TranscriptProcessingState::Queued,
                attempts,
                self.retry_config.max_attempts,
                None,
                None,
                correlation_id,
            )
            .await
            {
                warn!(error = %err, media_file_id = %job.media_file_id, "failed to mark transcript batch job queued");
            }
        }

        Ok(out)
    }

    async fn dequeue(
        &self,
        request: DequeueRequest,
    ) -> Result<Option<JobLease>> {
        use crate::domain::scan::orchestration::job::{
            JobPriority, JobRecord, JobState,
        };
        use uuid::Uuid;

        let mut tx = self.pool.begin().await.map_err(|e| {
            MediaError::Internal(format!("begin dequeue tx failed: {e}"))
        })?;

        // Select next eligible job for this kind
        let kind = request.kind as i16;

        struct SelectedRow {
            id: Uuid,
            payload: serde_json::Value,
            priority: i16,
            attempts: i32,
            available_at: chrono::DateTime<chrono::Utc>,
            dedupe_key: String,
            dependency_key: Option<String>,
            correlation_id: Option<Uuid>,
            created_at: chrono::DateTime<chrono::Utc>,
        }

        let row: Option<SelectedRow> = if let Some(selector) = request.selector
        {
            let priority: i16 = selector.priority as u8 as i16;
            sqlx::query!(
                r#"
                WITH next AS (
                    SELECT id
                    FROM orchestrator_jobs
                    WHERE state = 'ready'
                      AND kind = $1
                      AND available_at <= NOW()
                      AND library_id = $2
                      AND priority = $3
                    ORDER BY available_at, attempts, created_at
                    FOR UPDATE SKIP LOCKED
                    LIMIT 1
                )
                SELECT j.id, j.payload, j.priority, j.attempts,
                       j.available_at, j.dedupe_key, j.dependency_key,
                       j.correlation_id, j.created_at
                FROM orchestrator_jobs j
                JOIN next ON next.id = j.id
                "#,
                kind,
                selector.library_id.as_uuid(),
                priority
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("dequeue select failed: {e}"))
            })?
            .map(|row| SelectedRow {
                id: row.id,
                payload: row.payload,
                priority: row.priority,
                attempts: row.attempts,
                available_at: row.available_at,
                dedupe_key: row.dedupe_key,
                dependency_key: row.dependency_key,
                correlation_id: row.correlation_id,
                created_at: row.created_at,
            })
        } else {
            sqlx::query!(
                r#"
                SELECT id, payload, priority, attempts, available_at,
                       dedupe_key, dependency_key, correlation_id, created_at
                FROM orchestrator_jobs
                WHERE kind = $1
                  AND state = 'ready'
                  AND available_at <= NOW()
                ORDER BY priority ASC, available_at ASC, attempts ASC, created_at ASC
                LIMIT 1
                FOR UPDATE SKIP LOCKED
                "#,
                kind
            )
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| MediaError::Internal(format!("dequeue select failed: {e}")))?
                .map(|row| SelectedRow {
                    id: row.id,
                    payload: row.payload,
                    priority: row.priority,
                    attempts: row.attempts,
                    available_at: row.available_at,
                    dedupe_key: row.dedupe_key,
                    dependency_key: row.dependency_key,
                    correlation_id: row.correlation_id,
                    created_at: row.created_at,
                })
        };

        let Some(row) = row else {
            // Nothing ready
            drop(tx); // rollback implicit
            return Ok(None);
        };

        // Generate lease id and expiry timestamp
        let lease_id = LeaseId::new();
        let expires_at = chrono::Utc::now() + request.lease_ttl;

        // Update to leased state
        let updated = sqlx::query!(
            r#"
            UPDATE orchestrator_jobs
            SET state='leased',
                lease_owner=$1,
                lease_id=$2,
                lease_expires_at=$3,
                attempts = COALESCE(attempts, 0),
                updated_at=NOW()
            WHERE id = $4 AND state = 'ready'
            RETURNING lease_id
            "#,
            request.worker_id,
            lease_id.0,
            expires_at,
            row.id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("dequeue update->leased failed: {e}"))
        })?;

        if updated.is_none() {
            // Raced with state change; treat as empty
            drop(tx);
            return Ok(None);
        }

        // Build JobRecord from the selected row and new lease fields
        let payload: JobPayload =
            serde_json::from_value(row.payload).map_err(|e| {
                MediaError::Internal(format!(
                    "failed to deserialize job payload: {e}"
                ))
            })?;

        let priority = match row.priority {
            0 => JobPriority::P0,
            1 => JobPriority::P1,
            2 => JobPriority::P2,
            3 => JobPriority::P3,
            other => {
                return Err(MediaError::Internal(format!(
                    "invalid priority {}",
                    other
                )));
            }
        };

        let job = JobRecord {
            id: JobId(row.id),
            payload,
            priority,
            state: JobState::Leased,
            attempts: row.attempts.max(0) as u16,
            available_at: row.available_at,
            lease_owner: Some(request.worker_id.clone()),
            lease_expires_at: Some(expires_at),
            backoff_until: None,
            dedupe_key: row.dedupe_key,
            dependency_key: row.dependency_key.map(DependencyKey::from),
            correlation_id: row.correlation_id,
            created_at: row.created_at,
            updated_at: chrono::Utc::now(),
        };

        let lease = JobLease {
            lease_id,
            job,
            lease_owner: request.worker_id,
            expires_at,
            renewals: 0,
        };

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("dequeue tx commit failed: {e}"))
        })?;

        Ok(Some(lease))
    }

    async fn renew(&self, renewal: LeaseRenewal) -> Result<JobLease> {
        use crate::domain::scan::orchestration::job::{
            JobPriority, JobRecord, JobState,
        };

        // Single-statement renewal keeps row locks scoped to the SQL execution time (instead of
        // holding them across JSON deserialization and Rust-side bookkeeping).
        let extend_ms: i64 = renewal.extend_by.num_milliseconds();
        let row = sqlx::query!(
            r#"
            UPDATE orchestrator_jobs
            SET lease_expires_at = lease_expires_at + ($1::bigint) * INTERVAL '1 millisecond'
            WHERE lease_id = $2
              AND state = 'leased'
              AND lease_expires_at > NOW()
            RETURNING
                id, library_id, kind, payload, priority, attempts, available_at,
                dedupe_key, dependency_key, correlation_id, created_at,
                updated_at, lease_owner, lease_expires_at
            "#,
            extend_ms,
            renewal.lease_id.0
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("renew update failed: {e}")))?;

        let Some(row) = row else {
            warn!(
                "renewal failed: lease {:?} not found or expired",
                renewal.lease_id.0
            );
            return Err(MediaError::NotFound(
                "lease not found or expired".into(),
            ));
        };

        let payload: JobPayload =
            serde_json::from_value(row.payload).map_err(|e| {
                MediaError::Internal(format!(
                    "failed to deserialize job payload: {e}"
                ))
            })?;

        let priority = match row.priority {
            0 => JobPriority::P0,
            1 => JobPriority::P1,
            2 => JobPriority::P2,
            3 => JobPriority::P3,
            other => {
                return Err(MediaError::Internal(format!(
                    "invalid priority {}",
                    other
                )));
            }
        };

        let job = JobRecord {
            id: JobId(row.id),
            payload,
            priority,
            state: JobState::Leased,
            attempts: row.attempts.max(0) as u16,
            available_at: row.available_at,
            lease_owner: row.lease_owner,
            lease_expires_at: row.lease_expires_at,
            backoff_until: None,
            dedupe_key: row.dedupe_key,
            dependency_key: row.dependency_key.map(DependencyKey::from),
            correlation_id: row.correlation_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
        };

        let lease_owner_str = job.lease_owner.clone().unwrap_or_default();
        let expires_at = row.lease_expires_at.ok_or_else(|| {
            MediaError::Internal(
                "renewed lease returned NULL lease_expires_at".into(),
            )
        })?;
        let lease = JobLease {
            lease_id: renewal.lease_id,
            job,
            lease_owner: lease_owner_str,
            expires_at,
            renewals: 1, // local increment only
        };

        info!(
            "renewed lease {:?} until {}",
            lease.lease_id.0, lease.expires_at
        );
        Ok(lease)
    }

    async fn complete(&self, lease_id: LeaseId) -> Result<()> {
        let _ = self.complete_with_outcome(lease_id).await?;
        Ok(())
    }

    async fn complete_with_outcome(
        &self,
        lease_id: LeaseId,
    ) -> Result<QueueTransitionOutcome> {
        let res = sqlx::query!(
            r#"
            UPDATE orchestrator_jobs
            SET state='completed',
                lease_owner=NULL,
                lease_id=NULL,
                lease_expires_at=NULL
            WHERE lease_id = $1 AND state='leased'
            "#,
            lease_id.0
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("complete update failed: {e}"))
        })?;

        if res.rows_affected() > 0 {
            debug!("completed job with lease {:?}", lease_id.0);
            Ok(QueueTransitionOutcome::Applied)
        } else {
            Ok(QueueTransitionOutcome::Missing)
        }
    }

    async fn fail(
        &self,
        lease_id: LeaseId,
        retryable: bool,
        error: Option<String>,
    ) -> Result<()> {
        let _ = self.fail_with_outcome(lease_id, retryable, error).await?;
        Ok(())
    }

    async fn fail_with_outcome(
        &self,
        lease_id: LeaseId,
        retryable: bool,
        error: Option<String>,
    ) -> Result<FailOutcome> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            MediaError::Internal(format!("begin fail tx failed: {e}"))
        })?;

        // Lock the row and get current attempts
        let row = sqlx::query!(
            r#"
            SELECT id, attempts, library_id, payload, correlation_id
            FROM orchestrator_jobs
            WHERE lease_id = $1::uuid AND state = 'leased'
            FOR UPDATE
            "#,
            lease_id.0,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("fail select failed: {e}"))
        })?;

        let Some(row) = row else {
            drop(tx);
            return Ok(FailOutcome::Missing);
        };

        let row_id = row.id;
        let attempts_before = row.attempts;
        let row_correlation_id = row.correlation_id;
        let max_attempts = i32::from(self.retry_config.max_attempts);
        let attempt_next = attempts_before.saturating_add(1) as u16;
        let job_id = JobId(row_id);
        let library_id = LibraryId(row.library_id);
        let row_payload = row.payload;
        let payload: JobPayload = from_value(row_payload).map_err(|e| {
            MediaError::Internal(format!(
                "fail payload decode failed for job {}: {e}",
                row_id
            ))
        })?;
        let transcript_job = transcript_payload(&payload);

        let mut library_under_pressure =
            if self.retry_config.heavy_library_attempt_threshold == 0 {
                false
            } else {
                attempt_next as i32
                    >= i32::from(
                        self.retry_config.heavy_library_attempt_threshold,
                    )
            };

        if !library_under_pressure
            && self.retry_config.heavy_library_attempt_threshold > 0
        {
            let pressure_count: Option<i64> = sqlx::query_scalar!(
                r#"
                SELECT COUNT(*)::bigint
                FROM orchestrator_jobs
                WHERE library_id = $1
                  AND id <> $2
                  AND attempts >= $3
                  AND state IN ('ready','leased')
                "#,
                library_id.0,
                job_id.0,
                i32::from(self.retry_config.heavy_library_attempt_threshold)
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "fail pressure lookup failed: {e}"
                ))
            })?;
            library_under_pressure = pressure_count.unwrap_or(0) > 0;
        }

        if retryable && attempts_before < max_attempts {
            let delay_ms = self.compute_delay_ms(
                attempt_next,
                &payload,
                library_under_pressure,
                job_id,
            );

            sqlx::query!(
                r#"
                UPDATE orchestrator_jobs
                SET attempts = attempts + 1,
                    state = 'ready',
                    lease_owner = NULL,
                    lease_id = NULL,
                    lease_expires_at = NULL,
                    last_error = $2,
                    available_at = NOW() + ($3::bigint) * INTERVAL '1 millisecond',
                    updated_at = NOW()
                WHERE id = $1
                "#,
                row_id,
                error,
                delay_ms as i64
            )
                .execute(&mut *tx)
                .await
                .map_err(|e| MediaError::Internal(format!("fail retry update failed: {e}")))?;

            tx.commit().await.map_err(|e| {
                MediaError::Internal(format!("fail tx commit failed: {e}"))
            })?;

            if let Some(job) = transcript_job {
                let next_retry_at = chrono::Utc::now()
                    + chrono::Duration::milliseconds(delay_ms as i64);
                if let Err(err) = mark_transcript_queue_status(
                    &self.pool,
                    &job,
                    TranscriptProcessingState::Failed,
                    i32::from(attempt_next),
                    self.retry_config.max_attempts,
                    error.clone(),
                    Some(next_retry_at),
                    row_correlation_id,
                )
                .await
                {
                    warn!(error = %err, job = %row_id, "failed to mark transcript retry status");
                }
            }

            warn!(
                "job {} failed retryable; attempts now {}; scheduled retry in {}ms (pressure={})",
                row_id,
                attempts_before + 1,
                delay_ms,
                library_under_pressure
            );
            Ok(FailOutcome::RetryScheduled)
        } else {
            // Terminal: dead-letter or failed
            let new_state = if retryable { "dead_letter" } else { "failed" };
            let _ = sqlx::query!(
                r#"
                UPDATE orchestrator_jobs
                SET state = $2,
                    lease_owner = NULL,
                    lease_id = NULL,
                    lease_expires_at = NULL,
                    last_error = $3,
                    updated_at = NOW()
                WHERE id = $1
                "#,
                row_id,
                new_state,
                error
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "fail terminal update failed: {e}"
                ))
            })?;

            tx.commit().await.map_err(|e| {
                MediaError::Internal(format!("fail tx commit failed: {e}"))
            })?;

            if let Some(job) = transcript_job
                && let Err(err) = mark_transcript_queue_status(
                    &self.pool,
                    &job,
                    TranscriptProcessingState::Failed,
                    i32::from(attempt_next),
                    self.retry_config.max_attempts,
                    error.clone(),
                    None,
                    row_correlation_id,
                )
                .await
            {
                warn!(error = %err, job = %row_id, "failed to mark transcript terminal failure status");
            }

            warn!(
                "job {} moved to {} after attempts {}",
                row_id, new_state, attempts_before
            );
            Ok(FailOutcome::Terminal {
                state: if retryable {
                    JobState::DeadLetter
                } else {
                    JobState::Failed
                },
            })
        }
    }

    async fn dead_letter(
        &self,
        lease_id: LeaseId,
        error: Option<String>,
    ) -> Result<()> {
        let _ = self.dead_letter_with_outcome(lease_id, error).await?;
        Ok(())
    }

    async fn dead_letter_with_outcome(
        &self,
        lease_id: LeaseId,
        error: Option<String>,
    ) -> Result<QueueTransitionOutcome> {
        let row = sqlx::query!(
            r#"
            SELECT id, attempts, payload, correlation_id
            FROM orchestrator_jobs
            WHERE lease_id = $1::uuid AND state = 'leased'
            "#,
            lease_id.0,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("dead_letter select failed: {e}"))
        })?;

        let res = sqlx::query!(
            r#"
            UPDATE orchestrator_jobs
            SET state='dead_letter',
                lease_owner=NULL,
                lease_id=NULL,
                lease_expires_at=NULL,
                last_error=$2,
                updated_at=NOW()
            WHERE lease_id = $1::uuid AND state = 'leased'
            "#,
            lease_id.0,
            error
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("dead_letter update failed: {e}"))
        })?;

        if res.rows_affected() > 0 {
            if let Some(row) = row {
                let row_id = row.id;
                let attempts = row.attempts;
                let row_correlation_id = row.correlation_id;
                let row_payload = row.payload;
                if let Ok(payload) =
                    serde_json::from_value::<JobPayload>(row_payload)
                    && let Some(job) = transcript_payload(&payload)
                    && let Err(err) = mark_transcript_queue_status(
                        &self.pool,
                        &job,
                        TranscriptProcessingState::Failed,
                        attempts.saturating_add(1),
                        self.retry_config.max_attempts,
                        error.clone(),
                        None,
                        row_correlation_id,
                    )
                    .await
                {
                    warn!(error = %err, job = %row_id, "failed to mark transcript dead-letter status");
                }
            }
            warn!("job with lease {:?} moved to dead_letter", lease_id.0);
            Ok(QueueTransitionOutcome::Applied)
        } else {
            Ok(QueueTransitionOutcome::Missing)
        }
    }

    async fn cancel_job(&self, job_id: JobId) -> Result<()> {
        // Delete only non-leased jobs; leased jobs require different handling.
        let row = sqlx::query!(
            r#"
            SELECT attempts, payload, correlation_id
            FROM orchestrator_jobs
            WHERE id = $1 AND state IN ('ready','deferred')
            "#,
            job_id.0,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("cancel_job select failed: {e}"))
        })?;

        let res = sqlx::query!(
            r#"
            DELETE FROM orchestrator_jobs
            WHERE id = $1 AND state IN ('ready','deferred')
            "#,
            job_id.0
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("cancel_job delete failed: {e}"))
        })?;

        if res.rows_affected() > 0
            && let Some(row) = row
        {
            let attempts = row.attempts;
            let row_correlation_id = row.correlation_id;
            let row_payload = row.payload;
            if let Ok(payload) =
                serde_json::from_value::<JobPayload>(row_payload)
                && let Some(job) = transcript_payload(&payload)
                && let Err(err) = mark_transcript_queue_status(
                    &self.pool,
                    &job,
                    TranscriptProcessingState::Cancelled,
                    attempts,
                    self.retry_config.max_attempts,
                    Some("transcript extraction job cancelled".to_string()),
                    None,
                    row_correlation_id,
                )
                .await
            {
                warn!(error = %err, job = %job_id.0, "failed to mark transcript cancellation status");
            }
        }
        Ok(())
    }

    async fn queue_depth(&self, kind: JobKind) -> Result<usize> {
        let kind = kind as i16;
        let row = sqlx::query!(
            r#"
            SELECT COUNT(*)::bigint AS "count!"
            FROM orchestrator_jobs
            WHERE kind = $1 AND state = 'ready'
            "#,
            kind
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("queue_depth query failed: {e}"))
        })?;
        Ok(row.count as usize)
    }

    async fn durable_job_states(
        &self,
        correlation_id: uuid::Uuid,
        job_ids: &[JobId],
    ) -> Result<Vec<DurableJobState>> {
        let job_ids: Vec<uuid::Uuid> =
            job_ids.iter().map(|job_id| job_id.0).collect();
        let rows = sqlx::query!(
            r#"
            SELECT job.id, job.kind, job.state, job.attempts, job.payload,
                   job.dedupe_key, job.correlation_id, job.last_error,
                   job.created_at, job.updated_at
            FROM orchestrator_jobs job
            WHERE job.id = ANY($2::uuid[])
               OR EXISTS (
                    SELECT 1
                    FROM orchestrator_job_enrollments enrollment
                    WHERE enrollment.correlation_id = $1
                      AND enrollment.job_id = job.id
               )
            ORDER BY job.created_at ASC, job.id ASC
            "#,
            correlation_id,
            &job_ids,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "durable job reconciliation query failed: {err}"
            ))
        })?;

        let mut states = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: JobPayload = serde_json::from_value(row.payload)
                .map_err(|err| {
                    MediaError::Internal(format!(
                        "durable job payload decode failed: {err}"
                    ))
                })?;
            let kind = JobKind::from_i16(row.kind)?;

            states.push(DurableJobState {
                job_id: JobId(row.id),
                kind,
                state: Self::parse_state(&row.state)?,
                attempts: row.attempts.max(0) as u16,
                dedupe_key: row.dedupe_key,
                correlation_id: row.correlation_id,
                path_key: stable_path_key(&payload),
                last_error: row.last_error,
                created_at: row.created_at,
                updated_at: row.updated_at,
            });
        }

        Ok(states)
    }

    async fn release_dependency(
        &self,
        library_id: LibraryId,
        dependency_key: &DependencyKey,
    ) -> Result<u64> {
        let updated = sqlx::query!(
            r#"
            UPDATE orchestrator_jobs
            SET state = 'ready',
                dependency_key = NULL,
                available_at = NOW(),
                updated_at = NOW()
            WHERE library_id = $1
              AND state = 'deferred'
              AND dependency_key = $2
            "#,
            library_id.0,
            dependency_key.as_str()
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "release_dependency update failed: {e}"
            ))
        })?;

        Ok(updated.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::orchestration::{
        context::{FolderScanContext, MovieFolderScanContext, MovieRootPath},
        job::FolderScanJob,
        lease::QueueSelector,
    };
    use sqlx::Row;
    use std::collections::HashMap;

    fn folder_scan_payload(
        library_id: LibraryId,
        library_root: &str,
        folder_path: &str,
    ) -> JobPayload {
        JobPayload::FolderScan(FolderScanJob {
            context: FolderScanContext::Movie(MovieFolderScanContext {
                library_id,
                movie_root_path: MovieRootPath::try_new_under_library_root(
                    library_root,
                    folder_path,
                )
                .expect("folder path is directly under root"),
            }),
            scan_reason: ScanReason::UserRequested,
            enqueue_time: Utc::now(),
            device_id: None,
        })
    }

    async fn seed_library(
        pool: &PgPool,
        library_id: LibraryId,
        library_root: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at)
            VALUES ($1, $2, $3, 'movies', NOW(), NOW())
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(library_id.to_uuid())
        .bind(format!("Queue dedupe test {library_id}"))
        .bind(vec![library_root.to_string()])
        .execute(pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!("seed library failed: {err}"))
        })?;
        Ok(())
    }

    async fn active_dedupe_count(
        pool: &PgPool,
        dedupe_key: &str,
    ) -> Result<i64> {
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::bigint
            FROM orchestrator_jobs
            WHERE dedupe_key = $1
              AND state IN ('ready','deferred','leased')
            "#,
        )
        .bind(dedupe_key)
        .fetch_one(pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "active dedupe count query failed: {err}"
            ))
        })
    }

    async fn states_for_dedupe(
        pool: &PgPool,
        dedupe_key: &str,
    ) -> Result<HashMap<String, i64>> {
        let rows = sqlx::query(
            r#"
            SELECT state, COUNT(*)::bigint AS count
            FROM orchestrator_jobs
            WHERE dedupe_key = $1
            GROUP BY state
            "#,
        )
        .bind(dedupe_key)
        .fetch_all(pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!("dedupe state query failed: {err}"))
        })?;

        let mut states = HashMap::new();
        for row in rows {
            states.insert(row.try_get("state")?, row.try_get("count")?);
        }
        Ok(states)
    }

    #[tokio::test]
    async fn enqueue_reuses_ready_deferred_and_leased_dedupe_rows() {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping: DATABASE_URL not set");
                return;
            }
        };

        let pool = match PgPool::connect(&database_url).await {
            Ok(pool) => pool,
            Err(err) => {
                eprintln!(
                    "skipping: failed to connect to DATABASE_URL ({err})"
                );
                return;
            }
        };

        if let Err(err) = crate::MIGRATOR.run(&pool).await {
            eprintln!("skipping: migrations failed ({err})");
            return;
        }

        let queue = PostgresQueueService::new(pool.clone())
            .await
            .expect("queue service initializes");
        let missing_lease = LeaseId::new();
        assert_eq!(
            queue
                .complete_with_outcome(missing_lease)
                .await
                .expect("missing completion is reported"),
            QueueTransitionOutcome::Missing
        );
        assert_eq!(
            queue
                .fail_with_outcome(
                    missing_lease,
                    true,
                    Some("missing retry".into()),
                )
                .await
                .expect("missing failure is reported"),
            FailOutcome::Missing
        );
        assert_eq!(
            queue
                .dead_letter_with_outcome(
                    missing_lease,
                    Some("missing dead letter".into()),
                )
                .await
                .expect("missing dead letter is reported"),
            QueueTransitionOutcome::Missing
        );
        let library_id = LibraryId::new();
        let library_root = format!("/queue-dedupe/{}", library_id.as_uuid());
        seed_library(&pool, library_id, &library_root)
            .await
            .expect("library seeded");

        let ready_payload = folder_scan_payload(
            library_id,
            &library_root,
            &format!("{library_root}/ready-movie"),
        );
        let ready = queue
            .enqueue(EnqueueRequest::new(
                JobPriority::P1,
                ready_payload.clone(),
            ))
            .await
            .expect("ready enqueue succeeds");
        let ready_duplicate = queue
            .enqueue(EnqueueRequest::new(
                JobPriority::P0,
                ready_payload.clone(),
            ))
            .await
            .expect("ready duplicate enqueue succeeds");
        assert!(ready.accepted);
        assert!(!ready_duplicate.accepted);
        assert_eq!(ready_duplicate.job_id, ready.job_id);
        assert_eq!(ready_duplicate.merged_into, Some(ready.job_id));
        assert_eq!(
            active_dedupe_count(&pool, &ready.dedupe_key)
                .await
                .expect("ready count query succeeds"),
            1
        );
        assert_eq!(
            states_for_dedupe(&pool, &ready.dedupe_key)
                .await
                .expect("ready states query succeeds")
                .get("ready")
                .copied(),
            Some(1)
        );

        let ready_lease = queue
            .dequeue(DequeueRequest {
                kind: JobKind::FolderScan,
                worker_id: "ready-terminal-worker".to_string(),
                lease_ttl: chrono::Duration::seconds(30),
                selector: Some(QueueSelector {
                    library_id,
                    priority: JobPriority::P0,
                }),
            })
            .await
            .expect("ready dequeue succeeds")
            .expect("ready job leased");
        assert_eq!(ready_lease.job.id, ready.job_id);
        assert_eq!(
            queue
                .complete_with_outcome(ready_lease.lease_id)
                .await
                .expect("ready job completed"),
            QueueTransitionOutcome::Applied
        );
        assert_eq!(
            queue
                .complete_with_outcome(ready_lease.lease_id)
                .await
                .expect("repeated completion is reported"),
            QueueTransitionOutcome::Missing
        );
        assert_eq!(
            active_dedupe_count(&pool, &ready.dedupe_key)
                .await
                .expect("completed ready count query succeeds"),
            0,
            "completed rows should not participate in uq_jobs_dedupe_active"
        );

        let deferred_payload = folder_scan_payload(
            library_id,
            &library_root,
            &format!("{library_root}/deferred-movie"),
        );
        let deferred = queue
            .enqueue(
                EnqueueRequest::new(JobPriority::P1, deferred_payload.clone())
                    .with_dependency(DependencyKey::from(
                        "series-root:deferred-movie",
                    )),
            )
            .await
            .expect("deferred enqueue succeeds");
        let deferred_duplicate = queue
            .enqueue(EnqueueRequest::new(
                JobPriority::P0,
                deferred_payload.clone(),
            ))
            .await
            .expect("deferred duplicate enqueue succeeds");
        assert!(deferred.accepted);
        assert!(!deferred_duplicate.accepted);
        assert_eq!(deferred_duplicate.job_id, deferred.job_id);
        assert_eq!(deferred_duplicate.merged_into, Some(deferred.job_id));
        assert_eq!(
            active_dedupe_count(&pool, &deferred.dedupe_key)
                .await
                .expect("deferred count query succeeds"),
            1
        );
        assert_eq!(
            states_for_dedupe(&pool, &deferred.dedupe_key)
                .await
                .expect("deferred states query succeeds")
                .get("deferred")
                .copied(),
            Some(1)
        );

        let leased_payload = folder_scan_payload(
            library_id,
            &library_root,
            &format!("{library_root}/leased-movie"),
        );
        let leased = queue
            .enqueue(EnqueueRequest::new(
                JobPriority::P1,
                leased_payload.clone(),
            ))
            .await
            .expect("leased enqueue succeeds");
        let lease = queue
            .dequeue(DequeueRequest {
                kind: JobKind::FolderScan,
                worker_id: "leased-worker".to_string(),
                lease_ttl: chrono::Duration::seconds(30),
                selector: Some(QueueSelector {
                    library_id,
                    priority: JobPriority::P1,
                }),
            })
            .await
            .expect("leased dequeue succeeds")
            .expect("leased job exists");
        assert_eq!(lease.job.id, leased.job_id);

        let leased_duplicate = queue
            .enqueue(EnqueueRequest::new(JobPriority::P0, leased_payload))
            .await
            .expect("leased duplicate enqueue succeeds");
        assert!(!leased_duplicate.accepted);
        assert_eq!(leased_duplicate.job_id, leased.job_id);
        assert_eq!(leased_duplicate.merged_into, Some(leased.job_id));
        assert_eq!(
            active_dedupe_count(&pool, &leased.dedupe_key)
                .await
                .expect("leased count query succeeds"),
            1
        );
        assert_eq!(
            states_for_dedupe(&pool, &leased.dedupe_key)
                .await
                .expect("leased states query succeeds")
                .get("leased")
                .copied(),
            Some(1)
        );

        assert_eq!(
            queue
                .dead_letter_with_outcome(
                    lease.lease_id,
                    Some("terminal test".into()),
                )
                .await
                .expect("leased job dead-lettered"),
            QueueTransitionOutcome::Applied
        );
        let terminal_race_correlation = uuid::Uuid::now_v7();
        assert!(
            !enroll_active_job(
                &pool,
                Some(terminal_race_correlation),
                leased.job_id,
            )
            .await
            .expect("terminal merge enrollment is checked"),
            "a terminal lookup target must not satisfy a new enqueue"
        );
        let mut terminal_reenqueue_request = EnqueueRequest::new(
            JobPriority::P1,
            folder_scan_payload(
                library_id,
                &library_root,
                &format!("{library_root}/leased-movie"),
            ),
        );
        terminal_reenqueue_request.correlation_id =
            Some(terminal_race_correlation);
        let reenqueue_after_terminal = queue
            .enqueue(terminal_reenqueue_request)
            .await
            .expect("terminal re-enqueue succeeds");
        assert!(reenqueue_after_terminal.accepted);
        assert_ne!(reenqueue_after_terminal.job_id, leased.job_id);
        let terminal_race_jobs = queue
            .durable_job_states(terminal_race_correlation, &[])
            .await
            .expect("terminal race enrollment is queryable");
        assert_eq!(terminal_race_jobs.len(), 1);
        assert_eq!(
            terminal_race_jobs[0].job_id, reenqueue_after_terminal.job_id,
            "the new correlation must enroll only the runnable generation"
        );
        assert_eq!(
            active_dedupe_count(&pool, &leased.dedupe_key)
                .await
                .expect("terminal count query succeeds"),
            1
        );

        let retry_payload = folder_scan_payload(
            library_id,
            &library_root,
            &format!("{library_root}/retry-outcome"),
        );
        queue
            .enqueue(EnqueueRequest::new(JobPriority::P3, retry_payload))
            .await
            .expect("retry outcome job enqueue succeeds");
        let retry_lease = queue
            .dequeue(DequeueRequest {
                kind: JobKind::FolderScan,
                worker_id: "retry-outcome-worker".into(),
                lease_ttl: chrono::Duration::seconds(30),
                selector: Some(QueueSelector {
                    library_id,
                    priority: JobPriority::P3,
                }),
            })
            .await
            .expect("retry outcome dequeue succeeds")
            .expect("retry outcome job is leased");
        assert_eq!(
            queue
                .fail_with_outcome(
                    retry_lease.lease_id,
                    true,
                    Some("retryable".into()),
                )
                .await
                .expect("retry outcome persists"),
            FailOutcome::RetryScheduled
        );

        let failed_payload = folder_scan_payload(
            library_id,
            &library_root,
            &format!("{library_root}/failed-outcome"),
        );
        queue
            .enqueue(EnqueueRequest::new(JobPriority::P2, failed_payload))
            .await
            .expect("failed outcome job enqueue succeeds");
        let failed_lease = queue
            .dequeue(DequeueRequest {
                kind: JobKind::FolderScan,
                worker_id: "failed-outcome-worker".into(),
                lease_ttl: chrono::Duration::seconds(30),
                selector: Some(QueueSelector {
                    library_id,
                    priority: JobPriority::P2,
                }),
            })
            .await
            .expect("failed outcome dequeue succeeds")
            .expect("failed outcome job is leased");
        assert_eq!(
            queue
                .fail_with_outcome(
                    failed_lease.lease_id,
                    false,
                    Some("non-retryable".into()),
                )
                .await
                .expect("failed outcome persists"),
            FailOutcome::Terminal {
                state: JobState::Failed
            }
        );

        let exhausted_payload = folder_scan_payload(
            library_id,
            &library_root,
            &format!("{library_root}/exhausted-outcome"),
        );
        queue
            .enqueue(EnqueueRequest::new(JobPriority::P3, exhausted_payload))
            .await
            .expect("exhausted outcome job enqueue succeeds");
        let exhausted_lease = queue
            .dequeue(DequeueRequest {
                kind: JobKind::FolderScan,
                worker_id: "exhausted-outcome-worker".into(),
                lease_ttl: chrono::Duration::seconds(30),
                selector: Some(QueueSelector {
                    library_id,
                    priority: JobPriority::P3,
                }),
            })
            .await
            .expect("exhausted outcome dequeue succeeds")
            .expect("exhausted outcome job is leased");
        sqlx::query("UPDATE orchestrator_jobs SET attempts = $1 WHERE id = $2")
            .bind(i32::from(queue.retry_config.max_attempts))
            .bind(exhausted_lease.job.id.0)
            .execute(&pool)
            .await
            .expect("exhausted attempt count seeded");
        assert_eq!(
            queue
                .fail_with_outcome(
                    exhausted_lease.lease_id,
                    true,
                    Some("retry limit reached".into()),
                )
                .await
                .expect("exhausted outcome persists"),
            FailOutcome::Terminal {
                state: JobState::DeadLetter
            }
        );

        // A scan that merges onto active work durably enrolls each resolved
        // job under the new run without changing or broadly expanding the
        // older correlation.
        let old_correlation = uuid::Uuid::now_v7();
        let new_correlation = uuid::Uuid::now_v7();
        let lineage_root_payload = folder_scan_payload(
            library_id,
            &library_root,
            &format!("{library_root}/lineage-root"),
        );
        let mut old_root_request =
            EnqueueRequest::new(JobPriority::P1, lineage_root_payload.clone());
        old_root_request.correlation_id = Some(old_correlation);
        let old_root = queue
            .enqueue(old_root_request)
            .await
            .expect("old correlated root enqueue succeeds");

        let mut merged_root_request =
            EnqueueRequest::new(JobPriority::P1, lineage_root_payload);
        merged_root_request.correlation_id = Some(new_correlation);
        let merged_root = queue
            .enqueue(merged_root_request)
            .await
            .expect("new run merges onto old root");
        assert_eq!(merged_root.merged_into, Some(old_root.job_id));

        let shared_child_payload = folder_scan_payload(
            library_id,
            &library_root,
            &format!("{library_root}/shared-lineage-child"),
        );
        let mut old_shared_child_request =
            EnqueueRequest::new(JobPriority::P1, shared_child_payload.clone());
        old_shared_child_request.correlation_id = Some(old_correlation);
        let old_shared_child = queue
            .enqueue(old_shared_child_request)
            .await
            .expect("old shared child enqueue succeeds");

        let mut unrelated_sibling_request = EnqueueRequest::new(
            JobPriority::P1,
            folder_scan_payload(
                library_id,
                &library_root,
                &format!("{library_root}/unrelated-old-sibling"),
            ),
        );
        unrelated_sibling_request.correlation_id = Some(old_correlation);
        let unrelated_sibling = queue
            .enqueue(unrelated_sibling_request)
            .await
            .expect("unrelated old-correlation sibling enqueue succeeds");

        let mut accepted_child_request = EnqueueRequest::new(
            JobPriority::P1,
            folder_scan_payload(
                library_id,
                &library_root,
                &format!("{library_root}/new-lineage-child"),
            ),
        );
        accepted_child_request.correlation_id = Some(new_correlation);
        let mut merged_child_request =
            EnqueueRequest::new(JobPriority::P1, shared_child_payload);
        merged_child_request.correlation_id = Some(new_correlation);
        let descendants = queue
            .enqueue_many(vec![accepted_child_request, merged_child_request])
            .await
            .expect("new run descendant batch enqueue succeeds");
        assert!(descendants[0].accepted);
        assert_eq!(descendants[1].merged_into, Some(old_shared_child.job_id));

        let reconciled = queue
            .durable_job_states(new_correlation, &[])
            .await
            .expect("merged run durable reconciliation succeeds");
        let reconciled_ids: std::collections::HashSet<_> =
            reconciled.iter().map(|job| job.job_id).collect();
        assert!(reconciled_ids.contains(&old_root.job_id));
        assert!(
            reconciled_ids.contains(&descendants[0].job_id),
            "accepted descendants must be durably enrolled"
        );
        assert!(
            reconciled_ids.contains(&old_shared_child.job_id),
            "merged descendants must be durably enrolled"
        );
        assert!(
            !reconciled_ids.contains(&unrelated_sibling.job_id),
            "tracked merged root must not enroll unrelated old-correlation siblings"
        );

        sqlx::query("DELETE FROM libraries WHERE id = $1")
            .bind(library_id.to_uuid())
            .execute(&pool)
            .await
            .expect("cleanup library succeeds");
    }
}

/// Postgres-backed scan cursor repository. All methods are stubs for now.
pub struct PostgresCursorRepository {
    pool: PgPool,
}

impl PostgresCursorRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl fmt::Debug for PostgresCursorRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresCursorRepository")
            .field("pool_size", &self.pool.size())
            .field("idle_connections", &self.pool.num_idle())
            .finish()
    }
}

#[async_trait]
impl ScanCursorRepository for PostgresCursorRepository {
    async fn get(&self, id: &ScanCursorId) -> Result<Option<ScanCursor>> {
        let result = sqlx::query!(
            r#"
            SELECT folder_path_norm, listing_hash, entry_count, last_scan_at, last_modified_at, device_id
            FROM scan_cursors
            WHERE library_id = $1 AND path_hash = $2
            "#,
            id.library_id.0,
            id.path_hash as i64
        )
            .fetch_optional(&self.pool)
            .await?;

        Ok(result.map(|row| ScanCursor {
            id: id.clone(),
            folder_path_norm: row.folder_path_norm,
            listing_hash: row.listing_hash,
            entry_count: row.entry_count as usize,
            last_scan_at: row.last_scan_at,
            last_modified_at: row.last_modified_at,
            device_id: row.device_id,
        }))
    }

    async fn list_by_library(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<ScanCursor>> {
        let results = sqlx::query!(
            r#"
            SELECT path_hash, folder_path_norm, listing_hash, entry_count, last_scan_at, last_modified_at, device_id
            FROM scan_cursors
            WHERE library_id = $1
            ORDER BY folder_path_norm ASC
            "#,
            library_id.0
        )
            .fetch_all(&self.pool)
            .await?;

        Ok(results
            .into_iter()
            .map(|row| ScanCursor {
                id: ScanCursorId {
                    library_id,
                    path_hash: row.path_hash as u64,
                },
                folder_path_norm: row.folder_path_norm,
                listing_hash: row.listing_hash,
                entry_count: row.entry_count as usize,
                last_scan_at: row.last_scan_at,
                last_modified_at: row.last_modified_at,
                device_id: row.device_id,
            })
            .collect())
    }

    async fn upsert(&self, cursor: ScanCursor) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO scan_cursors (
                library_id, path_hash, folder_path_norm, listing_hash,
                entry_count, last_scan_at, last_modified_at, device_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (library_id, path_hash)
            DO UPDATE SET
                listing_hash = EXCLUDED.listing_hash,
                entry_count = EXCLUDED.entry_count,
                last_scan_at = EXCLUDED.last_scan_at,
                last_modified_at = EXCLUDED.last_modified_at,
                device_id = EXCLUDED.device_id
            "#,
            cursor.id.library_id.0,
            cursor.id.path_hash as i64,
            &cursor.folder_path_norm,
            &cursor.listing_hash,
            cursor.entry_count as i32,
            cursor.last_scan_at,
            cursor.last_modified_at,
            cursor.device_id.as_deref()
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn delete_by_library(&self, library_id: LibraryId) -> Result<usize> {
        let result = sqlx::query!(
            r#"
            DELETE FROM scan_cursors
            WHERE library_id = $1
            "#,
            library_id.0
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }

    async fn delete_by_path_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
    ) -> Result<usize> {
        if prefixes.is_empty() {
            return Ok(0);
        }

        let roots: Vec<String> = prefixes
            .iter()
            .map(|prefix| prefix.trim_end_matches('/').to_owned())
            .collect();

        let result = sqlx::query!(
            r#"
            WITH target_prefixes AS (
                SELECT root,
                       root || '/%' AS child_pattern
                FROM UNNEST($2::text[]) AS root
            )
            DELETE FROM scan_cursors AS sc
            USING target_prefixes AS p
            WHERE sc.library_id = $1
              AND (sc.folder_path_norm = p.root OR sc.folder_path_norm LIKE p.child_pattern)
            "#,
            library_id.0,
            &roots
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }

    async fn list_stale(
        &self,
        library_id: LibraryId,
        older_than: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<ScanCursor>> {
        let results = sqlx::query!(
            r#"
            SELECT path_hash, folder_path_norm, listing_hash, entry_count, last_scan_at, last_modified_at, device_id
            FROM scan_cursors
            WHERE library_id = $1 AND last_scan_at < $2
            ORDER BY last_scan_at ASC
            "#,
            library_id.0,
            older_than
        )
            .fetch_all(&self.pool)
            .await?;

        Ok(results
            .into_iter()
            .map(|row| ScanCursor {
                id: ScanCursorId {
                    library_id,
                    path_hash: row.path_hash as u64,
                },
                folder_path_norm: row.folder_path_norm,
                listing_hash: row.listing_hash,
                entry_count: row.entry_count as usize,
                last_scan_at: row.last_scan_at,
                last_modified_at: row.last_modified_at,
                device_id: row.device_id,
            })
            .collect())
    }
}
