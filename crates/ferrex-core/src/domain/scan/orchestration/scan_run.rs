use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

use crate::{
    api::types::{ScanLifecycleStatus, ScanRunMode, ScanStartDisposition},
    error::{MediaError, Result},
    types::LibraryId,
};

/// Active durable scan statuses for partial-index conflict handling.
const ACTIVE_STATUS_SQL: &str = "'pending','running','paused'";

/// Durable row representing one public library scan run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryScanRun {
    pub scan_id: Uuid,
    pub library_id: LibraryId,
    pub mode: ScanRunMode,
    pub run_key: String,
    pub correlation_id: Uuid,
    pub status: ScanLifecycleStatus,
    pub completed_items: u64,
    pub total_items: u64,
    pub retrying_items: u64,
    pub dead_lettered_items: u64,
    pub current_path: Option<String>,
    pub last_error: Option<String>,
    pub sequence: u64,
    pub started_at: DateTime<Utc>,
    pub terminal_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl LibraryScanRun {
    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }
}

/// Request for creating the active run for a library+mode pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewLibraryScanRun {
    pub library_id: LibraryId,
    pub mode: ScanRunMode,
    pub scan_id: Option<Uuid>,
    pub correlation_id: Option<Uuid>,
    pub status: ScanLifecycleStatus,
}

impl NewLibraryScanRun {
    pub fn new(library_id: LibraryId, mode: ScanRunMode) -> Self {
        Self {
            library_id,
            mode,
            scan_id: None,
            correlation_id: None,
            status: ScanLifecycleStatus::Pending,
        }
    }

    pub fn with_scan_id(mut self, scan_id: Uuid) -> Self {
        self.scan_id = Some(scan_id);
        self
    }

    pub fn with_correlation_id(mut self, correlation_id: Uuid) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    pub fn running(mut self) -> Self {
        self.status = ScanLifecycleStatus::Running;
        self
    }
}

/// Result of an idempotent get-or-create operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryScanRunGetOrCreate {
    pub run: LibraryScanRun,
    pub disposition: ScanStartDisposition,
}

/// Progress fields that can be persisted for an active scan run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryScanRunProgressUpdate {
    pub scan_id: Uuid,
    pub status: Option<ScanLifecycleStatus>,
    pub completed_items: u64,
    pub total_items: u64,
    pub retrying_items: u64,
    pub dead_lettered_items: u64,
    pub current_path: Option<String>,
    pub sequence: u64,
}

/// Repository contract for durable library scan runs.
#[async_trait]
pub trait ScanRunRepository: Send + Sync {
    async fn get_or_create_active(
        &self,
        request: NewLibraryScanRun,
    ) -> Result<LibraryScanRunGetOrCreate>;

    async fn load_active(
        &self,
        library_id: LibraryId,
        mode: ScanRunMode,
    ) -> Result<Option<LibraryScanRun>>;

    async fn list_active(&self) -> Result<Vec<LibraryScanRun>>;

    async fn update_progress(
        &self,
        update: LibraryScanRunProgressUpdate,
    ) -> Result<Option<LibraryScanRun>>;

    async fn mark_terminal(
        &self,
        scan_id: Uuid,
        status: ScanLifecycleStatus,
        terminal_at: DateTime<Utc>,
        last_error: Option<String>,
    ) -> Result<Option<LibraryScanRun>>;
}

#[derive(Clone)]
pub struct PostgresScanRunRepository {
    pool: PgPool,
}

impl std::fmt::Debug for PostgresScanRunRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresScanRunRepository")
            .field("pool_size", &self.pool.size())
            .field("idle_connections", &self.pool.num_idle())
            .finish()
    }
}

