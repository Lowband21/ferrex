use ferrex_model::SubjectKey;

use ferrex_core::{
    api::types::{
        ScanLifecycleStatus as ApiScanLifecycleStatus, ScanRunMode,
        ScanSnapshotDto, ScanStartDisposition,
    },
    application::unit_of_work::AppUnitOfWork,
    domain::scan::{
        actors::{
            FileSystemEvent, FileSystemEventKind, FolderScanOutcome,
            LibraryRootsId, index::IndexingOutcome,
        },
        orchestration::{
            DurableJobState, JobEvent, LibraryActorCommand, LibraryScanRun,
            LibraryScanRunProgressUpdate, NewLibraryScanRun, StartMode,
            context::SeriesRootPath,
            events::{JobEventPayload, ScanEvent, ScanSeedSummary},
            job::{JobId, JobKind, JobState},
            scan_cursor::{ScanCursor, ScanCursorRepository, normalize_path},
            series::{SeriesBundleFinalization, SeriesBundleTracker},
        },
    },
    types::{
        LibraryId, MediaEvent, ScanPathReasonCategory, ScanPathReasonDetail,
        ScanProgressEvent, ScanStageLatencySummary, events::ScanSseEventType,
    },
};

pub use super::media_event_bus::MediaEventFrame;

use super::{
    catalog_event_projection::CatalogEventProjection,
    media_event_bus::MediaEventBus,
    movie_batch_notifier::MovieBatchFinalizationNotifiers,
};
use crate::infra::orchestration::ScanOrchestrator;

use axum::http::StatusCode;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque, hash_map::Entry},
    fmt,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use tokio::{
    spawn,
    sync::{Mutex, RwLock, broadcast},
    time::{MissedTickBehavior, interval},
};

use tracing::{info, instrument, warn};
use uuid::Uuid;

const EVENT_VERSION: &str = "2";
const HISTORY_CAPACITY: usize = 256;
const EVENT_HISTORY_CAPACITY: usize = 512;
const MEDIA_EVENT_HISTORY_CAPACITY: usize = 512;
const MEDIA_EVENT_BROADCAST_CAPACITY: usize = 512;
const DEFAULT_LATENCIES: ScanStageLatencySummary = ScanStageLatencySummary {
    scan: 12,
    analyze: 210,
    index: 44,
};
const DEFAULT_QUIESCENCE: Duration = Duration::from_secs(3);
const STALLED_SCAN_TIMEOUT_MULTIPLIER: u32 = 5;
const SERIES_BUNDLE_TRACKER_IDLE_TTL: Duration = Duration::from_secs(10 * 60);
const SERIES_BUNDLE_POLL_INTERVAL: Duration = Duration::from_secs(1);
const DURABLE_RECONCILIATION_INITIAL_BACKOFF: Duration = Duration::from_secs(5);
const DURABLE_RECONCILIATION_MAX_BACKOFF: Duration = Duration::from_secs(30);

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

/// Command dispatcher + read model for scan orchestration state.
#[derive(Clone)]
pub struct ScanControlPlane {
    inner: Arc<ScanControlPlaneInner>,
}

impl fmt::Debug for ScanControlPlane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let active = self.inner.progress.active_count();
        let history = self.inner.progress.history_count();
        let receiver_count = self.inner.media_bus.receiver_count();
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
    progress: ScanRunProgressTracker,
    media_bus: Arc<MediaEventBus>,
    catalog_events: CatalogEventProjection,
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
        let media_bus = Arc::new(MediaEventBus::new(
            MEDIA_EVENT_HISTORY_CAPACITY,
            MEDIA_EVENT_BROADCAST_CAPACITY,
        ));
        let catalog_events = CatalogEventProjection::new(
            unit_of_work.clone(),
            Arc::clone(&media_bus),
        );
        let progress = ScanRunProgressTracker::new(
            Arc::clone(&unit_of_work),
            Arc::clone(&orchestrator),
            catalog_events.clone(),
            quiescence,
        );

        Self {
            inner: Arc::new(ScanControlPlaneInner {
                unit_of_work,
                orchestrator,
                progress,
                media_bus,
                catalog_events,
            }),
        }
    }

    pub fn orchestrator(&self) -> Arc<ScanOrchestrator> {
        Arc::clone(&self.inner.orchestrator)
    }

    /// Forget process-local state after a library and its durable scan rows
    /// have been deleted successfully.
    ///
    /// PostgreSQL owns deletion of the durable queue and scan-run rows. This
    /// removes their in-memory projections so they cannot remain visible as
    /// active scans or consume scheduler reservations.
    pub async fn forget_library(&self, library_id: LibraryId) {
        self.inner.progress.forget_library(library_id).await;
        self.inner
            .orchestrator
            .forget_library_scheduler_state(library_id)
            .await;
    }

    pub fn subscribe_media_events(
        &self,
    ) -> broadcast::Receiver<MediaEventFrame> {
        self.inner.media_bus.subscribe()
    }

    pub fn publish_media_event(&self, event: MediaEvent) {
        self.inner.catalog_events.publish_event(event);
    }

    pub fn media_event_history_since_sequence(
        &self,
        sequence: u64,
    ) -> Vec<MediaEventFrame> {
        self.inner.media_bus.history_since_sequence(sequence)
    }

    pub fn media_event_history_since_instant(
        &self,
        since: Instant,
    ) -> Vec<MediaEventFrame> {
        self.inner.media_bus.history_since_instant(since)
    }

    pub async fn subscribe_scan(
        &self,
        scan_id: Uuid,
    ) -> Result<broadcast::Receiver<ScanBroadcastFrame>, ScanControlError> {
        self.inner.progress.subscribe_scan(scan_id).await
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
        let run = self
            .inner
            .progress
            .register_durable_run(durable.clone())
            .await;

        if disposition == ScanStartDisposition::Created {
            run.begin().await;
        }

        // Seed ownership is serialized per run and acknowledged only after the
        // actor has made the complete seed batch durable. Reused concurrent
        // starts therefore cannot dispatch duplicate Start commands.
        if let Err(err) =
            ensure_run_seeded(&run, &self.inner.orchestrator).await
        {
            let finalized = run.fail_with_reason("start_command_failed").await;
            if finalized && run.start_mode() == StartMode::Bulk {
                let _ = self
                    .inner
                    .orchestrator
                    .command_library(
                        durable.library_id,
                        LibraryActorCommand::Start {
                            mode: StartMode::Maintenance,
                            correlation_id: Some(run.correlation_id()),
                        },
                    )
                    .await;
            }
            return Err(ScanControlError::internal(err.to_string()));
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
        self.inner.progress.rehydrate_active_runs().await
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
        let run = self
            .inner
            .progress
            .lookup_for_library(scan_id, library_id)
            .await?;
        let _transition = run.seed_transition.lock().await;
        run.validate_pause().await?;
        self.inner
            .orchestrator
            .command_library(library_id, LibraryActorCommand::Pause)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?;
        if let Err(err) = run.pause().await {
            let _ = self
                .inner
                .orchestrator
                .command_library(library_id, LibraryActorCommand::Resume)
                .await;
            return Err(err);
        }
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
        let run = self
            .inner
            .progress
            .lookup_for_library(scan_id, library_id)
            .await?;
        let _transition = run.seed_transition.lock().await;
        run.validate_resume().await?;
        self.inner
            .orchestrator
            .command_library(library_id, LibraryActorCommand::Resume)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?;
        if let Err(err) = run.resume().await {
            let _ = self
                .inner
                .orchestrator
                .command_library(library_id, LibraryActorCommand::Pause)
                .await;
            return Err(err);
        }
        if let Err(err) = seed_run_locked(&run, &self.inner.orchestrator).await
        {
            let finalized =
                run.fail_with_reason("resume_seed_command_failed").await;
            if finalized && run.start_mode() == StartMode::Bulk {
                let _ = self
                    .inner
                    .orchestrator
                    .command_library(
                        library_id,
                        LibraryActorCommand::Start {
                            mode: StartMode::Maintenance,
                            correlation_id: Some(run.correlation_id()),
                        },
                    )
                    .await;
            }
            return Err(ScanControlError::internal(err.to_string()));
        }
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
        let run = self
            .inner
            .progress
            .lookup_for_library(scan_id, library_id)
            .await?;
        let _transition = run.seed_transition.lock().await;
        let finalized = run.cancel().await?;
        if finalized
            && run.start_mode() == StartMode::Bulk
            && let Err(err) = self
                .inner
                .orchestrator
                .command_library(
                    library_id,
                    LibraryActorCommand::Start {
                        mode: StartMode::Maintenance,
                        correlation_id: Some(run.correlation_id()),
                    },
                )
                .await
        {
            warn!(
                library = %library_id,
                scan = %scan_id,
                error = %err,
                "scan canceled but library actor did not return to maintenance"
            );
        }
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
        self.inner.progress.active_scans().await
    }

    pub async fn history(&self, limit: usize) -> Vec<ScanHistoryEntry> {
        self.inner.progress.history(limit).await
    }

    pub async fn snapshot(&self, scan_id: &Uuid) -> Option<ScanSnapshot> {
        self.inner.progress.snapshot(scan_id).await
    }

    pub async fn events(
        &self,
        scan_id: &Uuid,
    ) -> Result<Vec<ScanBroadcastFrame>, ScanControlError> {
        self.inner.progress.events(scan_id).await
    }
}

#[derive(Clone)]
struct ScanRunProgressTracker {
    inner: Arc<ScanRunProgressTrackerInner>,
}

struct ScanRunProgressTrackerInner {
    unit_of_work: Arc<AppUnitOfWork>,
    orchestrator: Arc<ScanOrchestrator>,
    active_by_scan_id: RwLock<HashMap<Uuid, Arc<ScanRun>>>,
    active_by_run_key: RwLock<HashMap<String, Arc<ScanRun>>>,
    archive: ScanRunProgressArchive,
    catalog_events: CatalogEventProjection,
    aggregator: ScanRunAggregator,
    movie_batch_notifiers: MovieBatchFinalizationNotifiers,
}

struct ScanRunProgressArchive {
    history: RwLock<VecDeque<ScanHistoryEntry>>,
    final_events: RwLock<HashMap<Uuid, VecDeque<ScanBroadcastFrame>>>,
}

impl ScanRunProgressArchive {
    fn new() -> Self {
        Self {
            history: RwLock::new(VecDeque::with_capacity(HISTORY_CAPACITY)),
            final_events: RwLock::new(HashMap::new()),
        }
    }

    fn history_count(&self) -> Option<usize> {
        self.history.try_read().ok().map(|guard| guard.len())
    }

    async fn history(&self, limit: usize) -> Vec<ScanHistoryEntry> {
        let guard = self.history.read().await;
        guard.iter().rev().take(limit).cloned().collect()
    }

    async fn replay_events(
        &self,
        scan_id: &Uuid,
    ) -> Option<Vec<ScanBroadcastFrame>> {
        let final_events = self.final_events.read().await;
        final_events
            .get(scan_id)
            .map(|events| events.iter().cloned().collect())
    }

    async fn record_terminal(
        &self,
        snapshot: ScanHistoryEntry,
        final_events: Vec<ScanBroadcastFrame>,
    ) {
        let scan_id = snapshot.scan_id;
        {
            let mut events = self.final_events.write().await;
            events.insert(scan_id, final_events.into_iter().collect());
        }

        let evicted_scan_id = {
            let mut history = self.history.write().await;
            let evicted = if history.len() == HISTORY_CAPACITY {
                history.pop_front().map(|entry| entry.scan_id)
            } else {
                None
            };
            history.push_back(snapshot);
            evicted
        };

        if let Some(evicted_scan_id) = evicted_scan_id {
            let mut events = self.final_events.write().await;
            events.remove(&evicted_scan_id);
        }
    }

    async fn forget_library(&self, library_id: LibraryId) {
        let forgotten_scan_ids: HashSet<Uuid> = {
            let mut history = self.history.write().await;
            let scan_ids = history
                .iter()
                .filter(|entry| entry.library_id == library_id)
                .map(|entry| entry.scan_id)
                .collect();
            history.retain(|entry| entry.library_id != library_id);
            scan_ids
        };

        if !forgotten_scan_ids.is_empty() {
            self.final_events
                .write()
                .await
                .retain(|scan_id, _| !forgotten_scan_ids.contains(scan_id));
        }
    }
}

async fn forget_active_library_runs(
    active_by_scan_id: &RwLock<HashMap<Uuid, Arc<ScanRun>>>,
    active_by_run_key: &RwLock<HashMap<String, Arc<ScanRun>>>,
    library_id: LibraryId,
) -> HashSet<Uuid> {
    let removed_correlations = {
        let mut by_scan_id = active_by_scan_id.write().await;
        let correlations = by_scan_id
            .values()
            .filter(|run| run.library_id() == library_id)
            .map(|run| run.correlation_id())
            .collect();
        by_scan_id.retain(|_, run| run.library_id() != library_id);
        correlations
    };

    active_by_run_key
        .write()
        .await
        .retain(|_, run| run.library_id() != library_id);

    removed_correlations
}

impl ScanRunProgressTracker {
    fn new(
        unit_of_work: Arc<AppUnitOfWork>,
        orchestrator: Arc<ScanOrchestrator>,
        catalog_events: CatalogEventProjection,
        quiescence: Duration,
    ) -> Self {
        let aggregator = ScanRunAggregator::new(
            Arc::clone(&orchestrator),
            quiescence,
            catalog_events.clone(),
        );

        Self {
            inner: Arc::new(ScanRunProgressTrackerInner {
                unit_of_work,
                orchestrator,
                active_by_scan_id: RwLock::new(HashMap::new()),
                active_by_run_key: RwLock::new(HashMap::new()),
                archive: ScanRunProgressArchive::new(),
                catalog_events,
                aggregator,
                movie_batch_notifiers: MovieBatchFinalizationNotifiers::new(),
            }),
        }
    }

    fn active_count(&self) -> Option<usize> {
        self.inner
            .active_by_scan_id
            .try_read()
            .ok()
            .map(|guard| guard.len())
    }

