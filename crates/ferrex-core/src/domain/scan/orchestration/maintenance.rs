//! Cursor-based incremental maintenance sweep planning.
//!
//! The planner keeps maintenance bounded by combining durable scan cursors with
//! root-level discovery. Runtime code owns when plans are executed and when a
//! completed run updates library metadata.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use tokio::fs;

use crate::database::repository_ports::manifest::{
    ManifestPartitionCursorRecord, ManifestRepository,
};
use crate::domain::scan::actors::folder::is_media_file_path;
use crate::domain::scan::manifest::{
    ManifestPartitionScope, ManifestRootId, ManifestRootScope, ManifestScope,
};
use crate::domain::scan::orchestration::context::{
    FolderScanContext, MovieFolderScanContext, MovieRootPath, SeasonFolderPath,
    SeasonFolderScanContext, SeriesFolderScanContext, SeriesRootPath,
};
use crate::domain::scan::orchestration::job::{
    EnqueueRequest, FolderScanJob, JobPayload, JobPriority, ManifestScanJob,
    ManifestScanTrigger, ScanReason,
};
use crate::domain::scan::orchestration::scan_cursor::{
    ScanCursorRepository, normalize_path,
};
use crate::error::{MediaError, Result};
use crate::types::{ids::LibraryId, library::LibraryType};

/// Library fields needed by the maintenance planner.
#[derive(Clone, Debug)]
pub struct MaintenanceLibrary {
    pub id: LibraryId,
    pub name: String,
    pub library_type: LibraryType,
    pub paths: Vec<PathBuf>,
    pub scan_interval_minutes: u32,
    pub last_scan: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub auto_scan: bool,
    pub watch_for_changes: bool,
}

impl MaintenanceLibrary {
    pub fn from_library(library: &crate::types::library::Library) -> Self {
        Self {
            id: library.id,
            name: library.name.clone(),
            library_type: library.library_type,
            paths: library.paths.clone(),
            scan_interval_minutes: library.scan_interval_minutes,
            last_scan: library.last_scan,
            enabled: library.enabled,
            auto_scan: library.auto_scan,
            watch_for_changes: library.watch_for_changes,
        }
    }

    pub fn scan_interval(&self) -> Duration {
        Duration::minutes(self.scan_interval_minutes.max(1) as i64)
    }
}

/// Bounds applied to a single maintenance planning pass.
#[derive(Clone, Copy, Debug)]
pub struct MaintenancePlanningLimits {
    pub max_jobs_per_library: usize,
    pub max_root_entries_per_library: usize,
}

impl MaintenancePlanningLimits {
    pub fn new(
        max_jobs_per_library: usize,
        max_root_entries_per_library: usize,
    ) -> Self {
        Self {
            max_jobs_per_library: max_jobs_per_library.max(1),
            max_root_entries_per_library: max_root_entries_per_library.max(1),
        }
    }
}

/// Bounded work selected for one library at one scheduler tick.
#[derive(Clone, Debug)]
pub struct MaintenancePlan {
    pub library_id: LibraryId,
    pub due: bool,
    pub requests: Vec<EnqueueRequest>,
    pub stale_cursor_count: usize,
    pub new_root_folder_count: usize,
    pub skipped_root_entries: usize,
    pub errors: Vec<String>,
}

