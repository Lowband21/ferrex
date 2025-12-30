//! Postgres-backed persistence scaffolding for the orchestrator.
//! NOTE: This file only defines function signatures and stubs (todo!()).
//! Actual SQL implementations will be added after migrations are applied.

use crate::error::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::from_value;
use sqlx::PgPool;
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use tracing::{debug, info, trace, warn};

use crate::domain::scan::orchestration::{
    config::RetryConfig,
    job::{
        DependencyKey, EnqueueRequest, JobHandle, JobId, JobKind, JobPayload,
        JobPriority, ScanReason,
    },
    lease::{DequeueRequest, JobLease, LeaseId, LeaseRenewal},
    queue::{
        LeaseExpiryScanner, QueueInstrumentation, QueueService, QueueSnapshot,
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

impl fmt::Debug for PostgresQueueService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresQueueService")
            .field("pool_size", &self.pool.size())
            .field("idle_connections", &self.pool.num_idle())
            .field("retry_config", &self.retry_config)
            .finish()
    }
}

/// Aggregated ready-state counts grouped by queue dimensions.
#[derive(Clone, Debug)]
pub struct ReadyQueueCount {
    pub kind: JobKind,
    pub library_id: LibraryId,
    pub priority: JobPriority,
    pub ready: usize,
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

