//! Postgres-backed persistence scaffolding for the orchestrator.
//! NOTE: This file only defines function signatures and stubs (todo!()).
//! Actual SQL implementations will be added after migrations are applied.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use rand::{Rng, rngs::StdRng, thread_rng};
use sqlx::PgPool;
use tracing::{info, warn};

use crate::orchestration::{
    config::RetryConfig,
    job::{DedupeKey, EnqueueRequest, JobHandle, JobId, JobKind, JobPayload, JobPriority},
    lease::{CompletionOutcome, DequeueRequest, JobLease, LeaseId, LeaseRenewal},
    queue::{ALL_JOB_KINDS, LeaseExpiryScanner, QueueInstrumentation, QueueService, QueueSnapshot},
    scan_cursor::{ScanCursor, ScanCursorId, ScanCursorRepository},
};
use crate::{LibraryID, MediaError, Result};

/// Durable queue backed by Postgres. All methods are stubs for now.
#[derive(Clone)]
pub struct PostgresQueueService {
    pool: PgPool,
    retry_config: RetryConfig,
}

/// Aggregated ready-state counts grouped by queue dimensions.
#[derive(Clone, Debug)]
pub struct ReadyQueueCount {
    pub kind: JobKind,
    pub library_id: LibraryID,
    pub priority: JobPriority,
    pub ready: usize,
}

impl PostgresQueueService {
    /// Create a new Postgres-backed queue service and verify DB health + schema.
    pub async fn new(pool: PgPool) -> Result<Self> {
        Self::new_with_retry(pool, RetryConfig::default()).await
    }