impl MaintenancePlan {
    fn empty(library_id: LibraryId, due: bool) -> Self {
        Self {
            library_id,
            due,
            requests: Vec::new(),
            stale_cursor_count: 0,
            new_root_folder_count: 0,
            skipped_root_entries: 0,
            errors: Vec::new(),
        }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Returns whether a library is eligible and due for interval maintenance.
///
/// `watch_for_changes` deliberately does not participate in this predicate:
/// disabling live watchers should not disable interval maintenance.
pub fn library_due_for_maintenance(
    library: &MaintenanceLibrary,
    now: DateTime<Utc>,
) -> bool {
    if !library.enabled || !library.auto_scan {
        return false;
    }

    match library.last_scan {
        None => true,
        Some(last_scan) => {
            now.signed_duration_since(last_scan) >= library.scan_interval()
        }
    }
}

/// Build bounded `MaintenanceSweep` enqueue requests for one due library.
pub async fn plan_maintenance_sweep<C>(
    library: &MaintenanceLibrary,
    cursors: &C,
    limits: MaintenancePlanningLimits,
    now: DateTime<Utc>,
) -> Result<MaintenancePlan>
where
    C: ScanCursorRepository + ?Sized,
{
    let due = library_due_for_maintenance(library, now);
    let mut plan = MaintenancePlan::empty(library.id, due);
    if !due {
        return Ok(plan);
    }

    let all_cursors = cursors.list_by_library(library.id).await?;
    let known_cursor_paths: HashSet<String> = all_cursors
        .iter()
        .map(|cursor| cursor.folder_path_norm.clone())
        .collect();

    let mut planned_paths = HashSet::new();

    let root_discovery = discover_new_root_folders(
        library,
        &known_cursor_paths,
        limits.max_root_entries_per_library,
    )
    .await;
    plan.skipped_root_entries += root_discovery.skipped_entries;
    plan.errors.extend(root_discovery.errors);

    for folder_path_norm in root_discovery.folders {
        if plan.requests.len() >= limits.max_jobs_per_library {
            break;
        }
        if !planned_paths.insert(folder_path_norm.clone()) {
            continue;
        }
        match maintenance_request(library, folder_path_norm, now) {
            Ok(request) => {
                plan.new_root_folder_count += 1;
                plan.requests.push(request);
            }
            Err(err) => plan.errors.push(err.to_string()),
        }
    }

    if plan.requests.len() < limits.max_jobs_per_library {
        let older_than = now - library.scan_interval();
        let mut stale = cursors.list_stale(library.id, older_than).await?;
        stale.sort_by_key(|cursor| cursor.last_scan_at);
        plan.stale_cursor_count = stale.len();

        for cursor in stale {
            if plan.requests.len() >= limits.max_jobs_per_library {
                break;
            }
            if !planned_paths.insert(cursor.folder_path_norm.clone()) {
                continue;
            }
            match maintenance_request(
                library,
                cursor.folder_path_norm.clone(),
                now,
            ) {
                Ok(request) => plan.requests.push(request),
                Err(err) => plan.errors.push(format!(
                    "skipping stale cursor {}: {err}",
                    cursor.folder_path_norm
                )),
            }
        }
    } else {
        plan.stale_cursor_count = cursors
            .list_stale(library.id, now - library.scan_interval())
            .await?
            .len();
    }

    Ok(plan)
}

/// Build bounded manifest-backed maintenance work for one due library.
///
/// This reuses the legacy cursor/folder planner for due-ness and counters, then
/// schedules root manifest scans (so flat-root media and zero-folder roots are
/// observed) plus stale manifest partitions from previous runs or legacy scan
/// cursors.
pub async fn plan_manifest_maintenance_sweep<C, M>(
    library: &MaintenanceLibrary,
    cursors: &C,
    manifest: &M,
    limits: MaintenancePlanningLimits,
    now: DateTime<Utc>,
) -> Result<MaintenancePlan>
where
    C: ScanCursorRepository + ?Sized,
    M: ManifestRepository + ?Sized,
{
    let legacy_plan =
        plan_maintenance_sweep(library, cursors, limits, now).await?;
    let mut plan = MaintenancePlan::empty(library.id, legacy_plan.due);
    plan.stale_cursor_count = legacy_plan.stale_cursor_count;
    plan.new_root_folder_count = legacy_plan.new_root_folder_count;
    plan.skipped_root_entries = legacy_plan.skipped_root_entries;
    plan.errors = legacy_plan.errors;
    if !plan.due {
        return Ok(plan);
    }

    let mut planned_manifest_scopes = HashSet::new();

    for (idx, root) in library.paths.iter().enumerate() {
        if plan.requests.len() >= limits.max_jobs_per_library {
            break;
        }
        let root_id = match u16::try_from(idx) {
            Ok(root_id) => root_id,
            Err(_) => {
                plan.errors.push(format!(
                    "library {} has more than {} manifest roots",
                    library.id,
                    u16::MAX
                ));
                continue;
            }
        };
        let root_norm = normalize_path(root)
            .unwrap_or_else(|_| root.to_string_lossy().to_string());
        let scope = ManifestScope::Root(ManifestRootScope {
            library_id: library.id,
            library_type: library.library_type,
            root_id: ManifestRootId(root_id),
            root_path_norm: root_norm,
        });
        let key = manifest_maintenance_scope_key(&scope);
        if planned_manifest_scopes.insert(key) {
            plan.requests.push(manifest_request(
                scope,
                ScanReason::MaintenanceSweep,
                ManifestScanTrigger::Maintenance,
                now,
            ));
        }
    }

    if plan.requests.len() < limits.max_jobs_per_library {
        let older_than = now - library.scan_interval();
        let remaining = limits
            .max_jobs_per_library
            .saturating_sub(plan.requests.len())
            .min(u32::MAX as usize) as u32;
        let stale_partitions = manifest
            .list_stale_partitions(library.id, older_than, remaining)
            .await?;
        plan.stale_cursor_count = plan
            .stale_cursor_count
            .saturating_add(stale_partitions.len());

        for cursor in stale_partitions {
            if plan.requests.len() >= limits.max_jobs_per_library {
                break;
            }
            let scope = manifest_scope_from_cursor(cursor);
            let key = manifest_maintenance_scope_key(&scope);
            if planned_manifest_scopes.insert(key) {
                plan.requests.push(manifest_request(
                    scope,
                    ScanReason::MaintenanceSweep,
                    ManifestScanTrigger::Maintenance,
                    now,
                ));
            }
        }
    }

    Ok(plan)
}

fn maintenance_request(
    library: &MaintenanceLibrary,
    folder_path_norm: String,
    now: DateTime<Utc>,
) -> Result<EnqueueRequest> {
    let context = build_maintenance_context(library, &folder_path_norm)?;
    let job = FolderScanJob {
        context,
        scan_reason: ScanReason::MaintenanceSweep,
        enqueue_time: now,
        device_id: None,
    };
    Ok(EnqueueRequest::new(
        JobPriority::P2,
        JobPayload::FolderScan(job),
    ))
}

fn manifest_request(
    scope: ManifestScope,
    scan_reason: ScanReason,
    trigger: ManifestScanTrigger,
    now: DateTime<Utc>,
) -> EnqueueRequest {
    EnqueueRequest::new(
        JobPriority::P2,
        JobPayload::ManifestScan(ManifestScanJob {
            scope,
            scan_reason,
            enqueue_time: now,
            trigger,
        }),
    )
}

fn manifest_scope_from_cursor(
    cursor: ManifestPartitionCursorRecord,
) -> ManifestScope {
    let root = ManifestRootScope {
        library_id: cursor.library_id,
        library_type: cursor.library_type,
        root_id: ManifestRootId(cursor.root_id),
        root_path_norm: cursor.root_path_norm,
    };

    match cursor.partition_id {
        Some(partition_id) => {
            ManifestScope::Partition(ManifestPartitionScope {
                root,
                partition_id,
                prefix_norm: cursor.prefix_norm,
            })
        }
        None => ManifestScope::Root(root),
    }
}

fn manifest_maintenance_scope_key(scope: &ManifestScope) -> String {
    match scope {
        ManifestScope::Root(root) => {
            format!("root:{}:{}", root.root_id.0, root.root_path_norm)
        }
        ManifestScope::Partition(partition) => format!(
            "partition:{}:{}:{}",
            partition.root.root_id.0,
            partition.partition_id.0,
            partition
                .prefix_norm
                .as_deref()
                .unwrap_or(partition.root.root_path_norm.as_str())
        ),
    }
}

/// Reconstruct the scan context for a persisted cursor or root child.
pub fn build_maintenance_context(
    library: &MaintenanceLibrary,
    folder_path_norm: &str,
) -> Result<FolderScanContext> {
    let folder_path = Path::new(folder_path_norm);
    let (root_norm, root_path) = matching_root(library, folder_path)?;
    let relative = folder_path.strip_prefix(&root_path).map_err(|_| {
        MediaError::InvalidMedia(format!(
            "folder {folder_path_norm} is not under library root {root_norm}"
        ))
    })?;

    let components: Vec<_> = relative.components().collect();
    if components.is_empty() {
        return Err(MediaError::InvalidMedia(format!(
            "maintenance folder must be below a library root: {folder_path_norm}"
        )));
    }

    match library.library_type {
        LibraryType::Movies => {
            if components.len() != 1 {
                return Err(MediaError::InvalidMedia(format!(
                    "movie maintenance scans are limited to top-level movie folders: {folder_path_norm}"
                )));
            }
            let movie_root_path = MovieRootPath::try_new_under_library_root(
                &root_norm,
                folder_path_norm.to_owned(),
            )?;
            Ok(FolderScanContext::Movie(MovieFolderScanContext {
                library_id: library.id,
                movie_root_path,
            }))
        }
        LibraryType::Series => match components.len() {
            1 => {
                let series_root_path =
                    SeriesRootPath::try_new_under_library_root(
                        &root_norm,
                        folder_path_norm.to_owned(),
                    )?;
                Ok(FolderScanContext::Series(SeriesFolderScanContext {
                    library_id: library.id,
                    series_root_path,
                }))
            }
            2 => {
                let series_root_path_norm = root_path
                    .join(components[0].as_os_str())
                    .to_string_lossy()
                    .to_string();
                let series_root_path =
                    SeriesRootPath::try_new_under_library_root(
                        &root_norm,
                        series_root_path_norm,
                    )?;
                let (season_folder_path, season_number) =
                    SeasonFolderPath::try_new_under_series_root(
                        &series_root_path,
                        folder_path_norm.to_owned(),
                    )?;
                Ok(FolderScanContext::Season(SeasonFolderScanContext {
                    library_id: library.id,
                    series_root_path,
                    season_folder_path,
                    season_number,
                }))
            }
            _ => Err(MediaError::InvalidMedia(format!(
                "series maintenance scans are limited to series roots and season folders: {folder_path_norm}"
            ))),
        },
    }
}

fn matching_root(
    library: &MaintenanceLibrary,
    folder_path: &Path,
) -> Result<(String, PathBuf)> {
    let mut best: Option<(String, PathBuf)> = None;

    for root in &library.paths {
        let root_norm = normalize_path(root)
            .unwrap_or_else(|_| root.to_string_lossy().to_string());
        let root_path = PathBuf::from(&root_norm);
        if folder_path.starts_with(&root_path) {
            let replace = best
                .as_ref()
                .map(|(_, current)| {
                    root_path.components().count()
                        > current.components().count()
                })
                .unwrap_or(true);
            if replace {
                best = Some((root_norm, root_path));
            }
        }
    }

    best.ok_or_else(|| {
        MediaError::InvalidMedia(format!(
            "folder {} is not under any configured library root",
            folder_path.display()
        ))
    })
}

#[derive(Debug, Default)]
struct RootDiscovery {
    folders: Vec<String>,
    skipped_entries: usize,
    errors: Vec<String>,
}

async fn discover_new_root_folders(
    library: &MaintenanceLibrary,
    known_cursor_paths: &HashSet<String>,
    max_root_entries: usize,
) -> RootDiscovery {
    let mut discovery = RootDiscovery::default();
    let mut visited_entries = 0usize;

    'roots: for root in &library.paths {
        let root_norm = match normalize_path(root) {
            Ok(path) => path,
            Err(err) => {
                discovery.errors.push(format!(
                    "failed to normalize root {}: {err}",
                    root.display()
                ));
                continue;
            }
        };

        let mut rd = match fs::read_dir(&root_norm).await {
            Ok(rd) => rd,
            Err(err) => {
                discovery.errors.push(format!(
                    "failed to enumerate root {root_norm}: {err}"
                ));
                continue;
            }
        };

        loop {
            if visited_entries >= max_root_entries {
                break 'roots;
            }

            let entry = match rd.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(err) => {
                    discovery.skipped_entries += 1;
                    discovery.errors.push(format!(
                        "failed to read entry under {root_norm}: {err}"
                    ));
                    continue;
                }
            };
            visited_entries += 1;

            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }

            let file_type = match entry.file_type().await {
                Ok(file_type) => file_type,
                Err(err) => {
                    discovery.skipped_entries += 1;
                    discovery.errors.push(format!(
                        "failed to read file type for {}: {err}",
                        entry.path().display()
                    ));
                    continue;
                }
            };

            if !file_type.is_dir() {
                // The scanner's supported unit is a top-level folder. Media files
                // directly under a library root are intentionally ignored here.
                if is_media_file_path(&entry.path()) {
                    discovery.skipped_entries += 1;
                }
                continue;
            }

            let path_norm = match normalize_path(&entry.path()) {
                Ok(path) => path,
                Err(err) => {
                    discovery.skipped_entries += 1;
                    discovery.errors.push(format!(
                        "failed to normalize root child {}: {err}",
                        entry.path().display()
                    ));
                    continue;
                }
            };

            if !known_cursor_paths.contains(&path_norm) {
                discovery.folders.push(path_norm);
            }
        }
    }

    discovery
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repository_ports::manifest::{
        ManifestBackfillSummary, ManifestBatchUpsertSummary,
        ManifestDeferredWatchHintFilter, ManifestDeferredWatchHintInput,
        ManifestDeferredWatchHintRecord, ManifestDeferredWatchHintStatus,
        ManifestDiagnosticFilter, ManifestDiagnosticRecord,
        ManifestMissingEntryRecord, ManifestRunCompletion,
    };
    use crate::domain::scan::manifest::{ManifestPartitionId, ManifestRun};
    use crate::domain::scan::orchestration::scan_cursor::ScanCursor;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[derive(Clone, Default)]
    struct FakeCursorRepository {
        cursors: Arc<Mutex<Vec<ScanCursor>>>,
    }

    impl FakeCursorRepository {
        async fn insert(&self, cursor: ScanCursor) {
            self.cursors.lock().await.push(cursor);
        }
    }

    #[derive(Clone, Default)]
    struct FakeManifestRepository {
        stale_partitions: Arc<Mutex<Vec<ManifestPartitionCursorRecord>>>,
    }

    impl FakeManifestRepository {
        async fn insert_stale_partition(
            &self,
            cursor: ManifestPartitionCursorRecord,
        ) {
            self.stale_partitions.lock().await.push(cursor);
        }
    }

    #[async_trait]
    impl ManifestRepository for FakeManifestRepository {
        async fn start_run(&self, run: ManifestRun) -> Result<ManifestRun> {
            Ok(run)
        }

        async fn upsert_batch_entries(
            &self,
            _run_id: Uuid,
            _batch: &crate::domain::scan::manifest::ManifestEntryBatch,
        ) -> Result<ManifestBatchUpsertSummary> {
            Ok(ManifestBatchUpsertSummary::default())
        }

        async fn complete_run(
            &self,
            completion: ManifestRunCompletion,
        ) -> Result<ManifestRun> {
            Ok(ManifestRun {
                run_id: completion.run_id,
                scope: ManifestScope::Root(ManifestRootScope {
                    library_id: LibraryId::new(),
                    library_type: LibraryType::Movies,
                    root_id: ManifestRootId(0),
                    root_path_norm: String::new(),
                }),
                status: completion.status,
                started_at: completion.completed_at,
                completed_at: Some(completion.completed_at),
                entries_seen: completion.entries_seen,
                diagnostics_seen: completion.diagnostics_seen,
            })
        }

        async fn mark_missing_entries_after_successful_run(
            &self,
            _run_id: Uuid,
        ) -> Result<Vec<ManifestMissingEntryRecord>> {
            Ok(Vec::new())
        }

        async fn list_stale_partitions(
            &self,
            library_id: LibraryId,
            older_than: DateTime<Utc>,
            limit: u32,
        ) -> Result<Vec<ManifestPartitionCursorRecord>> {
            let mut rows = self
                .stale_partitions
                .lock()
                .await
                .iter()
                .filter(|cursor| {
                    cursor.library_id == library_id
                        && cursor
                            .last_successful_at
                            .map(|at| at < older_than)
                            .unwrap_or(true)
                })
                .cloned()
                .collect::<Vec<_>>();
            rows.truncate(limit as usize);
            Ok(rows)
        }

        async fn list_diagnostics(
            &self,
            _filter: ManifestDiagnosticFilter,
        ) -> Result<Vec<ManifestDiagnosticRecord>> {
            Ok(Vec::new())
        }

        async fn upsert_deferred_watch_hint(
            &self,
            _hint: ManifestDeferredWatchHintInput,
        ) -> Result<ManifestDeferredWatchHintRecord> {
            Err(MediaError::Internal(
                "fake manifest deferred hints are not used".into(),
            ))
        }

        async fn list_deferred_watch_hints(
            &self,
            _filter: ManifestDeferredWatchHintFilter,
        ) -> Result<Vec<ManifestDeferredWatchHintRecord>> {
            Ok(Vec::new())
        }

        async fn update_deferred_watch_hint_status(
            &self,
            _id: Uuid,
            _status: ManifestDeferredWatchHintStatus,
            _last_error: Option<String>,
        ) -> Result<Option<ManifestDeferredWatchHintRecord>> {
            Ok(None)
        }

        async fn backfill_legacy_manifest_state(
            &self,
            _library_id: Option<LibraryId>,
        ) -> Result<ManifestBackfillSummary> {
            Ok(ManifestBackfillSummary::default())
        }
    }

    #[async_trait]
    impl ScanCursorRepository for FakeCursorRepository {
        async fn get(
            &self,
            id: &crate::domain::scan::orchestration::scan_cursor::ScanCursorId,
        ) -> Result<Option<ScanCursor>> {
            Ok(self
                .cursors
                .lock()
                .await
                .iter()
                .find(|cursor| cursor.id == *id)
                .cloned())
        }

        async fn list_by_library(
            &self,
            library_id: LibraryId,
        ) -> Result<Vec<ScanCursor>> {
            Ok(self
                .cursors
                .lock()
                .await
                .iter()
                .filter(|cursor| cursor.id.library_id == library_id)
                .cloned()
                .collect())
        }

        async fn upsert(&self, cursor: ScanCursor) -> Result<()> {
            self.cursors.lock().await.push(cursor);
            Ok(())
        }

        async fn delete_by_library(
            &self,
            library_id: LibraryId,
        ) -> Result<usize> {
            let mut guard = self.cursors.lock().await;
            let before = guard.len();
            guard.retain(|cursor| cursor.id.library_id != library_id);
            Ok(before - guard.len())
        }

        async fn list_stale(
            &self,
            library_id: LibraryId,
            older_than: DateTime<Utc>,
        ) -> Result<Vec<ScanCursor>> {
            Ok(self
                .cursors
                .lock()
                .await
                .iter()
                .filter(|cursor| {
                    cursor.id.library_id == library_id
                        && cursor.last_scan_at < older_than
                })
                .cloned()
                .collect())
        }
    }

    fn test_library(
        root: PathBuf,
        last_scan: Option<DateTime<Utc>>,
        watch_for_changes: bool,
    ) -> MaintenanceLibrary {
        MaintenanceLibrary {
            id: LibraryId(Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)),
            name: "Test".into(),
            library_type: LibraryType::Movies,
            paths: vec![root],
            scan_interval_minutes: 60,
            last_scan,
            enabled: true,
            auto_scan: true,
            watch_for_changes,
        }
    }

    fn stale_manifest_cursor(
        library: &MaintenanceLibrary,
        root_path_norm: String,
        prefix_norm: String,
        last_successful_at: DateTime<Utc>,
    ) -> ManifestPartitionCursorRecord {
        ManifestPartitionCursorRecord {
            library_id: library.id,
            library_type: library.library_type,
            root_id: 0,
            root_path_norm,
            partition_key: "partition-7".into(),
            partition_id: Some(ManifestPartitionId(7)),
            prefix_norm: Some(prefix_norm),
            last_successful_run_id: Some(Uuid::now_v7()),
            last_successful_at: Some(last_successful_at),
            last_observed_at: Some(last_successful_at),
            entries_seen: 0,
            diagnostics_seen: 0,
            supported_media_seen: 0,
            first_path_norm: None,
            last_path_norm: None,
            legacy_scan_path_hash: None,
            backfilled_from_legacy: false,
            updated_at: last_successful_at,
        }
    }

    #[test]
    fn due_maintenance_does_not_require_live_watchers() {
        let now = Utc::now();
        let root = PathBuf::from("/library");
        let library =
            test_library(root, Some(now - Duration::minutes(61)), false);

        assert!(library_due_for_maintenance(&library, now));

        let mut disabled_auto = library.clone();
        disabled_auto.auto_scan = false;
        assert!(!library_due_for_maintenance(&disabled_auto, now));
    }

    #[tokio::test]
    async fn plan_discovers_new_top_level_folders_without_watch_events() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        std::fs::create_dir_all(root.join("New Movie")).expect("movie dir");
        let library = test_library(root, None, false);
        let repo = FakeCursorRepository::default();

        let plan = plan_maintenance_sweep(
            &library,
            &repo,
            MaintenancePlanningLimits::new(16, 16),
            Utc::now(),
        )
        .await
        .expect("plan");

        assert!(plan.due);
        assert_eq!(plan.new_root_folder_count, 1);
        assert_eq!(plan.requests.len(), 1);
        let request = &plan.requests[0];
        assert_eq!(request.priority, JobPriority::P2);
        let JobPayload::FolderScan(job) = &request.payload else {
            panic!("expected folder scan")
        };
        assert_eq!(job.scan_reason, ScanReason::MaintenanceSweep);
        assert!(job.context.folder_path_norm().ends_with("New Movie"));
    }

    #[tokio::test]
    async fn plan_enqueues_only_stale_existing_cursors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let stale_dir = root.join("Stale Movie");
        let fresh_dir = root.join("Fresh Movie");
        std::fs::create_dir_all(&stale_dir).expect("stale dir");
        std::fs::create_dir_all(&fresh_dir).expect("fresh dir");
        let now = Utc::now();
        let library =
            test_library(root.clone(), Some(now - Duration::minutes(61)), true);
        let repo = FakeCursorRepository::default();

        for (path, last_scan_at) in [
            (stale_dir, now - Duration::minutes(90)),
            (fresh_dir, now - Duration::minutes(5)),
        ] {
            let path_norm = normalize_path(&path).expect("normalize");
            repo.insert(ScanCursor {
                id: crate::domain::scan::orchestration::scan_cursor::ScanCursorId::new(
                    library.id,
                    &vec![PathBuf::from(&path_norm)],
                ),
                folder_path_norm: path_norm,
                listing_hash: "hash".into(),
                entry_count: 0,
                last_scan_at,
                last_modified_at: None,
                device_id: None,
            })
            .await;
        }

        let plan = plan_maintenance_sweep(
            &library,
            &repo,
            MaintenancePlanningLimits::new(16, 16),
            now,
        )
        .await
        .expect("plan");

        assert_eq!(plan.stale_cursor_count, 1);
        assert_eq!(plan.requests.len(), 1);
        let JobPayload::FolderScan(job) = &plan.requests[0].payload else {
            panic!("expected folder scan")
        };
        assert!(job.context.folder_path_norm().ends_with("Stale Movie"));
    }

    #[tokio::test]
    async fn manifest_plan_enqueues_root_scan_for_flat_root_media() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        std::fs::write(root.join("flat-root.mkv"), b"movie")
            .expect("flat media");
        let now = Utc::now();
        let library = test_library(root.clone(), None, true);
        let cursors = FakeCursorRepository::default();
        let manifest = FakeManifestRepository::default();

        let plan = plan_manifest_maintenance_sweep(
            &library,
            &cursors,
            &manifest,
            MaintenancePlanningLimits::new(4, 16),
            now,
        )
        .await
        .expect("manifest plan");

        assert!(plan.due);
        assert_eq!(plan.requests.len(), 1);
        assert!(plan.requests.iter().all(|request| {
            matches!(request.payload, JobPayload::ManifestScan(_))
        }));
        let JobPayload::ManifestScan(job) = &plan.requests[0].payload else {
            panic!("expected manifest scan")
        };
        assert_eq!(job.scan_reason, ScanReason::MaintenanceSweep);
        assert_eq!(job.trigger, ManifestScanTrigger::Maintenance);
        let ManifestScope::Root(scope) = &job.scope else {
            panic!("flat-root maintenance should scan the manifest root")
        };
        assert_eq!(scope.root_path_norm, normalize_path(&root).unwrap());
    }

    #[tokio::test]
    async fn manifest_plan_prioritizes_root_scan_when_legacy_folders_fill_limit()
     {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        std::fs::create_dir_all(root.join("Top Level Movie"))
            .expect("movie dir");
        std::fs::write(root.join("flat-root.mkv"), b"movie")
            .expect("flat media");
        let now = Utc::now();
        let library = test_library(root.clone(), None, true);
        let cursors = FakeCursorRepository::default();
        let manifest = FakeManifestRepository::default();

        let plan = plan_manifest_maintenance_sweep(
            &library,
            &cursors,
            &manifest,
            MaintenancePlanningLimits::new(1, 16),
            now,
        )
        .await
        .expect("manifest plan");

        assert_eq!(plan.requests.len(), 1);
        assert!(matches!(
            plan.requests[0].payload,
            JobPayload::ManifestScan(_)
        ));
        let JobPayload::ManifestScan(job) = &plan.requests[0].payload else {
            panic!("expected manifest scan")
        };
        assert!(matches!(job.scope, ManifestScope::Root(_)));
    }

    #[tokio::test]
    async fn manifest_plan_includes_stale_manifest_partitions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let stale_partition = root.join("Stale Movie");
        std::fs::create_dir_all(&stale_partition).expect("stale dir");
        let now = Utc::now();
        let library =
            test_library(root.clone(), Some(now - Duration::minutes(61)), true);
        let root_norm = normalize_path(&root).expect("root norm");
        let stale_norm = normalize_path(&stale_partition).expect("stale norm");
        let cursors = FakeCursorRepository::default();
        let manifest = FakeManifestRepository::default();
        manifest
            .insert_stale_partition(stale_manifest_cursor(
                &library,
                root_norm.clone(),
                stale_norm.clone(),
                now - Duration::minutes(120),
            ))
            .await;

        let plan = plan_manifest_maintenance_sweep(
            &library,
            &cursors,
            &manifest,
            MaintenancePlanningLimits::new(4, 16),
            now,
        )
        .await
        .expect("manifest plan");

        assert!(plan.requests.iter().all(|request| {
            matches!(request.payload, JobPayload::ManifestScan(_))
        }));
        assert!(plan.requests.iter().any(|request| {
            matches!(
                &request.payload,
                JobPayload::ManifestScan(ManifestScanJob {
                    scope: ManifestScope::Root(scope),
                    trigger: ManifestScanTrigger::Maintenance,
                    ..
                }) if scope.root_path_norm == root_norm
            )
        }));
        assert!(plan.requests.iter().any(|request| {
            matches!(
                &request.payload,
                JobPayload::ManifestScan(ManifestScanJob {
                    scope: ManifestScope::Partition(scope),
                    trigger: ManifestScanTrigger::Maintenance,
                    ..
                }) if scope.prefix_norm.as_deref() == Some(stale_norm.as_str())
            )
        }));
    }
}