    fn history_count(&self) -> Option<usize> {
        self.inner.archive.history_count()
    }

    async fn subscribe_scan(
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

    async fn register_durable_run(
        &self,
        durable: LibraryScanRun,
    ) -> Arc<ScanRun> {
        let run = ScanRun::from_durable(Arc::clone(&self.inner), durable);
        self.inner.register_run(run).await
    }

    async fn rehydrate_active_runs(&self) -> Result<usize, ScanControlError> {
        let active_runs = self
            .inner
            .unit_of_work
            .scan_runs
            .list_active()
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?;

        let mut restored = 0usize;
        for durable in active_runs {
            let already_active = {
                let guard = self.inner.active_by_scan_id.read().await;
                guard.contains_key(&durable.scan_id)
            };
            if already_active {
                continue;
            }

            let run = self.register_durable_run(durable).await;
            run.seed_rehydrated_progress().await;

            // A paused row remains paused across restart. If it was paused
            // before its seed completed, Resume owns the eventual seed claim.
            if run.lifecycle_status().await == ScanLifecycleStatus::Paused {
                if let Err(err) = self
                    .inner
                    .orchestrator
                    .command_library(
                        run.library_id(),
                        LibraryActorCommand::Pause,
                    )
                    .await
                {
                    warn!(
                        library = %run.library_id(),
                        scan = %run.scan_id(),
                        error = %err,
                        "failed to restore paused library actor during rehydration"
                    );
                }
            } else if let Err(err) =
                ensure_run_seeded(&run, &self.inner.orchestrator).await
            {
                warn!(
                    library = %run.library_id(),
                    scan = %run.scan_id(),
                    error = %err,
                    "failed to replay interrupted scan seed during rehydration"
                );
                let finalized = run
                    .fail_with_reason("rehydrated_start_command_failed")
                    .await;
                if finalized && run.start_mode() == StartMode::Bulk {
                    let _ = self
                        .inner
                        .orchestrator
                        .command_library(
                            run.library_id(),
                            LibraryActorCommand::Start {
                                mode: StartMode::Maintenance,
                                correlation_id: Some(run.correlation_id()),
                            },
                        )
                        .await;
                }
            }
            restored += 1;
        }

        Ok(restored)
    }

    async fn active_scans(&self) -> Vec<ScanSnapshot> {
        let guard = self.inner.active_by_scan_id.read().await;
        let runs: Vec<_> = guard.values().cloned().collect();
        drop(guard);

        let mut snapshots = Vec::with_capacity(runs.len());
        for run in runs {
            if let Ok(snapshot) = run.snapshot().await {
                snapshots.push(snapshot);
            }
        }
        snapshots
    }

    async fn forget_library(&self, library_id: LibraryId) {
        let removed_correlations = forget_active_library_runs(
            &self.inner.active_by_scan_id,
            &self.inner.active_by_run_key,
            library_id,
        )
        .await;

        self.inner.aggregator.forget_library(library_id).await;
        self.inner
            .movie_batch_notifiers
            .forget_library(library_id)
            .await;
        self.inner.archive.forget_library(library_id).await;

        info!(
            library = %library_id,
            active_runs_removed = removed_correlations.len(),
            "forgot deleted library scan progress"
        );
    }

    async fn history(&self, limit: usize) -> Vec<ScanHistoryEntry> {
        self.inner.archive.history(limit).await
    }

    async fn snapshot(&self, scan_id: &Uuid) -> Option<ScanSnapshot> {
        let guard = self.inner.active_by_scan_id.read().await;
        let run = guard.get(scan_id).cloned();
        drop(guard);
        if let Some(run) = run {
            (run.snapshot().await).ok()
        } else {
            None
        }
    }

    async fn events(
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

        self.inner
            .archive
            .replay_events(scan_id)
            .await
            .ok_or(ScanControlError::ScanNotFound)
    }

    async fn lookup(
        &self,
        scan_id: &Uuid,
    ) -> Result<Arc<ScanRun>, ScanControlError> {
        let guard = self.inner.active_by_scan_id.read().await;
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

impl ScanRunProgressTrackerInner {
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
                self.catalog_events.clone(),
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
        self.archive
            .record_terminal(snapshot.clone(), final_events)
            .await;
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

fn lifecycle_allows_seed(status: &ScanLifecycleStatus) -> bool {
    matches!(
        status,
        ScanLifecycleStatus::Pending | ScanLifecycleStatus::Running
    )
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
    inner: Weak<ScanRunProgressTrackerInner>,
    events: Mutex<VecDeque<ScanBroadcastFrame>>,
    start_mode: StartMode,
    log: Mutex<ScanLogWatermark>,
    seed_transition: Mutex<()>,
    finalization: Mutex<bool>,
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
    tracked_job_ids: HashSet<JobId>,
    item_states: HashMap<String, ScanItemState>,
    folder_outcomes_by_path: HashMap<String, FolderScanOutcome>,
    historical_cursor_count: u64,
    seed_completed: bool,
    durable_rebuild_pending: bool,
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

#[derive(Debug, Clone)]
struct ScanTerminalArtifacts {
    snapshot: ScanHistoryEntry,
    progress: LibraryScanRunProgressUpdate,
    terminal_at: DateTime<Utc>,
    last_error: Option<String>,
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
        inner: Arc<ScanRunProgressTrackerInner>,
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
                tracked_job_ids: durable.tracked_job_ids.into_iter().collect(),
                item_states: HashMap::new(),
                folder_outcomes_by_path: HashMap::new(),
                historical_cursor_count: 0,
                seed_completed: durable.seed_completed,
                durable_rebuild_pending: true,
            }),
            tx,
            inner: Arc::downgrade(&inner),
            events: Mutex::new(VecDeque::with_capacity(EVENT_HISTORY_CAPACITY)),
            start_mode: start_mode_from_scan_run_mode(durable.mode),
            log: Mutex::new(ScanLogWatermark::default()),
            seed_transition: Mutex::new(()),
            finalization: Mutex::new(false),
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

    async fn lifecycle_status(&self) -> ScanLifecycleStatus {
        self.state.lock().await.status.clone()
    }

    async fn validate_pause(&self) -> Result<(), ScanControlError> {
        match self.lifecycle_status().await {
            ScanLifecycleStatus::Running | ScanLifecycleStatus::Paused => {
                Ok(())
            }
            ScanLifecycleStatus::Completed
            | ScanLifecycleStatus::Failed
            | ScanLifecycleStatus::Canceled => {
                Err(ScanControlError::ScanTerminal)
            }
            ScanLifecycleStatus::Pending => {
                Err(ScanControlError::ScanNotRunning)
            }
        }
    }

    async fn validate_resume(&self) -> Result<(), ScanControlError> {
        match self.lifecycle_status().await {
            ScanLifecycleStatus::Paused | ScanLifecycleStatus::Running => {
                Ok(())
            }
            ScanLifecycleStatus::Completed
            | ScanLifecycleStatus::Failed
            | ScanLifecycleStatus::Canceled => {
                Err(ScanControlError::ScanTerminal)
            }
            ScanLifecycleStatus::Pending => {
                Err(ScanControlError::ScanNotRunning)
            }
        }
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

    async fn seed_completed(&self) -> bool {
        self.state.lock().await.seed_completed
    }

    async fn mark_seed_command_completed(&self) {
        let payload = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                return;
            }
            state.seed_completed = true;
            state.last_activity_at = Some(Utc::now());
            state.build_current_payload()
        };
        self.persist_progress_payload(&payload).await;
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

    async fn pause(&self) -> Result<(), ScanControlError> {
        if let Some(payload) = self
            .persist_control_status(ScanLifecycleStatus::Paused)
            .await?
        {
            self.publish_frame(ScanEventKind::Progress, payload).await;
        }
        Ok(())
    }

    async fn resume(&self) -> Result<(), ScanControlError> {
        if let Some(payload) = self
            .persist_control_status(ScanLifecycleStatus::Running)
            .await?
        {
            self.publish_frame(ScanEventKind::Progress, payload).await;
        }
        Ok(())
    }

    async fn persist_control_status(
        &self,
        target: ScanLifecycleStatus,
    ) -> Result<Option<ScanProgressEvent>, ScanControlError> {
        let mut state = self.state.lock().await;
        match (&state.status, &target) {
            (ScanLifecycleStatus::Running, ScanLifecycleStatus::Paused)
            | (ScanLifecycleStatus::Paused, ScanLifecycleStatus::Running) => {}
            (current, requested) if current == requested => return Ok(None),
            (
                ScanLifecycleStatus::Completed
                | ScanLifecycleStatus::Failed
                | ScanLifecycleStatus::Canceled,
                _,
            ) => return Err(ScanControlError::ScanTerminal),
            _ => return Err(ScanControlError::ScanNotRunning),
        }

        if let Some(inner) = self.inner.upgrade() {
            let counters = state.counter_snapshot();
            let update = LibraryScanRunProgressUpdate {
                scan_id: self.scan_id,
                status: Some(target.clone().into()),
                completed_items: counters.completed_items,
                total_items: state.total_items,
                retrying_items: counters.retrying_items,
                dead_lettered_items: counters.failed_items,
                current_path: state.current_path.clone(),
                tracked_job_ids: state.sorted_tracked_job_ids(),
                seed_completed: state.seed_completed,
                sequence: state.event_sequence.saturating_add(1),
            };
            let persisted = inner
                .unit_of_work
                .scan_runs
                .update_progress(update)
                .await
                .map_err(|err| {
                    ScanControlError::internal(format!(
                        "failed to persist scan lifecycle transition: {err}"
                    ))
                })?;
            if persisted.is_none() {
                return Err(ScanControlError::internal(
                    "scan lifecycle transition lost its active durable row"
                        .to_string(),
                ));
            }
        }

        state.status = target;
        Ok(Some(state.build_payload()))
    }

    async fn cancel(&self) -> Result<bool, ScanControlError> {
        let frame = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                return Err(ScanControlError::ScanTerminal);
            }
            state.last_error = Some("scan_cancelled".to_string());
            state
                .transition(ScanPhase::Canceled, Utc::now())
                .unwrap_or_else(|| QueuedFrame {
                    event: ScanEventKind::Failed,
                    payload: state.build_payload(),
                })
        };
        self.emit_frame(frame.event, frame.payload).await;
        Ok(self.finalize_history(ScanLifecycleStatus::Canceled).await)
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

    async fn tracked_job_ids(&self) -> Vec<JobId> {
        let state = self.state.lock().await;
        state.sorted_tracked_job_ids()
    }

    async fn tracks_job_id(&self, job_id: JobId) -> bool {
        self.state.lock().await.tracked_job_ids.contains(&job_id)
    }

    async fn needs_pre_stall_reconciliation(
        &self,
        stall_timeout: ChronoDuration,
    ) -> bool {
        let state = self.state.lock().await;
        !state.is_terminal()
            && matches!(
                state.phase,
                ScanPhase::Processing | ScanPhase::Discovering
            )
            && state.outstanding_items_stalled(stall_timeout, Utc::now())
    }

    async fn reconcile_durable_job_states(&self, jobs: &[DurableJobState]) {
        let frames = {
            let mut state = self.state.lock().await;
            state.reconcile_durable_job_states(jobs)
        };
        self.emit_frames(frames).await;
    }

    async fn has_unmaterialized_durable_progress(&self) -> bool {
        let state = self.state.lock().await;
        state.durable_rebuild_pending
            && (state.total_items > 0
                || state.completed_items > 0
                || state.dead_lettered_items > 0
                || state.retrying_items > 0)
    }

    async fn has_active_items(&self) -> bool {
        let state = self.state.lock().await;
        !state.is_terminal()
            && state.item_states.values().any(ScanItemState::is_active)
    }

    async fn record_job_enqueued(
        &self,
        idempotency_key: &str,
        job_id: JobId,
        kind: JobKind,
        path_key: Option<SubjectKey>,
    ) {
        let event_time = Utc::now();
        let path = path_key.clone();
        let frames = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                Vec::new()
            } else {
                let newly_tracked = state.tracked_job_ids.insert(job_id);
                let stale_terminal = state
                    .item_states
                    .get(idempotency_key)
                    .map(|item| {
                        item.is_terminal() && item.last_job_id == Some(job_id)
                    })
                    .unwrap_or(false);

                tracing::debug!(
                    target: "scan::state",
                    scan = %self.scan_id,
                    library = %self.library_id,
                    %job_id,
                    idempotency = idempotency_key,
                    stale_terminal,
                    phase = ?state.phase,
                    kind = ?kind,
                    "record_job_enqueued"
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
                    if newly_tracked {
                        vec![QueuedFrame {
                            event: ScanEventKind::Progress,
                            payload: state.build_payload(),
                        }]
                    } else {
                        Vec::new()
                    }
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

                    if let Some(payload) = state
                        .build_payload_if(changed || reopened || newly_tracked)
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

    async fn record_seed_completed(
        &self,
        summary: &ScanSeedSummary,
        terminalization_allowed: bool,
    ) -> bool {
        let frame = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                None
            } else {
                let has_enrollment = !summary.enrolled_job_ids.is_empty();
                summary.enrolled_job_ids.iter().copied().for_each(|job_id| {
                    state.tracked_job_ids.insert(job_id);
                });

                if !terminalization_allowed {
                    state.seed_completed = true;
                    state.last_activity_at = Some(summary.completed_at);
                    has_enrollment.then(|| QueuedFrame {
                        event: ScanEventKind::Progress,
                        payload: state.build_payload(),
                    })
                } else if let Some(frame) = state.mark_seed_completed(
                    summary.queued_folders,
                    summary.completed_at,
                ) {
                    Some(frame)
                } else {
                    has_enrollment.then(|| QueuedFrame {
                        event: ScanEventKind::Progress,
                        payload: state.build_payload(),
                    })
                }
            }
        };

        if let Some(frame) = frame {
            let event = frame.event.clone();
            self.emit_frame(frame.event, frame.payload).await;
            if matches!(event, ScanEventKind::Completed) {
                return self
                    .finalize_history(ScanLifecycleStatus::Completed)
                    .await;
            }
        }

        false
    }

    async fn record_job_completed(
        &self,
        idempotency_key: &str,
        job_id: JobId,
        kind: JobKind,
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
                    kind = ?kind,
                    "record_job_completed"
                );
                let mut frames = Vec::new();
                let target_status = if kind == JobKind::FolderScan {
                    path.as_ref()
                        .and_then(subject_key_path)
                        .and_then(|path| {
                            state.folder_outcomes_by_path.get(path)
                        })
                        .copied()
                        .map(ScanRunState::status_for_folder_outcome)
                        .unwrap_or(ScanItemStatus::Completed)
                } else {
                    ScanItemStatus::Completed
                };
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

    async fn record_job_lease_renewed(
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
        state.tracked_job_ids.insert(job_id);
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
                "record_job_lease_renewed"
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

    async fn record_job_failure(
        &self,
        idempotency_key: &str,
        job_id: JobId,
        kind: JobKind,
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
                    kind = ?kind,
                    "record_job_failure"
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

    async fn record_job_dead_lettered(
        &self,
        idempotency_key: &str,
        job_id: JobId,
        kind: JobKind,
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
                    kind = ?kind,
                    "record_job_dead_lettered"
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

    async fn fail_with_reason(&self, reason: &str) -> bool {
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
            return self.finalize_history(ScanLifecycleStatus::Failed).await;
        }
        false
    }

    async fn try_complete(
        &self,
        completion_quiescence: ChronoDuration,
        stall_timeout: ChronoDuration,
    ) -> bool {
        let (maybe_frame, finalize_status) = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                // Terminal state is applied in memory before its final
                // PostgreSQL writes. If either write failed transiently, the
                // run remains registered and this tick must retry durable
                // finalization instead of no-oping forever.
                (None, Some(state.status.clone()))
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
            self.emit_frame(frame.event, frame.payload).await;
            if let Some(status) = finalize_status {
                return self.finalize_history(status).await;
            }
            false
        } else if let Some(status) = finalize_status {
            self.finalize_history(status).await
        } else {
            false
        }
    }

    async fn emit_frame(
        &self,
        event: ScanEventKind,
        payload: ScanProgressEvent,
    ) {
        self.persist_progress_payload(&payload).await;
        self.publish_frame(event, payload).await;
    }

    async fn publish_frame(
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

        let _ = self.tx.send(frame.clone());
        let error = if matches!(event, ScanEventKind::Failed) {
            self.failure_reason().await
        } else {
            None
        };
        self.maybe_log_summary(&event, &payload).await;
        if let Some(inner) = self.inner.upgrade() {
            inner
                .catalog_events
                .publish_scan_progress_event(event, payload, error);
        }
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
        let (tracked_job_ids, seed_completed) = {
            let state = self.state.lock().await;
            (state.sorted_tracked_job_ids(), state.seed_completed)
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
                tracked_job_ids,
                seed_completed,
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

    async fn finalize_history(&self, terminal: ScanLifecycleStatus) -> bool {
        let mut finalized = self.finalization.lock().await;
        if *finalized {
            return false;
        }
        let repository_terminal: ApiScanLifecycleStatus =
            terminal.clone().into();

        let artifacts = {
            let state = self.state.lock().await;
            state.terminal_artifacts(terminal.clone())
        };
        let final_events = self.event_log().await;

        let Some(inner) = self.inner.upgrade() else {
            return false;
        };

        let progress_updated = match inner
            .unit_of_work
            .scan_runs
            .update_progress(artifacts.progress)
            .await
        {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(err) => {
                warn!(
                    scan = %self.scan_id,
                    status = ?terminal,
                    error = %err,
                    "failed to persist final scan progress; keeping run active"
                );
                return false;
            }
        };

        let authoritative = if progress_updated {
            match inner
                .unit_of_work
                .scan_runs
                .mark_terminal(
                    self.scan_id,
                    repository_terminal.clone(),
                    artifacts.terminal_at,
                    artifacts.last_error,
                )
                .await
            {
                Ok(Some(run)) => run.status == repository_terminal,
                Ok(None) => false,
                Err(err) => {
                    warn!(
                        scan = %self.scan_id,
                        status = ?terminal,
                        error = %err,
                        "failed to persist terminal scan state; keeping run active"
                    );
                    return false;
                }
            }
        } else {
            false
        };

        // `None` is an expected idempotency result when another finalizer won
        // the compare-and-set. It is success only if PostgreSQL confirms this
        // exact terminal state; a missing row or competing status remains
        // authoritative and must not be replaced by local history.
        let authoritative = if authoritative {
            true
        } else {
            match inner
                .unit_of_work
                .scan_runs
                .load_by_scan_id(self.scan_id)
                .await
            {
                Ok(Some(run)) if run.status == repository_terminal => true,
                Ok(Some(run)) => {
                    warn!(
                        scan = %self.scan_id,
                        requested_status = ?terminal,
                        durable_status = ?run.status,
                        "durable scan terminal state differs; keeping local run active"
                    );
                    false
                }
                Ok(None) => {
                    warn!(
                        scan = %self.scan_id,
                        status = ?terminal,
                        "durable scan row disappeared during finalization; keeping local run active"
                    );
                    false
                }
                Err(err) => {
                    warn!(
                        scan = %self.scan_id,
                        status = ?terminal,
                        error = %err,
                        "failed to verify terminal scan state; keeping local run active"
                    );
                    false
                }
            }
        };

        if !authoritative {
            return false;
        }

        inner
            .finalize_run(
                self.scan_id,
                self.run_key(),
                self.correlation_id,
                artifacts.snapshot,
                final_events,
            )
            .await;
        *finalized = true;
        warn!(scan = %self.scan_id, status = ?terminal, "finalized scan run");
        true
    }
}

async fn ensure_run_seeded(
    run: &Arc<ScanRun>,
    orchestrator: &ScanOrchestrator,
) -> Result<bool, ScanControlError> {
    let _seed_guard = run.seed_transition.lock().await;
    seed_run_locked(run, orchestrator).await
}

async fn seed_run_locked(
    run: &Arc<ScanRun>,
    orchestrator: &ScanOrchestrator,
) -> Result<bool, ScanControlError> {
    let status = run.lifecycle_status().await;
    if run.seed_completed().await || !lifecycle_allows_seed(&status) {
        return Ok(false);
    }

    orchestrator
        .command_library(
            run.library_id(),
            LibraryActorCommand::Start {
                mode: run.start_mode(),
                correlation_id: Some(run.correlation_id()),
            },
        )
        .await
        .map_err(|err| ScanControlError::internal(err.to_string()))?;

    // Mailbox acknowledgement occurs only after the complete seed batch has
    // been enqueued durably. Persist that fact independently of the bounded
    // SeedCompleted broadcast so restart recovery does not rescan it.
    run.mark_seed_command_completed().await;
    Ok(true)
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

    fn reconcile_durable_job_states(
        &mut self,
        jobs: &[DurableJobState],
    ) -> Vec<QueuedFrame> {
        if self.is_terminal() || jobs.is_empty() {
            return Vec::new();
        }

        if self.durable_rebuild_pending {
            // Rehydrated runs carry aggregate counters but not per-job state.
            // The first successful PostgreSQL read is a complete correlation
            // snapshot, so rebuild from it instead of adding to the persisted
            // aggregate and double-counting every recovered job.
            self.completed_items = 0;
            self.total_items = 0;
            self.dead_lettered_items = 0;
            self.retrying_items = 0;
            self.item_states.clear();
            self.durable_rebuild_pending = false;
        }

        // A dedupe key can have an old terminal row and a newer active row.
        // PostgreSQL allows a new generation after the terminal transition;
        // only the newest generation describes the current logical item.
        let mut latest_by_dedupe: HashMap<&str, &DurableJobState> =
            HashMap::with_capacity(jobs.len());
        for job in jobs {
            self.tracked_job_ids.insert(job.job_id);
            let replace = latest_by_dedupe
                .get(job.dedupe_key.as_str())
                .map(|current| {
                    job.created_at > current.created_at
                        || (job.created_at == current.created_at
                            && (job.updated_at > current.updated_at
                                || (job.updated_at == current.updated_at
                                    && job.job_id.0 > current.job_id.0)))
                })
                .unwrap_or(true);
            if replace {
                latest_by_dedupe.insert(job.dedupe_key.as_str(), job);
            }
        }
        let mut latest_jobs: Vec<_> = latest_by_dedupe.into_values().collect();
        latest_jobs.sort_unstable_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.job_id.0.cmp(&right.job_id.0))
        });

        let mut changed = false;
        let mut saw_active = false;
        let mut newest_activity: Option<(DateTime<Utc>, Option<SubjectKey>)> =
            None;

        for job in latest_jobs {
            let target_status = match job.state {
                JobState::Completed => {
                    if job.kind == JobKind::FolderScan {
                        job.path_key
                            .as_ref()
                            .and_then(subject_key_path)
                            .and_then(|path| {
                                self.folder_outcomes_by_path.get(path)
                            })
                            .copied()
                            .map(Self::status_for_folder_outcome)
                            .unwrap_or(ScanItemStatus::Completed)
                    } else {
                        ScanItemStatus::Completed
                    }
                }
                JobState::Failed | JobState::DeadLetter => {
                    ScanItemStatus::DeadLettered
                }
                JobState::Ready if job.attempts > 0 => ScanItemStatus::Retrying,
                JobState::Ready | JobState::Deferred | JobState::Leased => {
                    ScanItemStatus::InProgress
                }
            };
            saw_active |= target_status.is_active();

            changed |= self.update_item_status(
                &job.dedupe_key,
                Some(job.job_id),
                target_status,
                job.updated_at,
                job.path_key.clone(),
                job.last_error.clone(),
            );

            if newest_activity
                .as_ref()
                .map(|(updated_at, _)| job.updated_at > *updated_at)
                .unwrap_or(true)
            {
                newest_activity = Some((job.updated_at, job.path_key.clone()));
            }
        }

        if saw_active
            && matches!(
                self.phase,
                ScanPhase::Initializing
                    | ScanPhase::Discovering
                    | ScanPhase::Quiescing
            )
        {
            self.handle_state_event(ScanStateEvent::NewItemFound, Utc::now());
        } else if matches!(self.phase, ScanPhase::Initializing) {
            // A complete burst can disappear before any enqueue event is
            // observed. Walk through the active phase so the recovered
            // terminal rows can enter quiescence normally.
            self.handle_state_event(ScanStateEvent::NewItemFound, Utc::now());
        }

        if let Some((updated_at, path_key)) = newest_activity {
            let advances_activity = self
                .last_activity_at
                .map(|current| updated_at > current)
                .unwrap_or(true);
            if advances_activity {
                self.last_activity_at = Some(updated_at);
                if let Some(path_key) = path_key {
                    self.current_path = subject_key_path_owned(&path_key);
                    self.path_key = Some(path_key);
                }
            }
        }

        let mut frames = Vec::new();
        if changed {
            frames.push(QueuedFrame {
                event: ScanEventKind::Progress,
                payload: self.build_payload(),
            });
        }

        if self.can_enter_quiescing()
            && let Some(frame) = self.handle_state_event(
                ScanStateEvent::AllItemsProcessed,
                Utc::now(),
            )
        {
            frames.push(frame);
        }

        frames
    }

    fn terminal_artifacts(
        &self,
        terminal: ScanLifecycleStatus,
    ) -> ScanTerminalArtifacts {
        let counters = self.counter_snapshot();
        let completed_items = counters.completed_items;
        let retrying_items = counters.retrying_items;
        let failed_items = counters.failed_items;
        let terminal_at = self.terminal_at.unwrap_or_else(Utc::now);

        ScanTerminalArtifacts {
            snapshot: ScanHistoryEntry {
                scan_id: self.scan_id,
                library_id: self.library_id,
                status: terminal,
                completed_items,
                total_items: self.total_items,
                validated_items: counters.validated_items,
                known_unchanged_items: counters.known_unchanged_items,
                skipped_items: counters.skipped_items,
                failed_items,
                needs_attention_items: counters.needs_attention_items,
                retrying_items,
                started_at: self.started_at,
                terminal_at,
                reason_details: counters.reason_details,
            },
            progress: LibraryScanRunProgressUpdate {
                scan_id: self.scan_id,
                status: None,
                completed_items,
                total_items: self.total_items,
                retrying_items,
                dead_lettered_items: failed_items,
                current_path: self.current_path.clone(),
                tracked_job_ids: self.sorted_tracked_job_ids(),
                seed_completed: self.seed_completed,
                sequence: self.event_sequence,
            },
            terminal_at,
            last_error: self.last_error.clone(),
        }
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
        if let Some(job_id) = job_id {
            self.tracked_job_ids.insert(job_id);
        }
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
                let same_generation =
                    job_id.is_none() || item.last_job_id == job_id;

                // Refuse retrograde transitions within one durable job. A new
                // job ID for the same dedupe key is a newer generation and may
                // legitimately move the logical item back to active.
                if old_status.is_terminal()
                    && !status.is_terminal()
                    && same_generation
                {
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
                    && same_generation
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

    fn sorted_tracked_job_ids(&self) -> Vec<JobId> {
        let mut ids: Vec<_> = self.tracked_job_ids.iter().copied().collect();
        ids.sort_unstable_by_key(|job_id| job_id.0);
        ids
    }

    fn outstanding_items_stalled(
        &self,
        stall_timeout: ChronoDuration,
        now: DateTime<Utc>,
    ) -> bool {
        if self.retrying_items > 0 {
            return false;
        }

        // A large seed batch gives every ready item roughly the same old
        // enqueue timestamp. Those untouched backlog entries are not evidence
        // of a stalled run while other items are still completing. Require the
        // run itself to have been inactive for the full timeout before using
        // per-item timestamps to identify genuinely abandoned work.
        if self
            .last_activity_at
            .is_some_and(|last_activity| now - last_activity <= stall_timeout)
        {
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
            tracked_job_ids: HashSet::new(),
            item_states: HashMap::new(),
            folder_outcomes_by_path: HashMap::new(),
            historical_cursor_count: 0,
            seed_completed: false,
            durable_rebuild_pending: false,
        }
    }

    #[test]
    fn paused_and_terminal_runs_cannot_claim_a_seed() {
        assert!(lifecycle_allows_seed(&ScanLifecycleStatus::Pending));
        assert!(lifecycle_allows_seed(&ScanLifecycleStatus::Running));
        assert!(!lifecycle_allows_seed(&ScanLifecycleStatus::Paused));
        assert!(!lifecycle_allows_seed(&ScanLifecycleStatus::Completed));
        assert!(!lifecycle_allows_seed(&ScanLifecycleStatus::Failed));
        assert!(!lifecycle_allows_seed(&ScanLifecycleStatus::Canceled));
    }

    fn durable_job(
        correlation_id: Uuid,
        job_id: JobId,
        dedupe_key: impl Into<String>,
        state: JobState,
        attempts: u16,
        path: impl Into<String>,
        updated_at: DateTime<Utc>,
    ) -> DurableJobState {
        DurableJobState {
            job_id,
            kind: JobKind::FolderScan,
            media_id: None,
            indexing_change: None,
            series_identity: None,
            state,
            attempts,
            dedupe_key: dedupe_key.into(),
            correlation_id: Some(correlation_id),
            path_key: SubjectKey::path(path.into()).ok(),
            last_error: None,
            created_at: updated_at,
            updated_at,
        }
    }

    fn test_run(state: ScanRunState) -> Arc<ScanRun> {
        let (tx, _rx) = broadcast::channel(16);
        Arc::new(ScanRun {
            scan_id: state.scan_id,
            library_id: state.library_id,
            correlation_id: state.correlation_id,
            state: Mutex::new(state),
            tx,
            inner: Weak::new(),
            events: Mutex::new(VecDeque::new()),
            start_mode: StartMode::Bulk,
            log: Mutex::new(ScanLogWatermark::default()),
            seed_transition: Mutex::new(()),
            finalization: Mutex::new(false),
        })
    }

    #[tokio::test]
    async fn lifecycle_commands_keep_the_run_correlation_immutable() {
        let mut state = test_state();
        state.phase = ScanPhase::Discovering;
        state.status = ScanLifecycleStatus::Running;
        let correlation_id = state.correlation_id;
        let run = test_run(state);

        run.pause().await.expect("running scan pauses");
        assert_eq!(
            run.snapshot().await.unwrap().correlation_id,
            correlation_id
        );

        run.resume().await.expect("paused scan resumes");
        assert_eq!(
            run.snapshot().await.unwrap().correlation_id,
            correlation_id
        );

        assert!(
            !run.cancel().await.expect("running scan cancels"),
            "a local-only test run cannot claim durable finalization"
        );
        assert_eq!(
            run.snapshot().await.unwrap().correlation_id,
            correlation_id
        );
    }

    #[tokio::test]
    async fn forgetting_active_library_runs_removes_both_registry_keys() {
        let deleted_run = test_run(test_state());
        let retained_run = test_run(test_state());
        let deleted_scan_id = deleted_run.scan_id();
        let retained_scan_id = retained_run.scan_id();
        let deleted_correlation = deleted_run.correlation_id();
        let deleted_library = deleted_run.library_id();

        let active_by_scan_id = RwLock::new(HashMap::from([
            (deleted_scan_id, Arc::clone(&deleted_run)),
            (retained_scan_id, Arc::clone(&retained_run)),
        ]));
        let active_by_run_key = RwLock::new(HashMap::from([
            (deleted_run.run_key(), Arc::clone(&deleted_run)),
            (retained_run.run_key(), Arc::clone(&retained_run)),
        ]));

        let removed = forget_active_library_runs(
            &active_by_scan_id,
            &active_by_run_key,
            deleted_library,
        )
        .await;
        let removed_again = forget_active_library_runs(
            &active_by_scan_id,
            &active_by_run_key,
            deleted_library,
        )
        .await;

        assert_eq!(removed, HashSet::from([deleted_correlation]));
        assert!(removed_again.is_empty());
        assert!(
            !active_by_scan_id
                .read()
                .await
                .contains_key(&deleted_scan_id)
        );
        assert!(
            active_by_scan_id
                .read()
                .await
                .contains_key(&retained_scan_id)
        );
        assert_eq!(active_by_run_key.read().await.len(), 1);
        assert!(
            active_by_run_key
                .read()
                .await
                .contains_key(&retained_run.run_key())
        );
    }

    #[tokio::test]
    async fn durable_reconciliation_recovers_burst_larger_than_broadcast_capacity()
     {
        let mut state = test_state();
        let now = Utc::now();
        state.phase = ScanPhase::Discovering;
        state.status = ScanLifecycleStatus::Running;
        let (tx, mut receiver) = tokio::sync::broadcast::channel(256);
        let jobs = (0..300)
            .map(|index| {
                tx.send(index).expect("lag test receiver remains open");
                durable_job(
                    state.correlation_id,
                    JobId::new(),
                    format!("scan:{}:{index}", state.library_id),
                    JobState::Completed,
                    0,
                    format!("/library/movie-{index}"),
                    now + ChronoDuration::milliseconds(index),
                )
            })
            .collect::<Vec<_>>();

        let skipped = match receiver.recv().await {
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                skipped
            }
            other => panic!("expected >256 burst to lag, got {other:?}"),
        };
        assert!(skipped >= 44);

        let frames = state.reconcile_durable_job_states(&jobs);

        assert_eq!(state.total_items, 300);
        assert_eq!(state.completed_items, 300);
        assert_eq!(state.dead_lettered_items, 0);
        assert_eq!(state.phase, ScanPhase::Quiescing);
        assert!(frames.iter().any(|frame| {
            matches!(frame.event, ScanEventKind::Progress)
                && frame.payload.completed_items == 300
        }));
        assert!(
            frames
                .iter()
                .any(|frame| matches!(frame.event, ScanEventKind::Quiescing))
        );
    }

    #[tokio::test]
    async fn lag_gate_blocks_terminalization_until_reconciliation_succeeds() {
        let correlation_id = Uuid::now_v7();
        let gate = DurableReconciliationGate::default();
        let (tx, mut receiver) = tokio::sync::broadcast::channel(256);
        for sequence in 0..300 {
            tx.send(sequence).expect("lag test receiver remains open");
        }

        match receiver.recv().await {
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                gate.require_all([correlation_id]).await;
            }
            other => panic!("expected lagged receiver, got {other:?}"),
        }

        assert!(!gate.allows_terminal(correlation_id).await);
        // A failed durable read does not call `reconciled`, so later events and
        // quiescence ticks remain unable to terminalize the partial state.
        assert!(gate.is_required(correlation_id).await);
        assert!(!gate.allows_terminal(correlation_id).await);

        let ticket = gate.begin_reconciliation(correlation_id).await;
        assert!(gate.reconciled(correlation_id, &ticket).await);
        assert!(gate.allows_terminal(correlation_id).await);
    }

    #[tokio::test]
    async fn reconciliation_pacing_backs_off_until_a_new_trigger_resets_it() {
        let correlation_id = Uuid::now_v7();
        let pacing = DurableReconciliationPacing::default();
        let started_at = Instant::now();

        assert!(pacing.allows_attempt(correlation_id, started_at).await);
        pacing.defer_next_attempt(correlation_id, started_at).await;
        assert!(
            !pacing
                .allows_attempt(
                    correlation_id,
                    started_at + DURABLE_RECONCILIATION_INITIAL_BACKOFF
                        - Duration::from_millis(1),
                )
                .await
        );

        pacing.forget(correlation_id).await;
        assert!(
            pacing
                .allows_attempt(
                    correlation_id,
                    started_at + Duration::from_millis(1),
                )
                .await,
            "a new lag or enrollment trigger must force one immediate attempt"
        );
        pacing.defer_next_attempt(correlation_id, started_at).await;
        assert!(
            !pacing
                .allows_attempt(
                    correlation_id,
                    started_at + Duration::from_millis(2),
                )
                .await,
            "the same still-required gate must not requery every tick"
        );

        let second_attempt =
            started_at + DURABLE_RECONCILIATION_INITIAL_BACKOFF;
        assert!(pacing.allows_attempt(correlation_id, second_attempt).await);
        pacing
            .defer_next_attempt(correlation_id, second_attempt)
            .await;
        assert!(
            !pacing
                .allows_attempt(
                    correlation_id,
                    second_attempt + DURABLE_RECONCILIATION_INITIAL_BACKOFF,
                )
                .await,
            "an unchanged active snapshot doubles the retry delay"
        );
        assert!(
            pacing
                .allows_attempt(
                    correlation_id,
                    second_attempt + DURABLE_RECONCILIATION_INITIAL_BACKOFF * 2,
                )
                .await
        );

        pacing.forget(correlation_id).await;
        assert!(
            pacing.allows_attempt(correlation_id, second_attempt).await,
            "terminal or forgotten runs must discard their cooldown"
        );
    }

    #[tokio::test]
    async fn catalog_lag_gate_waits_for_seed_and_terminal_index_jobs() {
        let correlation_id = Uuid::now_v7();
        let gate = DurableReconciliationGate::default();
        gate.require_catalog_projection_recovery([correlation_id])
            .await;
        let ticket = gate.begin_reconciliation(correlation_id).await;

        assert!(ticket.catalog_projection_required);
        assert!(
            !gate
                .clear_after_seeded_snapshot(
                    correlation_id,
                    &ticket,
                    false,
                    true,
                )
                .await
        );
        assert!(
            !gate
                .clear_after_seeded_snapshot(
                    correlation_id,
                    &ticket,
                    true,
                    false,
                )
                .await
        );
        assert!(gate.is_required(correlation_id).await);
        assert!(
            gate.begin_reconciliation(correlation_id)
                .await
                .catalog_projection_required
        );

        assert!(
            gate.clear_after_seeded_snapshot(
                correlation_id,
                &ticket,
                true,
                true,
            )
            .await
        );
        assert!(gate.allows_terminal(correlation_id).await);
        assert!(
            !gate
                .begin_reconciliation(correlation_id)
                .await
                .catalog_projection_required
        );
    }

    #[tokio::test]
    async fn reconciliation_generation_preserves_triggers_arriving_in_flight() {
        let correlation_id = Uuid::now_v7();
        let retry_job_id = JobId::new();
        let gate = DurableReconciliationGate::default();

        gate.require_all([correlation_id]).await;
        let snapshot_ticket = gate.begin_reconciliation(correlation_id).await;

        // A second lag arrives while the durable snapshot is in flight.
        gate.require_all([correlation_id]).await;
        assert!(
            !gate
                .clear_after_seeded_snapshot(
                    correlation_id,
                    &snapshot_ticket,
                    true,
                    true,
                )
                .await
        );
        assert!(gate.is_required(correlation_id).await);

        let lag_ticket = gate.begin_reconciliation(correlation_id).await;
        // Catalog recovery is requested while that snapshot is projecting.
        gate.require_catalog_projection_recovery([correlation_id])
            .await;
        assert!(
            !gate
                .clear_after_seeded_snapshot(
                    correlation_id,
                    &lag_ticket,
                    true,
                    true,
                )
                .await
        );

        let catalog_ticket = gate.begin_reconciliation(correlation_id).await;
        assert!(catalog_ticket.catalog_projection_required);
        // A retry outcome becomes uncertain before the catalog pass settles.
        gate.require_retryable_failure(correlation_id, retry_job_id)
            .await;
        assert!(
            !gate
                .clear_after_seeded_snapshot(
                    correlation_id,
                    &catalog_ticket,
                    true,
                    true,
                )
                .await
        );

        let latest_ticket = gate.begin_reconciliation(correlation_id).await;
        assert!(latest_ticket.catalog_projection_required);
        assert!(latest_ticket.retryable_failures.contains(&retry_job_id));
        assert!(
            gate.clear_after_seeded_snapshot(
                correlation_id,
                &latest_ticket,
                true,
                true,
            )
            .await
        );
        assert!(gate.allows_terminal(correlation_id).await);
    }

    #[test]
    fn catalog_lag_recovery_remains_pending_while_index_job_is_active() {
        let correlation_id = Uuid::now_v7();
        let now = Utc::now();
        let mut active_index = durable_job(
            correlation_id,
            JobId::new(),
            "index:active",
            JobState::Leased,
            0,
            "/library/active.mkv",
            now,
        );
        active_index.kind = JobKind::IndexUpsert;
        active_index.media_id = Some(ferrex_core::types::MediaID::new(
            ferrex_core::types::VideoMediaType::Movie,
        ));
        active_index.indexing_change = Some(
            ferrex_core::domain::scan::actors::index::IndexingChange::Created,
        );

        assert!(!ScanRunAggregatorInner::catalog_index_jobs_terminal(
            std::slice::from_ref(&active_index)
        ));

        active_index.state = JobState::Completed;
        assert!(ScanRunAggregatorInner::catalog_index_jobs_terminal(&[
            active_index
        ]));
    }

    #[tokio::test]
    async fn retryable_failure_gate_waits_for_authoritative_queue_outcome() {
        let correlation_id = Uuid::now_v7();
        let job_id = JobId::new();
        let now = Utc::now();
        let gate = DurableReconciliationGate::default();
        gate.require_retryable_failure(correlation_id, job_id).await;
        let ticket = gate.begin_reconciliation(correlation_id).await;

        let leased = durable_job(
            correlation_id,
            job_id,
            "scan:retry-uncertain",
            JobState::Leased,
            0,
            "/library/retry-uncertain",
            now,
        );
        assert_eq!(
            gate.unresolved_retryable_failures(&ticket, &[leased]).await,
            vec![(job_id, Some(JobState::Leased))]
        );
        assert!(!gate.allows_terminal(correlation_id).await);

        let ready = durable_job(
            correlation_id,
            job_id,
            "scan:retry-uncertain",
            JobState::Ready,
            1,
            "/library/retry-uncertain",
            now + ChronoDuration::milliseconds(1),
        );
        assert!(
            gate.unresolved_retryable_failures(&ticket, &[ready])
                .await
                .is_empty()
        );
        assert!(gate.reconciled(correlation_id, &ticket).await);
        assert!(gate.allows_terminal(correlation_id).await);
    }

    #[test]
    fn durable_reconciliation_uses_newest_generation_per_dedupe_key() {
        let mut state = test_state();
        let correlation_id = state.correlation_id;
        let now = Utc::now();
        let old_job_id = JobId::new();
        let new_job_id = JobId::new();
        state.phase = ScanPhase::Processing;
        state.status = ScanLifecycleStatus::Running;

        let old_terminal = durable_job(
            correlation_id,
            old_job_id,
            "scan:shared-path",
            JobState::Completed,
            0,
            "/library/shared",
            now - ChronoDuration::seconds(2),
        );
        let new_active = durable_job(
            correlation_id,
            new_job_id,
            "scan:shared-path",
            JobState::Leased,
            0,
            "/library/shared",
            now,
        );

        state.reconcile_durable_job_states(&[old_terminal, new_active]);

        assert_eq!(state.total_items, 1);
        assert_eq!(state.completed_items, 0);
        assert_eq!(state.phase, ScanPhase::Processing);
        assert_eq!(
            state.item_states["scan:shared-path"].last_job_id,
            Some(new_job_id)
        );
        assert!(state.item_states["scan:shared-path"].is_active());
    }

    #[tokio::test]
    async fn merged_job_id_forces_progress_frame_when_status_is_unchanged() {
        let mut state = test_state();
        let old_job_id = JobId::new();
        let merged_job_id = JobId::new();
        state.phase = ScanPhase::Processing;
        state.status = ScanLifecycleStatus::Running;
        state.update_item_status(
            "scan:shared-path",
            Some(old_job_id),
            ScanItemStatus::InProgress,
            Utc::now(),
            SubjectKey::path("/library/shared".to_string()).ok(),
            None,
        );
        let run = test_run(state);
        let mut receiver = run.subscribe();

        run.record_job_enqueued(
            "scan:shared-path",
            merged_job_id,
            JobKind::FolderScan,
            SubjectKey::path("/library/shared".to_string()).ok(),
        )
        .await;

        let frame = receiver
            .try_recv()
            .expect("newly tracked merge emits persistence-bearing progress");
        assert!(matches!(frame.event, ScanEventKind::Progress));
        let tracked = run.tracked_job_ids().await;
        assert!(tracked.contains(&old_job_id));
        assert!(tracked.contains(&merged_job_id));
    }

    #[tokio::test]
    async fn pre_seed_empty_reconciliation_cannot_clear_enrollment_gate() {
        let mut state = test_state();
        state.phase = ScanPhase::Discovering;
        state.status = ScanLifecycleStatus::Running;
        let correlation_id = state.correlation_id;
        let library_id = state.library_id;
        let shared_job_id = JobId::new();
        let run = test_run(state);
        let mut receiver = run.subscribe();
        let gate = DurableReconciliationGate::default();
        gate.require_all([correlation_id]).await;
        let pre_seed_ticket = gate.begin_reconciliation(correlation_id).await;

        // The first ticker can race Start and observe an empty database before
        // its enqueue batch is complete. Production reconcile_run must not
        // clear registration's gate from this pre-seed snapshot.
        run.reconcile_durable_job_states(&[]).await;
        assert!(
            !gate
                .clear_after_seeded_snapshot(
                    correlation_id,
                    &pre_seed_ticket,
                    run.seed_completed().await,
                    true,
                )
                .await
        );
        assert!(gate.is_required(correlation_id).await);

        run.mark_seed_command_completed().await;
        let summary = ScanSeedSummary {
            library_id,
            correlation_id: Some(correlation_id),
            mode: ferrex_core::domain::scan::orchestration::ScanSeedMode::Bulk,
            queued_folders: 1,
            enrolled_job_ids: vec![shared_job_id],
            completed_at: Utc::now(),
        };

        let terminalization_allowed =
            gate.allows_terminal(correlation_id).await;
        assert!(!terminalization_allowed);
        assert!(
            !run.record_seed_completed(&summary, terminalization_allowed)
                .await
        );

        let frame = receiver.try_recv().expect(
            "seed enrollment emits progress while durable reconciliation is gated",
        );
        assert!(matches!(frame.event, ScanEventKind::Progress));
        assert!(run.tracks_job_id(shared_job_id).await);
        assert!(run.state.lock().await.seed_completed);
        // Seed completion alone cannot reinterpret the earlier empty snapshot
        // as a no-work scan. A post-seed read is still mandatory.
        assert!(gate.is_required(correlation_id).await);
        assert!(!gate.allows_terminal(correlation_id).await);
        let early_completion = if gate.allows_terminal(correlation_id).await {
            run.try_complete(
                ChronoDuration::zero(),
                ChronoDuration::seconds(30),
            )
            .await
        } else {
            false
        };
        assert!(!early_completion);
        assert_eq!(run.lifecycle_status().await, ScanLifecycleStatus::Running);

        let post_seed_ticket = gate.begin_reconciliation(correlation_id).await;
        run.reconcile_durable_job_states(&[durable_job(
            correlation_id,
            shared_job_id,
            "scan:seeded-shared",
            JobState::Leased,
            0,
            "/library/shared",
            Utc::now(),
        )])
        .await;
        assert!(
            gate.clear_after_seeded_snapshot(
                correlation_id,
                &post_seed_ticket,
                run.seed_completed().await,
                true,
            )
            .await
        );
        assert!(gate.allows_terminal(correlation_id).await);
        assert!(run.has_active_items().await);
        assert!(
            !run.try_complete(
                ChronoDuration::milliseconds(1),
                ChronoDuration::seconds(30),
            )
            .await
        );
    }

    #[test]
    fn durable_reconciliation_repairs_terminal_job_before_stall() {
        let mut state = test_state();
        let now = Utc::now();
        let stale_at = now - ChronoDuration::seconds(30);
        let job_id = JobId::new();
        state.phase = ScanPhase::Processing;
        state.status = ScanLifecycleStatus::Running;
        state.update_item_status(
            "scan:stalled",
            Some(job_id),
            ScanItemStatus::InProgress,
            stale_at,
            SubjectKey::path("/library/stalled".to_string()).ok(),
            None,
        );
        assert!(
            state.outstanding_items_stalled(ChronoDuration::seconds(5), now)
        );

        state.reconcile_durable_job_states(&[durable_job(
            state.correlation_id,
            job_id,
            "scan:stalled",
            JobState::Completed,
            0,
            "/library/stalled",
            now,
        )]);

        assert_eq!(state.completed_items, 1);
        assert_eq!(state.total_items, 1);
        assert_eq!(state.phase, ScanPhase::Quiescing);
        assert!(!state.outstanding_items_stalled(
            ChronoDuration::seconds(5),
            now + ChronoDuration::seconds(30)
        ));
    }

    #[tokio::test]
    async fn active_durable_snapshot_defers_same_tick_stall_terminalization() {
        let mut state = test_state();
        let now = Utc::now();
        let stale_at = now - ChronoDuration::seconds(30);
        let stall_timeout = ChronoDuration::seconds(5);
        let job_id = JobId::new();
        state.phase = ScanPhase::Processing;
        state.status = ScanLifecycleStatus::Running;
        state.last_activity_at = Some(stale_at);
        state.update_item_status(
            "scan:racing-completion",
            Some(job_id),
            ScanItemStatus::InProgress,
            stale_at,
            SubjectKey::path("/library/racing-completion".to_string()).ok(),
            None,
        );
        let correlation_id = state.correlation_id;
        let run = test_run(state);

        // PostgreSQL still reported the job active when the pre-stall query
        // completed. The timestamps are old enough that an immediate
        // try_complete would otherwise fail the run as stalled.
        run.reconcile_durable_job_states(&[durable_job(
            correlation_id,
            job_id,
            "scan:racing-completion",
            JobState::Leased,
            0,
            "/library/racing-completion",
            stale_at,
        )])
        .await;
        assert!(run.has_active_items().await);
        assert!(
            run.state
                .lock()
                .await
                .outstanding_items_stalled(stall_timeout, now)
        );

        // This is the check_quiescence gate: an active durable snapshot owns
        // this tick's decision, even if the job completes just after it.
        let terminalized = if run.has_active_items().await {
            false
        } else {
            run.try_complete(ChronoDuration::zero(), stall_timeout)
                .await
        };
        assert!(!terminalized);
        assert_eq!(run.state.lock().await.phase, ScanPhase::Processing);

        // The next durable observation sees the racing completion. Terminal
        // durable states do not retain the gate and can quiesce normally.
        run.reconcile_durable_job_states(&[durable_job(
            correlation_id,
            job_id,
            "scan:racing-completion",
            JobState::Completed,
            0,
            "/library/racing-completion",
            now,
        )])
        .await;
        assert!(!run.has_active_items().await);
        assert_eq!(run.state.lock().await.phase, ScanPhase::Quiescing);
        assert!(
            !run.try_complete(ChronoDuration::zero(), stall_timeout)
                .await,
            "in-memory completion is not reported before durable finalization"
        );
        assert_eq!(run.state.lock().await.phase, ScanPhase::Completed);
    }

    #[tokio::test]
    async fn terminal_run_does_not_report_completion_without_durable_confirmation()
     {
        let mut state = test_state();
        let now = Utc::now();
        state.phase = ScanPhase::Failed;
        state.status = ScanLifecycleStatus::Failed;
        state.update_item_status(
            "scan:terminal-but-not-persisted",
            Some(JobId::new()),
            ScanItemStatus::InProgress,
            now,
            SubjectKey::path("/library/terminal".to_string()).ok(),
            None,
        );
        let run = test_run(state);

        assert!(
            !run.has_active_items().await,
            "terminal memory state must not block retrying its durable terminal write"
        );
        assert!(
            !run.try_complete(
                ChronoDuration::milliseconds(1),
                ChronoDuration::seconds(30),
            )
            .await,
            "maintenance side effects must wait for authoritative finalization"
        );
    }

    #[test]
    fn old_ready_backlog_does_not_stall_while_run_is_making_progress() {
        let mut state = test_state();
        let now = Utc::now();
        let stall_timeout = ChronoDuration::seconds(25);
        let seeded_at = now - ChronoDuration::seconds(26);
        state.phase = ScanPhase::Processing;
        state.status = ScanLifecycleStatus::Running;

        for index in 0..3_131 {
            state.update_item_status(
                &format!("folder:{index}"),
                Some(JobId::new()),
                ScanItemStatus::InProgress,
                seeded_at,
                SubjectKey::path(format!("/library/movie-{index}")).ok(),
                None,
            );
        }

        let completed_at = now - ChronoDuration::seconds(1);
        state.update_item_status(
            "folder:0",
            None,
            ScanItemStatus::Completed,
            completed_at,
            SubjectKey::path("/library/movie-0".to_string()).ok(),
            None,
        );
        // Production completion handling records this run-level activity after
        // applying the item transition.
        state.last_activity_at = Some(completed_at);

        assert_eq!(state.completed_items, 1);
        assert_eq!(state.total_items, 3_131);
        assert!(state.item_states.values().any(ScanItemState::is_active));
        assert!(
            !state.outstanding_items_stalled(stall_timeout, now),
            "recent completion must keep an old ready backlog alive"
        );

        state.last_activity_at = Some(seeded_at);
        assert!(
            state.outstanding_items_stalled(stall_timeout, now),
            "the same backlog is stalled once run-level progress also expires"
        );
    }

    #[test]
    fn rehydrated_run_rebuilds_without_double_counting_persisted_progress() {
        let mut state = test_state();
        let now = Utc::now();
        state.phase = ScanPhase::Discovering;
        state.status = ScanLifecycleStatus::Running;
        state.total_items = 2;
        state.completed_items = 2;
        state.durable_rebuild_pending = true;
        let jobs = [
            durable_job(
                state.correlation_id,
                JobId::new(),
                "scan:rehydrated:1",
                JobState::Completed,
                0,
                "/library/one",
                now,
            ),
            durable_job(
                state.correlation_id,
                JobId::new(),
                "scan:rehydrated:2",
                JobState::Completed,
                0,
                "/library/two",
                now + ChronoDuration::milliseconds(1),
            ),
        ];

        state.reconcile_durable_job_states(&jobs);

        assert_eq!(state.total_items, 2);
        assert_eq!(state.completed_items, 2);
        assert_eq!(state.item_states.len(), 2);
        assert!(!state.durable_rebuild_pending);
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
    fn downstream_work_blocks_completion_after_folders_finish() {
        let now = Utc::now();
        let mut state = test_state();
        let folder_path = SubjectKey::path("/library/Movie".to_string()).ok();
        let media_path =
            SubjectKey::path("/library/Movie/feature.mkv".to_string()).ok();

        state.handle_state_event(ScanStateEvent::RunStarted, now);
        state.update_item_status(
            "folder-job",
            Some(JobId::new()),
            ScanItemStatus::InProgress,
            now + ChronoDuration::milliseconds(1),
            folder_path.clone(),
            None,
        );
        state.handle_state_event(
            ScanStateEvent::NewItemFound,
            now + ChronoDuration::milliseconds(1),
        );
        state.update_item_status(
            "index-job",
            Some(JobId::new()),
            ScanItemStatus::InProgress,
            now + ChronoDuration::milliseconds(2),
            media_path.clone(),
            None,
        );
        state.handle_state_event(
            ScanStateEvent::NewItemFound,
            now + ChronoDuration::milliseconds(2),
        );

        state.update_item_status(
            "folder-job",
            Some(JobId::new()),
            ScanItemStatus::Completed,
            now + ChronoDuration::milliseconds(3),
            folder_path,
            None,
        );

        assert_eq!(state.total_items, 2);
        assert_eq!(state.completed_items, 1);
        assert!(!state.can_enter_quiescing());
        assert!(
            state
                .handle_state_event(
                    ScanStateEvent::AllItemsProcessed,
                    now + ChronoDuration::milliseconds(4),
                )
                .is_none(),
            "folder completion alone must not quiesce while downstream work is active"
        );
        assert_eq!(state.status, ScanLifecycleStatus::Running);

        state.update_item_status(
            "index-job",
            Some(JobId::new()),
            ScanItemStatus::Completed,
            now + ChronoDuration::milliseconds(5),
            media_path,
            None,
        );

        assert!(state.can_enter_quiescing());
        let quiescing = state.handle_state_event(
            ScanStateEvent::AllItemsProcessed,
            now + ChronoDuration::milliseconds(6),
        );
        assert!(matches!(
            quiescing.map(|frame| frame.event),
            Some(ScanEventKind::Quiescing)
        ));
        let completed = state.handle_state_event(
            ScanStateEvent::QuiescenceComplete,
            now + ChronoDuration::milliseconds(7),
        );
        assert!(matches!(
            completed.map(|frame| frame.event),
            Some(ScanEventKind::Completed)
        ));
    }

    #[test]
    fn retrying_folder_or_downstream_items_block_terminal_completion() {
        let now = Utc::now();
        let cases = [
            ("folder retry", "folder-job", "/library/Movie"),
            (
                "downstream retry",
                "index-job",
                "/library/Movie/feature.mkv",
            ),
        ];

        for (case, retrying_key, retrying_path) in cases {
            let mut state = test_state();
            state.phase = ScanPhase::Processing;
            state.status = ScanLifecycleStatus::Running;
            state.update_item_status(
                "completed-peer",
                Some(JobId::new()),
                ScanItemStatus::Completed,
                now,
                SubjectKey::path("/library/Movie".to_string()).ok(),
                None,
            );
            state.update_item_status(
                retrying_key,
                Some(JobId::new()),
                ScanItemStatus::Retrying,
                now + ChronoDuration::milliseconds(1),
                SubjectKey::path(retrying_path.to_string()).ok(),
                Some("temporary worker failure".to_string()),
            );

            assert_eq!(state.total_items, 2, "{case}");
            assert_eq!(state.completed_items, 1, "{case}");
            assert_eq!(state.retrying_items, 1, "{case}");
            assert!(!state.can_enter_quiescing(), "{case}");
            assert!(
                state
                    .handle_state_event(
                        ScanStateEvent::AllItemsProcessed,
                        now + ChronoDuration::milliseconds(2),
                    )
                    .is_none(),
                "{case} should stay active while retrying"
            );
            assert!(
                !state.outstanding_items_stalled(
                    ChronoDuration::milliseconds(1),
                    now + ChronoDuration::seconds(5),
                ),
                "{case} should not fail a run while retrying"
            );
        }
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
        let job_id = JobId::new();

        state.handle_state_event(ScanStateEvent::RunStarted, now);
        state.update_item_status(
            "stable-job",
            Some(job_id),
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
            Some(job_id),
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

    #[test]
    fn terminal_artifacts_keep_final_progress_non_terminal() {
        let mut state = test_state();
        let now = Utc::now();
        state.event_sequence = 7;
        state.current_path = Some("/library/attention".to_string());
        state.terminal_at = Some(now);
        state.last_error = Some("scan_failed".to_string());

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
        state.update_item_status(
            "attention-job",
            Some(JobId::new()),
            ScanItemStatus::DeadLettered,
            now,
            SubjectKey::path("/library/attention".to_string()).ok(),
            Some("permission denied".to_string()),
        );

        let artifacts = state.terminal_artifacts(ScanLifecycleStatus::Failed);

        assert_eq!(artifacts.progress.status, None);
        assert_eq!(artifacts.progress.completed_items, 2);
        assert_eq!(artifacts.progress.total_items, 3);
        assert_eq!(artifacts.progress.dead_lettered_items, 1);
        assert_eq!(artifacts.progress.sequence, 7);
        assert_eq!(artifacts.terminal_at, now);
        assert_eq!(artifacts.last_error.as_deref(), Some("scan_failed"));

        assert_eq!(artifacts.snapshot.status, ScanLifecycleStatus::Failed);
        assert_eq!(artifacts.snapshot.completed_items, 2);
        assert_eq!(artifacts.snapshot.failed_items, 1);
        assert_eq!(artifacts.snapshot.needs_attention_items, 1);
        assert_eq!(artifacts.snapshot.terminal_at, now);
        assert!(
            artifacts
                .snapshot
                .reason_details
                .iter()
                .any(|detail| detail.reason_code == "permission_denied")
        );
    }

    fn history_entry(
        scan_id: Uuid,
        library_id: LibraryId,
        completed_items: u64,
    ) -> ScanHistoryEntry {
        ScanHistoryEntry {
            scan_id,
            library_id,
            status: ScanLifecycleStatus::Completed,
            completed_items,
            total_items: completed_items,
            validated_items: completed_items,
            known_unchanged_items: 0,
            skipped_items: 0,
            failed_items: 0,
            needs_attention_items: 0,
            retrying_items: 0,
            started_at: Utc::now(),
            terminal_at: Utc::now(),
            reason_details: Vec::new(),
        }
    }

    fn replay_frame(
        scan_id: Uuid,
        library_id: LibraryId,
        sequence: u64,
    ) -> ScanBroadcastFrame {
        let mut state = test_state();
        state.scan_id = scan_id;
        state.library_id = library_id;
        state.status = ScanLifecycleStatus::Completed;
        state.phase = ScanPhase::Completed;
        state.event_sequence = sequence;
        state.idempotency_prefix = format!("scan:{}:", scan_id);
        ScanBroadcastFrame {
            event: ScanEventKind::Completed,
            payload: state.build_current_payload(),
        }
    }

    #[tokio::test]
    async fn progress_archive_replays_terminal_frames_and_evicts() {
        let archive = ScanRunProgressArchive::new();
        let library_id = LibraryId::new();
        let first_scan_id = Uuid::now_v7();

        archive
            .record_terminal(
                history_entry(first_scan_id, library_id, 1),
                vec![replay_frame(first_scan_id, library_id, 1)],
            )
            .await;

        let replay = archive
            .replay_events(&first_scan_id)
            .await
            .expect("terminal frames should be replayable");
        assert_eq!(replay.len(), 1);
        assert!(matches!(replay[0].event, ScanEventKind::Completed));
        assert_eq!(replay[0].payload.sequence, 1);

        for sequence in 2..=HISTORY_CAPACITY as u64 {
            let scan_id = Uuid::now_v7();
            archive
                .record_terminal(
                    history_entry(scan_id, library_id, sequence),
                    vec![replay_frame(scan_id, library_id, sequence)],
                )
                .await;
        }
        assert!(archive.replay_events(&first_scan_id).await.is_some());

        let evicting_scan_id = Uuid::now_v7();
        archive
            .record_terminal(
                history_entry(
                    evicting_scan_id,
                    library_id,
                    HISTORY_CAPACITY as u64 + 1,
                ),
                vec![replay_frame(
                    evicting_scan_id,
                    library_id,
                    HISTORY_CAPACITY as u64 + 1,
                )],
            )
            .await;

        assert!(archive.replay_events(&first_scan_id).await.is_none());
        let latest = archive.history(1).await;
        assert_eq!(latest[0].scan_id, evicting_scan_id);
        assert_eq!(latest[0].completed_items, HISTORY_CAPACITY as u64 + 1);
    }

    #[tokio::test]
    async fn progress_archive_forgets_only_deleted_library_entries() {
        let archive = ScanRunProgressArchive::new();
        let deleted_library = LibraryId::new();
        let retained_library = LibraryId::new();
        let deleted_scan_id = Uuid::now_v7();
        let retained_scan_id = Uuid::now_v7();

        archive
            .record_terminal(
                history_entry(deleted_scan_id, deleted_library, 1),
                vec![replay_frame(deleted_scan_id, deleted_library, 1)],
            )
            .await;
        archive
            .record_terminal(
                history_entry(retained_scan_id, retained_library, 2),
                vec![replay_frame(retained_scan_id, retained_library, 2)],
            )
            .await;

        archive.forget_library(deleted_library).await;
        archive.forget_library(deleted_library).await;

        assert!(archive.replay_events(&deleted_scan_id).await.is_none());
        assert!(archive.replay_events(&retained_scan_id).await.is_some());
        let history = archive.history(HISTORY_CAPACITY).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].library_id, retained_library);
    }

    fn ready_series_bundle_entry(
        library_id: LibraryId,
        series_id: ferrex_core::types::SeriesID,
        series_root_path: SeriesRootPath,
    ) -> SeriesBundleTrackerEntry {
        use ferrex_core::domain::scan::{
            actors::FolderScanSummary,
            orchestration::{
                AnalyzeScanHierarchy,
                context::{
                    FolderScanContext, SeriesFolderScanContext, SeriesLink,
                    SeriesRef, SeriesScanHierarchy,
                },
            },
        };

        let context = FolderScanContext::Series(SeriesFolderScanContext {
            library_id,
            series_root_path: series_root_path.clone(),
        });
        let mut entry = SeriesBundleTrackerEntry::new(Instant::now());
        entry.tracker.observe_folder_discovered(&context);
        entry
            .tracker
            .observe_folder_scan_completed(&FolderScanSummary {
                context,
                discovered_files: 0,
                enqueued_subfolders: 0,
                listing_hash: "series-root".into(),
                outcome: FolderScanOutcome::Changed,
                completed_at: Utc::now(),
            });
        entry.tracker.observe_indexed(&IndexingOutcome {
            library_id,
            path_norm: series_root_path.as_str().to_owned(),
            media_id: ferrex_core::types::MediaID::Series(series_id),
            hierarchy: AnalyzeScanHierarchy::Series(SeriesScanHierarchy {
                series_root_path,
                series: SeriesLink::Resolved(SeriesRef {
                    id: series_id,
                    slug: Some("claim-race".into()),
                    title: Some("Claim Race".into()),
                }),
            }),
            indexed_at: Utc::now(),
            upserted: true,
            media: None,
            change: ferrex_core::domain::scan::actors::index::IndexingChange::Created,
        });
        entry
    }

    #[tokio::test]
    async fn concurrent_series_bundle_finalizers_claim_once() {
        let library_id = LibraryId::new();
        let series_id = ferrex_core::types::SeriesID(Uuid::now_v7());
        let series_root_path =
            SeriesRootPath::try_new("/library/Claim Race").unwrap();
        let entry = Arc::new(Mutex::new(ready_series_bundle_entry(
            library_id,
            series_id,
            series_root_path.clone(),
        )));
        let barrier = Arc::new(tokio::sync::Barrier::new(3));

        let first = {
            let entry = Arc::clone(&entry);
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                entry.lock().await.claim_next_finalization()
            })
        };
        let second = {
            let entry = Arc::clone(&entry);
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                entry.lock().await.claim_next_finalization()
            })
        };

