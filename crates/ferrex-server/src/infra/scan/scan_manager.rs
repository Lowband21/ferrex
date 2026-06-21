use ferrex_model::{LibraryType, MediaID, SubjectKey};

use ferrex_core::{
    api::types::{
        ScanLifecycleStatus as ApiScanLifecycleStatus, ScanRunMode,
        ScanSnapshotDto, ScanStartDisposition, SeriesBundleResponse,
    },
    application::unit_of_work::AppUnitOfWork,
    database::repository_ports::scan_observability::{
        NewScanRunEvent, ScanRunEventPageRequest, ScanRunEventRecord,
        ScanRunEventSequenceBounds, ScanRunFailurePage,
        ScanRunFailurePageRequest, ScanRunFailureSummary, ScanRunPage,
        ScanRunPageRequest, ScanRunRecord, ScanRunSource, ScanRunStatus,
        ScanRunUpdate,
    },
    domain::scan::{
        actors::{
            FileSystemEvent, FileSystemEventKind, FolderScanOutcome,
            LibraryRootsId,
            index::{IndexingChange, IndexingOutcome},
        },
        orchestration::{
            JobEvent, LibraryActorCommand, LibraryScanRun,
            LibraryScanRunProgressUpdate, NewLibraryScanRun, StartMode,
            context::{
                FolderScanContext, MovieFolderScanContext, MovieRootPath,
                SeasonFolderPath, SeasonFolderScanContext,
                SeriesFolderScanContext, SeriesRootPath,
            },
            events::{JobEventPayload, ScanEvent, ScanSeedSummary},
            job::{
                EnqueueRequest, FolderScanJob, JobHandle, JobId, JobKind,
                JobPayload, JobPriority, ScanReason,
            },
            scan_cursor::{ScanCursor, ScanCursorRepository, normalize_path},
        },
    },
    error::MediaError,
    player_prelude::MediaIDLike,
    types::{
        LibraryId, Media, ScanPathReasonCategory, ScanPathReasonDetail,
        ScanProgressEvent, ScanStageLatencySummary, events::ScanSseEventType,
    },
};

use crate::infra::{
    orchestration::ScanOrchestrator,
    scan::catalog_event_bus::{
        CatalogEvent, CatalogEventBus, CatalogEventFrame,
    },
    scan::movie_batch_notifier::MovieBatchFinalizationNotifiers,
    scan::series_bundle_tracker::{
        SeriesBundleFinalization, SeriesBundleTracker,
    },
};

use axum::http::StatusCode;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use sha2::Digest;

use std::{
    collections::{HashMap, HashSet, VecDeque, hash_map::Entry},
    fmt,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use tokio::{
    spawn,
    sync::{Mutex, RwLock, broadcast},
    time::interval,
};

use tracing::{error, info, instrument, warn};
use uuid::Uuid;

const EVENT_VERSION: &str = "2";
const HISTORY_CAPACITY: usize = 256;
const EVENT_HISTORY_CAPACITY: usize = 512;
const CATALOG_EVENT_HISTORY_CAPACITY: usize = 512;
const CATALOG_EVENT_BROADCAST_CAPACITY: usize = 512;
const DEFAULT_LATENCIES: ScanStageLatencySummary = ScanStageLatencySummary {
    scan: 12,
    analyze: 210,
    index: 44,
};
const DEFAULT_QUIESCENCE: Duration = Duration::from_secs(3);
const STALLED_SCAN_TIMEOUT_MULTIPLIER: u32 = 5;
const SERIES_BUNDLE_TRACKER_IDLE_TTL: Duration = Duration::from_secs(10 * 60);
const SERIES_BUNDLE_POLL_INTERVAL: Duration = Duration::from_secs(1);

fn subject_key_path(key: &SubjectKey) -> Option<&str> {
    match key {
        SubjectKey::Path(path) => Some(path.as_str()),
        SubjectKey::Opaque(_) => None,
    }
}

fn subject_key_path_owned(key: &SubjectKey) -> Option<String> {
    subject_key_path(key).map(str::to_string)
}

fn skipped_reason_code(outcome: Option<FolderScanOutcome>) -> &'static str {
    match outcome {
        Some(FolderScanOutcome::Missing) => "path_missing",
        Some(FolderScanOutcome::Empty) => "no_supported_media_found",
        Some(FolderScanOutcome::Unsupported) => "unsupported_media_layout",
        Some(FolderScanOutcome::UnchangedCursor) => "unchanged_since_last_scan",
        Some(FolderScanOutcome::Changed) | None => "skipped",
    }
}

fn user_safe_reason_code(raw: &str) -> String {
    let normalized: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = normalized.trim_matches('_');
    let mut collapsed = String::new();
    let mut previous_underscore = false;
    for ch in trimmed.chars() {
        if ch == '_' {
            if !previous_underscore {
                collapsed.push(ch);
            }
            previous_underscore = true;
        } else {
            collapsed.push(ch);
            previous_underscore = false;
        }
    }

    if collapsed.is_empty()
        || collapsed.contains("dead_letter")
        || collapsed.contains("deadletter")
    {
        "needs_attention".to_string()
    } else {
        collapsed
    }
}

fn reason_message(reason_code: &str) -> &'static str {
    match reason_code {
        "unchanged_since_last_scan" => {
            "Already up to date from a previous scan"
        }
        "path_missing" => "The path was not available during the scan",
        "no_supported_media_found" => {
            "No supported media files were found at this path"
        }
        "unsupported_media_layout" => {
            "This path does not contain a supported media layout"
        }
        "temporary_scan_issue" => "A temporary scan issue is being retried",
        "scan_cancelled" | "scan_canceled" => "The scan was canceled",
        _ => "Review this path and rescan when it is ready",
    }
}

fn subject_key_to_string(key: &SubjectKey) -> String {
    match key {
        SubjectKey::Path(path) => path.to_string(),
        SubjectKey::Opaque(key) => key.to_string(),
    }
}

fn lifecycle_to_observability_status(
    status: &ScanLifecycleStatus,
) -> ScanRunStatus {
    match status {
        ScanLifecycleStatus::Pending => ScanRunStatus::Pending,
        ScanLifecycleStatus::Running => ScanRunStatus::Running,
        ScanLifecycleStatus::Paused => ScanRunStatus::Paused,
        ScanLifecycleStatus::Completed => ScanRunStatus::Completed,
        ScanLifecycleStatus::Failed => ScanRunStatus::Failed,
        ScanLifecycleStatus::Canceled => ScanRunStatus::Canceled,
    }
}

fn start_mode_to_observability_source(mode: StartMode) -> ScanRunSource {
    match mode {
        StartMode::Bulk => ScanRunSource::Manual,
        StartMode::Maintenance => ScanRunSource::Maintenance,
        StartMode::Resume => ScanRunSource::Retry,
    }
}

fn observability_failure_category(
    reason: Option<&str>,
) -> (&'static str, &'static str) {
    let normalized = reason.unwrap_or("scan_failed").to_ascii_lowercase();
    if normalized.contains("permission denied")
        || normalized.contains("access denied")
    {
        ("filesystem_permission", "scan.folder_permission_denied")
    } else if normalized.contains("not found")
        || normalized.contains("no such file")
        || normalized.contains("missing")
    {
        ("filesystem_missing", "scan.folder_missing")
    } else if normalized.contains("timeout") || normalized.contains("timed out")
    {
        ("timeout", "scan.folder_timeout")
    } else if normalized.contains("no_root_match") {
        ("content_not_indexed", "scan.no_indexable_media")
    } else if normalized.contains("cancel") {
        ("scan_cancelled", "scan.cancelled")
    } else {
        ("job_failure", "scan.job_failed")
    }
}

/// Command dispatcher + read model for scan orchestration state.
#[derive(Clone)]
pub struct ScanControlPlane {
    inner: Arc<ScanControlPlaneInner>,
}

impl fmt::Debug for ScanControlPlane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let active = self
            .inner
            .active_by_scan_id
            .try_read()
            .ok()
            .map(|guard| guard.len());
        let history =
            self.inner.history.try_read().ok().map(|guard| guard.len());
        let receiver_count = self.inner.catalog_bus.receiver_count();
        let uow_ptr = Arc::as_ptr(&self.inner.unit_of_work);
        let orchestrator_ptr = Arc::as_ptr(&self.inner.orchestrator);

        f.debug_struct("ScanControlPlane")
            .field("active_scans", &active)
            .field("history_len", &history)
            .field("subscriber_count", &receiver_count)
            .field("unit_of_work_ptr", &uow_ptr)
            .field("orchestrator_ptr", &orchestrator_ptr)
            .finish()
    }
}

struct ScanControlPlaneInner {
    unit_of_work: Arc<AppUnitOfWork>,
    orchestrator: Arc<ScanOrchestrator>,
    active_by_scan_id: RwLock<HashMap<Uuid, Arc<ScanRun>>>,
    active_by_run_key: RwLock<HashMap<String, Arc<ScanRun>>>,
    history: RwLock<VecDeque<ScanHistoryEntry>>,
    final_events: RwLock<HashMap<Uuid, VecDeque<ScanBroadcastFrame>>>,
    catalog_bus: Arc<CatalogEventBus>,
    aggregator: ScanRunAggregator,
    movie_batch_notifiers: MovieBatchFinalizationNotifiers,
}

impl ScanControlPlane {
    pub fn new(
        unit_of_work: Arc<AppUnitOfWork>,
        orchestrator: Arc<ScanOrchestrator>,
    ) -> Self {
        Self::with_quiescence_window(
            unit_of_work,
            orchestrator,
            DEFAULT_QUIESCENCE,
        )
    }

    pub fn with_quiescence_window(
        unit_of_work: Arc<AppUnitOfWork>,
        orchestrator: Arc<ScanOrchestrator>,
        quiescence: Duration,
    ) -> Self {
        let catalog_bus = Arc::new(CatalogEventBus::new(
            CATALOG_EVENT_HISTORY_CAPACITY,
            CATALOG_EVENT_BROADCAST_CAPACITY,
        ));
        let aggregator = ScanRunAggregator::new(
            Arc::clone(&orchestrator),
            quiescence,
            Arc::clone(&catalog_bus),
            unit_of_work.clone(),
        );

        Self {
            inner: Arc::new(ScanControlPlaneInner {
                unit_of_work,
                orchestrator,
                active_by_scan_id: RwLock::new(HashMap::new()),
                active_by_run_key: RwLock::new(HashMap::new()),
                history: RwLock::new(VecDeque::with_capacity(HISTORY_CAPACITY)),
                final_events: RwLock::new(HashMap::new()),
                catalog_bus,
                aggregator,
                movie_batch_notifiers: MovieBatchFinalizationNotifiers::new(),
            }),
        }
    }

    pub fn orchestrator(&self) -> Arc<ScanOrchestrator> {
        Arc::clone(&self.inner.orchestrator)
    }

    pub fn subscribe_catalog_events(
        &self,
    ) -> broadcast::Receiver<CatalogEventFrame> {
        self.inner.catalog_bus.subscribe()
    }

    pub fn catalog_event_history_since_sequence(
        &self,
        sequence: u64,
    ) -> Vec<CatalogEventFrame> {
        self.inner.catalog_bus.history_since_sequence(sequence)
    }

    pub fn catalog_event_history_since_instant(
        &self,
        since: Instant,
    ) -> Vec<CatalogEventFrame> {
        self.inner.catalog_bus.history_since_instant(since)
    }

    pub async fn subscribe_scan(
        &self,
        scan_id: Uuid,
    ) -> Result<broadcast::Receiver<ScanBroadcastFrame>, ScanControlError> {
        let guard = self.inner.active_by_scan_id.read().await;
        guard
            .get(&scan_id)
            .cloned()
            .map(|run| run.subscribe())
            .ok_or(ScanControlError::ScanNotFound)
    }

    #[instrument(skip(self))]
    pub async fn start_library_scan(
        &self,
        library_id: LibraryId,
        correlation_id: Option<Uuid>,
        mode: ScanRunMode,
    ) -> Result<ScanCommandAccepted, ScanControlError> {
        let library = self
            .inner
            .unit_of_work
            .libraries
            .get_library(library_id)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?
            .ok_or(ScanControlError::LibraryNotFound)?;

        if !library.enabled {
            return Err(ScanControlError::LibraryDisabled);
        }

        let requested_id = correlation_id.unwrap_or_else(Uuid::now_v7);
        let get_or_create = self
            .inner
            .unit_of_work
            .scan_runs
            .get_or_create_active(
                NewLibraryScanRun::new(library_id, mode)
                    .with_scan_id(requested_id)
                    .with_correlation_id(requested_id)
                    .running(),
            )
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?;

        let durable = get_or_create.run;
        let disposition = get_or_create.disposition;
        let run = self.inner.register_durable_run(durable.clone()).await;

        if disposition == ScanStartDisposition::Created {
            run.begin().await;

            if let Err(err) = self
                .inner
                .orchestrator
                .command_library(
                    durable.library_id,
                    LibraryActorCommand::Start {
                        mode: start_mode_from_scan_run_mode(durable.mode),
                        correlation_id: Some(durable.correlation_id),
                    },
                )
                .await
            {
                run.fail_with_reason("start_command_failed").await;
                return Err(ScanControlError::internal(err.to_string()));
            }
        }

        let snapshot = run.snapshot().await?;
        Ok(ScanCommandAccepted {
            scan_id: durable.scan_id,
            correlation_id: durable.correlation_id,
            status: snapshot.status,
            mode: durable.mode,
            idempotency_key: durable.run_key.clone(),
            run_key: durable.run_key,
            disposition,
        })
    }

    pub async fn rehydrate_active_runs(
        &self,
    ) -> Result<usize, ScanControlError> {
        let active_runs = self
            .inner
            .unit_of_work
            .scan_runs
            .list_active()
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?;

        let mut restored = 0usize;
        for durable in active_runs {
            let run = self.inner.register_durable_run(durable).await;
            run.seed_rehydrated_progress().await;
            restored += 1;
        }

        Ok(restored)
    }

    pub async fn inject_created_folders(
        &self,
        library_id: LibraryId,
        folders: Vec<std::path::PathBuf>,
    ) -> Result<(), ScanControlError> {
        if folders.is_empty() {
            return Ok(());
        }

        let library = self
            .inner
            .unit_of_work
            .libraries
            .get_library(library_id)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?
            .ok_or(ScanControlError::LibraryNotFound)?;

        if !library.enabled {
            return Err(ScanControlError::LibraryDisabled);
        }

        let roots: Vec<(LibraryRootsId, std::path::PathBuf)> = library
            .paths
            .iter()
            .enumerate()
            .map(|(idx, path)| (LibraryRootsId(idx as u16), path.clone()))
            .collect();

        if roots.is_empty() {
            return Err(ScanControlError::internal(format!(
                "library {} has no root paths configured",
                library_id
            )));
        }

        let correlation_id = Uuid::now_v7();
        let occurred_at = chrono::Utc::now();

        let mut by_root: HashMap<LibraryRootsId, Vec<FileSystemEvent>> =
            HashMap::new();

        for folder in folders {
            let (root_id, _root_path) = roots
                .iter()
                .find(|(_id, root_path)| folder.starts_with(root_path))
                .cloned()
                .ok_or_else(|| {
                    ScanControlError::internal(format!(
                        "path {} not within any configured root for library {}",
                        folder.display(),
                        library_id
                    ))
                })?;

            let path_key = normalize_path(&folder)
                .map_err(|e| ScanControlError::Internal(e.to_string()))?;
            let idempotency_key =
                format!("demo:{}:{}", library_id, Uuid::now_v7());

            by_root.entry(root_id).or_default().push(FileSystemEvent {
                version: ferrex_core::domain::scan::fs_watch::EVENT_VERSION,
                correlation_id: Some(correlation_id),
                idempotency_key,
                library_id,
                path_key,
                fingerprint: None,
                path: folder,
                old_path: None,
                kind: FileSystemEventKind::Created,
                occurred_at,
            });
        }

        for (root_id, events) in by_root {
            self.inner
                .orchestrator
                .command_library(
                    library_id,
                    LibraryActorCommand::FsEvents {
                        root: root_id,
                        events,
                        correlation_id: Some(correlation_id),
                    },
                )
                .await
                .map_err(|err| ScanControlError::internal(err.to_string()))?;
        }

        Ok(())
    }

    pub async fn pause_scan(
        &self,
        library_id: LibraryId,
        scan_id: &Uuid,
    ) -> Result<ScanCommandAccepted, ScanControlError> {
        let run = self.inner.lookup_for_library(scan_id, library_id).await?;
        let requested_correlation_id = Uuid::now_v7();
        run.pause(requested_correlation_id).await?;
        let snapshot = run.snapshot().await?;
        let mode = scan_run_mode_from_start_mode(run.start_mode());
        let run_key = mode.run_key(run.library_id());
        Ok(ScanCommandAccepted {
            scan_id: *scan_id,
            correlation_id: snapshot.correlation_id,
            status: snapshot.status,
            mode,
            idempotency_key: run_key.clone(),
            run_key,
            disposition: ScanStartDisposition::Reused,
        })
    }

    pub async fn resume_scan(
        &self,
        library_id: LibraryId,
        scan_id: &Uuid,
    ) -> Result<ScanCommandAccepted, ScanControlError> {
        let run = self.inner.lookup_for_library(scan_id, library_id).await?;
        let requested_correlation_id = Uuid::now_v7();
        run.resume(requested_correlation_id).await?;
        let snapshot = run.snapshot().await?;
        let mode = scan_run_mode_from_start_mode(run.start_mode());
        let run_key = mode.run_key(run.library_id());
        Ok(ScanCommandAccepted {
            scan_id: *scan_id,
            correlation_id: snapshot.correlation_id,
            status: snapshot.status,
            mode,
            idempotency_key: run_key.clone(),
            run_key,
            disposition: ScanStartDisposition::Reused,
        })
    }

    pub async fn cancel_scan(
        &self,
        library_id: LibraryId,
        scan_id: &Uuid,
    ) -> Result<ScanCommandAccepted, ScanControlError> {
        let run = self.inner.lookup_for_library(scan_id, library_id).await?;
        let requested_correlation_id = Uuid::now_v7();
        run.cancel(requested_correlation_id).await?;
        let snapshot = run.snapshot().await?;
        let mode = scan_run_mode_from_start_mode(run.start_mode());
        let run_key = mode.run_key(run.library_id());
        Ok(ScanCommandAccepted {
            scan_id: *scan_id,
            correlation_id: snapshot.correlation_id,
            status: snapshot.status,
            mode,
            idempotency_key: run_key.clone(),
            run_key,
            disposition: ScanStartDisposition::Reused,
        })
    }

