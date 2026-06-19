use async_trait::async_trait;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    database::repository_ports::scan_observability::{
        NewScanRunEvent, ScanObservabilityRepository, ScanRunEventRecord,
        ScanRunFailureSummary, ScanRunRecord, ScanRunRetentionPolicy,
        ScanRunSource, ScanRunStatus, ScanRunUpdate,
    },
    error::{MediaError, Result},
    types::ids::LibraryId,
};

#[derive(Clone, Debug)]
pub struct PostgresScanObservabilityRepository {
    pool: PgPool,
}

impl PostgresScanObservabilityRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

fn decode_source(raw: String) -> Result<ScanRunSource> {
    ScanRunSource::from_str(&raw).ok_or_else(|| {
        MediaError::Internal(format!(
            "scan observability returned unknown run source {raw}"
        ))
    })
}

fn decode_status(raw: String) -> Result<ScanRunStatus> {
    ScanRunStatus::from_str(&raw).ok_or_else(|| {
        MediaError::Internal(format!(
            "scan observability returned unknown run status {raw}"
        ))
    })
}

fn row_to_run(row: sqlx::postgres::PgRow) -> Result<ScanRunRecord> {
    Ok(ScanRunRecord {
        id: row.try_get("id")?,
        library_id: LibraryId(row.try_get("library_id")?),
        source: decode_source(row.try_get("source")?)?,
        status: decode_status(row.try_get("status")?)?,
        correlation_id: row.try_get("correlation_id")?,
        idempotency_key: row.try_get("idempotency_key")?,
        sequence: row.try_get("sequence")?,
        started_at: row.try_get("started_at")?,
        last_event_at: row.try_get("last_event_at")?,
        terminal_at: row.try_get("terminal_at")?,
        current_path: row.try_get("current_path")?,
        completed_items: row.try_get("completed_items")?,
        total_items: row.try_get("total_items")?,
        retrying_items: row.try_get("retrying_items")?,
        dead_lettered_items: row.try_get("dead_lettered_items")?,
        terminal_summary: row.try_get("terminal_summary")?,
    })
}

fn row_to_event(row: sqlx::postgres::PgRow) -> Result<ScanRunEventRecord> {
    Ok(ScanRunEventRecord {
        id: row.try_get("id")?,
        run_id: row.try_get("run_id")?,
        library_id: LibraryId(row.try_get("library_id")?),
        event_version: row.try_get("event_version")?,
        event_kind: row.try_get("event_kind")?,
        status: row.try_get("status")?,
        correlation_id: row.try_get("correlation_id")?,
        idempotency_key: row.try_get("idempotency_key")?,
        sequence: row.try_get("sequence")?,
        subject_key: row.try_get("subject_key")?,
        current_path: row.try_get("current_path")?,
        occurred_at: row.try_get("occurred_at")?,
        completed_items: row.try_get("completed_items")?,
        total_items: row.try_get("total_items")?,
        retrying_items: row.try_get("retrying_items")?,
        dead_lettered_items: row.try_get("dead_lettered_items")?,
        payload: row.try_get("payload")?,
    })
}

fn row_to_failure(row: sqlx::postgres::PgRow) -> Result<ScanRunFailureSummary> {
    Ok(ScanRunFailureSummary {
        run_id: row.try_get("run_id")?,
        library_id: LibraryId(row.try_get("library_id")?),
        subject_key: row.try_get("subject_key")?,
        category: row.try_get("category")?,
        message_code: row.try_get("message_code")?,
        raw_debug_details: row.try_get("raw_debug_details")?,
        last_error: row.try_get("last_error")?,
        occurrences: row.try_get("occurrences")?,
        first_seen_at: row.try_get("first_seen_at")?,
        last_seen_at: row.try_get("last_seen_at")?,
        retryable: row.try_get("retryable")?,
        job_id: row.try_get("job_id")?,
        idempotency_key: row.try_get("idempotency_key")?,
    })
}

