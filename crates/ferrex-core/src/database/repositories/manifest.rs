use std::fmt;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    database::repository_ports::manifest::{
        ManifestBackfillSummary, ManifestBatchUpsertSummary,
        ManifestDeferredWatchHintFilter, ManifestDeferredWatchHintInput,
        ManifestDeferredWatchHintRecord, ManifestDeferredWatchHintStatus,
        ManifestDiagnosticFilter, ManifestDiagnosticRecord,
        ManifestMissingEntryRecord, ManifestPartitionCursorRecord,
        ManifestRepository, ManifestRunCompletion,
    },
    domain::scan::manifest::{
        ManifestDiagnostic, ManifestDiagnosticReason,
        ManifestDiagnosticSeverity, ManifestEntry, ManifestEntryBatch,
        ManifestEntryClassification, ManifestEntryKind, ManifestFingerprint,
        ManifestPartitionId, ManifestPartitionScope, ManifestRootId,
        ManifestRootScope, ManifestRun, ManifestRunStatus, ManifestScope,
        ManifestSupportedClassification,
    },
    error::{MediaError, Result},
    types::{ids::LibraryId, library::LibraryType},
};

#[derive(Clone)]
pub struct PostgresManifestRepository {
    pool: PgPool,
}

impl PostgresManifestRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn upsert_partition_cursor_for_run(
        &self,
        run: &ManifestRun,
    ) -> Result<()> {
        if !is_successful_status(run.status) {
            return Ok(());
        }

        let parts = ScopeParts::from_scope(&run.scope)?;
        let partition_key = parts
            .partition_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "root".to_string());
        let completed_at = run.completed_at.unwrap_or_else(Utc::now);

        let entries_seen = u64_to_i64(run.entries_seen, "entries_seen")?;
        let diagnostics_seen =
            u64_to_i64(run.diagnostics_seen, "diagnostics_seen")?;

        sqlx::query!(
            r#"
            INSERT INTO manifest_partition_cursors (
                library_id,
                library_type,
                root_id,
                root_path_norm,
                partition_key,
                partition_id,
                prefix_norm,
                last_successful_run_id,
                last_successful_at,
                last_observed_at,
                entries_seen,
                diagnostics_seen,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9, $10, $11, NOW())
            ON CONFLICT (library_id, root_id, partition_key)
            DO UPDATE SET
                library_type = EXCLUDED.library_type,
                root_path_norm = EXCLUDED.root_path_norm,
                partition_id = EXCLUDED.partition_id,
                prefix_norm = EXCLUDED.prefix_norm,
                last_successful_run_id = EXCLUDED.last_successful_run_id,
                last_successful_at = EXCLUDED.last_successful_at,
                last_observed_at = EXCLUDED.last_observed_at,
                entries_seen = EXCLUDED.entries_seen,
                diagnostics_seen = EXCLUDED.diagnostics_seen,
                backfilled_from_legacy = FALSE,
                updated_at = NOW()
            "#,
            parts.library_id.0,
            parts.library_type,
            parts.root_id,
            parts.root_path_norm,
            partition_key,
            parts.partition_id,
            parts.partition_prefix_norm,
            run.run_id,
            completed_at,
            entries_seen,
            diagnostics_seen
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

impl fmt::Debug for PostgresManifestRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresManifestRepository")
            .field("pool_size", &self.pool.size())
            .field("idle_connections", &self.pool.num_idle())
            .finish()
    }
}