    pub async fn active_scans(&self) -> Vec<ScanSnapshot> {
        let guard = self.inner.active_by_scan_id.read().await;
        let runs: Vec<_> = guard.values().cloned().collect();
        drop(guard);

        let mut snapshots = Vec::with_capacity(runs.len());
        let mut seen = HashSet::new();
        for run in runs {
            if let Ok(snapshot) = run.snapshot().await {
                seen.insert(snapshot.scan_id);
                snapshots.push(snapshot);
            }
        }

        match self
            .inner
            .unit_of_work
            .scan_observability
            .active_runs_all()
            .await
        {
            Ok(rows) => {
                for row in rows {
                    if seen.contains(&row.id) {
                        continue;
                    }
                    if let Some(snapshot) =
                        ScanSnapshot::from_observability(row)
                    {
                        snapshots.push(snapshot);
                    }
                }
            }
            Err(err) => {
                warn!(error = %err, "failed to load persisted active scan runs");
            }
        }

        snapshots
    }

    pub async fn history(&self, limit: usize) -> Vec<ScanHistoryEntry> {
        let persisted = self
            .inner
            .unit_of_work
            .scan_observability
            .recent_runs(None, limit as i64)
            .await;

        match persisted {
            Ok(rows) => {
                let entries: Vec<ScanHistoryEntry> = rows
                    .into_iter()
                    .filter(|run| !run.status.is_active())
                    .filter_map(ScanHistoryEntry::from_observability)
                    .take(limit)
                    .collect();
                if !entries.is_empty() {
                    return entries;
                }
            }
            Err(err) => {
                warn!(error = %err, "failed to load persisted scan history");
            }
        }

        let guard = self.inner.history.read().await;
        guard.iter().rev().take(limit).cloned().collect()
    }

    pub async fn snapshot(&self, scan_id: &Uuid) -> Option<ScanSnapshot> {
        let guard = self.inner.active_by_scan_id.read().await;
        let run = guard.get(scan_id).cloned();
        drop(guard);
        if let Some(run) = run {
            (run.snapshot().await).ok()
        } else {
            None
        }
    }

    pub async fn events(
        &self,
        scan_id: &Uuid,
    ) -> Result<Vec<ScanBroadcastFrame>, ScanControlError> {
        let run = {
            let guard = self.inner.active_by_scan_id.read().await;
            guard.get(scan_id).cloned()
        };
        if let Some(run) = run {
            return Ok(run.event_log().await);
        }

        {
            let final_events = self.inner.final_events.read().await;
            if let Some(events) = final_events.get(scan_id) {
                return Ok(events.iter().cloned().collect());
            }
        }

        let events = self
            .inner
            .unit_of_work
            .scan_observability
            .events_for_run(*scan_id)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?;
        let frames: Vec<ScanBroadcastFrame> = events
            .into_iter()
            .filter_map(ScanBroadcastFrame::from_observability)
            .collect();
        if frames.is_empty() {
            Err(ScanControlError::ScanNotFound)
        } else {
            Ok(frames)
        }
    }

    pub async fn runs_page(
        &self,
        request: ScanRunPageRequest,
    ) -> Result<ScanRunPage, ScanControlError> {
        self.inner
            .unit_of_work
            .scan_observability
            .runs_page(request)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))
    }

    pub async fn run_detail(
        &self,
        scan_id: Uuid,
    ) -> Result<ScanRunRecord, ScanControlError> {
        self.inner
            .unit_of_work
            .scan_observability
            .get_run(scan_id)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?
            .ok_or(ScanControlError::ScanNotFound)
    }

    pub async fn run_events_page(
        &self,
        scan_id: Uuid,
        after_sequence: Option<u64>,
        limit: i64,
    ) -> Result<ScanRunEventReplayPage, ScanControlError> {
        let after_i64 =
            after_sequence.map(|seq| seq.min(i64::MAX as u64) as i64);
        let repo = &self.inner.unit_of_work.scan_observability;
        let run = repo
            .get_run(scan_id)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?
            .ok_or(ScanControlError::ScanNotFound)?;
        let bounds = repo
            .event_sequence_bounds(scan_id)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?;

        validate_replay_gap(scan_id, after_sequence, &bounds, run.sequence)?;

        let events = repo
            .events_page_for_run(ScanRunEventPageRequest {
                run_id: scan_id,
                after_sequence: after_i64,
                limit,
            })
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?;
        let next_sequence = events
            .last()
            .map(|event| event.sequence.max(0) as u64)
            .or(after_sequence);

        Ok(ScanRunEventReplayPage {
            events,
            bounds,
            requested_after_sequence: after_sequence,
            next_sequence,
        })
    }

    pub async fn run_failures_page(
        &self,
        request: ScanRunFailurePageRequest,
    ) -> Result<ScanRunFailurePage, ScanControlError> {
        if self
            .inner
            .unit_of_work
            .scan_observability
            .get_run(request.run_id)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?
            .is_none()
        {
            return Err(ScanControlError::ScanNotFound);
        }

        self.inner
            .unit_of_work
            .scan_observability
            .failure_summaries_page_for_run(request)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))
    }

    pub async fn recover_path(
        &self,
        library_id: LibraryId,
        path: &str,
        correlation_id: Option<Uuid>,
    ) -> Result<ScanRecoveryAccepted, ScanControlError> {
        let library = self
            .inner
            .unit_of_work
            .libraries
            .get_library(library_id)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?
            .ok_or(ScanControlError::LibraryNotFound)?;

        if !library.enabled {
            return Err(ScanControlError::LibraryDisabled);
        }

        let normalized_path = normalize_path(std::path::Path::new(path))
            .map_err(|err| {
                ScanControlError::InvalidRecoveryTarget(err.to_string())
            })?;
        let owned_by_library = library
            .paths
            .iter()
            .filter_map(|root| normalize_path(root).ok())
            .any(|root| path_is_within(&root, &normalized_path));
        if !owned_by_library {
            return Err(ScanControlError::InvalidRecoveryTarget(
                "path_not_owned_by_library".to_string(),
            ));
        }

        let context = recovery_context_for_library(
            library.library_type,
            library_id,
            &normalized_path,
        )?;
        let payload = JobPayload::FolderScan(FolderScanJob {
            context,
            scan_reason: ScanReason::UserRequested,
            enqueue_time: Utc::now(),
            device_id: None,
        });
        let mut request = EnqueueRequest::new(JobPriority::P0, payload);
        request.allow_merge = true;
        request.correlation_id = correlation_id;

        let handle = self
            .inner
            .orchestrator
            .enqueue(request)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?;

        Ok(ScanRecoveryAccepted {
            library_id,
            original_path: path.to_string(),
            normalized_path,
            handle,
        })
    }
}

impl ScanControlPlaneInner {
    async fn register_durable_run(
        self: &Arc<Self>,
        durable: LibraryScanRun,
    ) -> Arc<ScanRun> {
        let run = ScanRun::from_durable(Arc::clone(self), durable);
        self.register_run(run).await
    }

    async fn register_run(&self, run: Arc<ScanRun>) -> Arc<ScanRun> {
        let scan_id = run.scan_id();
        let run_key = run.run_key();

        {
            let mut by_scan_id = self.active_by_scan_id.write().await;
            if let Some(existing) = by_scan_id.get(&scan_id).cloned() {
                return existing;
            }

            let mut by_run_key = self.active_by_run_key.write().await;
            if let Some(existing) = by_run_key.get(&run_key).cloned() {
                by_scan_id.insert(scan_id, Arc::clone(&existing));
                return existing;
            }

            by_scan_id.insert(scan_id, Arc::clone(&run));
            by_run_key.insert(run_key, Arc::clone(&run));
        }

        self.movie_batch_notifiers
            .on_run_started(
                run.library_id(),
                Arc::clone(&self.unit_of_work),
                Arc::clone(&self.catalog_bus),
            )
            .await;

        self.aggregator.register(Arc::clone(&run)).await;
        run
    }

    async fn finalize_run(
        &self,
        scan_id: Uuid,
        run_key: String,
        correlation_id: Uuid,
        snapshot: ScanHistoryEntry,
        final_events: Vec<ScanBroadcastFrame>,
    ) {
        {
            let mut events = self.final_events.write().await;
            events.insert(scan_id, final_events.into_iter().collect());
        }
        {
            let mut by_scan_id = self.active_by_scan_id.write().await;
            by_scan_id.remove(&scan_id);
            let mut by_run_key = self.active_by_run_key.write().await;
            by_run_key.remove(&run_key);
        }
        self.movie_batch_notifiers
            .on_run_finished(snapshot.library_id)
            .await;
        self.aggregator.drop(&correlation_id).await;

        let evicted_scan_id = {
            let mut history = self.history.write().await;
            let evicted = if history.len() == HISTORY_CAPACITY {
                history.pop_front().map(|entry| entry.scan_id)
            } else {
                None
            };
            history.push_back(snapshot.clone());
            evicted
        };

        if let Some(evicted_scan_id) = evicted_scan_id {
            let mut events = self.final_events.write().await;
            events.remove(&evicted_scan_id);
        }

        // Rebuild precomputed sort positions for the completed library scan
        if snapshot.status == ScanLifecycleStatus::Completed {
            let library_id = snapshot.library_id;
            if let Err(err) = self
                .unit_of_work
                .indices
                .rebuild_movie_sort_positions(library_id)
                .await
            {
                tracing::warn!(
                    "failed to rebuild movie_sort_positions for library {}: {}",
                    library_id.as_uuid(),
                    err
                );
            } else {
                tracing::info!(
                    "rebuilt precomputed movie positions for library {}",
                    library_id.as_uuid()
                );
            }
        }
    }

    async fn lookup(
        &self,
        scan_id: &Uuid,
    ) -> Result<Arc<ScanRun>, ScanControlError> {
        let guard = self.active_by_scan_id.read().await;
        guard
            .get(scan_id)
            .cloned()
            .ok_or(ScanControlError::ScanNotFound)
    }

    async fn lookup_for_library(
        &self,
        scan_id: &Uuid,
        library_id: LibraryId,
    ) -> Result<Arc<ScanRun>, ScanControlError> {
        let run = self.lookup(scan_id).await?;
        if run.library_id() != library_id {
            return Err(ScanControlError::LibraryMismatch);
        }
        Ok(run)
    }
}

#[derive(Debug, Clone)]
pub struct ScanCommandAccepted {
    pub scan_id: Uuid,
    pub correlation_id: Uuid,
    pub status: ScanLifecycleStatus,
    pub mode: ScanRunMode,
    pub idempotency_key: String,
    pub run_key: String,
    pub disposition: ScanStartDisposition,
}