        barrier.wait().await;
        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap();
        let second = second.unwrap();
        let claims =
            usize::from(first.is_some()) + usize::from(second.is_some());
        assert_eq!(claims, 1, "only one worker may claim the same bundle");
        let claimed = first.or(second).expect("one worker claimed the bundle");

        let mut guard = entry.lock().await;
        assert!(!guard.settle_finalization(&claimed, false));
        let retry = guard.claim_next_finalization().expect(
            "a failed publication releases its claim for polling retries",
        );
        assert!(guard.settle_finalization(&retry, true));
        assert!(
            guard.claim_next_finalization().is_none(),
            "a published bundle must remain finalized"
        );
    }

    #[test]
    fn series_bundle_claim_revalidates_work_enrolled_during_publish() {
        use ferrex_core::domain::scan::orchestration::job::JobPriority;

        let library_id = LibraryId::new();
        let series_id = ferrex_core::types::SeriesID(Uuid::now_v7());
        let series_root_path =
            SeriesRootPath::try_new("/library/Claim Race").unwrap();
        let episode_path = "/library/Claim Race/Season 1/S01E02.mkv";
        let mut entry = ready_series_bundle_entry(
            library_id,
            series_id,
            series_root_path.clone(),
        );

        let claimed = entry
            .claim_next_finalization()
            .expect("the initial bundle is ready");
        let late_job_id = JobId::new();

        // Simulate episode enrollment while catalog publication is awaiting its
        // database projection.
        entry.tracker.observe_job_event(&JobEvent::from_job(
            None,
            library_id,
            "episode:late:index".into(),
            SubjectKey::path(episode_path).ok(),
            JobEventPayload::Enqueued {
                job_id: late_job_id,
                kind: JobKind::IndexUpsert,
                priority: JobPriority::P0,
            },
        ));
        assert!(
            entry.claim_next_finalization().is_none(),
            "the original in-flight claim prevents a duplicate publisher"
        );

        // Even if the late episode terminalizes before publication returns, the
        // original claim still describes the pre-enrollment generation. It must
        // be rejected and published again from the updated generation.
        entry.tracker.observe_job_event(&JobEvent::from_job(
            None,
            library_id,
            "episode:late:index".into(),
            SubjectKey::path(episode_path).ok(),
            JobEventPayload::DeadLettered {
                job_id: late_job_id,
                kind: JobKind::IndexUpsert,
                priority: JobPriority::P0,
            },
        ));
        assert!(
            !entry.settle_finalization(&claimed, true),
            "a successful publication from a stale generation must not finalize the root"
        );

        let retry = entry
            .claim_next_finalization()
            .expect("the updated terminal generation must be published again");
        assert_eq!(retry.series_root_path, series_root_path);
        assert!(entry.settle_finalization(&retry, true));
        assert!(entry.claim_next_finalization().is_none());
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
    catalog_events: CatalogEventProjection,
    series_bundles: Mutex<HashMap<LibraryId, SeriesBundleTrackerEntry>>,
    reconciliation_gate: DurableReconciliationGate,
    durable_reconciliation_pacing: DurableReconciliationPacing,
}