#[async_trait]
impl ManifestRepository for PostgresManifestRepository {
    async fn start_run(&self, run: ManifestRun) -> Result<ManifestRun> {
        let parts = ScopeParts::from_scope(&run.scope)?;
        let entries_seen = u64_to_i64(run.entries_seen, "entries_seen")?;
        let diagnostics_seen =
            u64_to_i64(run.diagnostics_seen, "diagnostics_seen")?;
        let row = sqlx::query_as!(
            ManifestRunRow,
            r#"
            INSERT INTO manifest_runs (
                run_id,
                library_id,
                library_type,
                scope_kind,
                root_id,
                root_path_norm,
                partition_id,
                partition_prefix_norm,
                status,
                started_at,
                completed_at,
                entries_seen,
                diagnostics_seen
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (run_id)
            DO UPDATE SET
                library_id = EXCLUDED.library_id,
                library_type = EXCLUDED.library_type,
                scope_kind = EXCLUDED.scope_kind,
                root_id = EXCLUDED.root_id,
                root_path_norm = EXCLUDED.root_path_norm,
                partition_id = EXCLUDED.partition_id,
                partition_prefix_norm = EXCLUDED.partition_prefix_norm,
                status = EXCLUDED.status,
                started_at = EXCLUDED.started_at,
                completed_at = EXCLUDED.completed_at,
                entries_seen = EXCLUDED.entries_seen,
                diagnostics_seen = EXCLUDED.diagnostics_seen,
                updated_at = NOW()
            RETURNING
                run_id AS "run_id!",
                library_id AS "library_id!",
                library_type AS "library_type!",
                scope_kind AS "scope_kind!",
                root_id AS "root_id!",
                root_path_norm AS "root_path_norm!",
                partition_id,
                partition_prefix_norm,
                status AS "status!",
                started_at AS "started_at!",
                completed_at,
                entries_seen AS "entries_seen!",
                diagnostics_seen AS "diagnostics_seen!"
            "#,
            run.run_id,
            parts.library_id.0,
            parts.library_type,
            parts.scope_kind,
            parts.root_id,
            parts.root_path_norm,
            parts.partition_id,
            parts.partition_prefix_norm,
            encode_run_status(run.status),
            run.started_at,
            run.completed_at,
            entries_seen,
            diagnostics_seen
        )
        .fetch_one(&self.pool)
        .await?;

        row_to_manifest_run(row)
    }

    async fn upsert_batch_entries(
        &self,
        run_id: Uuid,
        batch: &ManifestEntryBatch,
    ) -> Result<ManifestBatchUpsertSummary> {
        if batch.entries.is_empty() {
            return Ok(ManifestBatchUpsertSummary::default());
        }

        let mut rows = Vec::with_capacity(batch.entries.len());
        let mut diagnostics = Vec::new();
        for entry in &batch.entries {
            let row = EntryUpsertRow::from_entry(run_id, entry)?;
            diagnostics.extend(DiagnosticUpsertRow::from_entry(run_id, entry)?);
            rows.push(row);
        }

        let library_ids: Vec<_> =
            rows.iter().map(|row| row.library_id).collect();
        let path_norms: Vec<_> =
            rows.iter().map(|row| row.path_norm.clone()).collect();
        let entry_kinds: Vec<_> =
            rows.iter().map(|row| row.entry_kind.to_string()).collect();
        let library_types: Vec<_> = rows
            .iter()
            .map(|row| row.library_type.to_string())
            .collect();
        let root_ids: Vec<_> = rows.iter().map(|row| row.root_id).collect();
        let root_path_norms: Vec<_> =
            rows.iter().map(|row| row.root_path_norm.clone()).collect();
        let partition_ids: Vec<_> =
            rows.iter().map(|row| row.partition_id).collect();
        let partition_prefix_norms: Vec<_> = rows
            .iter()
            .map(|row| row.partition_prefix_norm.clone())
            .collect();
        let relative_paths: Vec<_> =
            rows.iter().map(|row| row.relative_path.clone()).collect();
        let classification_statuses: Vec<_> = rows
            .iter()
            .map(|row| row.classification_status.to_string())
            .collect();
        let classification_kinds: Vec<_> = rows
            .iter()
            .map(|row| row.classification_kind.clone())
            .collect();
        let classification_payloads: Vec<_> = rows
            .iter()
            .map(|row| row.classification_payload.clone())
            .collect();
        let fingerprint_device_ids: Vec<_> = rows
            .iter()
            .map(|row| row.fingerprint_device_id.clone())
            .collect();
        let fingerprint_inodes: Vec<_> = rows
            .iter()
            .map(|row| row.fingerprint_inode.clone())
            .collect();
        let fingerprint_sizes: Vec<_> =
            rows.iter().map(|row| row.fingerprint_size).collect();
        let fingerprint_mtime_ms: Vec<_> =
            rows.iter().map(|row| row.fingerprint_mtime_ms).collect();
        let fingerprint_weak_hashes: Vec<_> = rows
            .iter()
            .map(|row| row.fingerprint_weak_hash.clone())
            .collect();

        let entries_upserted = sqlx::query!(
            r#"
            WITH input AS (
                SELECT *
                FROM UNNEST(
                    $1::uuid[],
                    $2::text[],
                    $3::text[],
                    $4::text[],
                    $5::integer[],
                    $6::text[],
                    $7::integer[],
                    $8::text[],
                    $9::text[],
                    $10::text[],
                    $11::text[],
                    $12::jsonb[],
                    $13::text[],
                    $14::text[],
                    $15::bigint[],
                    $16::bigint[],
                    $17::text[]
                ) AS row(
                    library_id,
                    path_norm,
                    entry_kind,
                    library_type,
                    root_id,
                    root_path_norm,
                    partition_id,
                    partition_prefix_norm,
                    relative_path,
                    classification_status,
                    classification_kind,
                    classification_payload,
                    fingerprint_device_id,
                    fingerprint_inode,
                    fingerprint_size,
                    fingerprint_mtime_ms,
                    fingerprint_weak_hash
                )
            )
            INSERT INTO manifest_entries (
                library_id,
                path_norm,
                entry_kind,
                library_type,
                root_id,
                root_path_norm,
                partition_id,
                partition_prefix_norm,
                relative_path,
                classification_status,
                classification_kind,
                classification_payload,
                fingerprint_device_id,
                fingerprint_inode,
                fingerprint_size,
                fingerprint_mtime_ms,
                fingerprint_weak_hash,
                first_seen_run_id,
                last_seen_run_id,
                availability,
                source
            )
            SELECT
                library_id,
                path_norm,
                entry_kind,
                library_type,
                root_id,
                root_path_norm,
                partition_id,
                partition_prefix_norm,
                relative_path,
                classification_status,
                classification_kind,
                classification_payload,
                fingerprint_device_id,
                fingerprint_inode,
                fingerprint_size,
                fingerprint_mtime_ms,
                fingerprint_weak_hash,
                $18::uuid,
                $18::uuid,
                'available',
                'manifest'
            FROM input
            ON CONFLICT (library_id, path_norm)
            DO UPDATE SET
                entry_kind = EXCLUDED.entry_kind,
                library_type = EXCLUDED.library_type,
                root_id = EXCLUDED.root_id,
                root_path_norm = EXCLUDED.root_path_norm,
                partition_id = EXCLUDED.partition_id,
                partition_prefix_norm = EXCLUDED.partition_prefix_norm,
                relative_path = EXCLUDED.relative_path,
                classification_status = EXCLUDED.classification_status,
                classification_kind = EXCLUDED.classification_kind,
                classification_payload = EXCLUDED.classification_payload,
                fingerprint_device_id = EXCLUDED.fingerprint_device_id,
                fingerprint_inode = EXCLUDED.fingerprint_inode,
                fingerprint_size = EXCLUDED.fingerprint_size,
                fingerprint_mtime_ms = EXCLUDED.fingerprint_mtime_ms,
                fingerprint_weak_hash = EXCLUDED.fingerprint_weak_hash,
                first_seen_run_id = COALESCE(manifest_entries.first_seen_run_id, EXCLUDED.first_seen_run_id),
                last_seen_run_id = EXCLUDED.last_seen_run_id,
                last_seen_at = NOW(),
                availability = 'available',
                source = 'manifest',
                updated_at = NOW()
            "#,
            &library_ids[..],
            &path_norms[..],
            &entry_kinds[..],
            &library_types[..],
            &root_ids[..],
            &root_path_norms[..],
            &partition_ids[..] as _,
            &partition_prefix_norms[..] as _,
            &relative_paths[..],
            &classification_statuses[..],
            &classification_kinds[..],
            &classification_payloads[..],
            &fingerprint_device_ids[..] as _,
            &fingerprint_inodes[..] as _,
            &fingerprint_sizes[..],
            &fingerprint_mtime_ms[..] as _,
            &fingerprint_weak_hashes[..] as _,
            run_id
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        let diagnostics_upserted = if diagnostics.is_empty() {
            0
        } else {
            let diagnostic_run_ids: Vec<_> =
                diagnostics.iter().map(|row| row.run_id).collect();
            let diagnostic_library_ids: Vec<_> =
                diagnostics.iter().map(|row| row.library_id).collect();
            let diagnostic_root_ids: Vec<_> =
                diagnostics.iter().map(|row| row.root_id).collect();
            let diagnostic_partition_ids: Vec<_> =
                diagnostics.iter().map(|row| row.partition_id).collect();
            let diagnostic_path_norms: Vec<_> = diagnostics
                .iter()
                .map(|row| row.path_norm.clone())
                .collect();
            let diagnostic_reasons: Vec<_> =
                diagnostics.iter().map(|row| row.reason.clone()).collect();
            let diagnostic_codes: Vec<_> =
                diagnostics.iter().map(|row| row.code.clone()).collect();
            let diagnostic_severities: Vec<_> = diagnostics
                .iter()
                .map(|row| row.severity.to_string())
                .collect();
            let diagnostic_remediations: Vec<_> = diagnostics
                .iter()
                .map(|row| row.remediation.clone())
                .collect();

            sqlx::query!(
                r#"
                WITH input AS (
                    SELECT *
                    FROM UNNEST(
                        $1::uuid[],
                        $2::uuid[],
                        $3::integer[],
                        $4::integer[],
                        $5::text[],
                        $6::text[],
                        $7::text[],
                        $8::text[],
                        $9::text[]
                    ) AS row(
                        run_id,
                        library_id,
                        root_id,
                        partition_id,
                        path_norm,
                        reason,
                        code,
                        severity,
                        remediation
                    )
                )
                INSERT INTO manifest_diagnostics (
                    run_id,
                    library_id,
                    root_id,
                    partition_id,
                    path_norm,
                    reason,
                    code,
                    severity,
                    remediation
                )
                SELECT
                    run_id,
                    library_id,
                    root_id,
                    partition_id,
                    path_norm,
                    reason,
                    code,
                    severity,
                    remediation
                FROM input
                ON CONFLICT (run_id, path_norm, code)
                DO UPDATE SET
                    reason = EXCLUDED.reason,
                    severity = EXCLUDED.severity,
                    remediation = EXCLUDED.remediation
                "#,
                &diagnostic_run_ids[..],
                &diagnostic_library_ids[..],
                &diagnostic_root_ids[..],
                &diagnostic_partition_ids[..] as _,
                &diagnostic_path_norms[..],
                &diagnostic_reasons[..],
                &diagnostic_codes[..],
                &diagnostic_severities[..],
                &diagnostic_remediations[..]
            )
            .execute(&self.pool)
            .await?
            .rows_affected()
        };

        Ok(ManifestBatchUpsertSummary {
            entries_upserted,
            diagnostics_upserted,
        })
    }

    async fn complete_run(
        &self,
        completion: ManifestRunCompletion,
    ) -> Result<ManifestRun> {
        let entries_seen = u64_to_i64(completion.entries_seen, "entries_seen")?;
        let diagnostics_seen =
            u64_to_i64(completion.diagnostics_seen, "diagnostics_seen")?;
        let row = sqlx::query_as!(
            ManifestRunRow,
            r#"
            UPDATE manifest_runs
            SET status = $2,
                completed_at = $3,
                entries_seen = $4,
                diagnostics_seen = $5,
                error_message = $6,
                updated_at = NOW()
            WHERE run_id = $1
            RETURNING
                run_id AS "run_id!",
                library_id AS "library_id!",
                library_type AS "library_type!",
                scope_kind AS "scope_kind!",
                root_id AS "root_id!",
                root_path_norm AS "root_path_norm!",
                partition_id,
                partition_prefix_norm,
                status AS "status!",
                started_at AS "started_at!",
                completed_at,
                entries_seen AS "entries_seen!",
                diagnostics_seen AS "diagnostics_seen!"
            "#,
            completion.run_id,
            encode_run_status(completion.status),
            completion.completed_at,
            entries_seen,
            diagnostics_seen,
            completion.error_message
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            MediaError::NotFound(format!(
                "manifest run {} was not found",
                completion.run_id
            ))
        })?;

        let run = row_to_manifest_run(row)?;
        self.upsert_partition_cursor_for_run(&run).await?;
        Ok(run)
    }

    async fn mark_missing_entries_after_successful_run(
        &self,
        run_id: Uuid,
    ) -> Result<Vec<ManifestMissingEntryRecord>> {
        let rows = sqlx::query_as!(
            ManifestMissingEntryRow,
            r#"
            WITH successful_run AS (
                SELECT
                    run_id,
                    library_id,
                    root_id,
                    scope_kind,
                    partition_prefix_norm
                FROM manifest_runs
                WHERE run_id = $1
                  AND status = ANY (ARRAY['completed', 'completed_with_diagnostics'])
            ), missing AS (
                UPDATE manifest_entries entries
                SET availability = 'missing',
                    updated_at = NOW()
                FROM successful_run run
                WHERE entries.library_id = run.library_id
                  AND entries.root_id = run.root_id
                  AND entries.availability = 'available'
                  AND entries.last_seen_run_id IS DISTINCT FROM run.run_id
                  AND (
                      run.scope_kind = 'root'
                      OR (
                          run.partition_prefix_norm IS NOT NULL
                          AND (
                              entries.path_norm = run.partition_prefix_norm
                              OR entries.path_norm LIKE run.partition_prefix_norm || '/%'
                          )
                      )
                  )
                RETURNING
                    entries.library_id,
                    entries.root_id,
                    entries.partition_id,
                    entries.path_norm,
                    entries.entry_kind
            )
            SELECT
                library_id AS "library_id!",
                root_id AS "root_id!",
                partition_id,
                path_norm AS "path_norm!",
                entry_kind AS "entry_kind!"
            FROM missing
            ORDER BY path_norm ASC
            "#,
            run_id
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_missing_entry).collect()
    }

    async fn list_stale_partitions(
        &self,
        library_id: LibraryId,
        older_than: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<ManifestPartitionCursorRecord>> {
        let limit = i64::from(limit.max(1));
        let rows = sqlx::query_as!(
            ManifestPartitionCursorRow,
            r#"
            SELECT
                library_id AS "library_id!",
                library_type AS "library_type!",
                root_id AS "root_id!",
                root_path_norm AS "root_path_norm!",
                partition_key AS "partition_key!",
                partition_id,
                prefix_norm,
                last_successful_run_id,
                last_successful_at,
                last_observed_at,
                entries_seen AS "entries_seen!",
                diagnostics_seen AS "diagnostics_seen!",
                supported_media_seen AS "supported_media_seen!",
                first_path_norm,
                last_path_norm,
                legacy_scan_path_hash,
                backfilled_from_legacy AS "backfilled_from_legacy!",
                updated_at AS "updated_at!"
            FROM manifest_partition_cursors
            WHERE library_id = $1
              AND (last_successful_at IS NULL OR last_successful_at < $2)
            ORDER BY last_successful_at ASC NULLS FIRST, updated_at ASC, root_id ASC, partition_key ASC
            LIMIT $3
            "#,
            library_id.0,
            older_than,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_partition_cursor).collect()
    }

    async fn list_diagnostics(
        &self,
        filter: ManifestDiagnosticFilter,
    ) -> Result<Vec<ManifestDiagnosticRecord>> {
        let library_id = filter.library_id.map(|id| id.0);
        let run_id = filter.run_id;
        let severity = filter
            .severity
            .map(|severity| encode_severity(severity).to_string());
        let code = filter.code;
        let limit = i64::from(filter.limit.unwrap_or(100).max(1));

        let rows = sqlx::query_as!(
            ManifestDiagnosticRow,
            r#"
            SELECT
                id AS "id!",
                run_id AS "run_id!",
                library_id AS "library_id!",
                root_id AS "root_id!",
                partition_id,
                path_norm AS "path_norm!",
                reason AS "reason!",
                code AS "code!",
                severity AS "severity!",
                remediation AS "remediation!",
                created_at AS "created_at!"
            FROM manifest_diagnostics
            WHERE ($1::uuid IS NULL OR library_id = $1)
              AND ($2::uuid IS NULL OR run_id = $2)
              AND ($3::text IS NULL OR severity = $3)
              AND ($4::text IS NULL OR code = $4)
            ORDER BY created_at DESC, id DESC
            LIMIT $5
            "#,
            library_id,
            run_id,
            severity,
            code,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_diagnostic).collect()
    }

    async fn upsert_deferred_watch_hint(
        &self,
        hint: ManifestDeferredWatchHintInput,
    ) -> Result<ManifestDeferredWatchHintRecord> {
        let id = hint.id.unwrap_or_else(Uuid::now_v7);
        let row = sqlx::query_as!(
            ManifestDeferredHintRow,
            r#"
            INSERT INTO manifest_deferred_watch_hints (
                id,
                library_id,
                root_id,
                root_path_norm,
                path_norm,
                hint_kind,
                payload,
                status,
                idempotency_key,
                available_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8, $9)
            ON CONFLICT (library_id, idempotency_key)
            DO UPDATE SET
                root_id = EXCLUDED.root_id,
                root_path_norm = EXCLUDED.root_path_norm,
                path_norm = EXCLUDED.path_norm,
                hint_kind = EXCLUDED.hint_kind,
                payload = EXCLUDED.payload,
                status = 'pending',
                available_at = EXCLUDED.available_at,
                last_error = NULL,
                updated_at = NOW()
            RETURNING
                id AS "id!",
                library_id AS "library_id!",
                root_id AS "root_id!",
                root_path_norm AS "root_path_norm!",
                path_norm AS "path_norm!",
                hint_kind AS "hint_kind!",
                payload AS "payload!",
                status AS "status!",
                idempotency_key AS "idempotency_key!",
                attempts AS "attempts!",
                available_at AS "available_at!",
                last_error,
                created_at AS "created_at!",
                updated_at AS "updated_at!"
            "#,
            id,
            hint.library_id.0,
            i32::from(hint.root_id),
            hint.root_path_norm,
            hint.path_norm,
            hint.hint_kind,
            hint.payload,
            hint.idempotency_key,
            hint.available_at
        )
        .fetch_one(&self.pool)
        .await?;

        row_to_deferred_hint(row)
    }

    async fn list_deferred_watch_hints(
        &self,
        filter: ManifestDeferredWatchHintFilter,
    ) -> Result<Vec<ManifestDeferredWatchHintRecord>> {
        let library_id = filter.library_id.map(|id| id.0);
        let status = filter
            .status
            .map(|status| encode_hint_status(status).to_string());
        let available_before = filter.available_before;
        let limit = i64::from(filter.limit.unwrap_or(100).max(1));

        let rows = sqlx::query_as!(
            ManifestDeferredHintRow,
            r#"
            SELECT
                id AS "id!",
                library_id AS "library_id!",
                root_id AS "root_id!",
                root_path_norm AS "root_path_norm!",
                path_norm AS "path_norm!",
                hint_kind AS "hint_kind!",
                payload AS "payload!",
                status AS "status!",
                idempotency_key AS "idempotency_key!",
                attempts AS "attempts!",
                available_at AS "available_at!",
                last_error,
                created_at AS "created_at!",
                updated_at AS "updated_at!"
            FROM manifest_deferred_watch_hints
            WHERE ($1::uuid IS NULL OR library_id = $1)
              AND ($2::text IS NULL OR status = $2)
              AND ($3::timestamptz IS NULL OR available_at <= $3)
            ORDER BY available_at ASC, created_at ASC
            LIMIT $4
            "#,
            library_id,
            status,
            available_before,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_deferred_hint).collect()
    }

    async fn update_deferred_watch_hint_status(
        &self,
        id: Uuid,
        status: ManifestDeferredWatchHintStatus,
        last_error: Option<String>,
    ) -> Result<Option<ManifestDeferredWatchHintRecord>> {
        let status = encode_hint_status(status);
        let row = sqlx::query_as!(
            ManifestDeferredHintRow,
            r#"
            UPDATE manifest_deferred_watch_hints
            SET status = $2,
                attempts = CASE WHEN $2 = 'pending' THEN attempts + 1 ELSE attempts END,
                last_error = $3,
                updated_at = NOW()
            WHERE id = $1
            RETURNING
                id AS "id!",
                library_id AS "library_id!",
                root_id AS "root_id!",
                root_path_norm AS "root_path_norm!",
                path_norm AS "path_norm!",
                hint_kind AS "hint_kind!",
                payload AS "payload!",
                status AS "status!",
                idempotency_key AS "idempotency_key!",
                attempts AS "attempts!",
                available_at AS "available_at!",
                last_error,
                created_at AS "created_at!",
                updated_at AS "updated_at!"
            "#,
            id,
            status,
            last_error
        )
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_deferred_hint).transpose()
    }

    async fn backfill_legacy_manifest_state(
        &self,
        library_id: Option<LibraryId>,
    ) -> Result<ManifestBackfillSummary> {
        let library_uuid = library_id.map(|id| id.0);

        let media_entries = sqlx::query!(
            r#"
WITH source AS (
    SELECT
        mf.library_id,
        l.library_type,
        mf.file_path,
        mf.filename,
        mf.file_size,
        mf.fingerprint_device_id,
        mf.fingerprint_inode,
        mf.fingerprint_size,
        mf.fingerprint_mtime_ms,
        mf.fingerprint_weak_hash,
        COALESCE(root.root_id, 0) AS root_id,
        COALESCE(root.root_path_norm, '') AS root_path_norm,
        CASE
            WHEN root.root_path_norm IS NULL OR root.root_path_norm = '' THEN mf.file_path
            WHEN root.root_path_norm = '/' THEN ltrim(mf.file_path, '/')
            WHEN mf.file_path = root.root_path_norm THEN ''
            WHEN mf.file_path LIKE root.root_path_norm || '/%' THEN substr(mf.file_path, length(root.root_path_norm) + 2)
            ELSE mf.file_path
        END AS relative_path
    FROM media_files mf
    JOIN libraries l ON l.id = mf.library_id
    LEFT JOIN LATERAL (
        SELECT candidate.root_id, candidate.root_path_norm
        FROM (
            SELECT
                (ord - 1)::integer AS root_id,
                COALESCE(NULLIF(regexp_replace(path_value, '/+$', ''), ''), '/') AS root_path_norm
            FROM unnest(l.paths) WITH ORDINALITY AS paths(path_value, ord)
        ) candidate
        WHERE mf.file_path = candidate.root_path_norm
           OR (candidate.root_path_norm = '/' AND mf.file_path LIKE '/%')
           OR (candidate.root_path_norm <> '/' AND mf.file_path LIKE candidate.root_path_norm || '/%')
        ORDER BY length(candidate.root_path_norm) DESC, candidate.root_id ASC
        LIMIT 1
    ) root ON TRUE
    WHERE mf.is_available = TRUE
      AND ($1::uuid IS NULL OR mf.library_id = $1)
)
INSERT INTO manifest_entries (
    library_id,
    path_norm,
    entry_kind,
    library_type,
    root_id,
    root_path_norm,
    relative_path,
    classification_status,
    classification_kind,
    classification_payload,
    fingerprint_device_id,
    fingerprint_inode,
    fingerprint_size,
    fingerprint_mtime_ms,
    fingerprint_weak_hash,
    availability,
    source
)
SELECT
    library_id,
    file_path,
    'file',
    library_type,
    root_id,
    root_path_norm,
    relative_path,
    'supported',
    'legacy_media_file',
    jsonb_build_object('source', 'media_files', 'filename', filename),
    fingerprint_device_id,
    CASE WHEN fingerprint_inode IS NULL THEN NULL ELSE fingerprint_inode::text END,
    COALESCE(fingerprint_size, file_size, 0),
    fingerprint_mtime_ms,
    fingerprint_weak_hash,
    'available',
    'backfill'
FROM source
ON CONFLICT (library_id, path_norm)
DO UPDATE SET
    entry_kind = EXCLUDED.entry_kind,
    library_type = EXCLUDED.library_type,
    root_id = EXCLUDED.root_id,
    root_path_norm = EXCLUDED.root_path_norm,
    relative_path = EXCLUDED.relative_path,
    classification_status = EXCLUDED.classification_status,
    classification_kind = EXCLUDED.classification_kind,
    classification_payload = EXCLUDED.classification_payload,
    fingerprint_device_id = EXCLUDED.fingerprint_device_id,
    fingerprint_inode = EXCLUDED.fingerprint_inode,
    fingerprint_size = EXCLUDED.fingerprint_size,
    fingerprint_mtime_ms = EXCLUDED.fingerprint_mtime_ms,
    fingerprint_weak_hash = EXCLUDED.fingerprint_weak_hash,
    availability = 'available',
    source = CASE WHEN manifest_entries.source = 'manifest' THEN manifest_entries.source ELSE 'backfill' END,
    updated_at = NOW()
            "#,
            library_uuid
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        let folder_entries = sqlx::query!(
            r#"
WITH source AS (
    SELECT
        fi.library_id,
        l.library_type,
        fi.folder_path,
        fi.folder_type,
        fi.processing_status,
        fi.discovery_source,
        COALESCE(root.root_id, 0) AS root_id,
        COALESCE(root.root_path_norm, '') AS root_path_norm,
        CASE
            WHEN root.root_path_norm IS NULL OR root.root_path_norm = '' THEN fi.folder_path
            WHEN root.root_path_norm = '/' THEN ltrim(fi.folder_path, '/')
            WHEN fi.folder_path = root.root_path_norm THEN ''
            WHEN fi.folder_path LIKE root.root_path_norm || '/%' THEN substr(fi.folder_path, length(root.root_path_norm) + 2)
            ELSE fi.folder_path
        END AS relative_path
    FROM folder_inventory fi
    JOIN libraries l ON l.id = fi.library_id
    LEFT JOIN LATERAL (
        SELECT candidate.root_id, candidate.root_path_norm
        FROM (
            SELECT
                (ord - 1)::integer AS root_id,
                COALESCE(NULLIF(regexp_replace(path_value, '/+$', ''), ''), '/') AS root_path_norm
            FROM unnest(l.paths) WITH ORDINALITY AS paths(path_value, ord)
        ) candidate
        WHERE fi.folder_path = candidate.root_path_norm
           OR (candidate.root_path_norm = '/' AND fi.folder_path LIKE '/%')
           OR (candidate.root_path_norm <> '/' AND fi.folder_path LIKE candidate.root_path_norm || '/%')
        ORDER BY length(candidate.root_path_norm) DESC, candidate.root_id ASC
        LIMIT 1
    ) root ON TRUE
    WHERE $1::uuid IS NULL OR fi.library_id = $1
)
INSERT INTO manifest_entries (
    library_id,
    path_norm,
    entry_kind,
    library_type,
    root_id,
    root_path_norm,
    relative_path,
    classification_status,
    classification_kind,
    classification_payload,
    fingerprint_size,
    availability,
    source
)
SELECT
    library_id,
    folder_path,
    'directory',
    library_type,
    root_id,
    root_path_norm,
    relative_path,
    'supported',
    'legacy_folder_' || folder_type,
    jsonb_build_object(
        'source', 'folder_inventory',
        'folder_type', folder_type,
        'processing_status', processing_status,
        'discovery_source', discovery_source
    ),
    0,
    'available',
    'backfill'
FROM source
ON CONFLICT (library_id, path_norm)
DO UPDATE SET
    entry_kind = EXCLUDED.entry_kind,
    library_type = EXCLUDED.library_type,
    root_id = EXCLUDED.root_id,
    root_path_norm = EXCLUDED.root_path_norm,
    relative_path = EXCLUDED.relative_path,
    classification_status = EXCLUDED.classification_status,
    classification_kind = EXCLUDED.classification_kind,
    classification_payload = EXCLUDED.classification_payload,
    fingerprint_size = EXCLUDED.fingerprint_size,
    availability = 'available',
    source = CASE WHEN manifest_entries.source = 'manifest' THEN manifest_entries.source ELSE 'backfill' END,
    updated_at = NOW()
            "#,
            library_uuid
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        let legacy_cursors = sqlx::query!(
            r#"
WITH source AS (
    SELECT
        sc.library_id,
        l.library_type,
        sc.path_hash,
        sc.folder_path_norm,
        sc.entry_count,
        sc.last_scan_at,
        COALESCE(root.root_id, 0) AS root_id,
        COALESCE(root.root_path_norm, '') AS root_path_norm
    FROM scan_cursors sc
    JOIN libraries l ON l.id = sc.library_id
    LEFT JOIN LATERAL (
        SELECT candidate.root_id, candidate.root_path_norm
        FROM (
            SELECT
                (ord - 1)::integer AS root_id,
                COALESCE(NULLIF(regexp_replace(path_value, '/+$', ''), ''), '/') AS root_path_norm
            FROM unnest(l.paths) WITH ORDINALITY AS paths(path_value, ord)
        ) candidate
        WHERE sc.folder_path_norm = candidate.root_path_norm
           OR (candidate.root_path_norm = '/' AND sc.folder_path_norm LIKE '/%')
           OR (candidate.root_path_norm <> '/' AND sc.folder_path_norm LIKE candidate.root_path_norm || '/%')
        ORDER BY length(candidate.root_path_norm) DESC, candidate.root_id ASC
        LIMIT 1
    ) root ON TRUE
    WHERE $1::uuid IS NULL OR sc.library_id = $1
)
INSERT INTO manifest_partition_cursors (
    library_id,
    library_type,
    root_id,
    root_path_norm,
    partition_key,
    prefix_norm,
    last_observed_at,
    entries_seen,
    first_path_norm,
    last_path_norm,
    legacy_scan_path_hash,
    backfilled_from_legacy,
    backfilled_at
)
SELECT
    library_id,
    library_type,
    root_id,
    root_path_norm,
    'legacy:' || path_hash::text,
    folder_path_norm,
    last_scan_at,
    entry_count,
    folder_path_norm,
    folder_path_norm,
    path_hash,
    TRUE,
    NOW()
FROM source
ON CONFLICT (library_id, root_id, partition_key)
DO UPDATE SET
    library_type = EXCLUDED.library_type,
    root_path_norm = EXCLUDED.root_path_norm,
    prefix_norm = EXCLUDED.prefix_norm,
    last_observed_at = EXCLUDED.last_observed_at,
    entries_seen = EXCLUDED.entries_seen,
    first_path_norm = EXCLUDED.first_path_norm,
    last_path_norm = EXCLUDED.last_path_norm,
    legacy_scan_path_hash = EXCLUDED.legacy_scan_path_hash,
    backfilled_from_legacy = TRUE,
    backfilled_at = COALESCE(manifest_partition_cursors.backfilled_at, NOW()),
    updated_at = NOW()
            "#,
            library_uuid
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        Ok(ManifestBackfillSummary {
            media_entries,
            folder_entries,
            legacy_cursors,
        })
    }
}

#[derive(Clone, Debug)]
struct ScopeParts {
    library_id: LibraryId,
    library_type: &'static str,
    scope_kind: &'static str,
    root_id: i32,
    root_path_norm: String,
    partition_id: Option<i32>,
    partition_prefix_norm: Option<String>,
}

impl ScopeParts {
    fn from_scope(scope: &ManifestScope) -> Result<Self> {
        match scope {
            ManifestScope::Root(root) => Ok(Self {
                library_id: root.library_id,
                library_type: encode_library_type(root.library_type),
                scope_kind: "root",
                root_id: i32::from(root.root_id.0),
                root_path_norm: root.root_path_norm.clone(),
                partition_id: None,
                partition_prefix_norm: None,
            }),
            ManifestScope::Partition(partition) => Ok(Self {
                library_id: partition.root.library_id,
                library_type: encode_library_type(partition.root.library_type),
                scope_kind: "partition",
                root_id: i32::from(partition.root.root_id.0),
                root_path_norm: partition.root.root_path_norm.clone(),
                partition_id: Some(i32::from(partition.partition_id.0)),
                partition_prefix_norm: partition.prefix_norm.clone(),
            }),
        }
    }
}

#[derive(Clone, Debug)]
struct EntryUpsertRow {
    library_id: Uuid,
    path_norm: String,
    entry_kind: &'static str,
    library_type: &'static str,
    root_id: i32,
    root_path_norm: String,
    partition_id: Option<i32>,
    partition_prefix_norm: Option<String>,
    relative_path: String,
    classification_status: &'static str,
    classification_kind: String,
    classification_payload: Value,
    fingerprint_device_id: Option<String>,
    fingerprint_inode: Option<String>,
    fingerprint_size: i64,
    fingerprint_mtime_ms: Option<i64>,
    fingerprint_weak_hash: Option<String>,
}

impl EntryUpsertRow {
    fn from_entry(_run_id: Uuid, entry: &ManifestEntry) -> Result<Self> {
        let parts = ScopeParts::from_scope(entry.scope())?;
        let (entry_kind, relative_path, fingerprint) = match entry {
            ManifestEntry::Media(media) => (
                encode_entry_kind(ManifestEntryKind::File),
                media.relative_path.clone(),
                media.fingerprint.clone(),
            ),
            ManifestEntry::Directory(directory) => (
                encode_entry_kind(ManifestEntryKind::Directory),
                directory.relative_path.clone(),
                ManifestFingerprint::default(),
            ),
        };
        let (classification_status, classification_kind) =
            encode_classification(entry.classification());
        let classification_payload =
            serde_json::to_value(entry.classification())?;

        Ok(Self {
            library_id: parts.library_id.0,
            path_norm: entry.path_norm().to_string(),
            entry_kind,
            library_type: parts.library_type,
            root_id: parts.root_id,
            root_path_norm: parts.root_path_norm,
            partition_id: parts.partition_id,
            partition_prefix_norm: parts.partition_prefix_norm,
            relative_path,
            classification_status,
            classification_kind,
            classification_payload,
            fingerprint_device_id: fingerprint
                .device_id
                .map(|id| id.to_string()),
            fingerprint_inode: fingerprint.inode.map(|id| id.to_string()),
            fingerprint_size: u64_to_i64(fingerprint.size, "fingerprint.size")?,
            fingerprint_mtime_ms: fingerprint.mtime_ms,
            fingerprint_weak_hash: fingerprint.weak_hash,
        })
    }
}

#[derive(Clone, Debug)]
struct DiagnosticUpsertRow {
    run_id: Uuid,
    library_id: Uuid,
    root_id: i32,
    partition_id: Option<i32>,
    path_norm: String,
    reason: String,
    code: String,
    severity: &'static str,
    remediation: String,
}

impl DiagnosticUpsertRow {
    fn from_entry(run_id: Uuid, entry: &ManifestEntry) -> Result<Vec<Self>> {
        let parts = ScopeParts::from_scope(entry.scope())?;
        Ok(entry
            .diagnostics()
            .iter()
            .map(|diagnostic| Self::from_diagnostic(run_id, &parts, diagnostic))
            .collect())
    }

    fn from_diagnostic(
        run_id: Uuid,
        parts: &ScopeParts,
        diagnostic: &ManifestDiagnostic,
    ) -> Self {
        Self {
            run_id,
            library_id: parts.library_id.0,
            root_id: parts.root_id,
            partition_id: parts.partition_id,
            path_norm: diagnostic.path_norm.clone(),
            reason: encode_reason(diagnostic.reason).to_string(),
            code: diagnostic.code.clone(),
            severity: encode_severity(diagnostic.severity),
            remediation: diagnostic.remediation.clone(),
        }
    }
}

#[derive(Debug)]
struct ManifestRunRow {
    run_id: Uuid,
    library_id: Uuid,
    library_type: String,
    scope_kind: String,
    root_id: i32,
    root_path_norm: String,
    partition_id: Option<i32>,
    partition_prefix_norm: Option<String>,
    status: String,
    started_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    entries_seen: i64,
    diagnostics_seen: i64,
}

#[derive(Debug)]
struct ManifestMissingEntryRow {
    library_id: Uuid,
    root_id: i32,
    partition_id: Option<i32>,
    path_norm: String,
    entry_kind: String,
}

#[derive(Debug)]
struct ManifestPartitionCursorRow {
    library_id: Uuid,
    library_type: String,
    root_id: i32,
    root_path_norm: String,
    partition_key: String,
    partition_id: Option<i32>,
    prefix_norm: Option<String>,
    last_successful_run_id: Option<Uuid>,
    last_successful_at: Option<DateTime<Utc>>,
    last_observed_at: Option<DateTime<Utc>>,
    entries_seen: i64,
    diagnostics_seen: i64,
    supported_media_seen: i64,
    first_path_norm: Option<String>,
    last_path_norm: Option<String>,
    legacy_scan_path_hash: Option<i64>,
    backfilled_from_legacy: bool,
    updated_at: DateTime<Utc>,
}

#[derive(Debug)]
struct ManifestDiagnosticRow {
    id: Uuid,
    run_id: Uuid,
    library_id: Uuid,
    root_id: i32,
    partition_id: Option<i32>,
    path_norm: String,
    reason: String,
    code: String,
    severity: String,
    remediation: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug)]
struct ManifestDeferredHintRow {
    id: Uuid,
    library_id: Uuid,
    root_id: i32,
    root_path_norm: String,
    path_norm: String,
    hint_kind: String,
    payload: Value,
    status: String,
    idempotency_key: String,
    attempts: i32,
    available_at: DateTime<Utc>,
    last_error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn row_to_manifest_run(row: ManifestRunRow) -> Result<ManifestRun> {
    let library_id = LibraryId(row.library_id);
    let library_type = decode_library_type(row.library_type.as_str())?;
    let root_id = checked_u16(row.root_id, "root_id")?;
    let root = ManifestRootScope {
        library_id,
        library_type,
        root_id: ManifestRootId(root_id),
        root_path_norm: row.root_path_norm,
    };
    let scope = match row.scope_kind.as_str() {
        "root" => ManifestScope::Root(root),
        "partition" => {
            let partition_id = row.partition_id.ok_or_else(|| {
                MediaError::Internal(
                    "manifest partition run is missing partition_id".into(),
                )
            })?;
            ManifestScope::Partition(ManifestPartitionScope {
                root,
                partition_id: ManifestPartitionId(checked_u16(
                    partition_id,
                    "partition_id",
                )?),
                prefix_norm: row.partition_prefix_norm,
            })
        }
        other => {
            return Err(MediaError::Internal(format!(
                "unknown manifest scope kind: {other}"
            )));
        }
    };

    Ok(ManifestRun {
        run_id: row.run_id,
        scope,
        status: decode_run_status(row.status.as_str())?,
        started_at: row.started_at,
        completed_at: row.completed_at,
        entries_seen: i64_to_u64(row.entries_seen, "entries_seen")?,
        diagnostics_seen: i64_to_u64(row.diagnostics_seen, "diagnostics_seen")?,
    })
}

fn row_to_missing_entry(
    row: ManifestMissingEntryRow,
) -> Result<ManifestMissingEntryRecord> {
    Ok(ManifestMissingEntryRecord {
        library_id: LibraryId(row.library_id),
        root_id: checked_u16(row.root_id, "root_id")?,
        partition_id: row
            .partition_id
            .map(|value| {
                checked_u16(value, "partition_id").map(ManifestPartitionId)
            })
            .transpose()?,
        path_norm: row.path_norm,
        entry_kind: decode_entry_kind(row.entry_kind.as_str())?,
    })
}

fn row_to_partition_cursor(
    row: ManifestPartitionCursorRow,
) -> Result<ManifestPartitionCursorRecord> {
    Ok(ManifestPartitionCursorRecord {
        library_id: LibraryId(row.library_id),
        library_type: decode_library_type(row.library_type.as_str())?,
        root_id: checked_u16(row.root_id, "root_id")?,
        root_path_norm: row.root_path_norm,
        partition_key: row.partition_key,
        partition_id: row
            .partition_id
            .map(|value| {
                checked_u16(value, "partition_id").map(ManifestPartitionId)
            })
            .transpose()?,
        prefix_norm: row.prefix_norm,
        last_successful_run_id: row.last_successful_run_id,
        last_successful_at: row.last_successful_at,
        last_observed_at: row.last_observed_at,
        entries_seen: i64_to_u64(row.entries_seen, "entries_seen")?,
        diagnostics_seen: i64_to_u64(row.diagnostics_seen, "diagnostics_seen")?,
        supported_media_seen: i64_to_u64(
            row.supported_media_seen,
            "supported_media_seen",
        )?,
        first_path_norm: row.first_path_norm,
        last_path_norm: row.last_path_norm,
        legacy_scan_path_hash: row.legacy_scan_path_hash,
        backfilled_from_legacy: row.backfilled_from_legacy,
        updated_at: row.updated_at,
    })
}

fn row_to_diagnostic(
    row: ManifestDiagnosticRow,
) -> Result<ManifestDiagnosticRecord> {
    Ok(ManifestDiagnosticRecord {
        id: row.id,
        run_id: row.run_id,
        library_id: LibraryId(row.library_id),
        root_id: checked_u16(row.root_id, "root_id")?,
        partition_id: row
            .partition_id
            .map(|value| {
                checked_u16(value, "partition_id").map(ManifestPartitionId)
            })
            .transpose()?,
        path_norm: row.path_norm,
        reason: row.reason,
        code: row.code,
        severity: decode_severity(row.severity.as_str())?,
        remediation: row.remediation,
        created_at: row.created_at,
    })
}

fn row_to_deferred_hint(
    row: ManifestDeferredHintRow,
) -> Result<ManifestDeferredWatchHintRecord> {
    Ok(ManifestDeferredWatchHintRecord {
        id: row.id,
        library_id: LibraryId(row.library_id),
        root_id: checked_u16(row.root_id, "root_id")?,
        root_path_norm: row.root_path_norm,
        path_norm: row.path_norm,
        hint_kind: row.hint_kind,
        payload: row.payload,
        status: decode_hint_status(row.status.as_str())?,
        idempotency_key: row.idempotency_key,
        attempts: i64_to_u64(i64::from(row.attempts), "attempts")? as u32,
        available_at: row.available_at,
        last_error: row.last_error,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn encode_library_type(library_type: LibraryType) -> &'static str {
    match library_type {
        LibraryType::Movies => "movies",
        LibraryType::Series => "tvshows",
    }
}

fn decode_library_type(value: &str) -> Result<LibraryType> {
    match value {
        "movies" => Ok(LibraryType::Movies),
        "tvshows" => Ok(LibraryType::Series),
        other => Err(MediaError::Internal(format!(
            "unknown manifest library type: {other}"
        ))),
    }
}

fn encode_run_status(status: ManifestRunStatus) -> &'static str {
    match status {
        ManifestRunStatus::Pending => "pending",
        ManifestRunStatus::Running => "running",
        ManifestRunStatus::Completed => "completed",
        ManifestRunStatus::CompletedWithDiagnostics => {
            "completed_with_diagnostics"
        }
        ManifestRunStatus::Failed => "failed",
        ManifestRunStatus::Canceled => "canceled",
        ManifestRunStatus::Stalled => "stalled",
    }
}

fn decode_run_status(value: &str) -> Result<ManifestRunStatus> {
    match value {
        "pending" => Ok(ManifestRunStatus::Pending),
        "running" => Ok(ManifestRunStatus::Running),
        "completed" => Ok(ManifestRunStatus::Completed),
        "completed_with_diagnostics" => {
            Ok(ManifestRunStatus::CompletedWithDiagnostics)
        }
        "failed" => Ok(ManifestRunStatus::Failed),
        "canceled" => Ok(ManifestRunStatus::Canceled),
        "stalled" => Ok(ManifestRunStatus::Stalled),
        other => Err(MediaError::Internal(format!(
            "unknown manifest run status: {other}"
        ))),
    }
}

fn is_successful_status(status: ManifestRunStatus) -> bool {
    matches!(
        status,
        ManifestRunStatus::Completed
            | ManifestRunStatus::CompletedWithDiagnostics
    )
}

fn encode_entry_kind(kind: ManifestEntryKind) -> &'static str {
    match kind {
        ManifestEntryKind::File => "file",
        ManifestEntryKind::Directory => "directory",
    }
}

fn decode_entry_kind(value: &str) -> Result<ManifestEntryKind> {
    match value {
        "file" => Ok(ManifestEntryKind::File),
        "directory" => Ok(ManifestEntryKind::Directory),
        other => Err(MediaError::Internal(format!(
            "unknown manifest entry kind: {other}"
        ))),
    }
}

fn encode_classification(
    classification: &ManifestEntryClassification,
) -> (&'static str, String) {
    match classification {
        ManifestEntryClassification::Supported(supported) => (
            "supported",
            encode_supported_classification(supported).to_string(),
        ),
        ManifestEntryClassification::Ignored(reason) => {
            ("ignored", encode_reason(*reason).to_string())
        }
        ManifestEntryClassification::Unsupported(reason) => {
            ("unsupported", encode_reason(*reason).to_string())
        }
    }
}

fn encode_supported_classification(
    classification: &ManifestSupportedClassification,
) -> &'static str {
    match classification {
        ManifestSupportedClassification::LibraryRoot => "library_root",
        ManifestSupportedClassification::MovieRootMedia => "movie_root_media",
        ManifestSupportedClassification::MovieFolder => "movie_folder",
        ManifestSupportedClassification::MovieFolderMedia => {
            "movie_folder_media"
        }
        ManifestSupportedClassification::SeriesRoot => "series_root",
        ManifestSupportedClassification::SeasonFolder { .. } => "season_folder",
        ManifestSupportedClassification::SeasonEpisode { .. } => {
            "season_episode"
        }
        ManifestSupportedClassification::DirectSeriesRootEpisode { .. } => {
            "direct_series_root_episode"
        }
    }
}

fn encode_reason(reason: ManifestDiagnosticReason) -> &'static str {
    match reason {
        ManifestDiagnosticReason::HiddenOrSystemPath => "hidden_or_system_path",
        ManifestDiagnosticReason::IgnoredExtension => "ignored_extension",
        ManifestDiagnosticReason::IgnoredPathPattern => "ignored_path_pattern",
        ManifestDiagnosticReason::NonMediaFile => "non_media_file",
        ManifestDiagnosticReason::PathOutsideRoot => "path_outside_root",
        ManifestDiagnosticReason::MovieNestedFolderUnsupported => {
            "movie_nested_folder_unsupported"
        }
        ManifestDiagnosticReason::MovieExtrasUnsupported => {
            "movie_extras_unsupported"
        }
        ManifestDiagnosticReason::SeriesLibraryRootMediaUnsupported => {
            "series_library_root_media_unsupported"
        }
        ManifestDiagnosticReason::SeriesDirectEpisodeParseFailed => {
            "series_direct_episode_parse_failed"
        }
        ManifestDiagnosticReason::SeriesEpisodeParseFailed => {
            "series_episode_parse_failed"
        }
        ManifestDiagnosticReason::SeriesSeasonMismatch => {
            "series_season_mismatch"
        }
        ManifestDiagnosticReason::SeriesNestedFolderUnsupported => {
            "series_nested_folder_unsupported"
        }
        ManifestDiagnosticReason::SeriesExtrasUnsupported => {
            "series_extras_unsupported"
        }
        ManifestDiagnosticReason::UnsupportedLayout => "unsupported_layout",
    }
}

fn encode_severity(severity: ManifestDiagnosticSeverity) -> &'static str {
    match severity {
        ManifestDiagnosticSeverity::Info => "info",
        ManifestDiagnosticSeverity::Warning => "warning",
        ManifestDiagnosticSeverity::Error => "error",
    }
}

fn decode_severity(value: &str) -> Result<ManifestDiagnosticSeverity> {
    match value {
        "info" => Ok(ManifestDiagnosticSeverity::Info),
        "warning" => Ok(ManifestDiagnosticSeverity::Warning),
        "error" => Ok(ManifestDiagnosticSeverity::Error),
        other => Err(MediaError::Internal(format!(
            "unknown manifest diagnostic severity: {other}"
        ))),
    }
}

fn encode_hint_status(status: ManifestDeferredWatchHintStatus) -> &'static str {
    match status {
        ManifestDeferredWatchHintStatus::Pending => "pending",
        ManifestDeferredWatchHintStatus::Applied => "applied",
        ManifestDeferredWatchHintStatus::Dropped => "dropped",
    }
}