#[derive(Debug, Clone)]
pub struct ScanRunEventReplayPage {
    pub events: Vec<ScanRunEventRecord>,
    pub bounds: ScanRunEventSequenceBounds,
    pub requested_after_sequence: Option<u64>,
    pub next_sequence: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ScanReplayGap {
    pub scan_id: Uuid,
    pub requested_after_sequence: u64,
    pub min_available_sequence: Option<u64>,
    pub max_available_sequence: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ScanRecoveryAccepted {
    pub library_id: LibraryId,
    pub original_path: String,
    pub normalized_path: String,
    pub handle: JobHandle,
}

fn validate_replay_gap(
    scan_id: Uuid,
    requested_after_sequence: Option<u64>,
    bounds: &ScanRunEventSequenceBounds,
    run_sequence: i64,
) -> Result<(), ScanControlError> {
    let Some(requested_after_sequence) = requested_after_sequence else {
        return Ok(());
    };

    let min_available_sequence =
        bounds.min_sequence.map(|value| value.max(0) as u64);
    let max_available_sequence =
        bounds.max_sequence.map(|value| value.max(0) as u64);
    let requested_next = requested_after_sequence.saturating_add(1);

    if let Some(min_available) = min_available_sequence {
        if requested_next < min_available {
            return Err(ScanControlError::ReplayGap(ScanReplayGap {
                scan_id,
                requested_after_sequence,
                min_available_sequence,
                max_available_sequence,
            }));
        }
    } else if requested_after_sequence < run_sequence.max(0) as u64 {
        return Err(ScanControlError::ReplayGap(ScanReplayGap {
            scan_id,
            requested_after_sequence,
            min_available_sequence,
            max_available_sequence,
        }));
    }

    Ok(())
}

fn path_is_within(root_norm: &str, candidate_norm: &str) -> bool {
    let root = std::path::Path::new(root_norm);
    let candidate = std::path::Path::new(candidate_norm);
    candidate == root || candidate.starts_with(root)
}

fn recovery_context_for_library(
    library_type: LibraryType,
    library_id: LibraryId,
    normalized_path: &str,
) -> Result<FolderScanContext, ScanControlError> {
    match library_type {
        LibraryType::Movies => {
            Ok(FolderScanContext::Movie(MovieFolderScanContext {
                library_id,
                movie_root_path: MovieRootPath::try_new(normalized_path)
                    .map_err(|err| {
                        ScanControlError::InvalidRecoveryTarget(err.to_string())
                    })?,
            }))
        }
        LibraryType::Series => {
            if let Ok(series_root_path) =
                SeriesRootPath::try_new(normalized_path)
            {
                return Ok(FolderScanContext::Series(
                    SeriesFolderScanContext {
                        library_id,
                        series_root_path,
                    },
                ));
            }

            let season_path = std::path::Path::new(normalized_path);
            let Some(series_root) = season_path.parent() else {
                return Err(ScanControlError::InvalidRecoveryTarget(
                    "series_path_missing_parent".to_string(),
                ));
            };
            let series_root_norm = series_root.to_string_lossy().to_string();
            let series_root_path = SeriesRootPath::try_new(series_root_norm)
                .map_err(|err| {
                    ScanControlError::InvalidRecoveryTarget(err.to_string())
                })?;
            let (season_folder_path, season_number) =
                SeasonFolderPath::try_new_under_series_root(
                    &series_root_path,
                    normalized_path,
                )
                .map_err(|err| {
                    ScanControlError::InvalidRecoveryTarget(err.to_string())
                })?;

            Ok(FolderScanContext::Season(SeasonFolderScanContext {
                library_id,
                series_root_path,
                season_folder_path,
                season_number,
            }))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanLifecycleStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Canceled,
}

impl ScanLifecycleStatus {
    fn from_observability(status: ScanRunStatus) -> Option<Self> {
        match status {
            ScanRunStatus::Pending => Some(Self::Pending),
            ScanRunStatus::Running => Some(Self::Running),
            ScanRunStatus::Paused => Some(Self::Paused),
            ScanRunStatus::Completed => Some(Self::Completed),
            ScanRunStatus::Failed => Some(Self::Failed),
            ScanRunStatus::Canceled => Some(Self::Canceled),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ScanLifecycleStatus::Pending => "pending",
            ScanLifecycleStatus::Running => "running",
            ScanLifecycleStatus::Paused => "paused",
            ScanLifecycleStatus::Completed => "completed",
            ScanLifecycleStatus::Failed => "failed",
            ScanLifecycleStatus::Canceled => "canceled",
        }
    }
}

fn start_mode_from_scan_run_mode(mode: ScanRunMode) -> StartMode {
    match mode {
        ScanRunMode::Manual => StartMode::Bulk,
        ScanRunMode::Maintenance => StartMode::Maintenance,
        ScanRunMode::Resume => StartMode::Resume,
    }
}

fn scan_run_mode_from_start_mode(mode: StartMode) -> ScanRunMode {
    match mode {
        StartMode::Bulk => ScanRunMode::Manual,
        StartMode::Maintenance => ScanRunMode::Maintenance,
        StartMode::Resume => ScanRunMode::Resume,
    }
}

fn scan_run_mode_from_observability_source(
    source: ScanRunSource,
) -> ScanRunMode {
    match source {
        ScanRunSource::Manual => ScanRunMode::Manual,
        ScanRunSource::Maintenance
        | ScanRunSource::Watcher
        | ScanRunSource::Orchestrator => ScanRunMode::Maintenance,
        ScanRunSource::Retry => ScanRunMode::Resume,
    }
}

fn repository_status_from_payload(
    status: &str,
) -> Option<ApiScanLifecycleStatus> {
    match status {
        "pending" => Some(ApiScanLifecycleStatus::Pending),
        "paused" => Some(ApiScanLifecycleStatus::Paused),
        "completed" | "failed" | "canceled" | "cancelled" => None,
        _ => Some(ApiScanLifecycleStatus::Running),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanBroadcastFrame {
    pub event: ScanEventKind,
    pub payload: ScanProgressEvent,
}

impl ScanBroadcastFrame {
    pub(crate) fn from_observability(
        record: ScanRunEventRecord,
    ) -> Option<Self> {
        let event =
            ScanEventKind::from_observability_kind(record.event_kind.as_str())?;
        let payload = serde_json::from_value(record.payload).ok()?;
        Some(Self { event, payload })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanEventKind {
    Started,
    Progress,
    Quiescing,
    Completed,
    Failed,
}

impl ScanEventKind {
    fn as_str(&self) -> &'static str {
        match self {
            ScanEventKind::Started => "started",
            ScanEventKind::Progress => "progress",
            ScanEventKind::Quiescing => "quiescing",
            ScanEventKind::Completed => "completed",
            ScanEventKind::Failed => "failed",
        }
    }

    fn from_observability_kind(value: &str) -> Option<Self> {
        match value {
            "started" => Some(Self::Started),
            "progress" => Some(Self::Progress),
            "quiescing" => Some(Self::Quiescing),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn as_sse_event_type(&self) -> ScanSseEventType {
        match self {
            ScanEventKind::Started => ScanSseEventType::Started,
            ScanEventKind::Progress => ScanSseEventType::Progress,
            ScanEventKind::Quiescing => ScanSseEventType::Quiescing,
            ScanEventKind::Completed => ScanSseEventType::Completed,
            ScanEventKind::Failed => ScanSseEventType::Failed,
        }
    }
}

struct ScanRun {
    scan_id: Uuid,
    library_id: LibraryId,
    correlation_id: Uuid,
    state: Mutex<ScanRunState>,
    tx: broadcast::Sender<ScanBroadcastFrame>,
    inner: Weak<ScanControlPlaneInner>,
    events: Mutex<VecDeque<ScanBroadcastFrame>>,
    start_mode: StartMode,
    log: Mutex<ScanLogWatermark>,
}

#[derive(Debug)]
struct ScanRunState {
    scan_id: Uuid,
    library_id: LibraryId,
    phase: ScanPhase,
    status: ScanLifecycleStatus,
    completed_items: u64,
    total_items: u64,
    dead_lettered_items: u64,
    retrying_items: u64,
    current_path: Option<String>,
    path_key: Option<SubjectKey>,
    correlation_id: Uuid,
    idempotency_prefix: String,
    event_sequence: u64,
    last_idempotency_key: String,
    started_at: DateTime<Utc>,
    terminal_at: Option<DateTime<Utc>>,
    last_activity_at: Option<DateTime<Utc>>,
    quiescence_started_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
    item_states: HashMap<String, ScanItemState>,
    folder_outcomes_by_path: HashMap<String, FolderScanOutcome>,
    historical_cursor_count: u64,
    seed_completed: bool,
}

#[derive(Debug, Clone, Default)]
struct ScanCounterSnapshot {
    validated_items: u64,
    known_unchanged_items: u64,
    skipped_items: u64,
    failed_items: u64,
    needs_attention_items: u64,
    retrying_items: u64,
    completed_items: u64,
    reason_details: Vec<ScanPathReasonDetail>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanPhase {
    Initializing,
    Discovering,
    Processing,
    Quiescing,
    Completed,
    Failed,
    Canceled,
}

impl ScanPhase {
    fn from_lifecycle_status(status: &ScanLifecycleStatus) -> Self {
        match status {
            ScanLifecycleStatus::Pending => Self::Initializing,
            ScanLifecycleStatus::Running => Self::Discovering,
            ScanLifecycleStatus::Paused => Self::Discovering,
            ScanLifecycleStatus::Completed => Self::Completed,
            ScanLifecycleStatus::Failed => Self::Failed,
            ScanLifecycleStatus::Canceled => Self::Canceled,
        }
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }

    fn status(&self) -> &'static str {
        match self {
            ScanPhase::Initializing => "initializing",
            ScanPhase::Discovering => "discovering",
            ScanPhase::Processing => "processing",
            ScanPhase::Quiescing => "quiescing",
            ScanPhase::Completed => "completed",
            ScanPhase::Failed => "failed",
            ScanPhase::Canceled => "canceled",
        }
    }
}

#[derive(Debug, Clone)]
enum ScanStateEvent {
    RunStarted,
    NewItemFound,
    AllItemsProcessed,
    QuiescenceComplete,
    Stalled { reason: String },
}

#[derive(Debug, Clone)]
struct QueuedFrame {
    event: ScanEventKind,
    payload: ScanProgressEvent,
}

#[derive(Debug, Clone)]
struct ScanItemState {
    status: ScanItemStatus,
    last_activity: DateTime<Utc>,
    path_key: Option<SubjectKey>,
    last_error: Option<String>,
    last_job_id: Option<JobId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanItemStatus {
    InProgress,
    Retrying,
    Completed,
    KnownUnchanged,
    Skipped,
    DeadLettered,
}

impl ScanItemStatus {
    fn is_active(self) -> bool {
        matches!(self, Self::InProgress | Self::Retrying)
    }

    fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::KnownUnchanged
                | Self::Skipped
                | Self::DeadLettered
        )
    }

    fn counts_as_completed(self) -> bool {
        matches!(self, Self::Completed | Self::KnownUnchanged | Self::Skipped)
    }
}

impl ScanItemState {
    fn is_active(&self) -> bool {
        self.status.is_active()
    }

    fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }
}

impl ScanRun {
    fn from_durable(
        inner: Arc<ScanControlPlaneInner>,
        durable: LibraryScanRun,
    ) -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(1024);
        let status = ScanLifecycleStatus::from(durable.status.clone());
        let phase = ScanPhase::from_lifecycle_status(&status);
        let path_key = durable
            .current_path
            .as_ref()
            .and_then(|path| SubjectKey::path(path.clone()).ok());
        let last_idempotency_key = if durable.sequence > 0 {
            format!("scan:{}:{}", durable.scan_id, durable.sequence)
        } else {
            String::new()
        };

        Arc::new(ScanRun {
            scan_id: durable.scan_id,
            library_id: durable.library_id,
            correlation_id: durable.correlation_id,
            state: Mutex::new(ScanRunState {
                scan_id: durable.scan_id,
                library_id: durable.library_id,
                phase,
                status,
                completed_items: durable.completed_items,
                total_items: durable.total_items,
                dead_lettered_items: durable.dead_lettered_items,
                retrying_items: durable.retrying_items,
                current_path: durable.current_path,
                path_key,
                correlation_id: durable.correlation_id,
                idempotency_prefix: format!("scan:{}:", durable.scan_id),
                event_sequence: durable.sequence,
                last_idempotency_key,
                started_at: durable.started_at,
                terminal_at: durable.terminal_at,
                last_activity_at: Some(durable.updated_at),
                quiescence_started_at: None,
                last_error: durable.last_error,
                item_states: HashMap::new(),
                folder_outcomes_by_path: HashMap::new(),
                historical_cursor_count: 0,
                seed_completed: false,
            }),
            tx,
            inner: Arc::downgrade(&inner),
            events: Mutex::new(VecDeque::with_capacity(EVENT_HISTORY_CAPACITY)),
            start_mode: start_mode_from_scan_run_mode(durable.mode),
            log: Mutex::new(ScanLogWatermark::default()),
        })
    }

    fn scan_id(&self) -> Uuid {
        self.scan_id
    }

    fn correlation_id(&self) -> Uuid {
        self.correlation_id
    }

    fn library_id(&self) -> LibraryId {
        self.library_id
    }

    fn start_mode(&self) -> StartMode {
        self.start_mode
    }

    fn run_key(&self) -> String {
        scan_run_mode_from_start_mode(self.start_mode).run_key(self.library_id)
    }

    fn subscribe(&self) -> broadcast::Receiver<ScanBroadcastFrame> {
        self.tx.subscribe()
    }

    async fn begin(self: &Arc<Self>) {
        self.rehydrate_from_cursors().await;
        let emitted = {
            let mut state = self.state.lock().await;
            state.status = ScanLifecycleStatus::Running;
            state.handle_state_event(ScanStateEvent::RunStarted, Utc::now());
            state.build_payload()
        };
        self.emit_frame(ScanEventKind::Started, emitted).await;
    }

    async fn seed_rehydrated_progress(&self) {
        let payload = {
            let state = self.state.lock().await;
            state.build_current_payload()
        };
        let frame = ScanBroadcastFrame {
            event: ScanEventKind::Progress,
            payload,
        };
        let mut history = self.events.lock().await;
        if history.len() == EVENT_HISTORY_CAPACITY {
            history.pop_front();
        }
        history.push_back(frame);
    }

    async fn rehydrate_from_cursors(&self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let repository = inner.orchestrator.cursor_repository();
        let cursors = match repository.list_by_library(self.library_id).await {
            Ok(entries) => entries,
            Err(err) => {
                warn!(
                    library = %self.library_id,
                    scan = %self.scan_id,
                    error = %err,
                    "failed to load persisted scan cursors"
                );
                return;
            }
        };

        if cursors.is_empty() {
            return;
        }

        let mut state = self.state.lock().await;
        state.rehydrate_from_cursors(&cursors);
    }

    async fn emit_frames(&self, frames: Vec<QueuedFrame>) {
        for frame in frames {
            self.emit_frame(frame.event, frame.payload).await;
        }
    }

    /// Record an index outcome (success/failure) for a given media file path.
    /// Successful outcomes are attributed to the parent folder of the file and
    /// used to verify folder-level scan completion reflects actual matches.
    async fn record_index_outcome(&self, file_path_norm: &str, success: bool) {
        if !success {
            return;
        }

        let file_path = std::path::Path::new(file_path_norm);
        let mut state = self.state.lock().await;

        // Gather scanned folder paths
        let mut scanned: Vec<String> = Vec::new();
        for item in state.item_states.values() {
            if let Some(path) = &item.path_key
                && let Some(path) = subject_key_path(path)
            {
                scanned.push(path.to_string());
            }
        }

        if scanned.is_empty() {
            return;
        }

        // Find the deepest scanned ancestor of the file
        let mut best: Option<String> = None;
        for folder in &scanned {
            let folder_path = std::path::Path::new(folder);
            if file_path.starts_with(folder_path) {
                match &best {
                    Some(current) => {
                        if folder.len() > current.len() {
                            best = Some(folder.clone());
                        }
                    }
                    None => best = Some(folder.clone()),
                }
            }
        }

        // If nothing matches, bail (should be rare)
        let Some(mut chosen) = best else {
            tracing::debug!(
                target: "scan::state",
                scan = %self.scan_id,
                library = %self.library_id,
                path = %file_path_norm,
                "no scanned ancestor found for indexed file"
            );
            return;
        };

        // Helper: identify non-entity folders to skip (seasons/extras)
        let is_non_entity_folder = |name: &str| {
            let lower = name.to_ascii_lowercase();
            // Extras-like
            if lower == "extras"
                || lower == "featurettes"
                || lower == "behind the scenes"
                || lower == "specials"
                || lower == "special"
            {
                return true;
            }
            // Season-like
            if lower.starts_with("season ") {
                return true;
            }
            // S01, S1, s1 etc.
            if lower.len() >= 2 && lower.starts_with('s') {
                let rest = &lower[1..];
                if rest.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }
            false
        };

        // If the deepest scanned folder is season/extras-like, walk up to a scanned parent
        // that looks like an entity root (movie/series folder).
        if let Some(name) = std::path::Path::new(&chosen)
            .file_name()
            .and_then(|s| s.to_str())
            && is_non_entity_folder(name)
        {
            let mut cur = std::path::Path::new(&chosen).parent();
            while let Some(dir) = cur {
                if let Some(dir_str) = dir.to_str()
                    && scanned.iter().any(|s| s == dir_str)
                {
                    // Check if this parent is still non-entity; if so, continue walking up
                    let parent_name =
                        dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if !is_non_entity_folder(parent_name) {
                        chosen = dir_str.to_string();
                        break;
                    }
                }
                cur = dir.parent();
            }
        }

        // Treat this as activity so quiescence waits for indexing to settle
        state.current_path = Some(chosen.clone());
        state.path_key = SubjectKey::path(&chosen).ok();

        state.last_activity_at = Some(chrono::Utc::now());

        tracing::debug!(
            target: "scan::state",
            scan = %self.scan_id,
            library = %self.library_id,
            file = %file_path_norm,
            credited_folder = %chosen,
            "credited match to entity root folder"
        );
    }

    async fn pause(
        &self,
        correlation_id: Uuid,
    ) -> Result<(), ScanControlError> {
        let payload = {
            let mut state = self.state.lock().await;
            match state.status {
                ScanLifecycleStatus::Running => {
                    state.status = ScanLifecycleStatus::Paused;
                    state.correlation_id = correlation_id;
                    state.build_payload()
                }
                ScanLifecycleStatus::Paused => return Ok(()),
                ScanLifecycleStatus::Completed
                | ScanLifecycleStatus::Failed
                | ScanLifecycleStatus::Canceled => {
                    return Err(ScanControlError::ScanTerminal);
                }
                ScanLifecycleStatus::Pending => {
                    return Err(ScanControlError::ScanNotRunning);
                }
            }
        };
        self.emit_frame(ScanEventKind::Progress, payload).await;
        Ok(())
    }

    async fn resume(
        &self,
        correlation_id: Uuid,
    ) -> Result<(), ScanControlError> {
        let payload = {
            let mut state = self.state.lock().await;
            match state.status {
                ScanLifecycleStatus::Paused => {
                    state.status = ScanLifecycleStatus::Running;
                    state.correlation_id = correlation_id;
                    state.build_payload()
                }
                ScanLifecycleStatus::Running => return Ok(()),
                ScanLifecycleStatus::Completed
                | ScanLifecycleStatus::Failed
                | ScanLifecycleStatus::Canceled => {
                    return Err(ScanControlError::ScanTerminal);
                }
                ScanLifecycleStatus::Pending => {
                    return Err(ScanControlError::ScanNotRunning);
                }
            }
        };
        self.emit_frame(ScanEventKind::Progress, payload).await;
        Ok(())
    }

    async fn cancel(
        &self,
        correlation_id: Uuid,
    ) -> Result<(), ScanControlError> {
        let frame = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                return Err(ScanControlError::ScanTerminal);
            }
            state.correlation_id = correlation_id;
            state.last_error = Some("scan_cancelled".to_string());
            state
                .transition(ScanPhase::Canceled, Utc::now())
                .unwrap_or_else(|| QueuedFrame {
                    event: ScanEventKind::Failed,
                    payload: state.build_payload(),
                })
        };
        self.emit_frame(frame.event, frame.payload).await;
        self.finalize_history(ScanLifecycleStatus::Canceled).await;
        Ok(())
    }

    async fn snapshot(&self) -> Result<ScanSnapshot, ScanControlError> {
        let state = self.state.lock().await;
        let mode = scan_run_mode_from_start_mode(self.start_mode);
        let counters = state.counter_snapshot();
        Ok(ScanSnapshot {
            scan_id: state.scan_id,
            library_id: state.library_id,
            status: state.status.clone(),
            mode,
            completed_items: counters.completed_items,
            total_items: state.total_items,
            validated_items: counters.validated_items,
            known_unchanged_items: counters.known_unchanged_items,
            skipped_items: counters.skipped_items,
            failed_items: counters.failed_items,
            needs_attention_items: counters.needs_attention_items,
            retrying_items: counters.retrying_items,
            correlation_id: state.correlation_id,
            idempotency_key: state.current_idempotency_key(),
            run_key: mode.run_key(state.library_id),
            disposition: None,
            current_path: state.current_path.clone(),
            started_at: state.started_at,
            terminal_at: state.terminal_at,
            sequence: state.event_sequence,
            reason_details: counters.reason_details,
        })
    }

    async fn event_log(&self) -> Vec<ScanBroadcastFrame> {
        let guard = self.events.lock().await;
        guard.iter().cloned().collect()
    }

    async fn record_folder_enqueued(
        &self,
        idempotency_key: &str,
        job_id: JobId,
        path_key: Option<SubjectKey>,
    ) {
        let event_time = Utc::now();
        let path = path_key.clone();
        let frames = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                Vec::new()
            } else {
                let stale_terminal = state
                    .item_states
                    .get(idempotency_key)
                    .map(|item| item.is_terminal())
                    .unwrap_or(false);

                tracing::debug!(
                    target: "scan::state",
                    scan = %self.scan_id,
                    library = %self.library_id,
                    %job_id,
                    idempotency = idempotency_key,
                    stale_terminal,
                    phase = ?state.phase,
                    "record_folder_enqueued"
                );

                if stale_terminal {
                    if let Some(item) =
                        state.item_states.get_mut(idempotency_key)
                    {
                        item.last_activity = event_time;
                        item.last_job_id = Some(job_id);
                        if let Some(p) = path.clone() {
                            item.path_key = Some(p);
                        }
                    }
                    // Do not bump run-level last_activity for stale retrograde events; avoid
                    // keeping quiescence open due to out-of-order noise.
                    Vec::new()
                } else {
                    let previous_phase = state.phase;
                    state.status = ScanLifecycleStatus::Running;
                    let changed = state.update_item_status(
                        idempotency_key,
                        Some(job_id),
                        ScanItemStatus::InProgress,
                        event_time,
                        path.clone(),
                        None,
                    );

                    state.last_activity_at = Some(event_time);
                    state.current_path = path
                        .as_ref()
                        .and_then(subject_key_path)
                        .map(str::to_string);
                    state.path_key = path.clone();

                    let mut frames = Vec::new();
                    if let Some(frame) = state.handle_state_event(
                        ScanStateEvent::NewItemFound,
                        event_time,
                    ) {
                        frames.push(frame);
                    }

                    let reopened =
                        matches!(previous_phase, ScanPhase::Quiescing)
                            && matches!(state.phase, ScanPhase::Processing);

                    if let Some(payload) =
                        state.build_payload_if(changed || reopened)
                    {
                        frames.push(QueuedFrame {
                            event: ScanEventKind::Progress,
                            payload,
                        });
                    }
                    frames
                }
            }
        };

        self.emit_frames(frames).await;
    }

    async fn record_folder_summary(
        &self,
        summary: &ferrex_core::domain::scan::FolderScanSummary,
    ) {
        let event_time = Utc::now();
        let path = summary.context.folder_path_norm().to_string();
        let frames = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                Vec::new()
            } else {
                let mut frames = Vec::new();
                let changed = state.remember_folder_outcome(
                    &path,
                    summary.outcome,
                    event_time,
                );
                if changed {
                    state.current_path = Some(path.clone());
                    state.path_key = SubjectKey::path(path).ok();
                    state.last_activity_at = Some(event_time);
                    frames.push(QueuedFrame {
                        event: ScanEventKind::Progress,
                        payload: state.build_payload(),
                    });

                    if state.can_enter_quiescing()
                        && let Some(frame) = state.handle_state_event(
                            ScanStateEvent::AllItemsProcessed,
                            event_time,
                        )
                    {
                        frames.push(frame);
                    }
                }
                frames
            }
        };

        self.emit_frames(frames).await;
    }