impl PostgresScanRunRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[derive(Debug, sqlx::FromRow)]
struct LibraryScanRunRow {
    scan_id: Uuid,
    library_id: Uuid,
    mode: String,
    run_key: String,
    correlation_id: Uuid,
    status: String,
    completed_items: i64,
    total_items: i64,
    retrying_items: i64,
    dead_lettered_items: i64,
    current_path: Option<String>,
    last_error: Option<String>,
    sequence: i64,
    started_at: DateTime<Utc>,
    terminal_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<LibraryScanRunRow> for LibraryScanRun {
    type Error = MediaError;

    fn try_from(row: LibraryScanRunRow) -> Result<Self> {
        let mode = ScanRunMode::from_db(&row.mode).ok_or_else(|| {
            MediaError::Internal(format!(
                "invalid library_scan_runs.mode '{}'",
                row.mode
            ))
        })?;
        let status =
            ScanLifecycleStatus::from_db(&row.status).ok_or_else(|| {
                MediaError::Internal(format!(
                    "invalid library_scan_runs.status '{}'",
                    row.status
                ))
            })?;

        Ok(Self {
            scan_id: row.scan_id,
            library_id: LibraryId(row.library_id),
            mode,
            run_key: row.run_key,
            correlation_id: row.correlation_id,
            status,
            completed_items: i64_to_u64(
                "completed_items",
                row.completed_items,
            )?,
            total_items: i64_to_u64("total_items", row.total_items)?,
            retrying_items: i64_to_u64("retrying_items", row.retrying_items)?,
            dead_lettered_items: i64_to_u64(
                "dead_lettered_items",
                row.dead_lettered_items,
            )?,
            current_path: row.current_path,
            last_error: row.last_error,
            sequence: i64_to_u64("sequence", row.sequence)?,
            started_at: row.started_at,
            terminal_at: row.terminal_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

fn i64_to_u64(field: &str, value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| {
        MediaError::Internal(format!(
            "library_scan_runs.{field} contained negative value {value}"
        ))
    })
}

fn u64_to_i64(field: &str, value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| {
        MediaError::InvalidMedia(format!(
            "library_scan_runs.{field} value {value} exceeds PostgreSQL bigint"
        ))
    })
}

fn row_to_run(row: LibraryScanRunRow) -> Result<LibraryScanRun> {
    LibraryScanRun::try_from(row)
}

#[async_trait]
impl ScanRunRepository for PostgresScanRunRepository {
    async fn get_or_create_active(
        &self,
        request: NewLibraryScanRun,
    ) -> Result<LibraryScanRunGetOrCreate> {
        let scan_id = request.scan_id.unwrap_or_else(Uuid::now_v7);
        let correlation_id = request.correlation_id.unwrap_or(scan_id);
        let mode = request.mode.as_str();
        let status = request.status.as_str();
        let library_id = request.library_id.to_uuid();

        for attempt in 0..2 {
            let mut tx = self.pool.begin().await.map_err(|e| {
                MediaError::Internal(format!(
                    "begin library scan run transaction failed: {e}"
                ))
            })?;

            let inserted = sqlx::query_as!(
                LibraryScanRunRow,
                r#"
                INSERT INTO library_scan_runs (
                    scan_id, library_id, mode, correlation_id, status,
                    started_at, created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, NOW(), NOW(), NOW())
                ON CONFLICT (run_key)
                    WHERE status::text IN ('pending','running','paused')
                    DO NOTHING
                RETURNING
                    scan_id,
                    library_id,
                    mode as "mode!",
                    run_key as "run_key!",
                    correlation_id,
                    status as "status!",
                    completed_items as "completed_items!",
                    total_items as "total_items!",
                    retrying_items as "retrying_items!",
                    dead_lettered_items as "dead_lettered_items!",
                    current_path,
                    last_error,
                    sequence as "sequence!",
                    started_at,
                    terminal_at,
                    created_at,
                    updated_at
                "#,
                scan_id,
                library_id,
                mode,
                correlation_id,
                status
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "insert library scan run failed: {e}"
                ))
            })?;

            if let Some(row) = inserted {
                tx.commit().await.map_err(|e| {
                    MediaError::Internal(format!(
                        "commit library scan run insert failed: {e}"
                    ))
                })?;
                return Ok(LibraryScanRunGetOrCreate {
                    run: row_to_run(row)?,
                    disposition: ScanStartDisposition::Created,
                });
            }

            let existing = sqlx::query_as!(
                LibraryScanRunRow,
                r#"
                SELECT
                    scan_id,
                    library_id,
                    mode as "mode!",
                    run_key as "run_key!",
                    correlation_id,
                    status as "status!",
                    completed_items as "completed_items!",
                    total_items as "total_items!",
                    retrying_items as "retrying_items!",
                    dead_lettered_items as "dead_lettered_items!",
                    current_path,
                    last_error,
                    sequence as "sequence!",
                    started_at,
                    terminal_at,
                    created_at,
                    updated_at
                FROM library_scan_runs
                WHERE library_id = $1
                  AND mode = $2
                  AND status::text IN ('pending','running','paused')
                ORDER BY started_at ASC
                LIMIT 1
                FOR UPDATE
                "#,
                library_id,
                mode
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "load active library scan run after conflict failed: {e}"
                ))
            })?;

            if let Some(row) = existing {
                tx.commit().await.map_err(|e| {
                    MediaError::Internal(format!(
                        "commit library scan run reuse failed: {e}"
                    ))
                })?;
                return Ok(LibraryScanRunGetOrCreate {
                    run: row_to_run(row)?,
                    disposition: ScanStartDisposition::Reused,
                });
            }

            tx.rollback().await.map_err(|e| {
                MediaError::Internal(format!(
                    "rollback library scan run conflict retry failed: {e}"
                ))
            })?;
            warn!(
                attempt,
                library = %request.library_id,
                mode = %mode,
                active_statuses = ACTIVE_STATUS_SQL,
                "active library scan run conflict raced with terminal transition; retrying"
            );
        }

        Err(MediaError::Conflict(format!(
            "could not get or create active scan run for library {} mode {}",
            request.library_id, mode
        )))
    }

    async fn load_active(
        &self,
        library_id: LibraryId,
        mode: ScanRunMode,
    ) -> Result<Option<LibraryScanRun>> {
        let row = sqlx::query_as!(
            LibraryScanRunRow,
            r#"
            SELECT
                scan_id,
                library_id,
                mode as "mode!",
                run_key as "run_key!",
                correlation_id,
                status as "status!",
                completed_items as "completed_items!",
                total_items as "total_items!",
                retrying_items as "retrying_items!",
                dead_lettered_items as "dead_lettered_items!",
                current_path,
                last_error,
                sequence as "sequence!",
                started_at,
                terminal_at,
                created_at,
                updated_at
            FROM library_scan_runs
            WHERE library_id = $1
              AND mode = $2
              AND status::text IN ('pending','running','paused')
            ORDER BY started_at ASC
            LIMIT 1
            "#,
            library_id.to_uuid(),
            mode.as_str()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "load active library scan run failed: {e}"
            ))
        })?;

        row.map(row_to_run).transpose()
    }

    async fn list_active(&self) -> Result<Vec<LibraryScanRun>> {
        let rows = sqlx::query_as::<_, LibraryScanRunRow>(
            r#"
            SELECT
                scan_id,
                library_id,
                mode,
                run_key,
                correlation_id,
                status,
                completed_items,
                total_items,
                retrying_items,
                dead_lettered_items,
                current_path,
                last_error,
                sequence,
                started_at,
                terminal_at,
                created_at,
                updated_at
            FROM library_scan_runs
            WHERE status::text IN ('pending','running','paused')
            ORDER BY started_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "list active library scan runs failed: {e}"
            ))
        })?;

        rows.into_iter().map(row_to_run).collect()
    }

    async fn update_progress(
        &self,
        update: LibraryScanRunProgressUpdate,
    ) -> Result<Option<LibraryScanRun>> {
        if let Some(status) = &update.status
            && status.is_terminal()
        {
            return Err(MediaError::InvalidMedia(
                "update_progress cannot mark a scan run terminal; use mark_terminal"
                    .into(),
            ));
        }

        let status = update.status.as_ref().map(|status| status.as_str());
        let completed_items =
            u64_to_i64("completed_items", update.completed_items)?;
        let total_items = u64_to_i64("total_items", update.total_items)?;
        let retrying_items =
            u64_to_i64("retrying_items", update.retrying_items)?;
        let dead_lettered_items =
            u64_to_i64("dead_lettered_items", update.dead_lettered_items)?;
        let sequence = u64_to_i64("sequence", update.sequence)?;

        let row = sqlx::query_as!(
            LibraryScanRunRow,
            r#"
            UPDATE library_scan_runs
            SET status = COALESCE($2::varchar, status),
                completed_items = $3,
                total_items = $4,
                retrying_items = $5,
                dead_lettered_items = $6,
                current_path = $7,
                sequence = GREATEST(sequence, $8),
                updated_at = NOW()
            WHERE scan_id = $1
              AND status::text IN ('pending','running','paused')
            RETURNING
                scan_id,
                library_id,
                mode as "mode!",
                run_key as "run_key!",
                correlation_id,
                status as "status!",
                completed_items as "completed_items!",
                total_items as "total_items!",
                retrying_items as "retrying_items!",
                dead_lettered_items as "dead_lettered_items!",
                current_path,
                last_error,
                sequence as "sequence!",
                started_at,
                terminal_at,
                created_at,
                updated_at
            "#,
            update.scan_id,
            status,
            completed_items,
            total_items,
            retrying_items,
            dead_lettered_items,
            update.current_path,
            sequence
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "update library scan run progress failed: {e}"
            ))
        })?;

        row.map(row_to_run).transpose()
    }

    async fn mark_terminal(
        &self,
        scan_id: Uuid,
        status: ScanLifecycleStatus,
        terminal_at: DateTime<Utc>,
        last_error: Option<String>,
    ) -> Result<Option<LibraryScanRun>> {
        if !status.is_terminal() {
            return Err(MediaError::InvalidMedia(format!(
                "mark_terminal requires a terminal status, got {}",
                status.as_str()
            )));
        }

        let row = sqlx::query_as!(
            LibraryScanRunRow,
            r#"
            UPDATE library_scan_runs
            SET status = $2,
                terminal_at = $3,
                last_error = $4,
                updated_at = NOW()
            WHERE scan_id = $1
              AND status::text IN ('pending','running','paused')
            RETURNING
                scan_id,
                library_id,
                mode as "mode!",
                run_key as "run_key!",
                correlation_id,
                status as "status!",
                completed_items as "completed_items!",
                total_items as "total_items!",
                retrying_items as "retrying_items!",
                dead_lettered_items as "dead_lettered_items!",
                current_path,
                last_error,
                sequence as "sequence!",
                started_at,
                terminal_at,
                created_at,
                updated_at
            "#,
            scan_id,
            status.as_str(),
            terminal_at,
            last_error
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "mark library scan run terminal failed: {e}"
            ))
        })?;

        row.map(row_to_run).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::{HashMap, HashSet},
        sync::Arc,
    };
    use tokio::sync::{Barrier, Mutex};

    #[derive(Default)]
    struct InMemoryScanRunRepository {
        runs: Mutex<HashMap<Uuid, LibraryScanRun>>,
    }

    #[async_trait]
    impl ScanRunRepository for InMemoryScanRunRepository {
        async fn get_or_create_active(
            &self,
            request: NewLibraryScanRun,
        ) -> Result<LibraryScanRunGetOrCreate> {
            let mut guard = self.runs.lock().await;
            if let Some(existing) = guard.values().find(|run| {
                run.library_id == request.library_id
                    && run.mode == request.mode
                    && run.is_active()
            }) {
                return Ok(LibraryScanRunGetOrCreate {
                    run: existing.clone(),
                    disposition: ScanStartDisposition::Reused,
                });
            }

            let scan_id = request.scan_id.unwrap_or_else(Uuid::now_v7);
            let correlation_id = request.correlation_id.unwrap_or(scan_id);
            let now = Utc::now();
            let run = LibraryScanRun {
                scan_id,
                library_id: request.library_id,
                mode: request.mode,
                run_key: request.mode.run_key(request.library_id),
                correlation_id,
                status: request.status,
                completed_items: 0,
                total_items: 0,
                retrying_items: 0,
                dead_lettered_items: 0,
                current_path: None,
                last_error: None,
                sequence: 0,
                started_at: now,
                terminal_at: None,
                created_at: now,
                updated_at: now,
            };
            guard.insert(scan_id, run.clone());
            Ok(LibraryScanRunGetOrCreate {
                run,
                disposition: ScanStartDisposition::Created,
            })
        }

        async fn load_active(
            &self,
            library_id: LibraryId,
            mode: ScanRunMode,
        ) -> Result<Option<LibraryScanRun>> {
            let guard = self.runs.lock().await;
            Ok(guard
                .values()
                .find(|run| {
                    run.library_id == library_id
                        && run.mode == mode
                        && run.is_active()
                })
                .cloned())
        }

        async fn list_active(&self) -> Result<Vec<LibraryScanRun>> {
            let guard = self.runs.lock().await;
            Ok(guard
                .values()
                .filter(|run| run.is_active())
                .cloned()
                .collect())
        }

        async fn update_progress(
            &self,
            update: LibraryScanRunProgressUpdate,
        ) -> Result<Option<LibraryScanRun>> {
            let mut guard = self.runs.lock().await;
            let Some(run) = guard.get_mut(&update.scan_id) else {
                return Ok(None);
            };
            if !run.is_active() {
                return Ok(None);
            }
            if let Some(status) = update.status {
                run.status = status;
            }
            run.completed_items = update.completed_items;
            run.total_items = update.total_items;
            run.retrying_items = update.retrying_items;
            run.dead_lettered_items = update.dead_lettered_items;
            run.current_path = update.current_path;
            run.sequence = run.sequence.max(update.sequence);
            run.updated_at = Utc::now();
            Ok(Some(run.clone()))
        }

        async fn mark_terminal(
            &self,
            scan_id: Uuid,
            status: ScanLifecycleStatus,
            terminal_at: DateTime<Utc>,
            last_error: Option<String>,
        ) -> Result<Option<LibraryScanRun>> {
            let mut guard = self.runs.lock().await;
            let Some(run) = guard.get_mut(&scan_id) else {
                return Ok(None);
            };
            if !run.is_active() {
                return Ok(None);
            }
            run.status = status;
            run.terminal_at = Some(terminal_at);
            run.last_error = last_error;
            run.updated_at = Utc::now();
            Ok(Some(run.clone()))
        }
    }

    #[tokio::test]
    async fn get_or_create_active_reuses_same_library_mode_run() {
        let repo = InMemoryScanRunRepository::default();
        let library_id = LibraryId(Uuid::now_v7());

        let first = repo
            .get_or_create_active(NewLibraryScanRun::new(
                library_id,
                ScanRunMode::Manual,
            ))
            .await
            .expect("first get_or_create succeeds");
        let second = repo
            .get_or_create_active(NewLibraryScanRun::new(
                library_id,
                ScanRunMode::Manual,
            ))
            .await
            .expect("second get_or_create succeeds");

        assert_eq!(first.disposition, ScanStartDisposition::Created);
        assert_eq!(second.disposition, ScanStartDisposition::Reused);
        assert_eq!(first.run.scan_id, second.run.scan_id);
        assert_eq!(first.run.run_key, second.run.run_key);
        assert_eq!(
            repo.load_active(library_id, ScanRunMode::Manual)
                .await
                .expect("load active succeeds")
                .expect("active run exists")
                .scan_id,
            first.run.scan_id
        );
    }

    #[tokio::test]
    async fn concurrent_get_or_create_active_reuses_one_run_per_library_mode() {
        let repo = Arc::new(InMemoryScanRunRepository::default());
        let library_id = LibraryId(Uuid::now_v7());
        let concurrent_requests = 8usize;
        let barrier = Arc::new(Barrier::new(concurrent_requests));
        let mut tasks = Vec::with_capacity(concurrent_requests);

        for _ in 0..concurrent_requests {
            let repo = Arc::clone(&repo);
            let barrier = Arc::clone(&barrier);
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                repo.get_or_create_active(
                    NewLibraryScanRun::new(library_id, ScanRunMode::Manual)
                        .with_scan_id(Uuid::now_v7())
                        .with_correlation_id(Uuid::now_v7())
                        .running(),
                )
                .await
                .expect("concurrent get_or_create succeeds")
            }));
        }

        let mut results = Vec::with_capacity(concurrent_requests);
        for task in tasks {
            results.push(task.await.expect("start task joins"));
        }

        let manual_scan_ids: HashSet<Uuid> =
            results.iter().map(|result| result.run.scan_id).collect();
        assert_eq!(manual_scan_ids.len(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| {
                    result.disposition == ScanStartDisposition::Created
                })
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| {
                    result.disposition == ScanStartDisposition::Reused
                })
                .count(),
            concurrent_requests - 1
        );

        let manual_scan_id = *manual_scan_ids.iter().next().unwrap();
        assert_eq!(
            repo.load_active(library_id, ScanRunMode::Manual)
                .await
                .expect("load active succeeds")
                .expect("manual run exists")
                .scan_id,
            manual_scan_id
        );

        let maintenance = repo
            .get_or_create_active(
                NewLibraryScanRun::new(library_id, ScanRunMode::Maintenance)
                    .running(),
            )
            .await
            .expect("maintenance get_or_create succeeds");
        assert_eq!(maintenance.disposition, ScanStartDisposition::Created);
        assert_ne!(maintenance.run.scan_id, manual_scan_id);

        let active_runs =
            repo.list_active().await.expect("list active succeeds");
        assert_eq!(active_runs.len(), 2);
        assert_eq!(
            active_runs
                .iter()
                .filter(|run| run.mode == ScanRunMode::Manual)
                .count(),
            1
        );
        assert_eq!(
            active_runs
                .iter()
                .filter(|run| run.mode == ScanRunMode::Maintenance)
                .count(),
            1
        );
    }

    async fn seed_library_for_postgres_test(
        pool: &PgPool,
        library_id: LibraryId,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at)
            VALUES ($1, $2, $3, 'movies', NOW(), NOW())
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(library_id.to_uuid())
        .bind(format!("Scan run dedupe test {library_id}"))
        .bind(vec![format!("/scan-run-dedupe/{library_id}")])
        .execute(pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!("seed library failed: {err}"))
        })?;
        Ok(())
    }

    async fn active_run_count(
        pool: &PgPool,
        library_id: LibraryId,
        mode: ScanRunMode,
    ) -> Result<i64> {
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::bigint
            FROM library_scan_runs
            WHERE library_id = $1
              AND mode = $2
              AND status IN ('pending','running','paused')
            "#,
        )
        .bind(library_id.to_uuid())
        .bind(mode.as_str())
        .fetch_one(pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!("active run count failed: {err}"))
        })
    }

    #[tokio::test]
    async fn postgres_get_or_create_active_reuses_concurrent_manual_run() {
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

        let repo = Arc::new(PostgresScanRunRepository::new(pool.clone()));
        let library_id = LibraryId(Uuid::now_v7());
        seed_library_for_postgres_test(&pool, library_id)
            .await
            .expect("library seeded");

        let concurrent_requests = 8usize;
        let barrier = Arc::new(Barrier::new(concurrent_requests));
        let mut tasks = Vec::with_capacity(concurrent_requests);

        for _ in 0..concurrent_requests {
            let repo = Arc::clone(&repo);
            let barrier = Arc::clone(&barrier);
            tasks.push(tokio::spawn(async move {
                let requested_id = Uuid::now_v7();
                barrier.wait().await;
                repo.get_or_create_active(
                    NewLibraryScanRun::new(library_id, ScanRunMode::Manual)
                        .with_scan_id(requested_id)
                        .with_correlation_id(requested_id)
                        .running(),
                )
                .await
                .expect("postgres get_or_create succeeds")
            }));
        }

        let mut results = Vec::with_capacity(concurrent_requests);
        for task in tasks {
            results.push(task.await.expect("start task joins"));
        }

        let scan_ids: HashSet<Uuid> =
            results.iter().map(|result| result.run.scan_id).collect();
        assert_eq!(scan_ids.len(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| {
                    result.disposition == ScanStartDisposition::Created
                })
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| {
                    result.disposition == ScanStartDisposition::Reused
                })
                .count(),
            concurrent_requests - 1
        );
        assert_eq!(
            active_run_count(&pool, library_id, ScanRunMode::Manual)
                .await
                .expect("manual active count succeeds"),
            1
        );

        let manual_scan_id = *scan_ids.iter().next().unwrap();
        let maintenance = repo
            .get_or_create_active(
                NewLibraryScanRun::new(library_id, ScanRunMode::Maintenance)
                    .with_scan_id(Uuid::now_v7())
                    .running(),
            )
            .await
            .expect("maintenance run starts");
        assert_eq!(maintenance.disposition, ScanStartDisposition::Created);
        assert_ne!(maintenance.run.scan_id, manual_scan_id);
        assert_eq!(
            active_run_count(&pool, library_id, ScanRunMode::Maintenance)
                .await
                .expect("maintenance active count succeeds"),
            1
        );

        sqlx::query("DELETE FROM libraries WHERE id = $1")
            .bind(library_id.to_uuid())
            .execute(&pool)
            .await
            .expect("cleanup library succeeds");
    }

    #[tokio::test]
    async fn list_active_returns_only_non_terminal_runs() {
        let repo = InMemoryScanRunRepository::default();
        let active_library_id = LibraryId(Uuid::now_v7());
        let terminal_library_id = LibraryId(Uuid::now_v7());

        let active = repo
            .get_or_create_active(NewLibraryScanRun::new(
                active_library_id,
                ScanRunMode::Manual,
            ))
            .await
            .expect("active get_or_create succeeds");
        let terminal = repo
            .get_or_create_active(NewLibraryScanRun::new(
                terminal_library_id,
                ScanRunMode::Manual,
            ))
            .await
            .expect("terminal get_or_create succeeds");
        repo.mark_terminal(
            terminal.run.scan_id,
            ScanLifecycleStatus::Completed,
            Utc::now(),
            None,
        )
        .await
        .expect("terminal mark succeeds");

        let active_runs =
            repo.list_active().await.expect("list active succeeds");

        assert_eq!(active_runs.len(), 1);
        assert_eq!(active_runs[0].scan_id, active.run.scan_id);
    }

    #[tokio::test]
    async fn terminal_run_allows_later_run_for_same_library_mode() {
        let repo = InMemoryScanRunRepository::default();
        let library_id = LibraryId(Uuid::now_v7());

        let first = repo
            .get_or_create_active(NewLibraryScanRun::new(
                library_id,
                ScanRunMode::Manual,
            ))
            .await
            .expect("first get_or_create succeeds");
        repo.mark_terminal(
            first.run.scan_id,
            ScanLifecycleStatus::Completed,
            Utc::now(),
            None,
        )
        .await
        .expect("mark terminal succeeds")
        .expect("active run marked terminal");

        let second = repo
            .get_or_create_active(NewLibraryScanRun::new(
                library_id,
                ScanRunMode::Manual,
            ))
            .await
            .expect("second get_or_create succeeds");

        assert_eq!(second.disposition, ScanStartDisposition::Created);
        assert_ne!(first.run.scan_id, second.run.scan_id);
        assert_eq!(first.run.run_key, second.run.run_key);
    }
}