    /// Fetch grouped ready counts directly from persistence. Used to prime the
    /// in-memory scheduler after a cold start.
    pub async fn ready_counts_grouped(&self) -> Result<Vec<ReadyQueueCount>> {
        let rows = sqlx::query!(
            r#"
            SELECT kind, library_id, priority, COUNT(*)::bigint AS ready
            FROM orchestrator_jobs
            WHERE state = 'ready'
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
            let ready = row.ready.unwrap_or(0).max(0i64) as usize;
            counts.push(ReadyQueueCount {
                kind,
                library_id: LibraryId(row.library_id),
                priority,
                ready,
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
        let dependency_key = request
            .dependency_key
            .as_ref()
            .map(|key| key.as_str().to_string());
        let state = if dependency_key.is_some() {
            "deferred"
        } else {
            "ready"
        };

        // Fast path: if an active job with the same dedupe_key exists, merge without
        // causing a unique violation. This avoids noisy ERROR logs in Postgres.
        if let Some(existing) = sqlx::query!(
            r#"
            SELECT id, priority
            FROM orchestrator_jobs
            WHERE dedupe_key = $1
              AND state IN ('ready','deferred','leased')
            ORDER BY created_at ASC
            LIMIT 1
            "#,
            dedupe_key
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("enqueue precheck failed: {e}"))
        })? {
            let existing_id =
                crate::domain::scan::orchestration::job::JobId(existing.id);
            let existing_priority: i16 = existing.priority;
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
                    existing.id
                )
                .execute(&self.pool)
                .await;
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
            INSERT INTO orchestrator_jobs (
                id, library_id, kind, payload, priority, state,
                attempts, available_at, lease_owner, lease_id, lease_expires_at,
                dedupe_key, dependency_key, last_error, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, 0, NOW(), NULL, NULL, NULL, $7, $8, NULL, NOW(), NOW())
            "#,
            job_id.0,
            library_id,
            kind,
            payload_json,
            priority_val,
            state,
            dedupe_key,
            dependency_key
        )
            .execute(&self.pool)
            .await;

        match insert_res {
            Ok(_) => {
                trace!("enqueue accepted new job {}", job_id.0);
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
                        SELECT id, priority, available_at, state
                        FROM orchestrator_jobs
                        WHERE dedupe_key = $1
                          AND state IN ('ready','deferred','leased')
                        ORDER BY created_at ASC
                        LIMIT 1
                        "#,
                        dedupe_key
                    )
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "enqueue conflict lookup failed: {e}"
                        ))
                    })?;

                    if let Some(row) = existing {
                        // Elevate priority if incoming is higher (lower numeric value)
                        let existing_pri: i16 = row.priority;
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
                                row.id
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
                                    row.id, priority_val
                                );
                            } else {
                                // Likely leased or moved terminal concurrently; best-effort merge only
                                info!(
                                    "enqueue merge: elevation skipped due to state transition for job {}",
                                    row.id
                                );
                            }
                        } else {
                            info!(
                                "enqueue merged into existing job {} without priority change",
                                row.id
                            );
                        }
                        return Ok(JobHandle::merged(
                            crate::domain::scan::orchestration::job::JobId(
                                row.id,
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
                            INSERT INTO orchestrator_jobs (
                                id, library_id, kind, payload, priority, state,
                                attempts, available_at, lease_owner, lease_id, lease_expires_at,
                                dedupe_key, last_error, created_at, updated_at
                            )
                            VALUES ($1, $2, $3, $4, $5, 'ready', 0, NOW(), NULL, NULL, NULL, $6, NULL, NOW(), NOW())
                            "#,
                            job_id2.0,
                            library_id,
                            kind,
                            payload_json,
                            priority_val,
                            dedupe_key
                        )
                            .execute(&self.pool)
                            .await;

                        match retry {
                            Ok(_) => {
                                info!(
                                    "enqueue accepted new job {} on retry",
                                    job_id2.0
                                );
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
                                    SELECT id
                                    FROM orchestrator_jobs
                                    WHERE dedupe_key = $1
                                      AND state IN ('ready','deferred','leased')
                                    ORDER BY created_at ASC
                                    LIMIT 1
                                    "#,
                                    dedupe_key
                                )
                                        .fetch_optional(&self.pool)
                                        .await
                                        .map_err(|e| {
                                            MediaError::Internal(format!(
                                                "enqueue conflict lookup (retry) failed: {e}"
                                            ))
                                        })?;

                                if let Some(w) = winner {
                                    return Ok(JobHandle::merged(
                                            crate::domain::scan::orchestration::job::JobId(
                                                w.id,
                                            ),
                                            &request.payload,
                                            request.priority,
                                        ));
                                }

                                return Err(MediaError::Internal(
                                        "enqueue conflict retry: could not resolve existing row".into(),
                                    ));
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

        for request in requests {
            request.validate()?;
            let job_id = crate::domain::scan::orchestration::job::JobId::new();
            let payload_json =
                serde_json::to_value(&request.payload).map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to serialize job payload: {e}"
                    ))
                })?;
            let library_id = request.payload.library_id().to_uuid();
            let kind = request.payload.kind();
            let dedupe_key = request.dedupe_key().to_string();
            let priority_val: i16 = request.priority as u8 as i16;
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
                SELECT id, priority
                FROM orchestrator_jobs
                WHERE dedupe_key = $1
                  AND state IN ('ready','deferred','leased')
                ORDER BY created_at ASC
                LIMIT 1
                "#,
                dedupe_key
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "enqueue_many precheck failed: {e}"
                ))
            })? {
                let existing_id =
                    crate::domain::scan::orchestration::job::JobId(existing.id);
                let existing_priority: i16 = existing.priority;
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
                        existing.id
                    )
                    .execute(&mut *tx)
                    .await;
                }
                out.push(JobHandle::merged(
                    existing_id,
                    &request.payload,
                    request.priority,
                ));
                continue;
            }

            // Try insert; merge on unique violation
            let insert_res = sqlx::query!(
                r#"
                INSERT INTO orchestrator_jobs (
                    id, library_id, kind, payload, priority, state,
                    attempts, available_at, lease_owner, lease_id, lease_expires_at,
                    dedupe_key, dependency_key, last_error, created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, 0, NOW(), NULL, NULL, NULL, $7, $8, NULL, NOW(), NOW())
                "#,
                job_id.0,
                library_id,
                kind as i16,
                payload_json,
                priority_val,
                state,
                dedupe_key,
                dependency_key
            )
                .execute(&mut *tx)
                .await;

            match insert_res {
                Ok(_) => {
                    info!("enqueue_many accepted new job {}", job_id.0);
                    out.push(JobHandle::accepted(
                        job_id,
                        &request.payload,
                        request.priority,
                    ));
                }
                Err(sqlx::Error::Database(db_err)) => {
                    let code = db_err.code().map(|c| c.to_string());
                    if code.as_deref() == Some("23505") {
                        let existing = sqlx::query!(
                            r#"
                            SELECT id, priority, available_at, state
                            FROM orchestrator_jobs
                            WHERE dedupe_key = $1
                              AND state IN ('ready','deferred','leased')
                            ORDER BY created_at ASC
                            LIMIT 1
                            "#,
                            request.dedupe_key().to_string()
                        )
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(|e| {
                            MediaError::Internal(format!(
                                "enqueue_many conflict lookup failed: {e}"
                            ))
                        })?;

                        if let Some(row) = existing {
                            let existing_pri: i16 = row.priority;
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
                                    row.id
                                )
                                    .execute(&mut *tx)
                                    .await
                                    .map_err(|e| {
                                        MediaError::Internal(format!(
                                            "enqueue_many merge elevation failed: {e}"
                                        ))
                                    })?;
                            }
                            out.push(JobHandle::merged(
                                crate::domain::scan::orchestration::job::JobId(
                                    row.id,
                                ),
                                &request.payload,
                                request.priority,
                            ));
                        } else {
                            return Err(MediaError::Internal(
                                "enqueue_many conflict but no existing job found".into(),
                            ));
                        }
                    } else {
                        // Unexpected DB error => abort whole batch
                        drop(tx.rollback().await);
                        return Err(MediaError::Internal(format!(
                            "enqueue_many insert failed: {db_err}"
                        )));
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

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("enqueue_many tx commit failed: {e}"))
        })?;

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
                ), fallback AS (
                    SELECT id
                    FROM orchestrator_jobs
                    WHERE state = 'ready'
                      AND kind = $1
                      AND available_at <= NOW()
                      AND NOT EXISTS (SELECT 1 FROM next)
                    ORDER BY available_at, attempts, created_at
                    FOR UPDATE SKIP LOCKED
                    LIMIT 1
                )
                SELECT j.id, j.payload, j.priority, j.attempts,
                       j.available_at, j.dedupe_key, j.dependency_key, j.created_at
                FROM orchestrator_jobs j
                JOIN (
                    SELECT id FROM next
                    UNION ALL
                    SELECT id FROM fallback
                    LIMIT 1
                ) pick ON pick.id = j.id
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
                    created_at: row.created_at,
                })
        } else {
            sqlx::query!(
                r#"
                SELECT id, payload, priority, attempts, available_at,
                       dedupe_key, dependency_key, created_at
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
                dedupe_key, dependency_key, created_at, updated_at,
                lease_owner, lease_expires_at
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
        }
        Ok(())
    }

    async fn fail(
        &self,
        lease_id: LeaseId,
        retryable: bool,
        error: Option<String>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            MediaError::Internal(format!("begin fail tx failed: {e}"))
        })?;

        // Lock the row and get current attempts
        let row = sqlx::query!(
            r#"
            SELECT id, attempts, library_id, payload
            FROM orchestrator_jobs
            WHERE lease_id = $1::uuid AND state = 'leased'
            FOR UPDATE
            "#,
            lease_id.0
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("fail select failed: {e}"))
        })?;

        let Some(row) = row else {
            drop(tx);
            return Ok(());
        };

        let attempts_before: i32 = row.attempts;
        let max_attempts = i32::from(self.retry_config.max_attempts);
        let attempt_next = attempts_before.saturating_add(1) as u16;
        let job_id = JobId(row.id);
        let library_id = LibraryId(row.library_id);
        let payload: JobPayload = from_value(row.payload).map_err(|e| {
            MediaError::Internal(format!(
                "fail payload decode failed for job {}: {e}",
                row.id
            ))
        })?;

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
                row.id,
                error,
                delay_ms as i64
            )
                .execute(&mut *tx)
                .await
                .map_err(|e| MediaError::Internal(format!("fail retry update failed: {e}")))?;

            tx.commit().await.map_err(|e| {
                MediaError::Internal(format!("fail tx commit failed: {e}"))
            })?;

            warn!(
                "job {} failed retryable; attempts now {}; scheduled retry in {}ms (pressure={})",
                row.id,
                attempts_before + 1,
                delay_ms,
                library_under_pressure
            );
            Ok(())
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
                row.id,
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

            warn!(
                "job {} moved to {} after attempts {}",
                row.id, new_state, attempts_before
            );
            Ok(())
        }
    }

    async fn dead_letter(
        &self,
        lease_id: LeaseId,
        error: Option<String>,
    ) -> Result<()> {
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
            warn!("job with lease {:?} moved to dead_letter", lease_id.0);
        }
        Ok(())
    }

    async fn cancel_job(&self, job_id: JobId) -> Result<()> {
        // Delete only non-leased jobs; leased jobs require different handling.
        let _ = sqlx::query!(
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