    async fn record_seed_completed(&self, summary: &ScanSeedSummary) -> bool {
        let frame = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                None
            } else {
                state.mark_seed_completed(
                    summary.queued_folders,
                    summary.completed_at,
                )
            }
        };

        if let Some(frame) = frame {
            let event = frame.event.clone();
            self.emit_frame(frame.event, frame.payload).await;
            if matches!(event, ScanEventKind::Completed) {
                self.finalize_history(ScanLifecycleStatus::Completed).await;
                return true;
            }
        }

        false
    }

    async fn record_folder_completed(
        &self,
        idempotency_key: &str,
        job_id: JobId,
        path_key: Option<SubjectKey>,
    ) {
        let event_time = Utc::now();
        let path = path_key.clone();
        let frames = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                Vec::new()
            } else {
                tracing::debug!(
                    target: "scan::state",
                    scan = %self.scan_id,
                    library = %self.library_id,
                    %job_id,
                    idempotency = idempotency_key,
                    phase = ?state.phase,
                    "record_folder_completed"
                );
                let mut frames = Vec::new();
                let target_status = path
                    .as_ref()
                    .and_then(subject_key_path)
                    .and_then(|path| state.folder_outcomes_by_path.get(path))
                    .copied()
                    .map(ScanRunState::status_for_folder_outcome)
                    .unwrap_or(ScanItemStatus::Completed);
                let changed = state.update_item_status(
                    idempotency_key,
                    Some(job_id),
                    target_status,
                    event_time,
                    path.clone(),
                    None,
                );
                if changed {
                    state.current_path = path
                        .as_ref()
                        .and_then(subject_key_path)
                        .map(str::to_string);
                    state.path_key = path.clone();
                    state.last_activity_at = Some(event_time);
                    let progress = state.build_payload();
                    frames.push(QueuedFrame {
                        event: ScanEventKind::Progress,
                        payload: progress,
                    });

                    if state.can_enter_quiescing()
                        && let Some(frame) = state.handle_state_event(
                            ScanStateEvent::AllItemsProcessed,
                            event_time,
                        )
                    {
                        frames.push(frame);
                    }
                }
                frames
            }
        };

        self.emit_frames(frames).await;
        // Do not persist cursors here; dispatcher persists accurate listing hashes
        // once folder scan completes with the computed plan. Persisting here risks
        // overwriting listing_hash with a placeholder and breaking incremental diffs.
    }

    async fn record_folder_lease_renewed(
        &self,
        idempotency_key: &str,
        job_id: JobId,
        path_key: Option<SubjectKey>,
    ) {
        let event_time = Utc::now();
        let path = path_key.clone();
        let mut state = self.state.lock().await;
        if state.is_terminal() {
            return;
        }
        if let Some(item) = state.item_states.get_mut(idempotency_key) {
            if item.is_terminal() && item.last_job_id == Some(job_id) {
                return;
            }
            if let Some(last) = item.last_job_id
                && last != job_id
            {
                // Ignore renewals from a stale job
                return;
            }
            tracing::debug!(
                target: "scan::state",
                scan = %self.scan_id,
                library = %self.library_id,
                %job_id,
                idempotency = idempotency_key,
                status = ?item.status,
                "record_folder_lease_renewed"
            );
            if item.last_job_id.is_none() {
                item.last_job_id = Some(job_id);
            }
            item.last_activity = event_time;
            if let Some(path_value) = path {
                let current_path = subject_key_path_owned(&path_value);
                item.path_key = Some(path_value.clone());
                state.current_path = current_path;
                state.path_key = Some(path_value);
            }
            state.last_activity_at = Some(event_time);
        }
    }

    async fn record_folder_failure(
        &self,
        idempotency_key: &str,
        job_id: JobId,
        error: Option<String>,
        path_key: Option<SubjectKey>,
        retryable: bool,
    ) {
        let event_time = Utc::now();
        let path = path_key.clone();
        let frames = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                Vec::new()
            } else {
                let target_status = if retryable {
                    ScanItemStatus::Retrying
                } else {
                    ScanItemStatus::DeadLettered
                };

                tracing::debug!(
                    target: "scan::state",
                    scan = %self.scan_id,
                    library = %self.library_id,
                    %job_id,
                    idempotency = idempotency_key,
                    retryable,
                    "record_folder_failure"
                );

                let mut frames = Vec::new();
                let changed = state.update_item_status(
                    idempotency_key,
                    Some(job_id),
                    target_status,
                    event_time,
                    path.clone(),
                    error.clone(),
                );

                if changed {
                    state.current_path = path
                        .as_ref()
                        .and_then(subject_key_path)
                        .map(str::to_string);
                    state.path_key = path.clone();
                    state.last_activity_at = Some(event_time);
                    let progress = state.build_payload();
                    frames.push(QueuedFrame {
                        event: ScanEventKind::Progress,
                        payload: progress,
                    });

                    if !retryable
                        && state.can_enter_quiescing()
                        && let Some(frame) = state.handle_state_event(
                            ScanStateEvent::AllItemsProcessed,
                            event_time,
                        )
                    {
                        frames.push(frame);
                    }
                }
                frames
            }
        };

        self.emit_frames(frames).await;
    }

    async fn record_folder_dead_lettered(
        &self,
        idempotency_key: &str,
        job_id: JobId,
        error: Option<String>,
        path_key: Option<SubjectKey>,
    ) {
        let event_time = Utc::now();
        let path = path_key.clone();
        let frames = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                Vec::new()
            } else {
                let mut frames = Vec::new();
                let changed = state.update_item_status(
                    idempotency_key,
                    Some(job_id),
                    ScanItemStatus::DeadLettered,
                    event_time,
                    path.clone(),
                    error,
                );
                tracing::debug!(
                    target: "scan::state",
                    scan = %self.scan_id,
                    library = %self.library_id,
                    %job_id,
                    idempotency = idempotency_key,
                    changed,
                    "record_folder_dead_lettered"
                );
                if changed {
                    state.current_path = path
                        .as_ref()
                        .and_then(subject_key_path)
                        .map(str::to_string);
                    state.path_key = path.clone();
                    state.last_activity_at = Some(event_time);
                    let progress = state.build_payload();
                    frames.push(QueuedFrame {
                        event: ScanEventKind::Progress,
                        payload: progress,
                    });

                    if state.can_enter_quiescing()
                        && let Some(frame) = state.handle_state_event(
                            ScanStateEvent::AllItemsProcessed,
                            event_time,
                        )
                    {
                        frames.push(frame);
                    }
                }
                frames
            }
        };

        self.emit_frames(frames).await;
    }

    async fn fail_with_reason(&self, reason: &str) {
        let outcome = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                None
            } else {
                state.last_error = Some(reason.to_string());
                state.transition(ScanPhase::Failed, Utc::now())
            }
        };

        if let Some(frame) = outcome {
            self.emit_frame(frame.event, frame.payload).await;
            self.finalize_history(ScanLifecycleStatus::Failed).await;
        }
    }

    async fn try_complete(
        &self,
        completion_quiescence: ChronoDuration,
        stall_timeout: ChronoDuration,
    ) -> bool {
        let (maybe_frame, finalize_status) = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                (None, None)
            } else {
                let now = Utc::now();
                let mut frame: Option<QueuedFrame> = None;

                if state.total_items == 0 && state.seed_completed {
                    frame = state.transition(ScanPhase::Completed, now);
                }

                if matches!(
                    state.phase,
                    ScanPhase::Processing | ScanPhase::Discovering
                ) && state.can_enter_quiescing()
                {
                    frame = state.handle_state_event(
                        ScanStateEvent::AllItemsProcessed,
                        now,
                    );
                }

                if frame.is_none()
                    && matches!(state.phase, ScanPhase::Quiescing)
                {
                    if state.reset_quiescence_after_activity(now) {
                        tracing::debug!(
                            target: "scan::state",
                            scan = %state.scan_id,
                            library = %state.library_id,
                            "reset scan quiescence after run-visible activity"
                        );
                    } else {
                        let quiesced = state
                            .quiescence_started_at
                            .map(|ts| now - ts >= completion_quiescence)
                            .unwrap_or(false);

                        if quiesced && state.can_enter_quiescing() {
                            frame = state.handle_state_event(
                                ScanStateEvent::QuiescenceComplete,
                                now,
                            );
                        }
                    }
                }

                if frame.is_none()
                    && matches!(
                        state.phase,
                        ScanPhase::Processing | ScanPhase::Discovering
                    )
                    && state.outstanding_items_stalled(stall_timeout, now)
                {
                    frame = state.handle_state_event(
                        ScanStateEvent::Stalled {
                            reason: "quiescence_timeout".to_string(),
                        },
                        now,
                    );
                }

                let finalize =
                    frame.as_ref().and_then(|queued| match queued.event {
                        ScanEventKind::Completed => {
                            Some(ScanLifecycleStatus::Completed)
                        }
                        ScanEventKind::Failed => {
                            Some(ScanLifecycleStatus::Failed)
                        }
                        _ => None,
                    });

                (frame, finalize)
            }
        };

        if let Some(frame) = maybe_frame {
            let event = frame.event.clone();
            self.emit_frame(frame.event, frame.payload).await;
            if let Some(status) = finalize_status {
                let failed = status == ScanLifecycleStatus::Failed;
                self.finalize_history(status).await;
                if failed {
                    self.schedule_stuck_recovery().await;
                }
            }
            matches!(event, ScanEventKind::Completed)
        } else {
            false
        }
    }

    async fn schedule_stuck_recovery(&self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let recovery_correlation = Uuid::now_v7();
        match inner
            .orchestrator
            .recover_library_scan_state(self.library_id, recovery_correlation)
            .await
        {
            Ok(()) => info!(
                library = %self.library_id,
                scan = %self.scan_id,
                recovery_correlation = %recovery_correlation,
                "scheduled manifest recovery sweep for stuck scan run"
            ),
            Err(err) => warn!(
                library = %self.library_id,
                scan = %self.scan_id,
                recovery_correlation = %recovery_correlation,
                error = %err,
                "failed to schedule manifest recovery sweep for stuck scan run"
            ),
        }
    }

    async fn persist_frame(
        &self,
        event: &ScanEventKind,
        payload: &ScanProgressEvent,
        error: Option<String>,
    ) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };

        let (run_record, update, subject_key, terminal_error) = {
            let state = self.state.lock().await;
            let terminal_error =
                error.clone().or_else(|| state.last_error.clone());
            let terminal_summary = if state.is_terminal() {
                serde_json::json!({
                    "status": state.status.as_str(),
                    "completed_items": state.completed_items,
                    "total_items": state.total_items,
                    "retrying_items": state.retrying_items,
                    "dead_lettered_items": state.dead_lettered_items,
                    "message_code": terminal_error.as_deref().map(|reason| observability_failure_category(Some(reason)).1),
                })
            } else {
                serde_json::json!({})
            };
            let status = lifecycle_to_observability_status(&state.status);
            let run_record = ScanRunRecord {
                id: state.scan_id,
                library_id: state.library_id,
                source: start_mode_to_observability_source(self.start_mode),
                status,
                correlation_id: state.correlation_id,
                idempotency_key: payload.idempotency_key.clone(),
                sequence: 0,
                started_at: state.started_at,
                last_event_at: payload.emitted_at,
                terminal_at: state.terminal_at,
                current_path: payload.current_path.clone(),
                completed_items: state.completed_items.min(i64::MAX as u64)
                    as i64,
                total_items: state.total_items.min(i64::MAX as u64) as i64,
                retrying_items: state.retrying_items.min(i64::MAX as u64)
                    as i64,
                dead_lettered_items: state
                    .dead_lettered_items
                    .min(i64::MAX as u64)
                    as i64,
                terminal_summary: terminal_summary.clone(),
            };
            let update = ScanRunUpdate {
                id: state.scan_id,
                status,
                idempotency_key: payload.idempotency_key.clone(),
                last_event_at: payload.emitted_at,
                terminal_at: state.terminal_at,
                current_path: payload.current_path.clone(),
                completed_items: state.completed_items.min(i64::MAX as u64)
                    as i64,
                total_items: state.total_items.min(i64::MAX as u64) as i64,
                retrying_items: state.retrying_items.min(i64::MAX as u64)
                    as i64,
                dead_lettered_items: state
                    .dead_lettered_items
                    .min(i64::MAX as u64)
                    as i64,
                terminal_summary,
            };
            let subject_key = payload
                .path_key
                .as_ref()
                .map(subject_key_to_string)
                .or_else(|| payload.current_path.clone());
            (run_record, update, subject_key, terminal_error)
        };

        let repo = &inner.unit_of_work.scan_observability;
        if let Err(err) = repo.create_run(&run_record).await {
            warn!(scan = %self.scan_id, error = %err, "failed to create persisted scan run");
            return;
        }
        if let Err(err) = repo.update_run(&update).await {
            warn!(scan = %self.scan_id, error = %err, "failed to update persisted scan run");
        }

        let record = NewScanRunEvent {
            run_id: payload.scan_id,
            library_id: payload.library_id,
            event_kind: event.as_str().to_string(),
            status: payload.status.clone(),
            correlation_id: payload.correlation_id,
            idempotency_key: payload.idempotency_key.clone(),
            subject_key: subject_key.clone(),
            current_path: payload.current_path.clone(),
            occurred_at: payload.emitted_at,
            completed_items: payload.completed_items.min(i64::MAX as u64)
                as i64,
            total_items: payload.total_items.min(i64::MAX as u64) as i64,
            retrying_items: payload.retrying_items.min(i64::MAX as u64) as i64,
            dead_lettered_items: payload.failed_items.min(i64::MAX as u64)
                as i64,
            payload: serde_json::to_value(payload)
                .unwrap_or_else(|_| serde_json::json!({})),
        };
        if let Err(err) = repo.append_event(&record).await {
            warn!(scan = %self.scan_id, error = %err, "failed to append persisted scan event");
        }

        if matches!(event, ScanEventKind::Failed) {
            let reason = terminal_error.or(error);
            let (category, message_code) =
                observability_failure_category(reason.as_deref());
            let failure = ScanRunFailureSummary {
                run_id: payload.scan_id,
                library_id: payload.library_id,
                subject_key: subject_key
                    .unwrap_or_else(|| format!("scan:{}", payload.scan_id)),
                category: category.to_string(),
                message_code: message_code.to_string(),
                raw_debug_details: serde_json::json!({
                    "reason": reason.clone(),
                    "status": payload.status.as_str(),
                    "scan_id": payload.scan_id,
                    "correlation_id": payload.correlation_id,
                }),
                last_error: reason,
                occurrences: 1,
                first_seen_at: payload.emitted_at,
                last_seen_at: payload.emitted_at,
                retryable: false,
                job_id: None,
                idempotency_key: payload.idempotency_key.clone(),
            };
            if let Err(err) = repo.upsert_failure_summary(&failure).await {
                warn!(scan = %self.scan_id, error = %err, "failed to upsert persisted scan failure");
            }
        }
    }

    async fn emit_frame(
        &self,
        event: ScanEventKind,
        payload: ScanProgressEvent,
    ) {
        let frame = ScanBroadcastFrame {
            event: event.clone(),
            payload: payload.clone(),
        };

        {
            let mut history = self.events.lock().await;
            if history.len() == EVENT_HISTORY_CAPACITY {
                history.pop_front();
            }
            history.push_back(frame.clone());
        }

        self.persist_progress_payload(&payload).await;

        let _ = self.tx.send(frame.clone());
        let error = if matches!(event, ScanEventKind::Failed) {
            self.failure_reason().await
        } else {
            None
        };
        self.persist_frame(&event, &payload, error).await;
        self.maybe_log_summary(&event, &payload).await;
    }

    fn progress_pct(completed: u64, dead: u64, total: u64) -> u8 {
        if total == 0 {
            return 0;
        }
        let done = completed.saturating_add(dead);
        let pct = (done as f32 / total as f32) * 100.0;
        pct.floor() as u8
    }

    async fn maybe_log_summary(
        &self,
        event: &ScanEventKind,
        payload: &ScanProgressEvent,
    ) {
        use ScanEventKind::*;
        let mut guard = self.log.lock().await;
        let now = Instant::now();
        let pct = Self::progress_pct(
            payload.completed_items,
            payload.needs_attention_items,
            payload.total_items,
        );

        let force = matches!(event, Started | Completed | Failed);
        let advanced_items = payload
            .completed_items
            .saturating_sub(guard.last_completed_items);
        let advanced_pct = pct.saturating_sub(guard.last_pct);
        let interval_elapsed = now
            .checked_duration_since(guard.last_log_at)
            .unwrap_or_else(|| Duration::from_secs(0))
            >= guard.min_interval;

        let (root_completed, root_total, historical_cursor_count) = {
            let state = self.state.lock().await;
            // Build set of scanned paths
            let mut scanned_paths: HashSet<String> = HashSet::new();
            for item in state.item_states.values() {
                if let Some(p) = &item.path_key
                    && let Some(path) = subject_key_path(p)
                {
                    scanned_paths.insert(path.to_string());
                }
            }
            let is_root = |path: &str| {
                let mut cur = std::path::Path::new(path).parent();
                while let Some(dir) = cur {
                    if let Some(dir_str) = dir.to_str()
                        && scanned_paths.contains(dir_str)
                    {
                        return false;
                    }
                    cur = dir.parent();
                }
                true
            };

            let mut roots_total = 0u64;
            let mut roots_completed = 0u64;
            for item in state.item_states.values() {
                if let Some(p) = &item.path_key
                    && let Some(path) = subject_key_path(p)
                    && is_root(path)
                {
                    roots_total += 1;
                    if item.status.counts_as_completed() {
                        roots_completed += 1;
                    }
                }
            }
            (roots_completed, roots_total, state.historical_cursor_count)
        };

        if force
            || interval_elapsed
            || advanced_items >= guard.item_step
            || advanced_pct >= guard.pct_step
        {
            tracing::info!(
                target: "scan::summary",
                scan = %payload.scan_id,
                library = %payload.library_id,
                status = %payload.status,
                completed = payload.completed_items,
                total = payload.total_items,
                retrying = payload.retrying_items,
                needs_attention = payload.needs_attention_items,
                pct = pct,
                root_completed = root_completed,
                root_total = root_total,
                historical_cursors = historical_cursor_count,
                path = ?payload.current_path,
                "scan progress"
            );

            guard.last_log_at = now;
            guard.last_sequence = payload.sequence;
            guard.last_completed_items = payload.completed_items;
            guard.last_pct = pct;
        }
    }

    async fn persist_progress_payload(&self, payload: &ScanProgressEvent) {
        let Some(status) = repository_status_from_payload(&payload.status)
        else {
            return;
        };
        let Some(inner) = self.inner.upgrade() else {
            return;
        };

        if let Err(err) = inner
            .unit_of_work
            .scan_runs
            .update_progress(LibraryScanRunProgressUpdate {
                scan_id: self.scan_id,
                status: Some(status),
                completed_items: payload.completed_items,
                total_items: payload.total_items,
                retrying_items: payload.retrying_items,
                dead_lettered_items: payload.failed_items,
                current_path: payload.current_path.clone(),
                sequence: payload.sequence,
            })
            .await
        {
            warn!(
                scan = %self.scan_id,
                library = %self.library_id,
                error = %err,
                "failed to persist scan progress"
            );
        }
    }

    async fn failure_reason(&self) -> Option<String> {
        let state = self.state.lock().await;
        state.last_error.clone()
    }

    async fn finalize_history(&self, terminal: ScanLifecycleStatus) {
        let (snapshot, progress, terminal_at, last_error) = {
            let state = self.state.lock().await;
            let counters = state.counter_snapshot();
            let completed_items = counters.completed_items;
            let retrying_items = counters.retrying_items;
            let failed_items = counters.failed_items;
            let terminal_at = state.terminal_at.unwrap_or_else(Utc::now);
            (
                ScanHistoryEntry {
                    scan_id: state.scan_id,
                    library_id: state.library_id,
                    status: terminal.clone(),
                    completed_items,
                    total_items: state.total_items,
                    validated_items: counters.validated_items,
                    known_unchanged_items: counters.known_unchanged_items,
                    skipped_items: counters.skipped_items,
                    failed_items,
                    needs_attention_items: counters.needs_attention_items,
                    retrying_items,
                    started_at: state.started_at,
                    terminal_at,
                    reason_details: counters.reason_details,
                },
                LibraryScanRunProgressUpdate {
                    scan_id: state.scan_id,
                    status: None,
                    completed_items,
                    total_items: state.total_items,
                    retrying_items,
                    dead_lettered_items: failed_items,
                    current_path: state.current_path.clone(),
                    sequence: state.event_sequence,
                },
                terminal_at,
                state.last_error.clone(),
            )
        };
        let final_events = self.event_log().await;

        if let Some(inner) = self.inner.upgrade() {
            if let Err(err) =
                inner.unit_of_work.scan_runs.update_progress(progress).await
            {
                warn!(
                    scan = %self.scan_id,
                    status = ?terminal,
                    error = %err,
                    "failed to persist final scan progress; keeping run active"
                );
                return;
            }

            if let Err(err) = inner
                .unit_of_work
                .scan_runs
                .mark_terminal(
                    self.scan_id,
                    terminal.clone().into(),
                    terminal_at,
                    last_error,
                )
                .await
            {
                warn!(
                    scan = %self.scan_id,
                    status = ?terminal,
                    error = %err,
                    "failed to persist terminal scan state; keeping run active"
                );
                return;
            }

            inner
                .finalize_run(
                    self.scan_id,
                    self.run_key(),
                    self.correlation_id,
                    snapshot,
                    final_events,
                )
                .await;
        }

        warn!(scan = %self.scan_id, status = ?terminal, "finalized scan run");
    }
}