#[derive(Default)]
struct DurableReconciliationPacing {
    state: Mutex<HashMap<Uuid, DurableReconciliationPacingState>>,
}

#[derive(Clone, Copy)]
struct DurableReconciliationPacingState {
    not_before: Instant,
    backoff: Duration,
}

impl DurableReconciliationPacing {
    async fn allows_attempt(&self, correlation_id: Uuid, now: Instant) -> bool {
        self.state
            .lock()
            .await
            .get(&correlation_id)
            .is_none_or(|state| now >= state.not_before)
    }

    async fn defer_next_attempt(&self, correlation_id: Uuid, now: Instant) {
        let mut state = self.state.lock().await;
        let backoff = state
            .get(&correlation_id)
            .map(|current| {
                current
                    .backoff
                    .checked_mul(2)
                    .unwrap_or(DURABLE_RECONCILIATION_MAX_BACKOFF)
                    .min(DURABLE_RECONCILIATION_MAX_BACKOFF)
            })
            .unwrap_or(DURABLE_RECONCILIATION_INITIAL_BACKOFF);
        state.insert(
            correlation_id,
            DurableReconciliationPacingState {
                not_before: now + backoff,
                backoff,
            },
        );
    }

    async fn forget(&self, correlation_id: Uuid) {
        self.state.lock().await.remove(&correlation_id);
    }
}

