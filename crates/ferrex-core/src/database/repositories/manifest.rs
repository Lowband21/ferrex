use std::fmt;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, QueryBuilder, Row, postgres::PgRow};
use uuid::Uuid;

use crate::{
    database::repository_ports::manifest::{
        ManifestBackfillSummary, ManifestBatchUpsertSummary,
        ManifestDeferredWatchHintFilter, ManifestDeferredWatchHintInput,
        ManifestDeferredWatchHintRecord, ManifestDeferredWatchHintStatus,
        ManifestDiagnosticFilter, ManifestDiagnosticRecord,
        ManifestPartitionCursorRecord, ManifestRepository,
        ManifestRunCompletion,
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

        sqlx::query(
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
        )
        .bind(parts.library_id.0)
        .bind(parts.library_type)
        .bind(parts.root_id)
        .bind(parts.root_path_norm)
        .bind(partition_key)
        .bind(parts.partition_id)
        .bind(parts.partition_prefix_norm)
        .bind(run.run_id)
        .bind(completed_at)
        .bind(u64_to_i64(run.entries_seen, "entries_seen")?)
        .bind(u64_to_i64(run.diagnostics_seen, "diagnostics_seen")?)
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
        let row = sqlx::query(
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
            "#,
        )
        .bind(run.run_id)
        .bind(parts.library_id.0)
        .bind(parts.library_type)
        .bind(parts.scope_kind)
        .bind(parts.root_id)
        .bind(parts.root_path_norm)
        .bind(parts.partition_id)
        .bind(parts.partition_prefix_norm)
        .bind(encode_run_status(run.status))
        .bind(run.started_at)
        .bind(run.completed_at)
        .bind(u64_to_i64(run.entries_seen, "entries_seen")?)
        .bind(u64_to_i64(run.diagnostics_seen, "diagnostics_seen")?)
        .fetch_one(&self.pool)
        .await?;

        row_to_manifest_run(&row)
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

        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
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
            "#,
        );

        builder.push_values(rows.iter(), |mut b, row| {
            b.push_bind(row.library_id)
                .push_bind(&row.path_norm)
                .push_bind(row.entry_kind)
                .push_bind(row.library_type)
                .push_bind(row.root_id)
                .push_bind(&row.root_path_norm)
                .push_bind(row.partition_id)
                .push_bind(&row.partition_prefix_norm)
                .push_bind(&row.relative_path)
                .push_bind(row.classification_status)
                .push_bind(&row.classification_kind)
                .push_bind(&row.classification_payload)
                .push_bind(&row.fingerprint_device_id)
                .push_bind(&row.fingerprint_inode)
                .push_bind(row.fingerprint_size)
                .push_bind(row.fingerprint_mtime_ms)
                .push_bind(&row.fingerprint_weak_hash)
                .push_bind(run_id)
                .push_bind(run_id)
                .push_bind("available")
                .push_bind("manifest");
        });

        builder.push(
            r#"
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
        );

        let entries_result = builder.build().execute(&self.pool).await?;

        let diagnostics_upserted = if diagnostics.is_empty() {
            0
        } else {
            let mut diag_builder = QueryBuilder::<Postgres>::new(
                r#"
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
                "#,
            );

            diag_builder.push_values(diagnostics.iter(), |mut b, row| {
                b.push_bind(row.run_id)
                    .push_bind(row.library_id)
                    .push_bind(row.root_id)
                    .push_bind(row.partition_id)
                    .push_bind(&row.path_norm)
                    .push_bind(&row.reason)
                    .push_bind(&row.code)
                    .push_bind(row.severity)
                    .push_bind(&row.remediation);
            });

            diag_builder.push(
                r#"
                ON CONFLICT (run_id, path_norm, code)
                DO UPDATE SET
                    reason = EXCLUDED.reason,
                    severity = EXCLUDED.severity,
                    remediation = EXCLUDED.remediation
                "#,
            );

            diag_builder
                .build()
                .execute(&self.pool)
                .await?
                .rows_affected()
        };

        Ok(ManifestBatchUpsertSummary {
            entries_upserted: entries_result.rows_affected(),
            diagnostics_upserted,
        })
    }

    async fn complete_run(
        &self,
        completion: ManifestRunCompletion,
    ) -> Result<ManifestRun> {
        let row = sqlx::query(
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
            "#,
        )
        .bind(completion.run_id)
        .bind(encode_run_status(completion.status))
        .bind(completion.completed_at)
        .bind(u64_to_i64(completion.entries_seen, "entries_seen")?)
        .bind(u64_to_i64(completion.diagnostics_seen, "diagnostics_seen")?)
        .bind(completion.error_message)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            MediaError::NotFound(format!(
                "manifest run {} was not found",
                completion.run_id
            ))
        })?;

        let run = row_to_manifest_run(&row)?;
        self.upsert_partition_cursor_for_run(&run).await?;
        Ok(run)
    }

    async fn list_stale_partitions(
        &self,
        library_id: LibraryId,
        older_than: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<ManifestPartitionCursorRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT
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
                supported_media_seen,
                first_path_norm,
                last_path_norm,
                legacy_scan_path_hash,
                backfilled_from_legacy,
                updated_at
            FROM manifest_partition_cursors
            WHERE library_id = $1
              AND (last_successful_at IS NULL OR last_successful_at < $2)
            ORDER BY last_successful_at ASC NULLS FIRST, updated_at ASC, root_id ASC, partition_key ASC
            LIMIT $3
            "#,
        )
        .bind(library_id.0)
        .bind(older_than)
        .bind(i64::from(limit.max(1)))
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_partition_cursor).collect()
    }

    async fn list_diagnostics(
        &self,
        filter: ManifestDiagnosticFilter,
    ) -> Result<Vec<ManifestDiagnosticRecord>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT
                id,
                run_id,
                library_id,
                root_id,
                partition_id,
                path_norm,
                reason,
                code,
                severity,
                remediation,
                created_at
            FROM manifest_diagnostics
            WHERE 1 = 1
            "#,
        );

        if let Some(library_id) = filter.library_id {
            builder.push(" AND library_id = ");
            builder.push_bind(library_id.0);
        }
        if let Some(run_id) = filter.run_id {
            builder.push(" AND run_id = ");
            builder.push_bind(run_id);
        }
        if let Some(severity) = filter.severity {
            builder.push(" AND severity = ");
            builder.push_bind(encode_severity(severity));
        }
        if let Some(code) = filter.code {
            builder.push(" AND code = ");
            builder.push_bind(code);
        }

        builder.push(" ORDER BY created_at DESC, id DESC LIMIT ");
        builder.push_bind(i64::from(filter.limit.unwrap_or(100).max(1)));

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.iter().map(row_to_diagnostic).collect()
    }

    async fn upsert_deferred_watch_hint(
        &self,
        hint: ManifestDeferredWatchHintInput,
    ) -> Result<ManifestDeferredWatchHintRecord> {
        let id = hint.id.unwrap_or_else(Uuid::now_v7);
        let row = sqlx::query(
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
                id,
                library_id,
                root_id,
                root_path_norm,
                path_norm,
                hint_kind,
                payload,
                status,
                idempotency_key,
                attempts,
                available_at,
                last_error,
                created_at,
                updated_at
            "#,
        )
        .bind(id)
        .bind(hint.library_id.0)
        .bind(i32::from(hint.root_id))
        .bind(hint.root_path_norm)
        .bind(hint.path_norm)
        .bind(hint.hint_kind)
        .bind(hint.payload)
        .bind(hint.idempotency_key)
        .bind(hint.available_at)
        .fetch_one(&self.pool)
        .await?;

        row_to_deferred_hint(&row)
    }

    async fn list_deferred_watch_hints(
        &self,
        filter: ManifestDeferredWatchHintFilter,
    ) -> Result<Vec<ManifestDeferredWatchHintRecord>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT
                id,
                library_id,
                root_id,
                root_path_norm,
                path_norm,
                hint_kind,
                payload,
                status,
                idempotency_key,
                attempts,
                available_at,
                last_error,
                created_at,
                updated_at
            FROM manifest_deferred_watch_hints
            WHERE 1 = 1
            "#,
        );

        if let Some(library_id) = filter.library_id {
            builder.push(" AND library_id = ");
            builder.push_bind(library_id.0);
        }
        if let Some(status) = filter.status {
            builder.push(" AND status = ");
            builder.push_bind(encode_hint_status(status));
        }
        if let Some(available_before) = filter.available_before {
            builder.push(" AND available_at <= ");
            builder.push_bind(available_before);
        }

        builder.push(" ORDER BY available_at ASC, created_at ASC LIMIT ");
        builder.push_bind(i64::from(filter.limit.unwrap_or(100).max(1)));

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.iter().map(row_to_deferred_hint).collect()
    }

    async fn update_deferred_watch_hint_status(
        &self,
        id: Uuid,
        status: ManifestDeferredWatchHintStatus,
        last_error: Option<String>,
    ) -> Result<Option<ManifestDeferredWatchHintRecord>> {
        let row = sqlx::query(
            r#"
            UPDATE manifest_deferred_watch_hints
            SET status = $2,
                attempts = CASE WHEN $2 = 'pending' THEN attempts + 1 ELSE attempts END,
                last_error = $3,
                updated_at = NOW()
            WHERE id = $1
            RETURNING
                id,
                library_id,
                root_id,
                root_path_norm,
                path_norm,
                hint_kind,
                payload,
                status,
                idempotency_key,
                attempts,
                available_at,
                last_error,
                created_at,
                updated_at
            "#,
        )
        .bind(id)
        .bind(encode_hint_status(status))
        .bind(last_error)
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_deferred_hint).transpose()
    }

    async fn backfill_legacy_manifest_state(
        &self,
        library_id: Option<LibraryId>,
    ) -> Result<ManifestBackfillSummary> {
        let library_uuid = library_id.map(|id| id.0);

        let media_entries = sqlx::query(MEDIA_BACKFILL_SQL)
            .bind(library_uuid)
            .execute(&self.pool)
            .await?
            .rows_affected();

        let folder_entries = sqlx::query(FOLDER_BACKFILL_SQL)
            .bind(library_uuid)
            .execute(&self.pool)
            .await?
            .rows_affected();

        let legacy_cursors = sqlx::query(SCAN_CURSOR_BACKFILL_SQL)
            .bind(library_uuid)
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

fn row_to_manifest_run(row: &PgRow) -> Result<ManifestRun> {
    let library_id = LibraryId(row.try_get("library_id")?);
    let library_type = decode_library_type(
        row.try_get::<String, _>("library_type")?.as_str(),
    )?;
    let root_id = checked_u16(row.try_get::<i32, _>("root_id")?, "root_id")?;
    let root = ManifestRootScope {
        library_id,
        library_type,
        root_id: ManifestRootId(root_id),
        root_path_norm: row.try_get("root_path_norm")?,
    };
    let scope_kind: String = row.try_get("scope_kind")?;
    let scope = match scope_kind.as_str() {
        "root" => ManifestScope::Root(root),
        "partition" => {
            let partition_id = row
                .try_get::<Option<i32>, _>("partition_id")?
                .ok_or_else(|| {
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
                prefix_norm: row.try_get("partition_prefix_norm")?,
            })
        }
        other => {
            return Err(MediaError::Internal(format!(
                "unknown manifest scope kind: {other}"
            )));
        }
    };

    Ok(ManifestRun {
        run_id: row.try_get("run_id")?,
        scope,
        status: decode_run_status(
            row.try_get::<String, _>("status")?.as_str(),
        )?,
        started_at: row.try_get("started_at")?,
        completed_at: row.try_get("completed_at")?,
        entries_seen: i64_to_u64(row.try_get("entries_seen")?, "entries_seen")?,
        diagnostics_seen: i64_to_u64(
            row.try_get("diagnostics_seen")?,
            "diagnostics_seen",
        )?,
    })
}

fn row_to_partition_cursor(
    row: &PgRow,
) -> Result<ManifestPartitionCursorRecord> {
    Ok(ManifestPartitionCursorRecord {
        library_id: LibraryId(row.try_get("library_id")?),
        library_type: decode_library_type(
            row.try_get::<String, _>("library_type")?.as_str(),
        )?,
        root_id: checked_u16(row.try_get::<i32, _>("root_id")?, "root_id")?,
        root_path_norm: row.try_get("root_path_norm")?,
        partition_key: row.try_get("partition_key")?,
        partition_id: row
            .try_get::<Option<i32>, _>("partition_id")?
            .map(|value| {
                checked_u16(value, "partition_id").map(ManifestPartitionId)
            })
            .transpose()?,
        prefix_norm: row.try_get("prefix_norm")?,
        last_successful_run_id: row.try_get("last_successful_run_id")?,
        last_successful_at: row.try_get("last_successful_at")?,
        last_observed_at: row.try_get("last_observed_at")?,
        entries_seen: i64_to_u64(row.try_get("entries_seen")?, "entries_seen")?,
        diagnostics_seen: i64_to_u64(
            row.try_get("diagnostics_seen")?,
            "diagnostics_seen",
        )?,
        supported_media_seen: i64_to_u64(
            row.try_get("supported_media_seen")?,
            "supported_media_seen",
        )?,
        first_path_norm: row.try_get("first_path_norm")?,
        last_path_norm: row.try_get("last_path_norm")?,
        legacy_scan_path_hash: row.try_get("legacy_scan_path_hash")?,
        backfilled_from_legacy: row.try_get("backfilled_from_legacy")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_diagnostic(row: &PgRow) -> Result<ManifestDiagnosticRecord> {
    Ok(ManifestDiagnosticRecord {
        id: row.try_get("id")?,
        run_id: row.try_get("run_id")?,
        library_id: LibraryId(row.try_get("library_id")?),
        root_id: checked_u16(row.try_get::<i32, _>("root_id")?, "root_id")?,
        partition_id: row
            .try_get::<Option<i32>, _>("partition_id")?
            .map(|value| {
                checked_u16(value, "partition_id").map(ManifestPartitionId)
            })
            .transpose()?,
        path_norm: row.try_get("path_norm")?,
        reason: row.try_get("reason")?,
        code: row.try_get("code")?,
        severity: decode_severity(
            row.try_get::<String, _>("severity")?.as_str(),
        )?,
        remediation: row.try_get("remediation")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_deferred_hint(
    row: &PgRow,
) -> Result<ManifestDeferredWatchHintRecord> {
    Ok(ManifestDeferredWatchHintRecord {
        id: row.try_get("id")?,
        library_id: LibraryId(row.try_get("library_id")?),
        root_id: checked_u16(row.try_get::<i32, _>("root_id")?, "root_id")?,
        root_path_norm: row.try_get("root_path_norm")?,
        path_norm: row.try_get("path_norm")?,
        hint_kind: row.try_get("hint_kind")?,
        payload: row.try_get("payload")?,
        status: decode_hint_status(
            row.try_get::<String, _>("status")?.as_str(),
        )?,
        idempotency_key: row.try_get("idempotency_key")?,
        attempts: i64_to_u64(
            i64::from(row.try_get::<i32, _>("attempts")?),
            "attempts",
        )? as u32,
        available_at: row.try_get("available_at")?,
        last_error: row.try_get("last_error")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
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

const MEDIA_BACKFILL_SQL: &str = r#"
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
"#;

const FOLDER_BACKFILL_SQL: &str = r#"
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
"#;

const SCAN_CURSOR_BACKFILL_SQL: &str = r#"
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
"#;

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
                        sqlx::query("SET search_path = ferrex, public")
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
        sqlx::query(
            r#"
            INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at)
            VALUES ($1, $2, $3, 'movies', NOW(), NOW())
            "#,
        )
        .bind(library_id.0)
        .bind(format!("Manifest Test Library {library_id}"))
        .bind(vec!["/media/movies".to_string()])
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
    async fn legacy_backfill_records_available_state_without_tombstoning()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        with_manifest_test_db("legacy_backfill_records_available_state_without_tombstoning", |pool| async move {
            let repo = PostgresManifestRepository::new(pool.clone());
            let library_id = LibraryId::new();
            insert_library(&pool, library_id).await?;

            let available_file_id = Uuid::now_v7();
            let unavailable_file_id = Uuid::now_v7();
            sqlx::query(
                r#"
                INSERT INTO media_files (
                    id, library_id, media_id, media_type, file_path, filename, file_size, is_available
                )
                VALUES
                    ($1, $2, $3, 'movie'::media_type, '/media/movies/Alien.mkv', 'Alien.mkv', 42, TRUE),
                    ($4, $2, $5, 'movie'::media_type, '/media/movies/Missing.mkv', 'Missing.mkv', 99, FALSE)
                "#,
            )
            .bind(available_file_id)
            .bind(library_id.0)
            .bind(Uuid::now_v7())
            .bind(unavailable_file_id)
            .bind(Uuid::now_v7())
            .execute(&pool)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO folder_inventory (library_id, folder_path, folder_type, processing_status)
                VALUES ($1, '/media/movies/Alien', 'movie', 'completed')
                "#,
            )
            .bind(library_id.0)
            .execute(&pool)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO scan_cursors (
                    library_id, path_hash, folder_path_norm, listing_hash, entry_count, last_scan_at
                )
                VALUES ($1, 12345, '/media/movies/Alien', 'listing', 1, NOW())
                "#,
            )
            .bind(library_id.0)
            .execute(&pool)
            .await?;

            let summary = repo
                .backfill_legacy_manifest_state(Some(library_id))
                .await?;
            assert_eq!(summary.media_entries, 1);
            assert_eq!(summary.folder_entries, 1);
            assert_eq!(summary.legacy_cursors, 1);

            let unavailable_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM media_files WHERE library_id = $1 AND is_available = FALSE",
            )
            .bind(library_id.0)
            .fetch_one(&pool)
            .await?;
            assert_eq!(unavailable_count, 1);

            let backfilled_media_count: i64 = sqlx::query_scalar(
                r#"
                SELECT COUNT(*)
                FROM manifest_entries
                WHERE library_id = $1
                  AND entry_kind = 'file'
                  AND source = 'backfill'
                  AND availability = 'available'
                "#,
            )
            .bind(library_id.0)
            .fetch_one(&pool)
            .await?;
            assert_eq!(backfilled_media_count, 1);

            let missing_manifest_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM manifest_entries WHERE library_id = $1 AND path_norm = '/media/movies/Missing.mkv'",
            )
            .bind(library_id.0)
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