#[derive(Clone, Debug)]
struct ScanLogWatermark {
    last_log_at: Instant,
    last_sequence: u64,
    last_completed_items: u64,
    last_pct: u8,
    min_interval: Duration,
    item_step: u64,
    pct_step: u8,
}

impl Default for ScanLogWatermark {
    fn default() -> Self {
        Self {
            last_log_at: Instant::now(),
            last_sequence: 0,
            last_completed_items: 0,
            last_pct: 0,
            min_interval: Duration::from_secs(5),
            item_step: 25,
            pct_step: 10,
        }
    }
}

impl ScanRunState {
    fn is_terminal(&self) -> bool {
        self.phase.is_terminal()
            || matches!(
                self.status,
                ScanLifecycleStatus::Completed
                    | ScanLifecycleStatus::Failed
                    | ScanLifecycleStatus::Canceled
            )
    }

    fn counter_snapshot(&self) -> ScanCounterSnapshot {
        let mut counters = ScanCounterSnapshot::default();
        for item in self.item_states.values() {
            match item.status {
                ScanItemStatus::Completed => counters.validated_items += 1,
                ScanItemStatus::KnownUnchanged => {
                    counters.known_unchanged_items += 1;
                }
                ScanItemStatus::Skipped => counters.skipped_items += 1,
                ScanItemStatus::DeadLettered => counters.failed_items += 1,
                ScanItemStatus::Retrying => counters.retrying_items += 1,
                ScanItemStatus::InProgress => {}
            }

            if let Some(detail) = self.reason_detail_for_item(item) {
                counters.reason_details.push(detail);
            }
        }

        counters.needs_attention_items = counters.failed_items;
        counters.completed_items = counters
            .validated_items
            .saturating_add(counters.known_unchanged_items)
            .saturating_add(counters.skipped_items);
        counters
    }

    fn reason_detail_for_item(
        &self,
        item: &ScanItemState,
    ) -> Option<ScanPathReasonDetail> {
        let path = item.path_key.as_ref().and_then(subject_key_path_owned);
        let outcome = path
            .as_deref()
            .and_then(|path| self.folder_outcomes_by_path.get(path))
            .copied();

        match item.status {
            ScanItemStatus::KnownUnchanged => None,
            ScanItemStatus::Skipped => {
                let reason_code = skipped_reason_code(outcome);
                Some(ScanPathReasonDetail {
                    category: ScanPathReasonCategory::Skipped,
                    message: Some(reason_message(reason_code).to_string()),
                    reason_code: reason_code.to_string(),
                    path,
                    path_key: item.path_key.clone(),
                    retryable: false,
                    action_hint: Some("rescan_library".to_string()),
                })
            }
            ScanItemStatus::Retrying => {
                let reason_code = item
                    .last_error
                    .as_deref()
                    .map(user_safe_reason_code)
                    .unwrap_or_else(|| "temporary_scan_issue".to_string());
                Some(ScanPathReasonDetail {
                    category: ScanPathReasonCategory::Retrying,
                    message: Some(reason_message(&reason_code).to_string()),
                    reason_code,
                    path,
                    path_key: item.path_key.clone(),
                    retryable: true,
                    action_hint: Some("wait_for_retry".to_string()),
                })
            }
            ScanItemStatus::DeadLettered => {
                let reason_code = item
                    .last_error
                    .as_deref()
                    .map(user_safe_reason_code)
                    .unwrap_or_else(|| "needs_attention".to_string());
                Some(ScanPathReasonDetail {
                    category: ScanPathReasonCategory::NeedsAttention,
                    message: Some(reason_message(&reason_code).to_string()),
                    reason_code,
                    path,
                    path_key: item.path_key.clone(),
                    retryable: false,
                    action_hint: Some("rescan_library".to_string()),
                })
            }
            ScanItemStatus::Completed | ScanItemStatus::InProgress => None,
        }
    }

    fn rehydrate_from_cursors(&mut self, cursors: &[ScanCursor]) {
        self.historical_cursor_count = self
            .historical_cursor_count
            .saturating_add(cursors.len() as u64);
    }

    fn handle_state_event(
        &mut self,
        event: ScanStateEvent,
        now: DateTime<Utc>,
    ) -> Option<QueuedFrame> {
        match event {
            ScanStateEvent::RunStarted => {
                if self.can_transition_to(ScanPhase::Discovering) {
                    self.transition(ScanPhase::Discovering, now)
                } else {
                    None
                }
            }
            ScanStateEvent::NewItemFound => {
                if self.can_transition_to(ScanPhase::Discovering) {
                    self.transition(ScanPhase::Discovering, now);
                }
                if self.can_transition_to(ScanPhase::Processing) {
                    self.transition(ScanPhase::Processing, now)
                } else {
                    None
                }
            }
            ScanStateEvent::AllItemsProcessed => {
                if self.can_transition_to(ScanPhase::Quiescing) {
                    self.transition(ScanPhase::Quiescing, now)
                } else {
                    None
                }
            }
            ScanStateEvent::QuiescenceComplete => {
                if self.can_transition_to(ScanPhase::Completed) {
                    self.transition(ScanPhase::Completed, now)
                } else {
                    None
                }
            }
            ScanStateEvent::Stalled { reason } => {
                if self.can_transition_to(ScanPhase::Failed) {
                    self.last_error = Some(reason);
                    self.transition(ScanPhase::Failed, now)
                } else {
                    None
                }
            }
        }
    }

    fn can_transition_to(&self, next: ScanPhase) -> bool {
        if self.phase == next {
            return false;
        }
        if self.phase.is_terminal() {
            return false;
        }

        match next {
            ScanPhase::Initializing => false,
            ScanPhase::Discovering => {
                matches!(self.phase, ScanPhase::Initializing)
            }
            ScanPhase::Processing => {
                matches!(
                    self.phase,
                    ScanPhase::Discovering | ScanPhase::Quiescing
                )
            }
            ScanPhase::Quiescing => {
                matches!(
                    self.phase,
                    ScanPhase::Processing | ScanPhase::Discovering
                ) && self.can_enter_quiescing()
            }
            ScanPhase::Completed => {
                let all_items_terminal =
                    matches!(self.phase, ScanPhase::Quiescing)
                        && self.completed_items + self.dead_lettered_items
                            == self.total_items;
                let no_work_seed_completed = self.seed_completed
                    && self.total_items == 0
                    && matches!(
                        self.phase,
                        ScanPhase::Initializing
                            | ScanPhase::Discovering
                            | ScanPhase::Processing
                    );
                all_items_terminal || no_work_seed_completed
            }
            ScanPhase::Failed | ScanPhase::Canceled => {
                !self.phase.is_terminal()
            }
        }
    }

    fn transition(
        &mut self,
        next: ScanPhase,
        now: DateTime<Utc>,
    ) -> Option<QueuedFrame> {
        if !self.can_transition_to(next) {
            return None;
        }

        self.phase = next;

        match next {
            ScanPhase::Discovering | ScanPhase::Processing => {
                if self.status != ScanLifecycleStatus::Paused {
                    self.status = ScanLifecycleStatus::Running;
                }
                if matches!(next, ScanPhase::Processing) {
                    self.quiescence_started_at = None;
                }
                None
            }
            ScanPhase::Quiescing => {
                self.status = ScanLifecycleStatus::Running;
                self.quiescence_started_at = Some(now);
                Some(QueuedFrame {
                    event: ScanEventKind::Quiescing,
                    payload: self.build_payload(),
                })
            }
            ScanPhase::Completed => {
                self.status = ScanLifecycleStatus::Completed;
                self.terminal_at = Some(now);
                self.quiescence_started_at = None;
                Some(QueuedFrame {
                    event: ScanEventKind::Completed,
                    payload: self.build_payload(),
                })
            }
            ScanPhase::Failed => {
                self.status = ScanLifecycleStatus::Failed;
                self.terminal_at = Some(now);
                self.quiescence_started_at = None;
                Some(QueuedFrame {
                    event: ScanEventKind::Failed,
                    payload: self.build_payload(),
                })
            }
            ScanPhase::Canceled => {
                self.status = ScanLifecycleStatus::Canceled;
                self.terminal_at = Some(now);
                self.quiescence_started_at = None;
                Some(QueuedFrame {
                    event: ScanEventKind::Failed,
                    payload: self.build_payload(),
                })
            }
            ScanPhase::Initializing => None,
        }
    }

    fn mark_seed_completed(
        &mut self,
        queued_folders: usize,
        completed_at: DateTime<Utc>,
    ) -> Option<QueuedFrame> {
        self.seed_completed = true;
        if queued_folders == 0 && self.total_items == 0 {
            self.last_activity_at = Some(completed_at);
            self.transition(ScanPhase::Completed, completed_at)
        } else {
            None
        }
    }

    fn status_for_folder_outcome(outcome: FolderScanOutcome) -> ScanItemStatus {
        match outcome {
            FolderScanOutcome::Changed => ScanItemStatus::Completed,
            FolderScanOutcome::UnchangedCursor => {
                ScanItemStatus::KnownUnchanged
            }
            FolderScanOutcome::Missing
            | FolderScanOutcome::Empty
            | FolderScanOutcome::Unsupported => ScanItemStatus::Skipped,
        }
    }

    fn remember_folder_outcome(
        &mut self,
        path: &str,
        outcome: FolderScanOutcome,
        now: DateTime<Utc>,
    ) -> bool {
        self.folder_outcomes_by_path
            .insert(path.to_string(), outcome);

        let target_status = Self::status_for_folder_outcome(outcome);
        if target_status == ScanItemStatus::Completed {
            return false;
        }

        let matching_items: Vec<String> = self
            .item_states
            .iter()
            .filter_map(|(idempotency, item)| {
                let item_path =
                    item.path_key.as_ref().and_then(subject_key_path)?;
                (item_path == path
                    && matches!(
                        item.status,
                        ScanItemStatus::Completed
                            | ScanItemStatus::KnownUnchanged
                            | ScanItemStatus::Skipped
                    ))
                .then(|| idempotency.clone())
            })
            .collect();

        let mut changed = false;
        for idempotency in matching_items {
            changed |= self.update_item_status(
                &idempotency,
                None,
                target_status,
                now,
                SubjectKey::path(path.to_string()).ok(),
                None,
            );
        }
        changed
    }

    fn reset_quiescence_after_activity(&mut self, now: DateTime<Utc>) -> bool {
        let (Some(started_at), Some(last_activity_at)) =
            (self.quiescence_started_at, self.last_activity_at)
        else {
            return false;
        };

        if last_activity_at > started_at {
            self.quiescence_started_at = Some(now);
            true
        } else {
            false
        }
    }

    fn build_payload(&mut self) -> ScanProgressEvent {
        self.event_sequence += 1;
        let idempotency_key =
            format!("{}{}", self.idempotency_prefix, self.event_sequence);
        self.last_idempotency_key = idempotency_key.clone();
        self.build_payload_with_idempotency_key(idempotency_key)
    }

    fn build_current_payload(&self) -> ScanProgressEvent {
        self.build_payload_with_idempotency_key(self.current_idempotency_key())
    }

    fn build_payload_with_idempotency_key(
        &self,
        idempotency_key: String,
    ) -> ScanProgressEvent {
        let counters = self.counter_snapshot();
        ScanProgressEvent {
            version: EVENT_VERSION.to_string(),
            scan_id: self.scan_id,
            library_id: self.library_id,
            status: self.status_string(),
            completed_items: counters.completed_items,
            total_items: self.total_items,
            validated_items: counters.validated_items,
            known_unchanged_items: counters.known_unchanged_items,
            skipped_items: counters.skipped_items,
            failed_items: counters.failed_items,
            needs_attention_items: counters.needs_attention_items,
            retrying_items: counters.retrying_items,
            sequence: self.event_sequence,
            current_path: self.current_path.clone(),
            path_key: self.path_key.clone(),
            p95_stage_latencies_ms: DEFAULT_LATENCIES,
            correlation_id: self.correlation_id,
            idempotency_key,
            emitted_at: Utc::now(),
            terminal_at: self.terminal_at,
            reason_details: counters.reason_details,
        }
    }

    fn build_payload_if(
        &mut self,
        condition: bool,
    ) -> Option<ScanProgressEvent> {
        condition.then(|| self.build_payload())
    }

    fn status_string(&self) -> String {
        if matches!(self.status, ScanLifecycleStatus::Running) {
            self.phase.status().to_string()
        } else {
            self.status.as_str().to_string()
        }
    }

    fn current_idempotency_key(&self) -> String {
        if self.last_idempotency_key.is_empty() {
            format!("{}{}", self.idempotency_prefix, self.event_sequence)
        } else {
            self.last_idempotency_key.clone()
        }
    }

    fn update_item_status(
        &mut self,
        idempotency_key: &str,
        job_id: Option<JobId>,
        status: ScanItemStatus,
        event_time: DateTime<Utc>,
        path_key: Option<SubjectKey>,
        error: Option<String>,
    ) -> bool {
        match self.item_states.entry(idempotency_key.to_string()) {
            Entry::Vacant(slot) => {
                self.total_items += 1;
                if status.counts_as_completed() {
                    self.completed_items += 1;
                } else {
                    match status {
                        ScanItemStatus::DeadLettered => {
                            self.dead_lettered_items += 1
                        }
                        ScanItemStatus::Retrying => self.retrying_items += 1,
                        _ => {}
                    }
                }
                slot.insert(ScanItemState {
                    status,
                    last_activity: event_time,
                    path_key: path_key.clone(),
                    last_error: error,
                    last_job_id: job_id,
                });
                true
            }
            Entry::Occupied(mut slot) => {
                let item = slot.get_mut();
                let old_status = item.status;

                // Refuse retrograde transitions: once terminal, never go back to active.
                if old_status.is_terminal() && !status.is_terminal() {
                    tracing::debug!(
                        target: "scan::state",
                        scan = %self.scan_id,
                        library = %self.library_id,
                        idempotency = idempotency_key,
                        from = ?old_status,
                        to = ?status,
                        "ignoring out-of-order retrograde status"
                    );
                    // Refresh liveness fields only
                    item.last_activity = event_time;
                    if let Some(path) = path_key.clone() {
                        item.path_key = Some(path);
                    }
                    if let Some(job) = job_id {
                        item.last_job_id = Some(job);
                    }
                    if let Some(err) = error {
                        item.last_error = Some(err);
                    }
                    return false;
                }
                if matches!(old_status, ScanItemStatus::DeadLettered)
                    && !matches!(status, ScanItemStatus::DeadLettered)
                {
                    return false;
                }
                if old_status == status {
                    item.last_activity = event_time;
                    if let Some(path) = path_key {
                        item.path_key = Some(path);
                    }
                    if let Some(err) = error {
                        item.last_error = Some(err);
                    } else if status.counts_as_completed()
                        || matches!(status, ScanItemStatus::InProgress)
                    {
                        item.last_error = None;
                    }
                    if let Some(job) = job_id {
                        item.last_job_id = Some(job);
                    }
                    return false;
                }

                if old_status.counts_as_completed() {
                    self.completed_items =
                        self.completed_items.saturating_sub(1);
                } else {
                    match old_status {
                        ScanItemStatus::DeadLettered => {
                            self.dead_lettered_items =
                                self.dead_lettered_items.saturating_sub(1);
                        }
                        ScanItemStatus::Retrying => {
                            self.retrying_items =
                                self.retrying_items.saturating_sub(1);
                        }
                        _ => {}
                    }
                }

                if status.counts_as_completed() {
                    self.completed_items += 1;
                } else {
                    match status {
                        ScanItemStatus::DeadLettered => {
                            self.dead_lettered_items += 1
                        }
                        ScanItemStatus::Retrying => self.retrying_items += 1,
                        _ => {}
                    }
                }

                item.status = status;
                item.last_activity = event_time;
                if let Some(path) = path_key {
                    item.path_key = Some(path);
                }
                match error {
                    Some(err) => item.last_error = Some(err),
                    None => {
                        if status.counts_as_completed()
                            || matches!(status, ScanItemStatus::InProgress)
                        {
                            item.last_error = None;
                        }
                    }
                }
                if let Some(job) = job_id {
                    item.last_job_id = Some(job);
                }
                true
            }
        }
    }

    fn can_enter_quiescing(&self) -> bool {
        self.total_items > 0
            && self.completed_items + self.dead_lettered_items
                == self.total_items
    }