#[derive(Default)]
struct DurableReconciliationGate {
    state: Mutex<DurableReconciliationRequirements>,
}

#[derive(Default)]
struct DurableReconciliationRequirements {
    next_generation: u64,
    by_correlation: HashMap<Uuid, DurableReconciliationRequirement>,
}

#[derive(Clone, Default)]
struct DurableReconciliationRequirement {
    generation: u64,
    catalog_projection_required: bool,
    retryable_failures: HashSet<JobId>,
}

#[derive(Clone, Default)]
struct DurableReconciliationTicket {
    generation: Option<u64>,
    catalog_projection_required: bool,
    retryable_failures: HashSet<JobId>,
}

impl DurableReconciliationRequirements {
    fn require(
        &mut self,
        correlation_id: Uuid,
    ) -> &mut DurableReconciliationRequirement {
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("durable reconciliation generation exhausted");
        let generation = self.next_generation;
        let requirement =
            self.by_correlation.entry(correlation_id).or_default();
        requirement.generation = generation;
        requirement
    }
}

impl DurableReconciliationGate {
    async fn require_all(&self, correlations: impl IntoIterator<Item = Uuid>) {
        let mut state = self.state.lock().await;
        for correlation_id in correlations {
            state.require(correlation_id);
        }
    }

    async fn require_catalog_projection_recovery(
        &self,
        correlations: impl IntoIterator<Item = Uuid>,
    ) {
        let mut state = self.state.lock().await;
        for correlation_id in correlations {
            state.require(correlation_id).catalog_projection_required = true;
        }
    }