    /// Create a new service with an explicit retry policy.
    pub async fn new_with_retry(pool: PgPool, retry_config: RetryConfig) -> Result<Self> {
        // Health check
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Queue service failed Postgres health check: {e}"))
            })?;
        info!("Queue service connected to Postgres");

        // Schema validation: ensure critical dequeue index exists
        let idx_exists = sqlx::query_scalar::<_, Option<i32>>(
            r#"
            SELECT 1
            FROM pg_indexes
            WHERE schemaname = 'public'
              AND indexname = $1
            LIMIT 1
            "#,
        )
        .bind("idx_jobs_ready_dequeue")
        .fetch_optional(&pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Queue service schema validation failed: {e}")))?
        .is_some();

        if !idx_exists {
            return Err(MediaError::Internal(
                "Required index idx_jobs_ready_dequeue is missing; run migrations".into(),
            ));
        }

        Ok(Self { pool, retry_config })
    }

    /// Housekeeping: scan for expired leases and resurrect them according to backoff policy.
    /// Returns the number of jobs transitioned back to Ready.
    pub async fn scan_expired_leases(&self) -> Result<u64> {
        let expired = sqlx::query!(
            r#"
            SELECT id, attempts
            FROM orchestrator_jobs
            WHERE state = 'leased'
              AND lease_expires_at IS NOT NULL
              AND lease_expires_at < NOW()
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("lease expiry scan failed: {e}")))?;

        let mut resurrected = 0u64;
        let max_attempts = i32::from(self.retry_config.max_attempts);

        for row in expired {
            let attempts_before = row.attempts;
            if attempts_before < max_attempts {
                let delay_ms = {
                    let mut rng = thread_rng();
                    self.jittered_delay_ms((attempts_before + 1) as u16, &mut rng)
                };
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
        let scaled = (self.retry_config.backoff_base_ms as f64) * 2f64.powi(exp);
        let capped = scaled.min(self.retry_config.backoff_max_ms as f64);
        capped.max(0.0) as u64
    }

    fn jittered_delay_ms(&self, attempt: u16, rng: &mut impl Rng) -> u64 {
        let base = self.base_delay_ms(attempt);
        if base == 0 {
            return 0;
        }

        let upper_cap = self.retry_config.backoff_max_ms.max(1);
        let capped = base.min(upper_cap);
        let spread = (capped as f64 * 0.25).max(1.0);
        let lower = (capped as f64 - spread).max(1.0);
        let upper = (capped as f64 + spread).min(upper_cap as f64);

        rng.gen_range(lower..=upper).round() as u64
    }

    fn parse_kind(kind: &str) -> Option<JobKind> {
        match kind {
            "scan" => Some(JobKind::FolderScan),
            "analyze" => Some(JobKind::MediaAnalyze),
            "metadata" => Some(JobKind::MetadataEnrich),
            "index" => Some(JobKind::IndexUpsert),
            "image" => Some(JobKind::ImageFetch),
            _ => None,
        }
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
        .map_err(|e| MediaError::Internal(format!("ready count aggregation failed: {e}")))?;

        let mut counts = Vec::with_capacity(rows.len());

        for row in rows {
            let Some(kind) = Self::parse_kind(&row.kind) else {
                continue;
            };

            let priority = Self::parse_priority(row.priority)?;
            let ready = row.ready.unwrap_or(0).max(0i64) as usize;
            counts.push(ReadyQueueCount {
                kind,
                library_id: LibraryID(row.library_id),
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
        .map_err(|e| MediaError::Internal(format!("metrics snapshot failed: {e}")))?;

        let mut snapshot = QueueSnapshot::new(Utc::now());
        for kind in ALL_JOB_KINDS {
            snapshot.entry_mut(kind);
        }

        for row in rows {
            let Some(kind) = Self::parse_kind(&row.kind) else {
                continue;
            };

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
        use crate::orchestration::job::JobPriority;

        let job_id = crate::orchestration::job::JobId::new();
        let payload_json = serde_json::to_value(&request.payload)
            .map_err(|e| MediaError::Internal(format!("failed to serialize job payload: {e}")))?;
        let library_id = request.payload.library_id().as_uuid();
        let kind_str = request.payload.kind().to_string();
        let dedupe_key = request.dedupe_key().to_string();
        let priority_val: i16 = request.priority as u8 as i16;

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
        .map_err(|e| MediaError::Internal(format!("enqueue precheck failed: {e}")))?
        {
            let existing_id = crate::orchestration::job::JobId(existing.id);
            let existing_priority: i16 = existing.priority as i16;
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
                dedupe_key, last_error, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, 'ready', 0, NOW(), NULL, NULL, NULL, $6, NULL, NOW(), NOW())
            "#,
            job_id.0,
            library_id,
            kind_str,
            payload_json,
            priority_val,
            dedupe_key
        )
        .execute(&self.pool)
        .await;

        match insert_res {
            Ok(_) => {
                info!("enqueue accepted new job {}", job_id.0);
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
                        MediaError::Internal(format!("enqueue conflict lookup failed: {e}"))
                    })?;

                    if let Some(row) = existing {
                        // Elevate priority if incoming is higher (lower numeric value)
                        let existing_pri: i16 = row.priority as i16;
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
                                MediaError::Internal(format!("enqueue merge elevation failed: {e}"))
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
                            crate::orchestration::job::JobId(row.id),
                            &request.payload,
                            request.priority,
                        ));
                    } else {
                        // No active row found; try a fresh insert once and, on conflict again, return the found ID
                        let job_id2 = crate::orchestration::job::JobId::new();
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
                            kind_str,
                            payload_json,
                            priority_val,
                            dedupe_key
                        )
                        .execute(&self.pool)
                        .await;

                        match retry {
                            Ok(_) => {
                                info!("enqueue accepted new job {} on retry", job_id2.0);
                                return Ok(JobHandle::accepted(
                                    job_id2,
                                    &request.payload,
                                    request.priority,
                                ));
                            }
                            Err(sqlx::Error::Database(db_err2))
                                if db_err2.code().map(|c| c.to_string()).as_deref()
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
                                        crate::orchestration::job::JobId(w.id),
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
            Err(e) => return Err(MediaError::Internal(format!("enqueue insert failed: {e}"))),
        }
    }

    async fn enqueue_many(&self, requests: Vec<EnqueueRequest>) -> Result<Vec<JobHandle>> {
        use crate::orchestration::job::JobPriority;

        if requests.is_empty() {
            return Ok(Vec::new());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("begin enqueue_many tx failed: {e}")))?;

        let mut out: Vec<JobHandle> = Vec::with_capacity(requests.len());

        for request in requests {
            let job_id = crate::orchestration::job::JobId::new();
            let payload_json = serde_json::to_value(&request.payload).map_err(|e| {
                MediaError::Internal(format!("failed to serialize job payload: {e}"))
            })?;
            let library_id = request.payload.library_id().as_uuid();
            let kind_str = request.payload.kind().to_string();
            let dedupe_key = request.dedupe_key().to_string();
            let priority_val: i16 = request.priority as u8 as i16;

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
            .map_err(|e| MediaError::Internal(format!("enqueue_many precheck failed: {e}")))?
            {
                let existing_id = crate::orchestration::job::JobId(existing.id);
                let existing_priority: i16 = existing.priority as i16;
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
                    dedupe_key, last_error, created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, 'ready', 0, NOW(), NULL, NULL, NULL, $6, NULL, NOW(), NOW())
                "#,
                job_id.0,
                library_id,
                kind_str,
                payload_json,
                priority_val,
                dedupe_key
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
                            let existing_pri: i16 = row.priority as i16;
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
                                crate::orchestration::job::JobId(row.id),
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

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("enqueue_many tx commit failed: {e}")))?;

        Ok(out)
    }

    async fn dequeue(&self, request: DequeueRequest) -> Result<Option<JobLease>> {
        use crate::orchestration::job::{JobPriority, JobRecord, JobState};
        use uuid::Uuid;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("begin dequeue tx failed: {e}")))?;

        // Select next eligible job for this kind
        let kind_str = request.kind.to_string();

        struct SelectedRow {
            id: Uuid,
            library_id: Uuid,
            kind: String,
            payload: serde_json::Value,
            priority: i16,
            attempts: i32,
            available_at: chrono::DateTime<chrono::Utc>,
            dedupe_key: String,
            created_at: chrono::DateTime<chrono::Utc>,
            updated_at: chrono::DateTime<chrono::Utc>,
        }

        let row: Option<SelectedRow> = if let Some(selector) = request.selector {
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
                SELECT j.id, j.library_id, j.kind, j.payload, j.priority, j.attempts,
                       j.available_at, j.dedupe_key, j.created_at, j.updated_at
                FROM orchestrator_jobs j
                JOIN (
                    SELECT id FROM next
                    UNION ALL
                    SELECT id FROM fallback
                    LIMIT 1
                ) pick ON pick.id = j.id
                "#,
                kind_str,
                selector.library_id.as_uuid(),
                priority
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("dequeue select failed: {e}")))?
            .map(|row| SelectedRow {
                id: row.id,
                library_id: row.library_id,
                kind: row.kind,
                payload: row.payload,
                priority: row.priority,
                attempts: row.attempts,
                available_at: row.available_at,
                dedupe_key: row.dedupe_key,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
        } else {
            sqlx::query!(
                r#"
                SELECT id, library_id, kind, payload, priority, attempts, available_at,
                       dedupe_key, created_at, updated_at
                FROM orchestrator_jobs
                WHERE kind = $1
                  AND state = 'ready'
                  AND available_at <= NOW()
                ORDER BY priority ASC, available_at ASC, attempts ASC, created_at ASC
                LIMIT 1
                FOR UPDATE SKIP LOCKED
                "#,
                kind_str
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("dequeue select failed: {e}")))?
            .map(|row| SelectedRow {
                id: row.id,
                library_id: row.library_id,
                kind: row.kind,
                payload: row.payload,
                priority: row.priority,
                attempts: row.attempts,
                available_at: row.available_at,
                dedupe_key: row.dedupe_key,
                created_at: row.created_at,
                updated_at: row.updated_at,
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
        .map_err(|e| MediaError::Internal(format!("dequeue update->leased failed: {e}")))?;

        if updated.is_none() {
            // Raced with state change; treat as empty
            drop(tx);
            return Ok(None);
        }

        // Build JobRecord from the selected row and new lease fields
        let payload: JobPayload = serde_json::from_value(row.payload)
            .map_err(|e| MediaError::Internal(format!("failed to deserialize job payload: {e}")))?;

        let priority = match row.priority as i16 {
            0 => JobPriority::P0,
            1 => JobPriority::P1,
            2 => JobPriority::P2,
            3 => JobPriority::P3,
            other => return Err(MediaError::Internal(format!("invalid priority {}", other))),
        };

        let job = JobRecord {
            id: JobId(row.id),
            payload,
            priority,
            state: JobState::Leased,
            attempts: (row.attempts as i32).max(0) as u16,
            available_at: row.available_at,
            lease_owner: Some(request.worker_id.clone()),
            lease_expires_at: Some(expires_at),
            backoff_until: None,
            dedupe_key: row.dedupe_key,
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

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("dequeue tx commit failed: {e}")))?;

        Ok(Some(lease))
    }

    async fn renew(&self, renewal: LeaseRenewal) -> Result<JobLease> {
        use crate::orchestration::job::{JobPriority, JobRecord, JobState};

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("begin renew tx failed: {e}")))?;

        // Lock the row to ensure consistent renewal
        let row = sqlx::query!(
            r#"
            SELECT id, library_id, kind, payload, priority, attempts, available_at,
                   dedupe_key, created_at, updated_at,
                   lease_owner, lease_expires_at
            FROM orchestrator_jobs
            WHERE lease_id = $1 AND state = 'leased' AND lease_expires_at > NOW()
            FOR UPDATE
            "#,
            renewal.lease_id.0
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("renew select failed: {e}")))?;

        let Some(row) = row else {
            warn!(
                "renewal failed: lease {:?} not found or expired",
                renewal.lease_id.0
            );
            drop(tx);
            return Err(MediaError::NotFound("lease not found or expired".into()));
        };

        // Perform the extension
        let extend_ms: i64 = renewal.extend_by.num_milliseconds() as i64;
        let updated = sqlx::query!(
            r#"
            UPDATE orchestrator_jobs
SET lease_expires_at = lease_expires_at + ($1::bigint) * INTERVAL '1 millisecond',
                updated_at = NOW()
            WHERE lease_id = $2 AND state = 'leased'
            RETURNING lease_expires_at
            "#,
            extend_ms,
            renewal.lease_id.0
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("renew update failed: {e}")))?;

        let payload: JobPayload = serde_json::from_value(row.payload)
            .map_err(|e| MediaError::Internal(format!("failed to deserialize job payload: {e}")))?;

        let priority = match row.priority as i16 {
            0 => JobPriority::P0,
            1 => JobPriority::P1,
            2 => JobPriority::P2,
            3 => JobPriority::P3,
            other => return Err(MediaError::Internal(format!("invalid priority {}", other))),
        };

        let job = JobRecord {
            id: JobId(row.id),
            payload,
            priority,
            state: JobState::Leased,
            attempts: (row.attempts as i32).max(0) as u16,
            available_at: row.available_at,
            lease_owner: row.lease_owner,
            lease_expires_at: updated.lease_expires_at,
            backoff_until: None,
            dedupe_key: row.dedupe_key,
            created_at: row.created_at,
            updated_at: chrono::Utc::now(),
        };

        let lease_owner_str = job.lease_owner.clone().unwrap_or_default();
        let lease = JobLease {
            lease_id: renewal.lease_id,
            job,
            lease_owner: lease_owner_str,
            expires_at: updated
                .lease_expires_at
                .expect("lease_expires_at must be set"),
            renewals: 1, // local increment only
        };

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("renew tx commit failed: {e}")))?;

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
                lease_expires_at=NULL,
                updated_at=NOW()
            WHERE lease_id = $1 AND state='leased'
            "#,
            lease_id.0
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("complete update failed: {e}")))?;

        if res.rows_affected() > 0 {
            info!("completed job with lease {:?}", lease_id.0);
        }
        Ok(())
    }

    async fn fail(&self, lease_id: LeaseId, retryable: bool, error: Option<String>) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("begin fail tx failed: {e}")))?;

        // Lock the row and get current attempts
        let row = sqlx::query!(
            r#"
            SELECT id, attempts
            FROM orchestrator_jobs
            WHERE lease_id = $1
            FOR UPDATE
            "#,
            lease_id.0
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("fail select failed: {e}")))?;

        let Some(row) = row else {
            drop(tx);
            return Ok(());
        };

        let attempts_before: i32 = row.attempts;
        let max_attempts = i32::from(self.retry_config.max_attempts);

        if retryable && attempts_before < max_attempts {
            let delay_ms = {
                let mut rng = thread_rng();
                self.jittered_delay_ms((attempts_before + 1) as u16, &mut rng)
            };

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

            tx.commit()
                .await
                .map_err(|e| MediaError::Internal(format!("fail tx commit failed: {e}")))?;

            warn!(
                "job {} failed retryable; attempts now {}; scheduled retry",
                row.id,
                attempts_before + 1
            );
            Ok(())
        } else {
            // Terminal: dead-letter or failed
            let new_state = if retryable { "dead_letter" } else { "failed" };
            let res = sqlx::query!(
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
            .map_err(|e| MediaError::Internal(format!("fail terminal update failed: {e}")))?;

            tx.commit()
                .await
                .map_err(|e| MediaError::Internal(format!("fail tx commit failed: {e}")))?;

            warn!(
                "job {} moved to {} after attempts {}",
                row.id, new_state, attempts_before
            );
            Ok(())
        }
    }

    async fn dead_letter(&self, lease_id: LeaseId, error: Option<String>) -> Result<()> {
        let res = sqlx::query!(
            r#"
            UPDATE orchestrator_jobs
            SET state='dead_letter',
                lease_owner=NULL,
                lease_id=NULL,
                lease_expires_at=NULL,
                last_error=$2,
                updated_at=NOW()
            WHERE lease_id = $1
            "#,
            lease_id.0,
            error
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("dead_letter update failed: {e}")))?;

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
        .map_err(|e| MediaError::Internal(format!("cancel_job delete failed: {e}")))?;
        Ok(())
    }

    async fn queue_depth(&self, kind: JobKind) -> Result<usize> {
        let kind_str = kind.to_string();
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::bigint
            FROM orchestrator_jobs
            WHERE kind = $1 AND state = 'ready'
            "#,
        )
        .bind(kind_str)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("queue_depth query failed: {e}")))?;
        Ok(count as usize)
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