    fn outstanding_items_stalled(
        &self,
        stall_timeout: ChronoDuration,
        now: DateTime<Utc>,
    ) -> bool {
        if self.retrying_items > 0 {
            return false;
        }

        let mut saw_active = false;
        for item in self.item_states.values() {
            if !item.is_active() {
                continue;
            }
            if matches!(item.status, ScanItemStatus::Retrying) {
                return false;
            }

            saw_active = true;
            if now - item.last_activity <= stall_timeout {
                return false;
            }
        }
        saw_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_core::domain::scan::orchestration::scan_cursor::ScanCursorId;
    use std::{collections::HashMap, path::PathBuf};

    fn test_state() -> ScanRunState {
        let scan_id = Uuid::now_v7();
        let library_id = LibraryId::new();
        ScanRunState {
            scan_id,
            library_id,
            phase: ScanPhase::Initializing,
            status: ScanLifecycleStatus::Pending,
            completed_items: 0,
            total_items: 0,
            dead_lettered_items: 0,
            retrying_items: 0,
            current_path: None,
            path_key: None,
            correlation_id: scan_id,
            idempotency_prefix: format!("scan:{}:", scan_id),
            event_sequence: 0,
            last_idempotency_key: String::new(),
            started_at: Utc::now(),
            terminal_at: None,
            last_activity_at: None,
            quiescence_started_at: None,
            last_error: None,
            item_states: HashMap::new(),
            folder_outcomes_by_path: HashMap::new(),
            historical_cursor_count: 0,
            seed_completed: false,
        }
    }

    #[test]
    fn persisted_cursors_are_informational_only() {
        let mut state = test_state();
        let folder = PathBuf::from("/library/Movie");
        let cursor = ScanCursor {
            id: ScanCursorId::new(state.library_id, &vec![folder.clone()]),
            folder_path_norm: folder.to_string_lossy().to_string(),
            listing_hash: "stable".into(),
            entry_count: 1,
            last_scan_at: Utc::now(),
            last_modified_at: None,
            device_id: None,
        };

        state.rehydrate_from_cursors(&[cursor]);

        assert_eq!(state.historical_cursor_count, 1);
        assert_eq!(state.total_items, 0);
        assert_eq!(state.completed_items, 0);
        assert!(state.item_states.is_empty());
    }

    #[test]
    fn folder_outcomes_preserve_known_and_skipped_states() {
        let mut state = test_state();
        let now = Utc::now();
        let unchanged_path = "/library/Stable Movie";
        let unsupported_path = "/library/Extras Only";

        state.update_item_status(
            "unchanged-job",
            Some(JobId::new()),
            ScanItemStatus::InProgress,
            now,
            SubjectKey::path(unchanged_path.to_string()).ok(),
            None,
        );
        state.remember_folder_outcome(
            unchanged_path,
            FolderScanOutcome::UnchangedCursor,
            now,
        );
        assert!(matches!(
            state.item_states["unchanged-job"].status,
            ScanItemStatus::InProgress
        ));

        let unchanged_status = state
            .folder_outcomes_by_path
            .get(unchanged_path)
            .copied()
            .map(ScanRunState::status_for_folder_outcome)
            .unwrap();
        state.update_item_status(
            "unchanged-job",
            Some(JobId::new()),
            unchanged_status,
            now,
            SubjectKey::path(unchanged_path.to_string()).ok(),
            None,
        );
        assert!(matches!(
            state.item_states["unchanged-job"].status,
            ScanItemStatus::KnownUnchanged
        ));

        state.update_item_status(
            "unsupported-job",
            Some(JobId::new()),
            ScanRunState::status_for_folder_outcome(
                FolderScanOutcome::Unsupported,
            ),
            now,
            SubjectKey::path(unsupported_path.to_string()).ok(),
            None,
        );
        assert!(matches!(
            state.item_states["unsupported-job"].status,
            ScanItemStatus::Skipped
        ));
        assert_eq!(state.total_items, 2);
        assert_eq!(state.completed_items, 2);
        assert_eq!(state.dead_lettered_items, 0);
    }

    #[test]
    fn no_work_seed_completes_without_items_once() {
        let mut state = test_state();
        let now = Utc::now();
        state.handle_state_event(ScanStateEvent::RunStarted, now);

        let frame = state
            .mark_seed_completed(0, now + ChronoDuration::milliseconds(1))
            .expect("seed completion should emit terminal frame");
        assert!(matches!(frame.event, ScanEventKind::Completed));
        assert_eq!(state.status, ScanLifecycleStatus::Completed);
        assert_eq!(state.total_items, 0);
        assert_eq!(state.completed_items, 0);

        let duplicate =
            state.mark_seed_completed(0, now + ChronoDuration::milliseconds(2));
        assert!(duplicate.is_none());
    }

    #[test]
    fn retryable_failures_do_not_quiesce_but_non_retryable_failures_do() {
        let now = Utc::now();
        let mut retrying = test_state();
        retrying.update_item_status(
            "retrying-job",
            Some(JobId::new()),
            ScanItemStatus::Retrying,
            now,
            SubjectKey::path("/library/retry".to_string()).ok(),
            Some("transient".into()),
        );
        assert_eq!(retrying.retrying_items, 1);
        assert!(!retrying.can_enter_quiescing());

        let mut dead = test_state();
        dead.update_item_status(
            "dead-job",
            Some(JobId::new()),
            ScanItemStatus::DeadLettered,
            now,
            SubjectKey::path("/library/dead".to_string()).ok(),
            Some("permanent".into()),
        );
        assert_eq!(dead.dead_lettered_items, 1);
        assert!(dead.can_enter_quiescing());
    }

    #[test]
    fn downstream_jobs_are_scan_progress_items() {
        for kind in JobKind::all_kinds() {
            assert!(
                ScanRunAggregatorInner::tracks_scan_progress_kind(*kind),
                "{kind:?} should keep an active scan run open"
            );
        }
    }

    #[test]
    fn downstream_in_progress_items_block_quiescence() {
        let now = Utc::now();
        let mut state = test_state();
        state.update_item_status(
            "folder-job",
            Some(JobId::new()),
            ScanItemStatus::Completed,
            now,
            SubjectKey::path("/library/Movie".to_string()).ok(),
            None,
        );
        assert!(state.can_enter_quiescing());

        state.update_item_status(
            "metadata-job",
            Some(JobId::new()),
            ScanItemStatus::InProgress,
            now + ChronoDuration::milliseconds(1),
            SubjectKey::path("/library/Movie/feature.mkv".to_string()).ok(),
            None,
        );
        assert!(!state.can_enter_quiescing());
        assert_eq!(state.total_items, 2);
        assert_eq!(state.completed_items, 1);

        state.update_item_status(
            "metadata-job",
            Some(JobId::new()),
            ScanItemStatus::DeadLettered,
            now + ChronoDuration::milliseconds(2),
            SubjectKey::path("/library/Movie/feature.mkv".to_string()).ok(),
            Some("Movie match not found".into()),
        );
        assert!(state.can_enter_quiescing());
        assert_eq!(state.dead_lettered_items, 1);
    }

    #[test]
    fn folder_failures_keep_retrying_separate_from_needs_attention() {
        let now = Utc::now();
        let mut state = test_state();
        state.update_item_status(
            "retrying-job",
            Some(JobId::new()),
            ScanItemStatus::Retrying,
            now,
            SubjectKey::path("/library/retrying".to_string()).ok(),
            Some("temporary scan issue".to_string()),
        );
        state.update_item_status(
            "attention-job",
            Some(JobId::new()),
            ScanItemStatus::DeadLettered,
            now,
            SubjectKey::path("/library/attention".to_string()).ok(),
            Some("dead_lettered_queue".to_string()),
        );

        let payload = state.build_payload();

        assert_eq!(payload.retrying_items, 1);
        assert_eq!(payload.failed_items, 1);
        assert_eq!(payload.needs_attention_items, 1);
        assert_eq!(payload.completed_items, 0);

        let retrying = payload
            .reason_details
            .iter()
            .find(|detail| detail.category == ScanPathReasonCategory::Retrying)
            .expect("retrying reason detail");
        assert!(retrying.retryable);
        assert_eq!(retrying.action_hint.as_deref(), Some("wait_for_retry"));
        assert!(!retrying.reason_code.contains("dead"));

        let attention = payload
            .reason_details
            .iter()
            .find(|detail| {
                detail.category == ScanPathReasonCategory::NeedsAttention
            })
            .expect("needs-attention reason detail");
        assert!(!attention.retryable);
        assert_eq!(attention.action_hint.as_deref(), Some("rescan_library"));
        assert_eq!(attention.reason_code, "needs_attention");
        assert!(
            payload
                .reason_details
                .iter()
                .all(|detail| !detail.reason_code.contains("dead"))
        );
    }

    #[test]
    fn progress_payload_exposes_user_safe_counters_and_reasons() {
        let now = Utc::now();
        let mut state = test_state();
        state.update_item_status(
            "validated-job",
            Some(JobId::new()),
            ScanItemStatus::Completed,
            now,
            SubjectKey::path("/library/validated".to_string()).ok(),
            None,
        );
        state.update_item_status(
            "unchanged-job",
            Some(JobId::new()),
            ScanItemStatus::KnownUnchanged,
            now,
            SubjectKey::path("/library/unchanged".to_string()).ok(),
            None,
        );
        state.folder_outcomes_by_path.insert(
            "/library/skipped".to_string(),
            FolderScanOutcome::Unsupported,
        );
        state.update_item_status(
            "skipped-job",
            Some(JobId::new()),
            ScanItemStatus::Skipped,
            now,
            SubjectKey::path("/library/skipped".to_string()).ok(),
            None,
        );
        state.update_item_status(
            "retrying-job",
            Some(JobId::new()),
            ScanItemStatus::Retrying,
            now,
            SubjectKey::path("/library/retrying".to_string()).ok(),
            Some("Permission denied".to_string()),
        );
        state.update_item_status(
            "attention-job",
            Some(JobId::new()),
            ScanItemStatus::DeadLettered,
            now,
            SubjectKey::path("/library/attention".to_string()).ok(),
            Some("dead_lettered_queue".to_string()),
        );

        let payload = state.build_payload();

        assert_eq!(payload.completed_items, 3);
        assert_eq!(payload.validated_items, 1);
        assert_eq!(payload.known_unchanged_items, 1);
        assert_eq!(payload.skipped_items, 1);
        assert_eq!(payload.retrying_items, 1);
        assert_eq!(payload.failed_items, 1);
        assert_eq!(payload.needs_attention_items, 1);
        assert!(
            payload
                .reason_details
                .iter()
                .any(|detail| detail.reason_code == "unsupported_media_layout")
        );
        assert!(
            payload
                .reason_details
                .iter()
                .any(|detail| detail.reason_code == "permission_denied")
        );
        assert!(
            payload
                .reason_details
                .iter()
                .any(|detail| detail.reason_code == "needs_attention")
        );
        assert!(
            payload
                .reason_details
                .iter()
                .all(|detail| !detail.reason_code.contains("dead"))
        );
    }

    #[test]
    fn quiescence_resets_after_reconciliation_activity() {
        let mut state = test_state();
        let started = Utc::now();
        let activity = started + ChronoDuration::milliseconds(5);
        let reset_at = activity + ChronoDuration::milliseconds(5);
        state.phase = ScanPhase::Quiescing;
        state.status = ScanLifecycleStatus::Running;
        state.total_items = 1;
        state.completed_items = 1;
        state.quiescence_started_at = Some(started);
        state.last_activity_at = Some(activity);

        assert!(state.reset_quiescence_after_activity(reset_at));
        assert_eq!(state.quiescence_started_at, Some(reset_at));
        assert!(!state.reset_quiescence_after_activity(
            reset_at + ChronoDuration::milliseconds(1)
        ));
    }

    #[test]
    fn retrograde_activity_after_quiescence_does_not_demote_terminal_item() {
        let mut state = test_state();
        let now = Utc::now();
        let path = SubjectKey::path("/library/stable".to_string()).ok();

        state.handle_state_event(ScanStateEvent::RunStarted, now);
        state.update_item_status(
            "stable-job",
            Some(JobId::new()),
            ScanItemStatus::Completed,
            now + ChronoDuration::milliseconds(1),
            path.clone(),
            None,
        );
        let quiescing = state.handle_state_event(
            ScanStateEvent::AllItemsProcessed,
            now + ChronoDuration::milliseconds(2),
        );
        assert!(matches!(
            quiescing.map(|frame| frame.event),
            Some(ScanEventKind::Quiescing)
        ));

        let demoted = state.update_item_status(
            "stable-job",
            Some(JobId::new()),
            ScanItemStatus::InProgress,
            now + ChronoDuration::milliseconds(3),
            path,
            None,
        );

        assert!(!demoted);
        assert!(matches!(
            state.item_states["stable-job"].status,
            ScanItemStatus::Completed
        ));
        assert_eq!(state.completed_items, 1);
        assert_eq!(state.total_items, 1);
        assert_eq!(state.dead_lettered_items, 0);
        assert_eq!(state.status, ScanLifecycleStatus::Running);
        assert!(!state.reset_quiescence_after_activity(
            now + ChronoDuration::milliseconds(4)
        ));

        let completed = state.handle_state_event(
            ScanStateEvent::QuiescenceComplete,
            now + ChronoDuration::milliseconds(5),
        );
        assert!(matches!(
            completed.map(|frame| frame.event),
            Some(ScanEventKind::Completed)
        ));
        assert_eq!(state.status, ScanLifecycleStatus::Completed);
    }

    #[test]
    fn terminal_completion_transition_emits_once() {
        let mut state = test_state();
        let now = Utc::now();
        state.phase = ScanPhase::Quiescing;
        state.status = ScanLifecycleStatus::Running;
        state.total_items = 1;
        state.completed_items = 1;

        let first =
            state.handle_state_event(ScanStateEvent::QuiescenceComplete, now);
        assert!(matches!(
            first.map(|frame| frame.event),
            Some(ScanEventKind::Completed)
        ));

        let second = state.handle_state_event(
            ScanStateEvent::QuiescenceComplete,
            now + ChronoDuration::milliseconds(1),
        );
        assert!(second.is_none());
    }
}

#[derive(Clone)]
struct ScanRunAggregator {
    inner: Arc<ScanRunAggregatorInner>,
}

struct ScanRunAggregatorInner {
    orchestrator: Arc<ScanOrchestrator>,
    runs: RwLock<HashMap<Uuid, Arc<ScanRun>>>,
    quiescence_chrono: ChronoDuration,
    stall_timeout: ChronoDuration,
    catalog_bus: Arc<CatalogEventBus>,
    unit_of_work: Arc<AppUnitOfWork>,
    seen_media: Mutex<HashSet<Uuid>>,
    series_bundles: Mutex<HashMap<LibraryId, SeriesBundleTrackerEntry>>,
}

#[derive(Debug)]
struct SeriesBundleTrackerEntry {
    tracker: SeriesBundleTracker,
    last_touched_at: Instant,
    last_polled_at: Instant,
}

impl SeriesBundleTrackerEntry {
    fn new(now: Instant) -> Self {
        Self {
            tracker: SeriesBundleTracker::default(),
            last_touched_at: now,
            last_polled_at: now,
        }
    }

    fn touch(&mut self, now: Instant) {
        self.last_touched_at = now;
    }
}

impl ScanRunAggregator {
    fn new(
        orchestrator: Arc<ScanOrchestrator>,
        quiescence: Duration,
        catalog_bus: Arc<CatalogEventBus>,
        unit_of_work: Arc<AppUnitOfWork>,
    ) -> Self {
        let chrono_window = ChronoDuration::from_std(quiescence)
            .unwrap_or_else(|_| ChronoDuration::seconds(3));
        let stall_std = quiescence
            .checked_mul(STALLED_SCAN_TIMEOUT_MULTIPLIER)
            .unwrap_or(Duration::from_secs(60));
        let stall_window = ChronoDuration::from_std(stall_std)
            .unwrap_or_else(|_| ChronoDuration::seconds(60));
        let inner = Arc::new(ScanRunAggregatorInner {
            orchestrator,
            runs: RwLock::new(HashMap::new()),
            quiescence_chrono: chrono_window,
            stall_timeout: stall_window,
            catalog_bus,
            unit_of_work,
            seen_media: Mutex::new(HashSet::new()),
            series_bundles: Mutex::new(HashMap::new()),
        });

        let aggregator = Self {
            inner: inner.clone(),
        };
        aggregator.spawn_worker();
        aggregator
    }

    fn spawn_worker(&self) {
        let inner = Arc::clone(&self.inner);
        spawn(async move {
            ScanRunAggregatorInner::run(inner).await;
        });
    }

    async fn register(&self, run: Arc<ScanRun>) {
        let mut guard = self.inner.runs.write().await;
        guard.insert(run.correlation_id(), run);
    }

    async fn drop(&self, correlation_id: &Uuid) {
        let mut guard = self.inner.runs.write().await;
        guard.remove(correlation_id);
    }
}

impl ScanRunAggregatorInner {
    async fn run(self: Arc<Self>) {
        use tokio::sync::broadcast::error::RecvError;

        let mut receiver = self.orchestrator.subscribe_job_events();
        let mut domain_rx = self.orchestrator.subscribe_scan_events();
        let mut ticker = interval(Duration::from_millis(500));

        loop {
            tokio::select! {
                biased;
                result = receiver.recv() => {
                    match result {
                        Ok(event) => self.handle_job_event(event).await,
                        Err(RecvError::Lagged(skipped)) => {
                            warn!("scan aggregator lagged {skipped} events");
                        }
                        Err(RecvError::Closed) => break,
                    }
                }
                result = domain_rx.recv() => {
                    match result {
                        Ok(event) => self.handle_scan_event(event).await,
                        Err(RecvError::Lagged(skipped)) => {
                            warn!("domain event stream lagged {skipped} events");
                        }
                        Err(RecvError::Closed) => break,
                    }
                }
                _ = ticker.tick() => {
                    self.check_quiescence().await;
                }
            }
        }
    }

    async fn check_quiescence(&self) {
        let runs: Vec<Arc<ScanRun>> = {
            let guard = self.runs.read().await;
            guard.values().cloned().collect()
        };

        for run in runs {
            if run
                .try_complete(self.quiescence_chrono, self.stall_timeout)
                .await
            {
                self.on_run_completed(run.clone()).await;
            }
        }

        self.poll_series_bundle_finalizations().await;
        self.cleanup_series_bundle_trackers().await;
    }

    async fn poll_series_bundle_finalizations(&self) {
        let now = Instant::now();

        let poll_libraries: Vec<LibraryId> = {
            let mut guard = self.series_bundles.lock().await;
            let mut out = Vec::new();

            for (library_id, entry) in guard.iter_mut() {
                if now.duration_since(entry.last_polled_at)
                    < SERIES_BUNDLE_POLL_INTERVAL
                {
                    continue;
                }

                if entry.tracker.finalization_candidates().is_empty() {
                    continue;
                }

                entry.last_polled_at = now;
                out.push(*library_id);
            }

            out
        };

        for library_id in poll_libraries {
            self.try_emit_series_bundle_finalized(library_id).await;
        }
    }

    async fn cleanup_series_bundle_trackers(&self) {
        let now = Instant::now();

        let active_libraries: HashSet<LibraryId> = {
            let guard = self.runs.read().await;
            guard.values().map(|run| run.library_id()).collect()
        };

        let mut guard = self.series_bundles.lock().await;
        guard.retain(|library_id, entry| {
            active_libraries.contains(library_id)
                || now.duration_since(entry.last_touched_at)
                    < SERIES_BUNDLE_TRACKER_IDLE_TTL
        });
    }