    async fn require_retryable_failure(
        &self,
        correlation_id: Uuid,
        job_id: JobId,
    ) {
        let mut state = self.state.lock().await;
        state
            .require(correlation_id)
            .retryable_failures
            .insert(job_id);
    }

    async fn is_required(&self, correlation_id: Uuid) -> bool {
        self.state
            .lock()
            .await
            .by_correlation
            .contains_key(&correlation_id)
    }

    async fn allows_terminal(&self, correlation_id: Uuid) -> bool {
        !self.is_required(correlation_id).await
    }

    async fn begin_reconciliation(
        &self,
        correlation_id: Uuid,
    ) -> DurableReconciliationTicket {
        self.state
            .lock()
            .await
            .by_correlation
            .get(&correlation_id)
            .map(|requirement| DurableReconciliationTicket {
                generation: Some(requirement.generation),
                catalog_projection_required: requirement
                    .catalog_projection_required,
                retryable_failures: requirement.retryable_failures.clone(),
            })
            .unwrap_or_default()
    }

    async fn reconciled(
        &self,
        correlation_id: Uuid,
        ticket: &DurableReconciliationTicket,
    ) -> bool {
        let mut state = self.state.lock().await;
        let current_generation = state
            .by_correlation
            .get(&correlation_id)
            .map(|requirement| requirement.generation);
        if current_generation != ticket.generation {
            return false;
        }
        state.by_correlation.remove(&correlation_id);
        true
    }

