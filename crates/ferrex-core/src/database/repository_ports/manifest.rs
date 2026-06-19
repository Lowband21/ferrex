use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::scan::manifest::{
    ManifestDiagnosticSeverity, ManifestPartitionId, ManifestRun,
    ManifestRunStatus,
};
use crate::error::Result;
use crate::types::ids::LibraryId;

/// Aggregate counts returned after upserting one manifest entry batch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ManifestBatchUpsertSummary {
    pub entries_upserted: u64,
    pub diagnostics_upserted: u64,
}

/// Final state for a manifest run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestRunCompletion {
    pub run_id: Uuid,
    pub status: ManifestRunStatus,
    pub completed_at: DateTime<Utc>,
    pub entries_seen: u64,
    pub diagnostics_seen: u64,
    pub error_message: Option<String>,
}

/// Filter for operator-visible manifest diagnostics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManifestDiagnosticFilter {
    pub library_id: Option<LibraryId>,
    pub run_id: Option<Uuid>,
    pub severity: Option<ManifestDiagnosticSeverity>,
    pub code: Option<String>,
    pub limit: Option<u32>,
}

/// Persisted diagnostic row returned by the manifest repository.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestDiagnosticRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub library_id: LibraryId,
    pub root_id: u16,
    pub partition_id: Option<ManifestPartitionId>,
    pub path_norm: String,
    pub reason: String,
    pub code: String,
    pub severity: ManifestDiagnosticSeverity,
    pub remediation: String,
    pub created_at: DateTime<Utc>,
}

/// Durable cursor for a manifest root/partition or imported legacy scan cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestPartitionCursorRecord {
    pub library_id: LibraryId,
    pub library_type: crate::types::library::LibraryType,
    pub root_id: u16,
    pub root_path_norm: String,
    pub partition_key: String,
    pub partition_id: Option<ManifestPartitionId>,
    pub prefix_norm: Option<String>,
    pub last_successful_run_id: Option<Uuid>,
    pub last_successful_at: Option<DateTime<Utc>>,
    pub last_observed_at: Option<DateTime<Utc>>,
    pub entries_seen: u64,
    pub diagnostics_seen: u64,
    pub supported_media_seen: u64,
    pub first_path_norm: Option<String>,
    pub last_path_norm: Option<String>,
    pub legacy_scan_path_hash: Option<i64>,
    pub backfilled_from_legacy: bool,
    pub updated_at: DateTime<Utc>,
}

/// Status for a deferred filesystem-watch hint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestDeferredWatchHintStatus {
    Pending,
    Applied,
    Dropped,
}

/// Input used to insert or refresh deferred watch hint state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestDeferredWatchHintInput {
    pub id: Option<Uuid>,
    pub library_id: LibraryId,
    pub root_id: u16,
    pub root_path_norm: String,
    pub path_norm: String,
    pub hint_kind: String,
    pub payload: Value,
    pub idempotency_key: String,
    pub available_at: DateTime<Utc>,
}

/// Filter for deferred watch hints.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManifestDeferredWatchHintFilter {
    pub library_id: Option<LibraryId>,
    pub status: Option<ManifestDeferredWatchHintStatus>,
    pub available_before: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

/// Persisted deferred watch hint state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestDeferredWatchHintRecord {
    pub id: Uuid,
    pub library_id: LibraryId,
    pub root_id: u16,
    pub root_path_norm: String,
    pub path_norm: String,
    pub hint_kind: String,
    pub payload: Value,
    pub status: ManifestDeferredWatchHintStatus,
    pub idempotency_key: String,
    pub attempts: u32,
    pub available_at: DateTime<Utc>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Counts returned by the safe legacy backfill path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ManifestBackfillSummary {
    pub media_entries: u64,
    pub folder_entries: u64,
    pub legacy_cursors: u64,
}

#[async_trait]
pub trait ManifestRepository: Send + Sync {
    async fn start_run(&self, run: ManifestRun) -> Result<ManifestRun>;

    async fn upsert_batch_entries(
        &self,
        run_id: Uuid,
        batch: &crate::domain::scan::manifest::ManifestEntryBatch,
    ) -> Result<ManifestBatchUpsertSummary>;

    async fn complete_run(
        &self,
        completion: ManifestRunCompletion,
    ) -> Result<ManifestRun>;

    async fn list_stale_partitions(
        &self,
        library_id: LibraryId,
        older_than: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<ManifestPartitionCursorRecord>>;

    async fn list_diagnostics(
        &self,
        filter: ManifestDiagnosticFilter,
    ) -> Result<Vec<ManifestDiagnosticRecord>>;

    async fn upsert_deferred_watch_hint(
        &self,
        hint: ManifestDeferredWatchHintInput,
    ) -> Result<ManifestDeferredWatchHintRecord>;

    async fn list_deferred_watch_hints(
        &self,
        filter: ManifestDeferredWatchHintFilter,
    ) -> Result<Vec<ManifestDeferredWatchHintRecord>>;

    async fn update_deferred_watch_hint_status(
        &self,
        id: Uuid,
        status: ManifestDeferredWatchHintStatus,
        last_error: Option<String>,
    ) -> Result<Option<ManifestDeferredWatchHintRecord>>;

    async fn backfill_legacy_manifest_state(
        &self,
        library_id: Option<LibraryId>,
    ) -> Result<ManifestBackfillSummary>;
}