#[async_trait]
impl ScanObservabilityRepository for PostgresScanObservabilityRepository {
    async fn create_run(&self, run: &ScanRunRecord) -> Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT INTO scan_runs (
                id,
                library_id,
                source,
                status,
                correlation_id,
                idempotency_key,
                sequence,
                started_at,
                last_event_at,
                terminal_at,
                current_path,
                completed_items,
                total_items,
                retrying_items,
                dead_lettered_items,
                terminal_summary
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(run.id)
        .bind(run.library_id.to_uuid())
        .bind(run.source.as_str())
        .bind(run.status.as_str())
        .bind(run.correlation_id)
        .bind(&run.idempotency_key)
        .bind(run.sequence)
        .bind(run.started_at)
        .bind(run.last_event_at)
        .bind(run.terminal_at)
        .bind(&run.current_path)
        .bind(run.completed_items)
        .bind(run.total_items)
        .bind(run.retrying_items)
        .bind(run.dead_lettered_items)
        .bind(&run.terminal_summary)
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to create scan run observability row: {e}"
            ))
        })?;

        Ok(result.rows_affected() == 1)
    }

    async fn update_run(&self, update: &ScanRunUpdate) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE scan_runs
            SET status = $2,
                idempotency_key = $3,
                last_event_at = $4,
                terminal_at = $5,
                current_path = $6,
                completed_items = $7,
                total_items = $8,
                retrying_items = $9,
                dead_lettered_items = $10,
                terminal_summary = $11,
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(update.id)
        .bind(update.status.as_str())
        .bind(&update.idempotency_key)
        .bind(update.last_event_at)
        .bind(update.terminal_at)
        .bind(&update.current_path)
        .bind(update.completed_items)
        .bind(update.total_items)
        .bind(update.retrying_items)
        .bind(update.dead_lettered_items)
        .bind(&update.terminal_summary)
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to update scan run observability row: {e}"
            ))
        })?;

        Ok(())
    }

    async fn append_event(
        &self,
        event: &NewScanRunEvent,
    ) -> Result<ScanRunEventRecord> {
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to open scan event transaction: {e}"
            ))
        })?;

        let current_sequence: i64 = sqlx::query_scalar(
            r#"
            SELECT sequence
            FROM scan_runs
            WHERE id = $1
            FOR UPDATE
            "#,
        )
        .bind(event.run_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to lock scan run for event append: {e}"
            ))
        })?;

        let sequence = current_sequence.saturating_add(1);
        let id = Uuid::now_v7();

        let row = sqlx::query(
            r#"
            INSERT INTO scan_run_events (
                id,
                run_id,
                library_id,
                event_version,
                event_kind,
                status,
                correlation_id,
                idempotency_key,
                sequence,
                subject_key,
                current_path,
                occurred_at,
                completed_items,
                total_items,
                retrying_items,
                dead_lettered_items,
                payload
            )
            VALUES ($1, $2, $3, 1, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            RETURNING
                id,
                run_id,
                library_id,
                event_version,
                event_kind,
                status,
                correlation_id,
                idempotency_key,
                sequence,
                subject_key,
                current_path,
                occurred_at,
                completed_items,
                total_items,
                retrying_items,
                dead_lettered_items,
                payload
            "#,
        )
        .bind(id)
        .bind(event.run_id)
        .bind(event.library_id.to_uuid())
        .bind(&event.event_kind)
        .bind(&event.status)
        .bind(event.correlation_id)
        .bind(&event.idempotency_key)
        .bind(sequence)
        .bind(&event.subject_key)
        .bind(&event.current_path)
        .bind(event.occurred_at)
        .bind(event.completed_items)
        .bind(event.total_items)
        .bind(event.retrying_items)
        .bind(event.dead_lettered_items)
        .bind(&event.payload)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to append scan run observability event: {e}"
            ))
        })?;

        sqlx::query(
            r#"
            UPDATE scan_runs
            SET sequence = $2,
                last_event_at = GREATEST(last_event_at, $3),
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(event.run_id)
        .bind(sequence)
        .bind(event.occurred_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to advance scan run event sequence: {e}"
            ))
        })?;

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to commit scan event transaction: {e}"
            ))
        })?;

        row_to_event(row)
    }

    async fn upsert_failure_summary(
        &self,
        failure: &ScanRunFailureSummary,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO scan_run_failures (
                id,
                run_id,
                library_id,
                subject_key,
                category,
                message_code,
                raw_debug_details,
                last_error,
                occurrences,
                first_seen_at,
                last_seen_at,
                retryable,
                job_id,
                idempotency_key
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, GREATEST($9, 1), $10, $11, $12, $13, $14)
            ON CONFLICT (run_id, subject_key, category, message_code)
            DO UPDATE SET
                raw_debug_details = EXCLUDED.raw_debug_details,
                last_error = EXCLUDED.last_error,
                occurrences = scan_run_failures.occurrences + GREATEST(EXCLUDED.occurrences, 1),
                last_seen_at = GREATEST(scan_run_failures.last_seen_at, EXCLUDED.last_seen_at),
                retryable = EXCLUDED.retryable,
                job_id = COALESCE(EXCLUDED.job_id, scan_run_failures.job_id),
                idempotency_key = EXCLUDED.idempotency_key,
                updated_at = NOW()
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(failure.run_id)
        .bind(failure.library_id.to_uuid())
        .bind(&failure.subject_key)
        .bind(&failure.category)
        .bind(&failure.message_code)
        .bind(&failure.raw_debug_details)
        .bind(&failure.last_error)
        .bind(failure.occurrences)
        .bind(failure.first_seen_at)
        .bind(failure.last_seen_at)
        .bind(failure.retryable)
        .bind(failure.job_id)
        .bind(&failure.idempotency_key)
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to upsert scan run failure summary: {e}"
            ))
        })?;

        Ok(())
    }

    async fn active_runs(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<ScanRunRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                library_id,
                source,
                status,
                correlation_id,
                idempotency_key,
                sequence,
                started_at,
                last_event_at,
                terminal_at,
                current_path,
                completed_items,
                total_items,
                retrying_items,
                dead_lettered_items,
                terminal_summary
            FROM scan_runs
            WHERE library_id = $1
              AND status IN ('pending', 'running', 'paused')
            ORDER BY last_event_at DESC, started_at DESC
            "#,
        )
        .bind(library_id.to_uuid())
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to query active scan runs: {e}"
            ))
        })?;

        rows.into_iter().map(row_to_run).collect()
    }

    async fn recent_runs(
        &self,
        library_id: Option<LibraryId>,
        limit: i64,
    ) -> Result<Vec<ScanRunRecord>> {
        let limit = limit.clamp(1, 500);
        let rows = if let Some(library_id) = library_id {
            sqlx::query(
                r#"
                SELECT
                    id,
                    library_id,
                    source,
                    status,
                    correlation_id,
                    idempotency_key,
                    sequence,
                    started_at,
                    last_event_at,
                    terminal_at,
                    current_path,
                    completed_items,
                    total_items,
                    retrying_items,
                    dead_lettered_items,
                    terminal_summary
                FROM scan_runs
                WHERE library_id = $1
                ORDER BY COALESCE(terminal_at, last_event_at, started_at) DESC, started_at DESC
                LIMIT $2
                "#,
            )
            .bind(library_id.to_uuid())
            .bind(limit)
            .fetch_all(self.pool())
            .await
        } else {
            sqlx::query(
                r#"
                SELECT
                    id,
                    library_id,
                    source,
                    status,
                    correlation_id,
                    idempotency_key,
                    sequence,
                    started_at,
                    last_event_at,
                    terminal_at,
                    current_path,
                    completed_items,
                    total_items,
                    retrying_items,
                    dead_lettered_items,
                    terminal_summary
                FROM scan_runs
                ORDER BY COALESCE(terminal_at, last_event_at, started_at) DESC, started_at DESC
                LIMIT $1
                "#,
            )
            .bind(limit)
            .fetch_all(self.pool())
            .await
        }
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to query recent scan runs: {e}"
            ))
        })?;

        rows.into_iter().map(row_to_run).collect()
    }

    async fn events_for_run(
        &self,
        run_id: Uuid,
    ) -> Result<Vec<ScanRunEventRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                run_id,
                library_id,
                event_version,
                event_kind,
                status,
                correlation_id,
                idempotency_key,
                sequence,
                subject_key,
                current_path,
                occurred_at,
                completed_items,
                total_items,
                retrying_items,
                dead_lettered_items,
                payload
            FROM scan_run_events
            WHERE run_id = $1
            ORDER BY sequence ASC, occurred_at ASC, id ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to query scan run events: {e}"
            ))
        })?;

        rows.into_iter().map(row_to_event).collect()
    }

    async fn failure_summaries_for_run(
        &self,
        run_id: Uuid,
    ) -> Result<Vec<ScanRunFailureSummary>> {
        let rows = sqlx::query(
            r#"
            SELECT
                run_id,
                library_id,
                subject_key,
                category,
                message_code,
                raw_debug_details,
                last_error,
                occurrences,
                first_seen_at,
                last_seen_at,
                retryable,
                job_id,
                idempotency_key
            FROM scan_run_failures
            WHERE run_id = $1
            ORDER BY last_seen_at DESC, subject_key ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to query scan run failures: {e}"
            ))
        })?;

        rows.into_iter().map(row_to_failure).collect()
    }

    async fn prune(&self, policy: ScanRunRetentionPolicy) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM scan_runs
            WHERE status NOT IN ('pending', 'running', 'paused')
              AND terminal_at IS NOT NULL
              AND terminal_at < $1
            "#,
        )
        .bind(policy.terminal_before)
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to prune scan observability rows: {e}"
            ))
        })?;

        Ok(result.rows_affected())
    }
}