    async fn clear_after_seeded_snapshot(
        &self,
        correlation_id: Uuid,
        ticket: &DurableReconciliationTicket,
        seed_completed: bool,
        catalog_projection_complete: bool,
    ) -> bool {
        if !seed_completed || !catalog_projection_complete {
            return false;
        }
        self.reconciled(correlation_id, ticket).await
    }

    async fn unresolved_retryable_failures(
        &self,
        ticket: &DurableReconciliationTicket,
        jobs: &[DurableJobState],
    ) -> Vec<(JobId, Option<JobState>)> {
        if ticket.retryable_failures.is_empty() {
            return Vec::new();
        }

        let durable_states: HashMap<_, _> =
            jobs.iter().map(|job| (job.job_id, job.state)).collect();
        ticket
            .retryable_failures
            .iter()
            .copied()
            .filter_map(|job_id| {
                let state = durable_states.get(&job_id).copied();
                (!matches!(
                    state,
                    Some(
                        JobState::Ready
                            | JobState::DeadLetter
                            | JobState::Completed
                    )
                ))
                .then_some((job_id, state))
            })
            .collect()
    }

    async fn forget(&self, correlation_id: Uuid) {
        self.state
            .lock()
            .await
            .by_correlation
            .remove(&correlation_id);
    }
}

#[derive(Debug)]
struct SeriesBundleTrackerEntry {
    tracker: SeriesBundleTracker,
    finalizations_in_flight: HashSet<SeriesRootPath>,
    last_touched_at: Instant,
    last_polled_at: Instant,
}

impl SeriesBundleTrackerEntry {
    fn new(now: Instant) -> Self {
        Self {
            tracker: SeriesBundleTracker::default(),
            finalizations_in_flight: HashSet::new(),
            last_touched_at: now,
            last_polled_at: now,
        }
    }

    fn touch(&mut self, now: Instant) {
        self.last_touched_at = now;
    }

    fn claim_next_finalization(&mut self) -> Option<SeriesBundleFinalization> {
        self.tracker
            .finalization_candidates()
            .into_iter()
            .find(|candidate| {
                self.finalizations_in_flight
                    .insert(candidate.series_root_path.clone())
            })
    }

    fn settle_finalization(
        &mut self,
        finalization: &SeriesBundleFinalization,
        published: bool,
    ) -> bool {
        if !self
            .finalizations_in_flight
            .remove(&finalization.series_root_path)
        {
            return false;
        }

        published && self.tracker.mark_finalized_if_still_eligible(finalization)
    }
}

impl ScanRunAggregator {
    fn new(
        orchestrator: Arc<ScanOrchestrator>,
        quiescence: Duration,
        catalog_events: CatalogEventProjection,
    ) -> Self {
        let chrono_window = ChronoDuration::from_std(quiescence)
            .unwrap_or_else(|_| ChronoDuration::seconds(3));
        let stall_std = quiescence
            .checked_mul(STALLED_SCAN_TIMEOUT_MULTIPLIER)
            .unwrap_or(Duration::from_secs(60));
        let stall_window = ChronoDuration::from_std(stall_std)
            .unwrap_or_else(|_| ChronoDuration::seconds(60));
        let durable_reconciliation_pacing =
            DurableReconciliationPacing::default();
        let inner = Arc::new(ScanRunAggregatorInner {
            orchestrator,
            runs: RwLock::new(HashMap::new()),
            quiescence_chrono: chrono_window,
            stall_timeout: stall_window,
            catalog_events,
            series_bundles: Mutex::new(HashMap::new()),
            reconciliation_gate: DurableReconciliationGate::default(),
            durable_reconciliation_pacing,
        });

        let aggregator = Self {
            inner: inner.clone(),
        };
        aggregator.spawn_worker();
        aggregator
    }

    fn spawn_worker(&self) {
        let job_events = Arc::clone(&self.inner);
        spawn(async move {
            ScanRunAggregatorInner::run_job_events(job_events).await;
        });

        let durable_reconciliation = Arc::clone(&self.inner);
        spawn(async move {
            ScanRunAggregatorInner::run_durable_reconciliation(
                durable_reconciliation,
            )
            .await;
        });

        let scan_events = Arc::clone(&self.inner);
        spawn(async move {
            ScanRunAggregatorInner::run_scan_events(scan_events).await;
        });
    }

    async fn register(&self, run: Arc<ScanRun>) {
        let correlation_id = run.correlation_id();
        let mut guard = self.inner.runs.write().await;
        guard.insert(correlation_id, run);
        drop(guard);
        // Registration covers both new and restart-rehydrated runs. Require a
        // successful durable read before either path can terminalize; for a
        // new run the initial snapshot is simply empty.
        self.inner
            .reconciliation_gate
            .require_all([correlation_id])
            .await;
        self.inner
            .durable_reconciliation_pacing
            .forget(correlation_id)
            .await;
    }

    async fn drop(&self, correlation_id: &Uuid) {
        let mut guard = self.inner.runs.write().await;
        guard.remove(correlation_id);
        drop(guard);
        self.inner.reconciliation_gate.forget(*correlation_id).await;
        self.inner
            .durable_reconciliation_pacing
            .forget(*correlation_id)
            .await;
    }

    async fn forget_library(&self, library_id: LibraryId) {
        let correlations = {
            let mut runs = self.inner.runs.write().await;
            let correlations: Vec<_> = runs
                .iter()
                .filter(|(_, run)| run.library_id() == library_id)
                .map(|(correlation_id, _)| *correlation_id)
                .collect();
            for correlation_id in &correlations {
                runs.remove(correlation_id);
            }
            correlations
        };

        for correlation_id in correlations {
            self.inner.reconciliation_gate.forget(correlation_id).await;
            self.inner
                .durable_reconciliation_pacing
                .forget(correlation_id)
                .await;
        }
        self.inner.series_bundles.lock().await.remove(&library_id);
    }
}

impl ScanRunAggregatorInner {
    async fn run_job_events(self: Arc<Self>) {
        use tokio::sync::broadcast::error::RecvError;

        let mut receiver = self.orchestrator.subscribe_job_events();

        loop {
            match receiver.recv().await {
                Ok(event) => self.handle_job_event(event).await,
                Err(RecvError::Lagged(skipped)) => {
                    warn!("scan aggregator lagged {skipped} events");
                    // Keep draining immediately. PostgreSQL reconciliation can
                    // require decoding every job in a large run; doing that in
                    // this receiver loop creates a feedback cycle where the
                    // durable read itself causes another broadcast lag.
                    self.mark_all_runs_reconciliation_required().await;
                }
                Err(RecvError::Closed) => break,
            }
        }
    }