fn decode_hint_status(value: &str) -> Result<ManifestDeferredWatchHintStatus> {
    match value {
        "pending" => Ok(ManifestDeferredWatchHintStatus::Pending),
        "applied" => Ok(ManifestDeferredWatchHintStatus::Applied),
        "dropped" => Ok(ManifestDeferredWatchHintStatus::Dropped),
        other => Err(MediaError::Internal(format!(
            "unknown manifest deferred watch hint status: {other}"
        ))),
    }
}

fn checked_u16(value: i32, field: &str) -> Result<u16> {
    u16::try_from(value).map_err(|_| {
        MediaError::Internal(format!(
            "{field} value {value} is outside u16 range"
        ))
    })
}

fn u64_to_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| {
        MediaError::Internal(format!(
            "{field} value {value} is outside i64 range"
        ))
    })
}

fn i64_to_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).map_err(|_| {
        MediaError::Internal(format!("{field} value {value} is negative"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::manifest::{
        ManifestDirectoryEntry, ManifestMediaEntry,
    };
    use std::str::FromStr;

    use sqlx::{Connection, Executor, PgConnection, postgres::PgPoolOptions};

    async fn with_manifest_test_db<F, Fut>(
        name: &str,
        test: F,
    ) -> std::result::Result<(), Box<dyn std::error::Error>>
    where
        F: FnOnce(PgPool) -> Fut,
        Fut: std::future::Future<
                Output = std::result::Result<(), Box<dyn std::error::Error>>,
            >,
    {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping {name}: DATABASE_URL is not set");
            return Ok(());
        };

        let master_options =
            sqlx::postgres::PgConnectOptions::from_str(&database_url)?;
        let db_name = format!("ferrex_manifest_{}", Uuid::new_v4().simple());
        let quoted_db = format!("\"{}\"", db_name.replace('"', "\"\""));

        let mut master = PgConnection::connect_with(&master_options).await?;
        master
            .execute(format!("CREATE DATABASE {quoted_db}").as_str())
            .await?;

        let test_result = async {
            let pool = PgPoolOptions::new()
                .max_connections(5)
                .after_connect(|conn, _| {
                    Box::pin(async move {
                        sqlx::query!("SET search_path = ferrex, public")
                            .execute(&mut *conn)
                            .await?;
                        Ok(())
                    })
                })
                .connect_with(master_options.clone().database(&db_name))
                .await?;
            crate::MIGRATOR.run(&pool).await?;
            let result = test(pool.clone()).await;
            pool.close().await;
            result
        }
        .await;

        let mut cleanup = PgConnection::connect_with(&master_options).await?;
        cleanup
            .execute(format!("DROP DATABASE IF EXISTS {quoted_db}").as_str())
            .await?;

        test_result
    }

    async fn insert_library(
        pool: &PgPool,
        library_id: LibraryId,
    ) -> std::result::Result<(), sqlx::Error> {
        let name = format!("Manifest Test Library {library_id}");
        let paths = vec!["/media/movies".to_string()];
        sqlx::query!(
            r#"
            INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at)
            VALUES ($1, $2, $3, 'movies', NOW(), NOW())
            "#,
            library_id.0,
            name,
            &paths[..]
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    fn root_scope(library_id: LibraryId) -> ManifestScope {
        ManifestScope::Root(ManifestRootScope {
            library_id,
            library_type: LibraryType::Movies,
            root_id: ManifestRootId(0),
            root_path_norm: "/media/movies".to_string(),
        })
    }

    #[tokio::test]
    async fn manifest_repository_upserts_runs_entries_diagnostics_and_hints()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        with_manifest_test_db("manifest_repository_upserts_runs_entries_diagnostics_and_hints", |pool| async move {
            let repo = PostgresManifestRepository::new(pool.clone());
            let library_id = LibraryId::new();
            insert_library(&pool, library_id).await?;

            let scope = root_scope(library_id);
            let run_id = Uuid::now_v7();
            let started_at = Utc::now();
            repo.start_run(ManifestRun {
                run_id,
                scope: scope.clone(),
                status: ManifestRunStatus::Running,
                started_at,
                completed_at: None,
                entries_seen: 0,
                diagnostics_seen: 0,
            })
            .await?;

            let unsupported_path = "/media/movies/Alien/Extras/Trailer.mkv";
            let batch = ManifestEntryBatch {
                scope: scope.clone(),
                entries: vec![
                    ManifestEntry::Directory(ManifestDirectoryEntry {
                        scope: scope.clone(),
                        path_norm: "/media/movies".to_string(),
                        relative_path: String::new(),
                        classification: ManifestEntryClassification::Supported(
                            ManifestSupportedClassification::LibraryRoot,
                        ),
                        diagnostics: Vec::new(),
                    }),
                    ManifestEntry::Media(ManifestMediaEntry {
                        scope: scope.clone(),
                        path_norm: "/media/movies/Alien.mkv".to_string(),
                        relative_path: "Alien.mkv".to_string(),
                        fingerprint: ManifestFingerprint {
                            size: 42,
                            mtime_ms: Some(1234),
                            ..ManifestFingerprint::default()
                        },
                        classification: ManifestEntryClassification::Supported(
                            ManifestSupportedClassification::MovieRootMedia,
                        ),
                        diagnostics: Vec::new(),
                    }),
                    ManifestEntry::Media(ManifestMediaEntry {
                        scope: scope.clone(),
                        path_norm: unsupported_path.to_string(),
                        relative_path: "Alien/Extras/Trailer.mkv".to_string(),
                        fingerprint: ManifestFingerprint {
                            size: 7,
                            ..ManifestFingerprint::default()
                        },
                        classification: ManifestEntryClassification::Unsupported(
                            ManifestDiagnosticReason::MovieExtrasUnsupported,
                        ),
                        diagnostics: vec![ManifestDiagnostic::new(
                            unsupported_path,
                            ManifestDiagnosticReason::MovieExtrasUnsupported,
                        )],
                    }),
                ],
            };

            let upsert = repo.upsert_batch_entries(run_id, &batch).await?;
            assert_eq!(upsert.entries_upserted, 3);
            assert_eq!(upsert.diagnostics_upserted, 1);

            let completed = repo
                .complete_run(ManifestRunCompletion {
                    run_id,
                    status: ManifestRunStatus::CompletedWithDiagnostics,
                    completed_at: Utc::now(),
                    entries_seen: 3,
                    diagnostics_seen: 1,
                    error_message: None,
                })
                .await?;
            assert_eq!(completed.status, ManifestRunStatus::CompletedWithDiagnostics);
            assert_eq!(completed.entries_seen, 3);

            let diagnostics = repo
                .list_diagnostics(ManifestDiagnosticFilter {
                    library_id: Some(library_id),
                    ..ManifestDiagnosticFilter::default()
                })
                .await?;
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(
                diagnostics[0].code,
                ManifestDiagnosticReason::MovieExtrasUnsupported.code()
            );

            let stale = repo
                .list_stale_partitions(
                    library_id,
                    Utc::now() + chrono::Duration::seconds(1),
                    10,
                )
                .await?;
            assert_eq!(stale.len(), 1);
            assert_eq!(stale[0].partition_key, "root");
            assert_eq!(stale[0].last_successful_run_id, Some(run_id));

            let hint = repo
                .upsert_deferred_watch_hint(ManifestDeferredWatchHintInput {
                    id: None,
                    library_id,
                    root_id: 0,
                    root_path_norm: "/media/movies".to_string(),
                    path_norm: "/media/movies/Alien.mkv".to_string(),
                    hint_kind: "modified".to_string(),
                    payload: serde_json::json!({"path":"/media/movies/Alien.mkv"}),
                    idempotency_key: "modified:/media/movies/Alien.mkv".to_string(),
                    available_at: Utc::now(),
                })
                .await?;
            assert_eq!(hint.status, ManifestDeferredWatchHintStatus::Pending);

            let pending = repo
                .list_deferred_watch_hints(ManifestDeferredWatchHintFilter {
                    library_id: Some(library_id),
                    status: Some(ManifestDeferredWatchHintStatus::Pending),
                    available_before: Some(Utc::now() + chrono::Duration::seconds(1)),
                    limit: Some(10),
                })
                .await?;
            assert_eq!(pending.len(), 1);

            let applied = repo
                .update_deferred_watch_hint_status(
                    hint.id,
                    ManifestDeferredWatchHintStatus::Applied,
                    None,
                )
                .await?
                .expect("updated hint");
            assert_eq!(applied.status, ManifestDeferredWatchHintStatus::Applied);

            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn successful_manifest_run_marks_stale_entries_missing()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        with_manifest_test_db("successful_manifest_run_marks_stale_entries_missing", |pool| async move {
            let repo = PostgresManifestRepository::new(pool.clone());
            let library_id = LibraryId::new();
            insert_library(&pool, library_id).await?;

            let scope = root_scope(library_id);
            let first_run = Uuid::now_v7();
            repo.start_run(ManifestRun {
                run_id: first_run,
                scope: scope.clone(),
                status: ManifestRunStatus::Running,
                started_at: Utc::now(),
                completed_at: None,
                entries_seen: 0,
                diagnostics_seen: 0,
            })
            .await?;
            repo.upsert_batch_entries(
                first_run,
                &ManifestEntryBatch {
                    scope: scope.clone(),
                    entries: vec![
                        ManifestEntry::Media(ManifestMediaEntry {
                            scope: scope.clone(),
                            path_norm: "/media/movies/Keep.mkv".to_string(),
                            relative_path: "Keep.mkv".to_string(),
                            fingerprint: ManifestFingerprint {
                                size: 10,
                                mtime_ms: Some(20),
                                ..ManifestFingerprint::default()
                            },
                            classification: ManifestEntryClassification::Supported(
                                ManifestSupportedClassification::MovieRootMedia,
                            ),
                            diagnostics: Vec::new(),
                        }),
                        ManifestEntry::Media(ManifestMediaEntry {
                            scope: scope.clone(),
                            path_norm: "/media/movies/Missing.mkv".to_string(),
                            relative_path: "Missing.mkv".to_string(),
                            fingerprint: ManifestFingerprint {
                                size: 11,
                                mtime_ms: Some(21),
                                ..ManifestFingerprint::default()
                            },
                            classification: ManifestEntryClassification::Supported(
                                ManifestSupportedClassification::MovieRootMedia,
                            ),
                            diagnostics: Vec::new(),
                        }),
                    ],
                },
            )
            .await?;
            repo.complete_run(ManifestRunCompletion {
                run_id: first_run,
                status: ManifestRunStatus::Completed,
                completed_at: Utc::now(),
                entries_seen: 2,
                diagnostics_seen: 0,
                error_message: None,
            })
            .await?;

            let second_run = Uuid::now_v7();
            repo.start_run(ManifestRun {
                run_id: second_run,
                scope: scope.clone(),
                status: ManifestRunStatus::Running,
                started_at: Utc::now(),
                completed_at: None,
                entries_seen: 0,
                diagnostics_seen: 0,
            })
            .await?;
            repo.upsert_batch_entries(
                second_run,
                &ManifestEntryBatch {
                    scope: scope.clone(),
                    entries: vec![ManifestEntry::Media(ManifestMediaEntry {
                        scope: scope.clone(),
                        path_norm: "/media/movies/Keep.mkv".to_string(),
                        relative_path: "Keep.mkv".to_string(),
                        fingerprint: ManifestFingerprint {
                            size: 10,
                            mtime_ms: Some(20),
                            ..ManifestFingerprint::default()
                        },
                        classification: ManifestEntryClassification::Supported(
                            ManifestSupportedClassification::MovieRootMedia,
                        ),
                        diagnostics: Vec::new(),
                    })],
                },
            )
            .await?;
            repo.complete_run(ManifestRunCompletion {
                run_id: second_run,
                status: ManifestRunStatus::Completed,
                completed_at: Utc::now(),
                entries_seen: 1,
                diagnostics_seen: 0,
                error_message: None,
            })
            .await?;

            let missing = repo
                .mark_missing_entries_after_successful_run(second_run)
                .await?;
            assert_eq!(missing.len(), 1);
            assert_eq!(missing[0].path_norm, "/media/movies/Missing.mkv");
            assert_eq!(missing[0].entry_kind, ManifestEntryKind::File);

            let keep_availability = sqlx::query_scalar!(
                "SELECT availability FROM manifest_entries WHERE library_id = $1 AND path_norm = '/media/movies/Keep.mkv'",
                library_id.0
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(keep_availability, "available");

            let missing_availability = sqlx::query_scalar!(
                "SELECT availability FROM manifest_entries WHERE library_id = $1 AND path_norm = '/media/movies/Missing.mkv'",
                library_id.0
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(missing_availability, "missing");

            let failed_run = Uuid::now_v7();
            repo.start_run(ManifestRun {
                run_id: failed_run,
                scope,
                status: ManifestRunStatus::Running,
                started_at: Utc::now(),
                completed_at: None,
                entries_seen: 0,
                diagnostics_seen: 0,
            })
            .await?;
            repo.complete_run(ManifestRunCompletion {
                run_id: failed_run,
                status: ManifestRunStatus::Failed,
                completed_at: Utc::now(),
                entries_seen: 0,
                diagnostics_seen: 0,
                error_message: Some("walk failed".to_string()),
            })
            .await?;
            let failed_missing = repo
                .mark_missing_entries_after_successful_run(failed_run)
                .await?;
            assert!(failed_missing.is_empty());

            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn legacy_backfill_records_available_state_without_tombstoning()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        with_manifest_test_db("legacy_backfill_records_available_state_without_tombstoning", |pool| async move {
            let repo = PostgresManifestRepository::new(pool.clone());
            let library_id = LibraryId::new();
            insert_library(&pool, library_id).await?;

            let available_file_id = Uuid::now_v7();
            let unavailable_file_id = Uuid::now_v7();
            let available_media_id = Uuid::now_v7();
            let unavailable_media_id = Uuid::now_v7();
            sqlx::query!(
                r#"
                INSERT INTO media_files (
                    id, library_id, media_id, media_type, file_path, filename, file_size, is_available
                )
                VALUES
                    ($1, $2, $3, 'movie'::media_type, '/media/movies/Alien.mkv', 'Alien.mkv', 42, TRUE),
                    ($4, $2, $5, 'movie'::media_type, '/media/movies/Missing.mkv', 'Missing.mkv', 99, FALSE)
                "#,
                available_file_id,
                library_id.0,
                available_media_id,
                unavailable_file_id,
                unavailable_media_id
            )
            .execute(&pool)
            .await?;

            sqlx::query!(
                r#"
                INSERT INTO folder_inventory (library_id, folder_path, folder_type, processing_status)
                VALUES ($1, '/media/movies/Alien', 'movie', 'completed')
                "#,
                library_id.0
            )
            .execute(&pool)
            .await?;

            sqlx::query!(
                r#"
                INSERT INTO scan_cursors (
                    library_id, path_hash, folder_path_norm, listing_hash, entry_count, last_scan_at
                )
                VALUES ($1, 12345, '/media/movies/Alien', 'listing', 1, NOW())
                "#,
                library_id.0
            )
            .execute(&pool)
            .await?;

            let summary = repo
                .backfill_legacy_manifest_state(Some(library_id))
                .await?;
            assert_eq!(summary.media_entries, 1);
            assert_eq!(summary.folder_entries, 1);
            assert_eq!(summary.legacy_cursors, 1);

            let unavailable_count = sqlx::query_scalar!(
                "SELECT COUNT(*)::bigint AS \"count!\" FROM media_files WHERE library_id = $1 AND is_available = FALSE",
                library_id.0
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(unavailable_count, 1);

            let backfilled_media_count = sqlx::query_scalar!(
                r#"
                SELECT COUNT(*)::bigint AS "count!"
                FROM manifest_entries
                WHERE library_id = $1
                  AND entry_kind = 'file'
                  AND source = 'backfill'
                  AND availability = 'available'
                "#,
                library_id.0
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(backfilled_media_count, 1);

            let missing_manifest_count = sqlx::query_scalar!(
                "SELECT COUNT(*)::bigint AS \"count!\" FROM manifest_entries WHERE library_id = $1 AND path_norm = '/media/movies/Missing.mkv'",
                library_id.0
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(missing_manifest_count, 0);

            let stale = repo
                .list_stale_partitions(library_id, Utc::now(), 10)
                .await?;
            assert_eq!(stale.len(), 1);
            assert!(stale[0].backfilled_from_legacy);
            assert_eq!(stale[0].last_successful_run_id, None);
            assert_eq!(stale[0].legacy_scan_path_hash, Some(12345));

            Ok(())
        })
        .await
    }
}