#[async_trait]
impl ScanCursorRepository for PostgresCursorRepository {
    async fn get(&self, id: &ScanCursorId) -> Result<Option<ScanCursor>> {
        let result = sqlx::query_as::<
            _,
            (
                String, // folder_path_norm
                String, // listing_hash
                i32,    // entry_count
                chrono::DateTime<Utc>,
                Option<chrono::DateTime<Utc>>,
                Option<String>,
            ),
        >(
            r#"
            SELECT folder_path_norm, listing_hash, entry_count, last_scan_at, last_modified_at, device_id
            FROM scan_cursors
            WHERE library_id = $1 AND path_hash = $2
            "#,
        )
        .bind(id.library_id.0)
        .bind(id.path_hash as i64)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(
            |(
                folder_path_norm,
                listing_hash,
                entry_count,
                last_scan_at,
                last_modified_at,
                device_id,
            )| ScanCursor {
                id: id.clone(),
                folder_path_norm,
                listing_hash,
                entry_count: entry_count as usize,
                last_scan_at,
                last_modified_at,
                device_id,
            },
        ))
    }

    async fn list_by_library(&self, library_id: LibraryID) -> Result<Vec<ScanCursor>> {
        let results = sqlx::query_as::<
            _,
            (
                i64,    // path_hash
                String, // folder_path_norm
                String, // listing_hash
                i32,    // entry_count
                chrono::DateTime<Utc>,
                Option<chrono::DateTime<Utc>>,
                Option<String>,
            ),
        >(
            r#"
            SELECT path_hash, folder_path_norm, listing_hash, entry_count, last_scan_at, last_modified_at, device_id
            FROM scan_cursors
            WHERE library_id = $1
            ORDER BY folder_path_norm ASC
            "#,
        )
        .bind(library_id.0)
        .fetch_all(&self.pool)
        .await?;

        Ok(results
            .into_iter()
            .map(
                |(
                    path_hash,
                    folder_path_norm,
                    listing_hash,
                    entry_count,
                    last_scan_at,
                    last_modified_at,
                    device_id,
                )| {
                    ScanCursor {
                        id: ScanCursorId {
                            library_id,
                            path_hash: path_hash as u64,
                        },
                        folder_path_norm,
                        listing_hash,
                        entry_count: entry_count as usize,
                        last_scan_at,
                        last_modified_at,
                        device_id,
                    }
                },
            )
            .collect())
    }

    async fn upsert(&self, cursor: ScanCursor) -> Result<()> {
        sqlx::query(
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
        )
        .bind(cursor.id.library_id.0)
        .bind(cursor.id.path_hash as i64)
        .bind(&cursor.folder_path_norm)
        .bind(&cursor.listing_hash)
        .bind(cursor.entry_count as i32)
        .bind(cursor.last_scan_at)
        .bind(cursor.last_modified_at)
        .bind(cursor.device_id.as_deref())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn delete_by_library(&self, library_id: LibraryID) -> Result<usize> {
        let result = sqlx::query(
            r#"
            DELETE FROM scan_cursors
            WHERE library_id = $1
            "#,
        )
        .bind(library_id.0)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }

    async fn list_stale(
        &self,
        library_id: LibraryID,
        older_than: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<ScanCursor>> {
        let results = sqlx::query_as::<
            _,
            (
                i64,    // path_hash
                String, // folder_path_norm
                String, // listing_hash
                i32,    // entry_count
                chrono::DateTime<Utc>,
                Option<chrono::DateTime<Utc>>,
                Option<String>,
            ),
        >(
            r#"
            SELECT path_hash, folder_path_norm, listing_hash, entry_count, last_scan_at, last_modified_at, device_id
            FROM scan_cursors
            WHERE library_id = $1 AND last_scan_at < $2
            ORDER BY last_scan_at ASC
            "#,
        )
        .bind(library_id.0)
        .bind(older_than)
        .fetch_all(&self.pool)
        .await?;

        Ok(results
            .into_iter()
            .map(
                |(
                    path_hash,
                    folder_path_norm,
                    listing_hash,
                    entry_count,
                    last_scan_at,
                    last_modified_at,
                    device_id,
                )| ScanCursor {
                    id: ScanCursorId {
                        library_id,
                        path_hash: path_hash as u64,
                    },
                    folder_path_norm,
                    listing_hash,
                    entry_count: entry_count as usize,
                    last_scan_at,
                    last_modified_at,
                    device_id,
                },
            )
            .collect())
    }
}