    async fn run_durable_reconciliation(self: Arc<Self>) {
        let mut ticker = interval(Duration::from_millis(500));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            self.check_quiescence().await;
        }
    }

    async fn run_scan_events(self: Arc<Self>) {
        use tokio::sync::broadcast::error::RecvError;

        let mut receiver = self.orchestrator.subscribe_scan_events();
        loop {
            match receiver.recv().await {
                Ok(event) => self.handle_scan_event(event).await,
                Err(RecvError::Lagged(skipped)) => {
                    warn!("domain event stream lagged {skipped} events");
                    // Keep PostgreSQL authoritative for terminal progress, but
                    // never block catalog projection behind that durable read.
                    // The independent progress worker will reconcile this gate
                    // on its next quiescence tick.
                    self.mark_all_runs_catalog_reconciliation_required().await;
                }
                Err(RecvError::Closed) => break,
            }
        }
    }

    async fn check_quiescence(&self) {
        let runs: Vec<Arc<ScanRun>> = {
            let guard = self.runs.read().await;
            guard.values().cloned().collect()
        };

        for run in runs {
            let explicit_reconciliation_required = self
                .reconciliation_gate
                .is_required(run.correlation_id())
                .await;
            let pre_stall =
                run.needs_pre_stall_reconciliation(self.stall_timeout).await;
            let reconciliation_requested =
                explicit_reconciliation_required || pre_stall;
            let reconciliation_due = reconciliation_requested
                && self
                    .durable_reconciliation_pacing
                    .allows_attempt(run.correlation_id(), Instant::now())
                    .await;

            if reconciliation_requested && !reconciliation_due {
                // The last attempt still observed active jobs, an incomplete
                // seed, or an unavailable durable store. Preserve that gate
                // until its bounded retry is due instead of re-reading the
                // entire run every 500 ms.
                continue;
            }

            if reconciliation_due {
                let reason = if explicit_reconciliation_required {
                    "lag_retry"
                } else {
                    "pre_stall"
                };
                if let Err(err) = self.reconcile_run(&run, reason).await {
                    warn!(
                        scan = %run.scan_id(),
                        library = %run.library_id(),
                        error = %err,
                        reason,
                        "durable scan reconciliation failed; deferring terminal decision"
                    );
                    self.durable_reconciliation_pacing
                        .defer_next_attempt(
                            run.correlation_id(),
                            Instant::now(),
                        )
                        .await;
                    continue;
                }

                // PostgreSQL is authoritative for job lifecycle state. If a
                // successful snapshot still contains active work, do not use
                // the same stale timestamps to terminalize the run on this
                // tick. A completion racing just after the snapshot will be
                // observed by its event or by the next reconciliation.
                if run.has_active_items().await {
                    tracing::debug!(
                        scan = %run.scan_id(),
                        library = %run.library_id(),
                        reason,
                        "durable jobs remain active; deferring terminal decision"
                    );
                    continue;
                }
            }

            // A registration gate must survive an empty snapshot taken while
            // Start is still enqueueing its seed batch. Seed completion is
            // persisted separately; until a successful snapshot after that
            // marker, total_items == 0 is not authoritative evidence of a
            // no-work scan.
            if !self
                .reconciliation_gate
                .allows_terminal(run.correlation_id())
                .await
            {
                continue;
            }

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

    async fn mark_all_runs_reconciliation_required(&self) {
        let correlations: Vec<Uuid> =
            self.runs.read().await.keys().copied().collect();
        self.reconciliation_gate
            .require_all(correlations.iter().copied())
            .await;
        for correlation_id in correlations {
            self.durable_reconciliation_pacing
                .forget(correlation_id)
                .await;
        }
    }

    async fn mark_all_runs_catalog_reconciliation_required(&self) {
        let correlations: Vec<Uuid> =
            self.runs.read().await.keys().copied().collect();
        self.reconciliation_gate
            .require_catalog_projection_recovery(correlations.iter().copied())
            .await;
        for correlation_id in correlations {
            self.durable_reconciliation_pacing
                .forget(correlation_id)
                .await;
        }
    }

    async fn reconcile_run(
        &self,
        run: &Arc<ScanRun>,
        reason: &'static str,
    ) -> ferrex_core::error::Result<()> {
        // This ticket fences every async read and projection below. A lag,
        // retry, or catalog trigger that arrives while reconciliation is in
        // flight advances the generation and cannot be cleared by this older
        // snapshot.
        let reconciliation_ticket = self
            .reconciliation_gate
            .begin_reconciliation(run.correlation_id())
            .await;
        let tracked_job_ids = run.tracked_job_ids().await;
        let jobs = self
            .orchestrator
            .durable_job_states(run.correlation_id(), &tracked_job_ids)
            .await?;
        if jobs.is_empty()
            && (!tracked_job_ids.is_empty()
                || run.has_unmaterialized_durable_progress().await)
        {
            return Err(ferrex_core::error::MediaError::Internal(format!(
                "durable job snapshot for scan {} was empty despite tracked jobs or persisted progress",
                run.scan_id()
            )));
        }
        let durable_job_ids: HashSet<_> =
            jobs.iter().map(|job| job.job_id).collect();
        let missing_job_ids: Vec<_> = tracked_job_ids
            .iter()
            .copied()
            .filter(|job_id| !durable_job_ids.contains(job_id))
            .collect();
        if !missing_job_ids.is_empty() {
            return Err(ferrex_core::error::MediaError::Internal(format!(
                "durable job snapshot for scan {} omitted {} tracked jobs",
                run.scan_id(),
                missing_job_ids.len()
            )));
        }
        let unresolved_retryable_failures = self
            .reconciliation_gate
            .unresolved_retryable_failures(&reconciliation_ticket, &jobs)
            .await;
        if !unresolved_retryable_failures.is_empty() {
            return Err(ferrex_core::error::MediaError::Internal(format!(
                "durable retry outcome for scan {} remains unresolved: {:?}",
                run.scan_id(),
                unresolved_retryable_failures
            )));
        }

        let catalog_projection_complete =
            if reconciliation_ticket.catalog_projection_required {
                self.reconcile_catalog_projections(run, &jobs).await
            } else {
                true
            };

        tracing::debug!(
            scan = %run.scan_id(),
            library = %run.library_id(),
            correlation = %run.correlation_id(),
            tracked_jobs = tracked_job_ids.len(),
            durable_jobs = jobs.len(),
            reason,
            "reconciling scan progress from durable job state"
        );
        run.reconcile_durable_job_states(&jobs).await;
        if jobs.iter().any(|job| job.series_identity.is_some()) {
            let now = Instant::now();
            let mut guard = self.series_bundles.lock().await;
            let entry = guard
                .entry(run.library_id())
                .or_insert_with(|| SeriesBundleTrackerEntry::new(now));
            entry.touch(now);
            entry
                .tracker
                .reconcile_durable_job_states(run.library_id(), &jobs);
        }
        if !self
            .reconciliation_gate
            .clear_after_seeded_snapshot(
                run.correlation_id(),
                &reconciliation_ticket,
                run.seed_completed().await,
                catalog_projection_complete,
            )
            .await
        {
            tracing::debug!(
                scan = %run.scan_id(),
                library = %run.library_id(),
                reason,
                "durable snapshot did not satisfy the current reconciliation generation; retaining gate"
            );
        }

        if self
            .reconciliation_gate
            .is_required(run.correlation_id())
            .await
            || run.has_active_items().await
        {
            self.durable_reconciliation_pacing
                .defer_next_attempt(run.correlation_id(), Instant::now())
                .await;
        } else {
            self.durable_reconciliation_pacing
                .forget(run.correlation_id())
                .await;
        }
        Ok(())
    }

    async fn reconcile_catalog_projections(
        &self,
        run: &Arc<ScanRun>,
        jobs: &[DurableJobState],
    ) -> bool {
        let mut complete = Self::catalog_index_jobs_terminal(jobs);
        for job in jobs.iter().filter(|job| {
            job.kind == JobKind::IndexUpsert && job.state == JobState::Completed
        }) {
            let Some(media_id) = job.media_id else {
                warn!(
                    scan = %run.scan_id(),
                    library = %run.library_id(),
                    job = %job.job_id.0,
                    "completed index job omitted durable media identity"
                );
                complete = false;
                continue;
            };
            let Some(change) = job.indexing_change else {
                warn!(
                    scan = %run.scan_id(),
                    library = %run.library_id(),
                    job = %job.job_id.0,
                    "completed index job omitted durable catalog change semantics"
                );
                complete = false;
                continue;
            };
            let Some(path_norm) =
                job.path_key.as_ref().and_then(subject_key_path)
            else {
                warn!(
                    scan = %run.scan_id(),
                    library = %run.library_id(),
                    job = %job.job_id.0,
                    "completed index job omitted durable path identity"
                );
                complete = false;
                continue;
            };

            match self
                .catalog_events
                .publish_reconciled_indexed_media(
                    run.library_id(),
                    path_norm,
                    media_id,
                    change,
                )
                .await
            {
                Ok(Some(frame)) => {
                    tracing::debug!(
                        scan = %run.scan_id(),
                        library = %run.library_id(),
                        job = %job.job_id.0,
                        sequence = frame.sequence,
                        "recovered catalog projection from durable index job"
                    );
                }
                Ok(None) => {}
                Err(err) => {
                    complete = false;
                    warn!(
                        scan = %run.scan_id(),
                        library = %run.library_id(),
                        job = %job.job_id.0,
                        error = %err,
                        "failed to recover catalog projection from durable index job"
                    );
                }
            }
        }

        complete
    }

    fn catalog_index_jobs_terminal(jobs: &[DurableJobState]) -> bool {
        jobs.iter()
            .filter(|job| job.kind == JobKind::IndexUpsert)
            .all(DurableJobState::is_terminal)
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
                || !entry.finalizations_in_flight.is_empty()
                || now.duration_since(entry.last_touched_at)
                    < SERIES_BUNDLE_TRACKER_IDLE_TTL
        });
    }

    async fn handle_job_event(&self, event: JobEvent) {
        let candidates: Vec<Arc<ScanRun>> = {
            let guard = self.runs.read().await;
            guard.values().cloned().collect()
        };
        let shared_job_id = match &event.payload {
            JobEventPayload::Completed { job_id, .. }
            | JobEventPayload::Failed { job_id, .. }
            | JobEventPayload::DeadLettered { job_id, .. } => Some(*job_id),
            _ => None,
        };
        let mut runs = Vec::new();
        for candidate in candidates {
            let correlation_match =
                candidate.correlation_id() == event.meta.correlation_id;
            let tracked_job_match = if correlation_match {
                false
            } else if let Some(job_id) = shared_job_id {
                candidate.library_id() == event.meta.library_id
                    && candidate.tracks_job_id(job_id).await
            } else {
                false
            };
            if correlation_match || tracked_job_match {
                runs.push(candidate);
            }
        }

        self.observe_series_bundle_job_event(&event).await;

        if runs.is_empty() {
            self.handle_orphan_event(&event).await;
            return;
        }

        for run in runs {
            let terminalization_allowed = self
                .reconciliation_gate
                .allows_terminal(run.correlation_id())
                .await;
            let completed = match &event.payload {
                JobEventPayload::Enqueued { kind, job_id, .. }
                | JobEventPayload::Dequeued { kind, job_id, .. } => {
                    run.record_job_enqueued(
                        &event.meta.idempotency_key,
                        *job_id,
                        *kind,
                        event.meta.path_key.clone(),
                    )
                    .await;
                    false
                }
                JobEventPayload::Merged {
                    existing_job_id,
                    kind,
                    ..
                } => {
                    run.record_job_enqueued(
                        &event.meta.idempotency_key,
                        *existing_job_id,
                        *kind,
                        event.meta.path_key.clone(),
                    )
                    .await;
                    false
                }
                JobEventPayload::Completed { kind, job_id, .. } => {
                    run.record_job_completed(
                        &event.meta.idempotency_key,
                        *job_id,
                        *kind,
                        event.meta.path_key.clone(),
                    )
                    .await;
                    terminalization_allowed
                        && run
                            .try_complete(
                                self.quiescence_chrono,
                                self.stall_timeout,
                            )
                            .await
                }
                JobEventPayload::Failed {
                    kind,
                    retryable,
                    job_id,
                    ..
                } => {
                    if *retryable {
                        // The runtime can publish this event even when
                        // q.fail returned an error, so it is not proof that
                        // PostgreSQL actually moved the lease to Ready (or
                        // exhausted it to DeadLetter). Keep terminalization
                        // gated until an exact job-ID read observes the
                        // authoritative retry outcome.
                        self.reconciliation_gate
                            .require_retryable_failure(
                                run.correlation_id(),
                                *job_id,
                            )
                            .await;
                        self.durable_reconciliation_pacing
                            .forget(run.correlation_id())
                            .await;
                    }
                    run.record_job_failure(
                        &event.meta.idempotency_key,
                        *job_id,
                        *kind,
                        None,
                        event.meta.path_key.clone(),
                        *retryable,
                    )
                    .await;

                    if *retryable {
                        // Keep the broadcast receiver draining. The dedicated
                        // durable worker observes the reset gate on its next
                        // tick and confirms the authoritative retry outcome.
                        false
                    } else if terminalization_allowed {
                        run.try_complete(
                            self.quiescence_chrono,
                            self.stall_timeout,
                        )
                        .await
                    } else {
                        false
                    }
                }
                JobEventPayload::DeadLettered { kind, job_id, .. } => {
                    run.record_job_dead_lettered(
                        &event.meta.idempotency_key,
                        *job_id,
                        *kind,
                        None,
                        event.meta.path_key.clone(),
                    )
                    .await;
                    terminalization_allowed
                        && run
                            .try_complete(
                                self.quiescence_chrono,
                                self.stall_timeout,
                            )
                            .await
                }
                JobEventPayload::LeaseRenewed { job_id, .. } => {
                    run.record_job_lease_renewed(
                        &event.meta.idempotency_key,
                        *job_id,
                        event.meta.path_key.clone(),
                    )
                    .await;
                    false
                }
                _ => false,
            };

            if completed {
                self.on_run_completed(run.clone()).await;
            }
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

                    if let Some(run) = run {
                        let has_enrollment =
                            !summary.enrolled_job_ids.is_empty();
                        // Seed completion changes whether the registration
                        // snapshot is authoritative, even for an empty run.
                        // Let the dedicated worker retry immediately.
                        self.durable_reconciliation_pacing
                            .forget(correlation_id)
                            .await;
                        if has_enrollment {
                            // Even if every enqueue notification was dropped,
                            // the seed mailbox has the exact durable job IDs.
                            // Force a PostgreSQL read before a zero-item run
                            // can make a terminal decision.
                            self.reconciliation_gate
                                .require_all([correlation_id])
                                .await;
                        }
                        let terminalization_allowed = self
                            .reconciliation_gate
                            .allows_terminal(correlation_id)
                            .await;
                        let completed = run
                            .record_seed_completed(
                                &summary,
                                terminalization_allowed,
                            )
                            .await;
                        if has_enrollment
                            && let Err(err) = self
                                .reconcile_run(&run, "post_seed_enrollment")
                                .await
                        {
                            self.durable_reconciliation_pacing
                                .defer_next_attempt(
                                    correlation_id,
                                    Instant::now(),
                                )
                                .await;
                            warn!(
                                scan = %run.scan_id(),
                                library = %run.library_id(),
                                error = %err,
                                "post-seed durable reconciliation failed; terminalization remains gated"
                            );
                        }
                        if completed {
                            self.on_run_completed(run).await;
                        }
                    }
                }
            }
            ScanEvent::Indexed(outcome) => {
                let outcome = *outcome;
                let result = self
                    .catalog_events
                    .publish_indexed_outcome(outcome.clone())
                    .await;
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
        loop {
            // The job-progress and scan-domain workers can both make the same
            // bundle eligible at nearly the same time. Claim the candidate
            // while holding the tracker mutex so only one worker can publish
            // it across the asynchronous database/event projection below.
            let finalization = {
                let mut guard = self.series_bundles.lock().await;
                guard
                    .get_mut(&library_id)
                    .and_then(SeriesBundleTrackerEntry::claim_next_finalization)
            };
            let Some(finalization) = finalization else {
                break;
            };

            let receivers = self.catalog_events.receiver_count();
            let frame = self
                .catalog_events
                .publish_series_bundle_finalized(
                    finalization.library_id,
                    finalization.series_id,
                )
                .await;

            let mut guard = self.series_bundles.lock().await;
            if let Some(entry) = guard.get_mut(&library_id) {
                entry.settle_finalization(&finalization, frame.is_some());
            }
            drop(guard);

            let Some(frame) = frame else {
                // Publication failures are retried by the periodic poll. Do
                // not immediately reclaim the same candidate in a tight loop.
                break;
            };

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

    async fn on_run_completed(&self, run: Arc<ScanRun>) {
        if run.start_mode() != StartMode::Bulk {
            return;
        }

        let library_id = run.library_id();
        let command = LibraryActorCommand::Start {
            mode: StartMode::Maintenance,
            // The actor uses the ending run correlation as a generation guard:
            // a delayed Maintenance(A) cannot replace a newer Bulk(B).
            correlation_id: Some(run.correlation_id()),
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

        let should_persist = match event.payload {
            JobEventPayload::Completed {
                kind: FolderScan, ..
            } => true,
            JobEventPayload::DeadLettered {
                kind: FolderScan, ..
            } => true,
            JobEventPayload::Failed {
                kind, retryable, ..
            } if matches!(kind, FolderScan) && !retryable => true,
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
            } if !retryable => Some(*job_id),
            _ => None,
        };

        for run in targets {
            if let Some(job_id) = job_id {
                run.record_job_completed(
                    &event.meta.idempotency_key,
                    job_id,
                    JobKind::FolderScan,
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
}