    fn job_event_kind(payload: &JobEventPayload) -> &'static str {
        match payload {
            JobEventPayload::Enqueued { .. } => "job_enqueued",
            JobEventPayload::Merged { .. } => "job_merged",
            JobEventPayload::Dequeued { .. } => "job_dequeued",
            JobEventPayload::LeaseRenewed { .. } => "job_lease_renewed",
            JobEventPayload::LeaseExpired { .. } => "job_lease_expired",
            JobEventPayload::Completed { .. } => "job_completed",
            JobEventPayload::Failed {
                retryable: true, ..
            } => "job_retrying",
            JobEventPayload::Failed {
                retryable: false, ..
            } => "job_failed",
            JobEventPayload::DeadLettered { .. } => "job_dead_lettered",
            JobEventPayload::ThroughputTick { .. } => "throughput_tick",
        }
    }

    fn job_event_status(payload: &JobEventPayload) -> ScanRunStatus {
        match payload {
            JobEventPayload::Completed { .. } => ScanRunStatus::Completed,
            JobEventPayload::Failed {
                retryable: false, ..
            }
            | JobEventPayload::DeadLettered { .. } => ScanRunStatus::Failed,
            _ => ScanRunStatus::Running,
        }
    }

    fn job_event_error(payload: &JobEventPayload) -> Option<&str> {
        match payload {
            JobEventPayload::Failed { error, .. }
            | JobEventPayload::DeadLettered { error, .. } => error.as_deref(),
            JobEventPayload::LeaseExpired { .. } => Some("lease_expired"),
            _ => None,
        }
    }

    fn job_event_retryable(payload: &JobEventPayload) -> bool {
        matches!(
            payload,
            JobEventPayload::Failed {
                retryable: true,
                ..
            }
        )
    }

    fn job_event_job_id(payload: &JobEventPayload) -> Option<Uuid> {
        match payload {
            JobEventPayload::Enqueued { job_id, .. }
            | JobEventPayload::Dequeued { job_id, .. }
            | JobEventPayload::LeaseRenewed { job_id, .. }
            | JobEventPayload::LeaseExpired { job_id, .. }
            | JobEventPayload::Completed { job_id, .. }
            | JobEventPayload::Failed { job_id, .. }
            | JobEventPayload::DeadLettered { job_id, .. } => Some(job_id.0),
            JobEventPayload::Merged { merged_job_id, .. } => {
                Some(merged_job_id.0)
            }
            JobEventPayload::ThroughputTick { .. } => None,
        }
    }

    async fn persist_job_event(
        &self,
        event: &JobEvent,
        run: Option<&Arc<ScanRun>>,
    ) {
        let now = Utc::now();
        let event_kind = Self::job_event_kind(&event.payload);
        let fallback_status = Self::job_event_status(&event.payload);
        let source = run
            .map(|run| start_mode_to_observability_source(run.start_mode()))
            .unwrap_or_else(|| {
                if event.meta.path_key.is_some() {
                    ScanRunSource::Watcher
                } else {
                    ScanRunSource::Orchestrator
                }
            });

        let run_id = run
            .map(|run| run.scan_id())
            .unwrap_or(event.meta.correlation_id);
        let subject_key =
            event.meta.path_key.as_ref().map(subject_key_to_string);
        let current_path = event
            .meta
            .path_key
            .as_ref()
            .and_then(subject_key_path_owned);

        let (completed, total, retrying, dead_lettered, started_at, status): (
            i64,
            i64,
            i64,
            i64,
            DateTime<Utc>,
            ScanRunStatus,
        ) = if let Some(run) = run {
            match run.snapshot().await {
                Ok(snapshot) => (
                    snapshot.completed_items.min(i64::MAX as u64) as i64,
                    snapshot.total_items.min(i64::MAX as u64) as i64,
                    snapshot.retrying_items.min(i64::MAX as u64) as i64,
                    snapshot.failed_items.min(i64::MAX as u64) as i64,
                    snapshot.started_at,
                    lifecycle_to_observability_status(&snapshot.status),
                ),
                Err(_) => (0, 0, 0, 0, now, ScanRunStatus::Running),
            }
        } else {
            let retrying = if Self::job_event_retryable(&event.payload) {
                1
            } else {
                0
            };
            let dead_lettered = if matches!(
                &event.payload,
                JobEventPayload::DeadLettered { .. }
            ) {
                1
            } else {
                0
            };
            let completed = if matches!(
                &event.payload,
                JobEventPayload::Completed { .. }
            ) {
                1
            } else {
                0
            };
            (completed, 0, retrying, dead_lettered, now, fallback_status)
        };

        let terminal_at = (!status.is_active()).then_some(now);
        let terminal_summary = if status.is_active() {
            serde_json::json!({})
        } else {
            let (category, message_code) = observability_failure_category(
                Self::job_event_error(&event.payload),
            );
            serde_json::json!({
                "event_kind": event_kind,
                "category": category,
                "message_code": message_code,
            })
        };

        let run_record = ScanRunRecord {
            id: run_id,
            library_id: event.meta.library_id,
            source,
            status,
            correlation_id: event.meta.correlation_id,
            idempotency_key: event.meta.idempotency_key.clone(),
            sequence: 0,
            started_at,
            last_event_at: now,
            terminal_at,
            current_path: current_path.clone(),
            completed_items: completed,
            total_items: total,
            retrying_items: retrying,
            dead_lettered_items: dead_lettered,
            terminal_summary: terminal_summary.clone(),
        };
        let update = ScanRunUpdate {
            id: run_id,
            status,
            idempotency_key: event.meta.idempotency_key.clone(),
            last_event_at: now,
            terminal_at,
            current_path: current_path.clone(),
            completed_items: completed,
            total_items: total,
            retrying_items: retrying,
            dead_lettered_items: dead_lettered,
            terminal_summary,
        };

        let repo = &self.unit_of_work.scan_observability;
        if let Err(err) = repo.create_run(&run_record).await {
            warn!(run = %run_id, error = %err, "failed to create scan job observability run");
            return;
        }
        if let Err(err) = repo.update_run(&update).await {
            warn!(run = %run_id, error = %err, "failed to update scan job observability run");
        }

        let payload = serde_json::to_value(event)
            .unwrap_or_else(|_| serde_json::json!({}));
        let row = NewScanRunEvent {
            run_id,
            library_id: event.meta.library_id,
            event_kind: event_kind.to_string(),
            status: status.as_str().to_string(),
            correlation_id: event.meta.correlation_id,
            idempotency_key: event.meta.idempotency_key.clone(),
            subject_key: subject_key.clone(),
            current_path,
            occurred_at: now,
            completed_items: completed,
            total_items: total,
            retrying_items: retrying,
            dead_lettered_items: dead_lettered,
            payload,
        };
        if let Err(err) = repo.append_event(&row).await {
            warn!(run = %run_id, error = %err, "failed to append scan job observability event");
        }

        if matches!(
            &event.payload,
            JobEventPayload::Failed { .. }
                | JobEventPayload::DeadLettered { .. }
                | JobEventPayload::LeaseExpired { .. }
        ) {
            let reason = Self::job_event_error(&event.payload)
                .unwrap_or("job_failed")
                .to_string();
            let (category, message_code) =
                observability_failure_category(Some(reason.as_str()));
            let failure = ScanRunFailureSummary {
                run_id,
                library_id: event.meta.library_id,
                subject_key: subject_key.unwrap_or_else(|| {
                    format!("job:{}", event.meta.idempotency_key)
                }),
                category: category.to_string(),
                message_code: message_code.to_string(),
                raw_debug_details: serde_json::json!({
                    "reason": reason.clone(),
                    "event": event,
                }),
                last_error: Some(reason),
                occurrences: 1,
                first_seen_at: now,
                last_seen_at: now,
                retryable: Self::job_event_retryable(&event.payload),
                job_id: Self::job_event_job_id(&event.payload),
                idempotency_key: event.meta.idempotency_key.clone(),
            };
            if let Err(err) = repo.upsert_failure_summary(&failure).await {
                warn!(run = %run_id, error = %err, "failed to upsert scan job failure summary");
            }
        }
    }

    fn tracks_scan_progress_kind(kind: JobKind) -> bool {
        matches!(
            kind,
            JobKind::FolderScan
                | JobKind::SeriesResolve
                | JobKind::MediaAnalyze
                | JobKind::MetadataEnrich
                | JobKind::IndexUpsert
                | JobKind::ImageFetch
                | JobKind::EpisodeMatch
                | JobKind::ManifestScan
        )
    }

    async fn handle_job_event(&self, event: JobEvent) {
        let run = {
            let guard = self.runs.read().await;
            guard.get(&event.meta.correlation_id).cloned()
        };

        self.observe_series_bundle_job_event(&event).await;

        let completed = if let Some(run) = run.as_ref() {
            match &event.payload {
                JobEventPayload::Enqueued { kind, job_id, .. } => {
                    if Self::tracks_scan_progress_kind(*kind) {
                        run.record_folder_enqueued(
                            &event.meta.idempotency_key,
                            *job_id,
                            event.meta.path_key.clone(),
                        )
                        .await;
                    }
                    false
                }
                JobEventPayload::Completed { kind, job_id, .. } => {
                    if Self::tracks_scan_progress_kind(*kind) {
                        run.record_folder_completed(
                            &event.meta.idempotency_key,
                            *job_id,
                            event.meta.path_key.clone(),
                        )
                        .await;
                        run.try_complete(
                            self.quiescence_chrono,
                            self.stall_timeout,
                        )
                        .await
                    } else {
                        false
                    }
                }
                JobEventPayload::Failed {
                    kind,
                    retryable,
                    job_id,
                    error,
                    ..
                } => {
                    if Self::tracks_scan_progress_kind(*kind) {
                        run.record_folder_failure(
                            &event.meta.idempotency_key,
                            *job_id,
                            error.clone(),
                            event.meta.path_key.clone(),
                            *retryable,
                        )
                        .await;

                        if !*retryable {
                            run.try_complete(
                                self.quiescence_chrono,
                                self.stall_timeout,
                            )
                            .await
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                JobEventPayload::DeadLettered {
                    kind,
                    job_id,
                    error,
                    ..
                } => {
                    if Self::tracks_scan_progress_kind(*kind) {
                        run.record_folder_dead_lettered(
                            &event.meta.idempotency_key,
                            *job_id,
                            error.clone(),
                            event.meta.path_key.clone(),
                        )
                        .await;
                        run.try_complete(
                            self.quiescence_chrono,
                            self.stall_timeout,
                        )
                        .await
                    } else {
                        false
                    }
                }
                JobEventPayload::LeaseRenewed { job_id, .. } => {
                    // Lease-renewed events do not carry a kind; update the item
                    // only if a tracked enqueue/dequeue already established it.
                    run.record_folder_lease_renewed(
                        &event.meta.idempotency_key,
                        *job_id,
                        event.meta.path_key.clone(),
                    )
                    .await;
                    false
                }
                _ => false,
            }
        } else {
            self.handle_orphan_event(&event).await;
            false
        };

        // Keep the progress state current before durable diagnostics I/O. The
        // persisted event snapshots now include the event's own state change,
        // and the hot receiver loop is less likely to finalize from stale
        // counters while the observability repository is busy.
        self.persist_job_event(&event, run.as_ref()).await;

        if completed && let Some(run) = run {
            self.on_run_completed(run).await;
        }
    }

    async fn observe_series_bundle_job_event(&self, event: &JobEvent) {
        let library_id = event.meta.library_id;
        let now = Instant::now();

        let mut guard = self.series_bundles.lock().await;
        let entry = guard
            .entry(library_id)
            .or_insert_with(|| SeriesBundleTrackerEntry::new(now));
        entry.touch(now);
        entry.tracker.observe_job_event(event);

        drop(guard);

        match &event.payload {
            JobEventPayload::Completed { .. }
            | JobEventPayload::DeadLettered { .. }
            | JobEventPayload::Failed {
                retryable: false, ..
            } => {
                self.try_emit_series_bundle_finalized(library_id).await;
            }
            _ => {}
        }
    }

    async fn handle_scan_event(&self, event: ScanEvent) {
        match event {
            ScanEvent::FolderDiscovered { context, .. } => {
                self.observe_series_bundle_folder_discovered(&context).await;
            }
            ScanEvent::MediaFileDiscovered(event) => {
                self.observe_series_bundle_media_discovered(&event).await;
            }
            ScanEvent::FolderScanCompleted(summary) => {
                self.observe_series_bundle_folder_completed(&summary).await;

                let runs: Vec<Arc<ScanRun>> = {
                    let guard = self.runs.read().await;
                    guard
                        .values()
                        .filter(|run| {
                            run.library_id() == summary.context.library_id()
                        })
                        .cloned()
                        .collect()
                };

                for run in runs {
                    run.record_folder_summary(&summary).await;
                }
            }
            ScanEvent::SeedCompleted(summary) => {
                if let Some(correlation_id) = summary.correlation_id {
                    let run = {
                        let guard = self.runs.read().await;
                        guard.get(&correlation_id).cloned()
                    };

                    if let Some(run) = run
                        && run.record_seed_completed(&summary).await
                    {
                        self.on_run_completed(run).await;
                    }
                }
            }
            ScanEvent::Indexed(outcome) => {
                let outcome = *outcome;
                let result = self.handle_indexed_outcome(outcome.clone()).await;
                let ok = result.is_ok();

                // Attribute index outcome to any active runs for this library
                let runs: Vec<Arc<ScanRun>> = {
                    let guard = self.runs.read().await;
                    guard
                        .values()
                        .filter(|r| r.library_id() == outcome.library_id)
                        .cloned()
                        .collect()
                };

                for run in runs {
                    run.record_index_outcome(&outcome.path_norm, ok).await;
                }

                if let Err(err) = result {
                    warn!(
                        library = %outcome.library_id,
                        path = %outcome.path_norm,
                        error = %err,
                        "failed to process indexed outcome"
                    );
                }

                self.observe_series_bundle_indexed(&outcome).await;
            }
            _ => {}
        }
    }

    async fn observe_series_bundle_folder_discovered(
        &self,
        context: &ferrex_core::domain::scan::orchestration::context::FolderScanContext,
    ) {
        let library_id = context.library_id();
        let now = Instant::now();

        let mut guard = self.series_bundles.lock().await;
        let entry = guard
            .entry(library_id)
            .or_insert_with(|| SeriesBundleTrackerEntry::new(now));
        entry.touch(now);
        entry.tracker.observe_folder_discovered(context);
    }

    async fn observe_series_bundle_media_discovered(
        &self,
        event: &ferrex_core::domain::scan::MediaFileDiscovered,
    ) {
        let library_id = event.library_id;
        let now = Instant::now();

        let mut guard = self.series_bundles.lock().await;
        let entry = guard
            .entry(library_id)
            .or_insert_with(|| SeriesBundleTrackerEntry::new(now));
        entry.touch(now);
        entry.tracker.observe_media_discovered(event);
    }

    async fn observe_series_bundle_folder_completed(
        &self,
        summary: &ferrex_core::domain::scan::FolderScanSummary,
    ) {
        let library_id = summary.context.library_id();
        let now = Instant::now();

        let mut guard = self.series_bundles.lock().await;
        let entry = guard
            .entry(library_id)
            .or_insert_with(|| SeriesBundleTrackerEntry::new(now));
        entry.touch(now);
        entry.tracker.observe_folder_scan_completed(summary);

        drop(guard);

        self.try_emit_series_bundle_finalized(library_id).await;
    }

    async fn observe_series_bundle_indexed(&self, outcome: &IndexingOutcome) {
        let library_id = outcome.library_id;
        let now = Instant::now();

        let mut guard = self.series_bundles.lock().await;
        let entry = guard
            .entry(library_id)
            .or_insert_with(|| SeriesBundleTrackerEntry::new(now));
        entry.touch(now);
        entry.tracker.observe_indexed(outcome);

        drop(guard);

        self.try_emit_series_bundle_finalized(library_id).await;
    }

    async fn try_emit_series_bundle_finalized(&self, library_id: LibraryId) {
        let candidates: Vec<SeriesBundleFinalization> = {
            let guard = self.series_bundles.lock().await;
            guard
                .get(&library_id)
                .map(|entry| entry.tracker.finalization_candidates())
                .unwrap_or_default()
        };

        for finalization in candidates {
            if !self
                .confirm_series_bundle_ready(
                    finalization.library_id,
                    finalization.series_id,
                )
                .await
            {
                continue;
            }

            let event = CatalogEvent::SeriesBundleFinalized {
                library_id: finalization.library_id,
                series_id: finalization.series_id,
            };

            let receivers = self.catalog_bus.receiver_count();
            let frame = self.catalog_bus.publish(event);

            let mut guard = self.series_bundles.lock().await;
            if let Some(entry) = guard.get_mut(&library_id) {
                entry.tracker.mark_finalized(&finalization.series_root_path);
            }

            info!(
                library = %finalization.library_id,
                series_id = %finalization.series_id,
                series_root = %finalization.series_root_path.as_str(),
                receivers = receivers,
                sequence = frame.sequence,
                "published series bundle finalization"
            );
        }
    }

    async fn confirm_series_bundle_ready(
        &self,
        library_id: LibraryId,
        series_id: ferrex_core::types::SeriesID,
    ) -> bool {
        let uow = &self.unit_of_work;

        let (series, seasons, episodes) = tokio::join!(
            uow.media_refs.get_series_reference(&series_id),
            uow.media_refs.get_series_seasons(&series_id),
            uow.media_refs.get_series_episodes(&series_id),
        );

        let mut series = match series {
            Ok(series) if series.library_id == library_id => series,
            Ok(_) => {
                warn!(
                    library = %library_id,
                    series_id = %series_id,
                    "series bundle finalization library mismatch"
                );
                return false;
            }
            Err(err) => {
                warn!(
                    library = %library_id,
                    series_id = %series_id,
                    error = %err,
                    "series bundle finalization failed to hydrate series"
                );
                return false;
            }
        };

        let seasons = match seasons {
            Ok(seasons) => seasons,
            Err(err) => {
                warn!(
                    library = %library_id,
                    series_id = %series_id,
                    error = %err,
                    "series bundle finalization failed to hydrate seasons"
                );
                return false;
            }
        };

        let episodes = match episodes {
            Ok(episodes) => episodes,
            Err(err) => {
                warn!(
                    library = %library_id,
                    series_id = %series_id,
                    error = %err,
                    "series bundle finalization failed to hydrate episodes"
                );
                return false;
            }
        };

        // Ensure the server-side versioning record is up to date at the point
        // we consider a series bundle "finalized".
        //
        // This keeps the version monotonic only when the serialized bundle
        // payload changes, which is what the player-side cache invalidation
        // relies on.
        series.details.available_seasons = Some(seasons.len() as u16);
        series.details.available_episodes = Some(episodes.len() as u16);

        let response = SeriesBundleResponse {
            library_id,
            series_id,
            series,
            seasons,
            episodes,
        };

        let bytes = match rkyv::to_bytes::<rkyv::rancor::Error>(&response) {
            Ok(bytes) => bytes,
            Err(err) => {
                warn!(
                    library = %library_id,
                    series_id = %series_id,
                    error = ?err,
                    "series bundle finalization failed to serialize bundle response"
                );
                return false;
            }
        };

        let digest = sha2::Sha256::digest(bytes.as_slice());
        let hash = u64::from_be_bytes(
            digest[..8]
                .try_into()
                .expect("sha256 digest must be at least 8 bytes"),
        );

        match uow
            .media_refs
            .upsert_series_bundle_hash(&library_id, &series_id, hash)
            .await
        {
            Ok(()) => true,
            Err(err) => {
                error!(
                    library = %library_id,
                    series_id = %series_id,
                    error = %err,
                    "failed to upsert series bundle hash during finalization"
                );
                false
            }
        }
    }

    async fn handle_indexed_outcome(
        &self,
        outcome: IndexingOutcome,
    ) -> Result<(), String> {
        let mut media = outcome.media.clone();

        if media.is_none() {
            media = self.load_media(outcome.media_id).await;
        }

        let media = match media {
            Some(media) => media,
            None => {
                return Err(format!(
                    "missing media reference for library {} path {}",
                    outcome.library_id, outcome.path_norm
                ));
            }
        };

        let mut seen = self.seen_media.lock().await;
        let first_seen = seen.insert(outcome.media_id.to_uuid());
        drop(seen);

        let change = match outcome.change {
            IndexingChange::Created if first_seen => IndexingChange::Created,
            _ => IndexingChange::Updated,
        };

        let event = match (media, change) {
            (Media::Movie(movie), IndexingChange::Created) => {
                CatalogEvent::MovieAdded { movie: *movie }
            }
            (Media::Movie(movie), IndexingChange::Updated) => {
                CatalogEvent::MovieUpdated { movie: *movie }
            }
            (Media::Series(series), IndexingChange::Created) => {
                CatalogEvent::SeriesAdded { series: *series }
            }
            (Media::Series(series), IndexingChange::Updated) => {
                CatalogEvent::SeriesUpdated { series: *series }
            }
            (_, _) => return Ok(()),
        };

        let _ = self.catalog_bus.publish(event);

        Ok(())
    }

    async fn load_media(&self, mid: MediaID) -> Option<Media> {
        let media_refs = &self.unit_of_work.media_refs;

        match mid {
            MediaID::Movie(movie_id) => {
                match media_refs.get_movie_reference(&movie_id).await {
                    Ok(movie) => Some(Media::Movie(Box::new(movie))),
                    Err(MediaError::NotFound(_)) => None,
                    Err(err) => {
                        warn!("failed to hydrate movie reference {mid}: {err}");
                        None
                    }
                }
            }
            MediaID::Series(series_id) => {
                match media_refs.get_series_reference(&series_id).await {
                    Ok(series) => Some(Media::Series(Box::new(series))),
                    Err(MediaError::NotFound(_)) => None,
                    Err(err) => {
                        warn!(
                            "failed to hydrate series reference {mid}: {err}"
                        );
                        None
                    }
                }
            }
            MediaID::Season(season_id) => {
                match media_refs.get_season_reference(&season_id).await {
                    Ok(season) => Some(Media::Season(Box::new(season))),
                    Err(MediaError::NotFound(_)) => None,
                    Err(err) => {
                        warn!(
                            "failed to hydrate season reference {mid}: {err}"
                        );
                        None
                    }
                }
            }
            MediaID::Episode(episode_id) => {
                match media_refs.get_episode_reference(&episode_id).await {
                    Ok(episode) => Some(Media::Episode(Box::new(episode))),
                    Err(MediaError::NotFound(_)) => None,
                    Err(err) => {
                        warn!(
                            "failed to hydrate episode reference {mid}: {err}"
                        );
                        None
                    }
                }
            }
        }
    }

    async fn persist_activity_window_started(
        &self,
        library_id: LibraryId,
        run_id: Uuid,
        source: ScanRunSource,
        event_kind: &str,
    ) {
        let now = Utc::now();
        let idempotency_key =
            format!("{}:{}:{}", event_kind, library_id, run_id);
        let run_record = ScanRunRecord {
            id: run_id,
            library_id,
            source,
            status: ScanRunStatus::Running,
            correlation_id: run_id,
            idempotency_key: idempotency_key.clone(),
            sequence: 0,
            started_at: now,
            last_event_at: now,
            terminal_at: None,
            current_path: None,
            completed_items: 0,
            total_items: 0,
            retrying_items: 0,
            dead_lettered_items: 0,
            terminal_summary: serde_json::json!({}),
        };
        let update = ScanRunUpdate {
            id: run_id,
            status: ScanRunStatus::Running,
            idempotency_key: idempotency_key.clone(),
            last_event_at: now,
            terminal_at: None,
            current_path: None,
            completed_items: 0,
            total_items: 0,
            retrying_items: 0,
            dead_lettered_items: 0,
            terminal_summary: serde_json::json!({}),
        };
        let repo = &self.unit_of_work.scan_observability;
        if let Err(err) = repo.create_run(&run_record).await {
            warn!(run = %run_id, error = %err, "failed to create scan activity window");
            return;
        }
        if let Err(err) = repo.update_run(&update).await {
            warn!(run = %run_id, error = %err, "failed to update scan activity window");
        }
        let event = NewScanRunEvent {
            run_id,
            library_id,
            event_kind: event_kind.to_string(),
            status: ScanRunStatus::Running.as_str().to_string(),
            correlation_id: run_id,
            idempotency_key,
            subject_key: None,
            current_path: None,
            occurred_at: now,
            completed_items: 0,
            total_items: 0,
            retrying_items: 0,
            dead_lettered_items: 0,
            payload: serde_json::json!({
                "source": source.as_str(),
                "library_id": library_id,
                "correlation_id": run_id,
            }),
        };
        if let Err(err) = repo.append_event(&event).await {
            warn!(run = %run_id, error = %err, "failed to append scan activity event");
        }
    }

    async fn on_run_completed(&self, run: Arc<ScanRun>) {
        if run.start_mode() != StartMode::Bulk {
            return;
        }

        let library_id = run.library_id();
        if let Err(err) = self
            .orchestrator
            .command_library(
                library_id,
                LibraryActorCommand::ScanRunTerminal {
                    correlation_id: Some(run.correlation_id()),
                },
            )
            .await
        {
            warn!(
                library = %library_id,
                scan = %run.scan_id(),
                error = %err,
                "failed to clear library bulk mode after terminal scan"
            );
        }

        let correlation_id = Uuid::now_v7();
        self.persist_activity_window_started(
            library_id,
            correlation_id,
            ScanRunSource::Maintenance,
            "maintenance_started",
        )
        .await;
        let command = LibraryActorCommand::Start {
            mode: StartMode::Maintenance,
            correlation_id: Some(correlation_id),
        };

        match self.orchestrator.command_library(library_id, command).await {
            Ok(()) => info!(
                library = %library_id,
                scan = %run.scan_id(),
                "initial bulk scan complete; switching to maintenance"
            ),
            Err(err) => warn!(
                library = %library_id,
                scan = %run.scan_id(),
                error = %err,
                "failed to transition library to maintenance"
            ),
        }
    }

    async fn handle_orphan_event(&self, event: &JobEvent) {
        use ferrex_core::domain::scan::orchestration::job::JobKind::FolderScan;

        let Some(path_norm) =
            event.meta.path_key.as_ref().and_then(subject_key_path)
        else {
            return;
        };

        let should_persist = match &event.payload {
            JobEventPayload::Completed {
                kind: FolderScan, ..
            } => true,
            JobEventPayload::DeadLettered {
                kind: FolderScan, ..
            } => true,
            JobEventPayload::Failed {
                kind, retryable, ..
            } if *kind == FolderScan && !*retryable => true,
            _ => false,
        };

        if !should_persist {
            return;
        }

        // Avoid persisting a cursor without an accurate listing hash here.
        // The dispatcher is responsible for writing cursors with the
        // true listing_hash derived from the folder listing plan. Persisting
        // a placeholder from this orphan path risks overwriting correct data
        // and breaking incremental diff detection.

        let targets: Vec<Arc<ScanRun>> = {
            let guard = self.runs.read().await;
            guard
                .values()
                .filter(|run| run.library_id() == event.meta.library_id)
                .cloned()
                .collect()
        };

        if targets.is_empty() {
            return;
        }

        let path_owned = path_norm.to_string();
        let job_id = match &event.payload {
            JobEventPayload::Completed { job_id, .. } => Some(*job_id),
            JobEventPayload::DeadLettered { job_id, .. } => Some(*job_id),
            JobEventPayload::Failed {
                job_id, retryable, ..
            } if !*retryable => Some(*job_id),
            _ => None,
        };

        for run in targets {
            if let Some(job_id) = job_id {
                run.record_folder_completed(
                    &event.meta.idempotency_key,
                    job_id,
                    SubjectKey::path(path_owned.clone()).ok(),
                )
                .await;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanHistoryEntry {
    pub scan_id: Uuid,
    pub library_id: LibraryId,
    pub status: ScanLifecycleStatus,
    pub completed_items: u64,
    pub total_items: u64,
    pub validated_items: u64,
    pub known_unchanged_items: u64,
    pub skipped_items: u64,
    pub failed_items: u64,
    pub needs_attention_items: u64,
    pub retrying_items: u64,
    pub started_at: DateTime<Utc>,
    pub terminal_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_details: Vec<ScanPathReasonDetail>,
}

impl ScanHistoryEntry {
    fn from_observability(run: ScanRunRecord) -> Option<Self> {
        let status = ScanLifecycleStatus::from_observability(run.status)?;
        let completed_items = run.completed_items.max(0) as u64;
        let failed_items = run.dead_lettered_items.max(0) as u64;
        Some(Self {
            scan_id: run.id,
            library_id: run.library_id,
            status,
            completed_items,
            total_items: run.total_items.max(0) as u64,
            validated_items: completed_items,
            known_unchanged_items: 0,
            skipped_items: 0,
            failed_items,
            needs_attention_items: failed_items,
            retrying_items: run.retrying_items.max(0) as u64,
            started_at: run.started_at,
            terminal_at: run.terminal_at.unwrap_or(run.last_event_at),
            reason_details: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSnapshot {
    pub scan_id: Uuid,
    pub library_id: LibraryId,
    pub status: ScanLifecycleStatus,
    pub mode: ScanRunMode,
    pub completed_items: u64,
    pub total_items: u64,
    pub validated_items: u64,
    pub known_unchanged_items: u64,
    pub skipped_items: u64,
    pub failed_items: u64,
    pub needs_attention_items: u64,
    pub retrying_items: u64,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub run_key: String,
    pub disposition: Option<ScanStartDisposition>,
    pub current_path: Option<String>,
    pub started_at: DateTime<Utc>,
    pub terminal_at: Option<DateTime<Utc>>,
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_details: Vec<ScanPathReasonDetail>,
}

impl ScanSnapshot {
    fn from_observability(run: ScanRunRecord) -> Option<Self> {
        let mode = scan_run_mode_from_observability_source(run.source);
        let completed_items = run.completed_items.max(0) as u64;
        let failed_items = run.dead_lettered_items.max(0) as u64;
        Some(Self {
            scan_id: run.id,
            library_id: run.library_id,
            status: ScanLifecycleStatus::from_observability(run.status)?,
            mode,
            completed_items,
            total_items: run.total_items.max(0) as u64,
            validated_items: completed_items,
            known_unchanged_items: 0,
            skipped_items: 0,
            failed_items,
            needs_attention_items: failed_items,
            retrying_items: run.retrying_items.max(0) as u64,
            correlation_id: run.correlation_id,
            idempotency_key: run.idempotency_key,
            run_key: mode.run_key(run.library_id),
            disposition: None,
            current_path: run.current_path,
            started_at: run.started_at,
            terminal_at: run.terminal_at,
            sequence: run.sequence.max(0) as u64,
            reason_details: Vec::new(),
        })
    }
}

impl From<ScanSnapshot> for ScanSnapshotDto {
    fn from(snapshot: ScanSnapshot) -> Self {
        ScanSnapshotDto {
            scan_id: snapshot.scan_id,
            library_id: snapshot.library_id,
            status: snapshot.status.into(),
            mode: snapshot.mode,
            completed_items: snapshot.completed_items,
            total_items: snapshot.total_items,
            validated_items: snapshot.validated_items,
            known_unchanged_items: snapshot.known_unchanged_items,
            skipped_items: snapshot.skipped_items,
            failed_items: snapshot.failed_items,
            needs_attention_items: snapshot.needs_attention_items,
            retrying_items: snapshot.retrying_items,
            correlation_id: snapshot.correlation_id,
            idempotency_key: snapshot.idempotency_key,
            run_key: snapshot.run_key,
            disposition: snapshot.disposition,
            current_path: snapshot.current_path,
            started_at: snapshot.started_at,
            terminal_at: snapshot.terminal_at,
            sequence: snapshot.sequence,
            reason_details: snapshot.reason_details,
        }
    }
}

impl From<ScanLifecycleStatus> for ApiScanLifecycleStatus {
    fn from(value: ScanLifecycleStatus) -> Self {
        match value {
            ScanLifecycleStatus::Pending => ApiScanLifecycleStatus::Pending,
            ScanLifecycleStatus::Running => ApiScanLifecycleStatus::Running,
            ScanLifecycleStatus::Paused => ApiScanLifecycleStatus::Paused,
            ScanLifecycleStatus::Completed => ApiScanLifecycleStatus::Completed,
            ScanLifecycleStatus::Failed => ApiScanLifecycleStatus::Failed,
            ScanLifecycleStatus::Canceled => ApiScanLifecycleStatus::Canceled,
        }
    }
}

impl From<ApiScanLifecycleStatus> for ScanLifecycleStatus {
    fn from(value: ApiScanLifecycleStatus) -> Self {
        match value {
            ApiScanLifecycleStatus::Pending => ScanLifecycleStatus::Pending,
            ApiScanLifecycleStatus::Running => ScanLifecycleStatus::Running,
            ApiScanLifecycleStatus::Paused => ScanLifecycleStatus::Paused,
            ApiScanLifecycleStatus::Completed => ScanLifecycleStatus::Completed,
            ApiScanLifecycleStatus::Failed => ScanLifecycleStatus::Failed,
            ApiScanLifecycleStatus::Canceled => ScanLifecycleStatus::Canceled,
        }
    }
}

#[derive(Debug)]
pub enum ScanControlError {
    LibraryNotFound,
    LibraryDisabled,
    LibraryMismatch,
    ScanNotFound,
    ScanNotRunning,
    ScanTerminal,
    ReplayGap(ScanReplayGap),
    InvalidRecoveryTarget(String),
    Internal(String),
}

impl ScanControlError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            ScanControlError::LibraryNotFound => StatusCode::NOT_FOUND,
            ScanControlError::LibraryDisabled => StatusCode::CONFLICT,
            ScanControlError::LibraryMismatch => StatusCode::BAD_REQUEST,
            ScanControlError::ScanNotFound => StatusCode::NOT_FOUND,
            ScanControlError::ScanNotRunning => StatusCode::CONFLICT,
            ScanControlError::ScanTerminal => StatusCode::GONE,
            ScanControlError::ReplayGap(_) => StatusCode::CONFLICT,
            ScanControlError::InvalidRecoveryTarget(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            ScanControlError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(&self) -> String {
        match self {
            ScanControlError::LibraryNotFound => "library_not_found".into(),
            ScanControlError::LibraryDisabled => "library_disabled".into(),
            ScanControlError::LibraryMismatch => "scan_library_mismatch".into(),
            ScanControlError::ScanNotFound => "scan_not_found".into(),
            ScanControlError::ScanNotRunning => "scan_not_running".into(),
            ScanControlError::ScanTerminal => "scan_already_terminal".into(),
            ScanControlError::ReplayGap(_) => "scan_event_replay_gap".into(),
            ScanControlError::InvalidRecoveryTarget(reason) => reason.clone(),
            ScanControlError::Internal(reason) => reason.clone(),
        }
    }

    fn internal(msg: String) -> Self {
        ScanControlError::Internal(msg)
    }
}

impl fmt::Display for ScanControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for ScanControlError {}

#[cfg(test)]
mod durable_status_tests {
    use super::*;

    #[test]
    fn repository_status_maps_runtime_phases_to_durable_running() {
        assert_eq!(
            repository_status_from_payload("discovering"),
            Some(ApiScanLifecycleStatus::Running)
        );
        assert_eq!(
            repository_status_from_payload("quiescing"),
            Some(ApiScanLifecycleStatus::Running)
        );
        assert_eq!(
            repository_status_from_payload("paused"),
            Some(ApiScanLifecycleStatus::Paused)
        );
        assert_eq!(repository_status_from_payload("completed"), None);
    }

    #[test]
    fn library_mismatch_reports_client_error() {
        assert_eq!(
            ScanControlError::LibraryMismatch.status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ScanControlError::LibraryMismatch.message(),
            "scan_library_mismatch"
        );
    }

    #[test]
    fn recovery_path_ownership_uses_path_components() {
        assert!(path_is_within("/media/movies", "/media/movies"));
        assert!(path_is_within("/media/movies", "/media/movies/A"));
        assert!(!path_is_within("/media/movies", "/media/movies2/A"));
    }

    #[test]
    fn replay_gap_detects_pruned_sequences() {
        let scan_id = Uuid::now_v7();
        let err = validate_replay_gap(
            scan_id,
            Some(1),
            &ScanRunEventSequenceBounds {
                min_sequence: Some(4),
                max_sequence: Some(8),
            },
            8,
        )
        .unwrap_err();

        match err {
            ScanControlError::ReplayGap(gap) => {
                assert_eq!(gap.scan_id, scan_id);
                assert_eq!(gap.requested_after_sequence, 1);
                assert_eq!(gap.min_available_sequence, Some(4));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn replay_gap_allows_contiguous_terminal_replay() {
        validate_replay_gap(
            Uuid::now_v7(),
            Some(3),
            &ScanRunEventSequenceBounds {
                min_sequence: Some(4),
                max_sequence: Some(8),
            },
            8,
        )
        .expect("sequence 4 is retained and contiguous");
    }
}
