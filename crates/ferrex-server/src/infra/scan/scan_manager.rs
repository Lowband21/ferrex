use ferrex_model::{MediaID, SubjectKey};

use ferrex_core::{
    api::types::{
        ScanLifecycleStatus as ApiScanLifecycleStatus, ScanSnapshotDto,
        SeriesBundleResponse,
    },
    application::unit_of_work::AppUnitOfWork,
    domain::scan::{
        actors::{
            FileSystemEvent, FileSystemEventKind, LibraryRootsId,
            index::{IndexingChange, IndexingOutcome},
        },
        orchestration::{
            JobEvent, LibraryActorCommand, StartMode,
            events::{JobEventPayload, ScanEvent},
            job::{JobId, JobKind},
            scan_cursor::{ScanCursor, ScanCursorRepository, normalize_path},
        },
    },
    error::MediaError,
    player_prelude::MediaIDLike,
    types::{
        LibraryId, Media, MediaEvent, ScanEventMetadata, ScanProgressEvent,
        ScanStageLatencySummary, events::ScanSseEventType,
    },
};

use crate::infra::{
    orchestration::ScanOrchestrator,
    scan::media_event_bus::{MediaEventBus, MediaEventFrame},
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

const EVENT_VERSION: &str = "1";
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

fn subject_key_path(key: &SubjectKey) -> Option<&str> {
    match key {
        SubjectKey::Path(path) => Some(path.as_str()),
        SubjectKey::Opaque(_) => None,
    }
}

fn subject_key_path_owned(key: &SubjectKey) -> Option<String> {
    subject_key_path(key).map(str::to_string)
}

/// Command dispatcher + read model for scan orchestration state.
#[derive(Clone)]
pub struct ScanControlPlane {
    inner: Arc<ScanControlPlaneInner>,
}

impl fmt::Debug for ScanControlPlane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let active = self.inner.active.try_read().ok().map(|guard| guard.len());
        let history =
            self.inner.history.try_read().ok().map(|guard| guard.len());
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
    active: RwLock<HashMap<Uuid, Arc<ScanRun>>>,
    history: RwLock<VecDeque<ScanHistoryEntry>>,
    media_bus: Arc<MediaEventBus>,
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
        let media_bus = Arc::new(MediaEventBus::new(
            MEDIA_EVENT_HISTORY_CAPACITY,
            MEDIA_EVENT_BROADCAST_CAPACITY,
        ));
        let aggregator = ScanRunAggregator::new(
            Arc::clone(&orchestrator),
            quiescence,
            Arc::clone(&media_bus),
            unit_of_work.clone(),
        );

        Self {
            inner: Arc::new(ScanControlPlaneInner {
                unit_of_work,
                orchestrator,
                active: RwLock::new(HashMap::new()),
                history: RwLock::new(VecDeque::with_capacity(HISTORY_CAPACITY)),
                media_bus,
                aggregator,
                movie_batch_notifiers: MovieBatchFinalizationNotifiers::new(),
            }),
        }
    }

    pub fn orchestrator(&self) -> Arc<ScanOrchestrator> {
        Arc::clone(&self.inner.orchestrator)
    }

    pub fn subscribe_media_events(
        &self,
    ) -> broadcast::Receiver<MediaEventFrame> {
        self.inner.media_bus.subscribe()
    }

    pub fn publish_media_event(&self, event: MediaEvent) {
        self.inner.media_bus.publish(event);
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
        let guard = self.inner.active.read().await;
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

        let correlation_id = correlation_id.unwrap_or_else(Uuid::now_v7);
        let scan_id = correlation_id;
        let run = ScanRun::new(
            Arc::clone(&self.inner),
            scan_id,
            library_id,
            correlation_id,
            StartMode::Bulk,
        );

        self.inner.register_run(run.clone()).await;
        run.begin().await;

        if let Err(err) = self
            .inner
            .orchestrator
            .command_library(
                library_id,
                LibraryActorCommand::Start {
                    mode: StartMode::Bulk,
                    correlation_id: Some(correlation_id),
                },
            )
            .await
        {
            run.fail_with_reason("start_command_failed").await;
            return Err(ScanControlError::internal(err.to_string()));
        }

        Ok(ScanCommandAccepted {
            scan_id,
            correlation_id,
        })
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
        scan_id: &Uuid,
    ) -> Result<ScanCommandAccepted, ScanControlError> {
        let run = self.inner.lookup(scan_id).await?;
        let correlation_id = Uuid::now_v7();
        run.pause(correlation_id).await?;
        Ok(ScanCommandAccepted {
            scan_id: *scan_id,
            correlation_id,
        })
    }

    pub async fn resume_scan(
        &self,
        scan_id: &Uuid,
    ) -> Result<ScanCommandAccepted, ScanControlError> {
        let run = self.inner.lookup(scan_id).await?;
        let correlation_id = Uuid::now_v7();
        run.resume(correlation_id).await?;
        Ok(ScanCommandAccepted {
            scan_id: *scan_id,
            correlation_id,
        })
    }

    pub async fn cancel_scan(
        &self,
        scan_id: &Uuid,
    ) -> Result<ScanCommandAccepted, ScanControlError> {
        let run = self.inner.lookup(scan_id).await?;
        let correlation_id = Uuid::now_v7();
        run.cancel(correlation_id).await?;
        Ok(ScanCommandAccepted {
            scan_id: *scan_id,
            correlation_id,
        })
    }

    pub async fn active_scans(&self) -> Vec<ScanSnapshot> {
        let guard = self.inner.active.read().await;
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

    pub async fn history(&self, limit: usize) -> Vec<ScanHistoryEntry> {
        let guard = self.inner.history.read().await;
        guard.iter().rev().take(limit).cloned().collect()
    }

    pub async fn snapshot(&self, scan_id: &Uuid) -> Option<ScanSnapshot> {
        let guard = self.inner.active.read().await;
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
        let run = self.inner.lookup(scan_id).await?;
        Ok(run.event_log().await)
    }
}

impl ScanControlPlaneInner {
    async fn register_run(&self, run: Arc<ScanRun>) {
        {
            let mut guard = self.active.write().await;
            guard.insert(run.scan_id(), Arc::clone(&run));
        }

        self.movie_batch_notifiers
            .on_run_started(
                run.library_id(),
                Arc::clone(&self.unit_of_work),
                Arc::clone(&self.media_bus),
            )
            .await;

        self.aggregator.register(run).await;
    }

    async fn finalize_run(
        &self,
        scan_id: Uuid,
        correlation_id: Uuid,
        snapshot: ScanHistoryEntry,
    ) {
        {
            let mut guard = self.active.write().await;
            guard.remove(&scan_id);
        }
        self.movie_batch_notifiers
            .on_run_finished(snapshot.library_id)
            .await;
        self.aggregator.drop(&correlation_id).await;

        let mut history = self.history.write().await;
        if history.len() == HISTORY_CAPACITY {
            history.pop_front();
        }
        history.push_back(snapshot.clone());

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
        let guard = self.active.read().await;
        guard
            .get(scan_id)
            .cloned()
            .ok_or(ScanControlError::ScanNotFound)
    }
}

#[derive(Debug, Clone)]
pub struct ScanCommandAccepted {
    pub scan_id: Uuid,
    pub correlation_id: Uuid,
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
    // Count of successful indexed media per folder path
    index_successes_by_folder: HashMap<String, u32>,
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
    DeadLettered,
}

impl ScanItemStatus {
    fn is_active(self) -> bool {
        matches!(self, Self::InProgress | Self::Retrying)
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::DeadLettered)
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
    fn new(
        inner: Arc<ScanControlPlaneInner>,
        scan_id: Uuid,
        library_id: LibraryId,
        correlation_id: Uuid,
        mode: StartMode,
    ) -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(1024);
        Arc::new(ScanRun {
            scan_id,
            library_id,
            correlation_id,
            state: Mutex::new(ScanRunState {
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
                correlation_id,
                idempotency_prefix: format!("scan:{}:", scan_id),
                event_sequence: 0,
                last_idempotency_key: String::new(),
                started_at: Utc::now(),
                terminal_at: None,
                last_activity_at: None,
                quiescence_started_at: None,
                last_error: None,
                item_states: HashMap::new(),
                index_successes_by_folder: HashMap::new(),
            }),
            tx,
            inner: Arc::downgrade(&inner),
            events: Mutex::new(VecDeque::with_capacity(EVENT_HISTORY_CAPACITY)),
            start_mode: mode,
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

        // Credit the chosen entity root folder
        let entry = state
            .index_successes_by_folder
            .entry(chosen.clone())
            .or_insert(0);
        *entry = entry.saturating_add(1);

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
        Ok(ScanSnapshot {
            scan_id: state.scan_id,
            library_id: state.library_id,
            status: state.status.clone(),
            completed_items: state.completed_items,
            total_items: state.total_items,
            retrying_items: state.retrying_items,
            dead_lettered_items: state.dead_lettered_items,
            correlation_id: state.correlation_id,
            idempotency_key: state.current_idempotency_key(),
            current_path: state.current_path.clone(),
            started_at: state.started_at,
            terminal_at: state.terminal_at,
            sequence: state.event_sequence,
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
                let changed = state.update_item_status(
                    idempotency_key,
                    Some(job_id),
                    ScanItemStatus::Completed,
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
            if state.is_terminal() || state.total_items == 0 {
                (None, None)
            } else {
                let now = Utc::now();
                let mut frame: Option<QueuedFrame> = None;

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
                    let quiesced = state
                        .quiescence_started_at
                        .map(|ts| now - ts >= completion_quiescence)
                        .unwrap_or(false);

                    if quiesced && state.can_enter_quiescing() {
                        // Only complete if no new activity has occurred since quiescing began.
                        let no_new_activity = match (
                            state.quiescence_started_at,
                            state.last_activity_at,
                        ) {
                            (Some(qts), Some(last)) => last <= qts,
                            (Some(_), None) => true,
                            _ => true,
                        };

                        if no_new_activity {
                            // Final pass: demote unmatched root items before completing.
                            let demoted = state
                                .demote_completed_without_index_matches(now);
                            if demoted > 0 {
                                // Emit updated progress and extend quiescence window
                                let progress = state.build_payload();
                                frame = Some(QueuedFrame {
                                    event: ScanEventKind::Progress,
                                    payload: progress,
                                });
                                tracing::info!(
                                    target: "scan::summary",
                                    scan = %state.scan_id,
                                    library = %state.library_id,
                                    demoted,
                                    "demoted unmatched root folders prior to completion"
                                );
                            } else {
                                frame = state.handle_state_event(
                                    ScanStateEvent::QuiescenceComplete,
                                    now,
                                );
                            }
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
                self.finalize_history(status).await;
            }
            matches!(event, ScanEventKind::Completed)
        } else {
            false
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

        let _ = self.tx.send(frame.clone());
        let error = if matches!(event, ScanEventKind::Failed) {
            self.failure_reason().await
        } else {
            None
        };
        self.maybe_log_summary(&event, &payload).await;
        self.emit_media_event(event, payload, error);
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
            payload.dead_lettered_items.unwrap_or(0),
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

        let (root_completed, root_total) = {
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
                    if matches!(item.status, ScanItemStatus::Completed) {
                        roots_completed += 1;
                    }
                }
            }
            (roots_completed, roots_total)
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
                retrying = payload.retrying_items.unwrap_or(0),
                dead_lettered = payload.dead_lettered_items.unwrap_or(0),
                pct = pct,
                root_completed = root_completed,
                root_total = root_total,
                path = ?payload.current_path,
                "scan progress"
            );

            guard.last_log_at = now;
            guard.last_sequence = payload.sequence;
            guard.last_completed_items = payload.completed_items;
            guard.last_pct = pct;
        }
    }

    async fn failure_reason(&self) -> Option<String> {
        let state = self.state.lock().await;
        state.last_error.clone()
    }

    async fn finalize_history(&self, terminal: ScanLifecycleStatus) {
        let snapshot = {
            let state = self.state.lock().await;
            ScanHistoryEntry {
                scan_id: state.scan_id,
                library_id: state.library_id,
                status: terminal.clone(),
                completed_items: state.completed_items,
                total_items: state.total_items,
                started_at: state.started_at,
                terminal_at: state.terminal_at.unwrap_or_else(Utc::now),
            }
        };

        if let Some(inner) = self.inner.upgrade() {
            inner
                .finalize_run(self.scan_id, self.correlation_id, snapshot)
                .await;
        }

        warn!(scan = %self.scan_id, status = ?terminal, "finalized scan run");
    }

    fn emit_media_event(
        &self,
        event: ScanEventKind,
        payload: ScanProgressEvent,
        error: Option<String>,
    ) {
        if let Some(inner) = self.inner.upgrade() {
            let message = match event {
                ScanEventKind::Started => MediaEvent::ScanStarted {
                    scan_id: payload.scan_id,
                    metadata: ScanEventMetadata {
                        version: payload.version.clone(),
                        correlation_id: payload.correlation_id,
                        idempotency_key: payload.idempotency_key.clone(),
                        library_id: payload.library_id,
                    },
                },
                ScanEventKind::Progress => MediaEvent::ScanProgress {
                    scan_id: payload.scan_id,
                    progress: payload.clone(),
                },
                ScanEventKind::Quiescing => MediaEvent::ScanProgress {
                    scan_id: payload.scan_id,
                    progress: payload.clone(),
                },
                ScanEventKind::Completed => MediaEvent::ScanCompleted {
                    scan_id: payload.scan_id,
                    metadata: ScanEventMetadata {
                        version: payload.version.clone(),
                        correlation_id: payload.correlation_id,
                        idempotency_key: payload.idempotency_key.clone(),
                        library_id: payload.library_id,
                    },
                },
                ScanEventKind::Failed => MediaEvent::ScanFailed {
                    scan_id: payload.scan_id,
                    error: error.unwrap_or_else(|| "scan_failed".to_string()),
                    metadata: ScanEventMetadata {
                        version: payload.version.clone(),
                        correlation_id: payload.correlation_id,
                        idempotency_key: payload.idempotency_key.clone(),
                        library_id: payload.library_id,
                    },
                },
            };
            inner.media_bus.publish(message);
        }
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

    fn rehydrate_from_cursors(&mut self, cursors: &[ScanCursor]) {
        let mut latest = self.last_activity_at;

        for cursor in cursors {
            let idempotency_key =
                format!("scan:{}:{}", self.library_id, cursor.folder_path_norm);
            if self.item_states.contains_key(&idempotency_key) {
                continue;
            }

            self.total_items = self.total_items.saturating_add(1);
            self.completed_items = self.completed_items.saturating_add(1);

            let last_activity = cursor.last_scan_at;
            latest = Some(match latest {
                Some(existing) => existing.max(last_activity),
                None => last_activity,
            });

            self.item_states.insert(
                idempotency_key,
                ScanItemState {
                    status: ScanItemStatus::Completed,
                    last_activity,
                    path_key: SubjectKey::path(cursor.folder_path_norm.clone())
                        .ok(),
                    last_error: None,
                    last_job_id: None,
                },
            );
        }

        if let Some(activity) = latest {
            self.last_activity_at = Some(activity);
        }
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
                matches!(self.phase, ScanPhase::Quiescing)
                    && self.completed_items + self.dead_lettered_items
                        == self.total_items
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

    fn demote_completed_without_index_matches(
        &mut self,
        now: DateTime<Utc>,
    ) -> usize {
        let mut to_demote: Vec<(String, Option<String>)> = Vec::new();

        // Build a set of all tracked folder paths to detect root-level items
        let mut scanned_paths: HashSet<String> = HashSet::new();
        for item in self.item_states.values() {
            if let Some(p) = &item.path_key
                && let Some(path) = subject_key_path(p)
            {
                scanned_paths.insert(path.to_string());
            }
        }

        // Helper to check whether a scanned path has any scanned ancestor
        let is_root_item = |path: &str| -> bool {
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

        for (idempotency, item) in self.item_states.iter() {
            if !matches!(item.status, ScanItemStatus::Completed) {
                continue;
            }
            let Some(path) = item.path_key.as_ref() else {
                continue;
            };
            let Some(path) = subject_key_path(path) else {
                continue;
            };
            // Only demote root-level scanned folders based on matching status
            if !is_root_item(path) {
                continue;
            }
            let success_count = self
                .index_successes_by_folder
                .get(path)
                .cloned()
                .unwrap_or(0);
            if success_count == 0 {
                to_demote.push((idempotency.clone(), Some(path.to_string())));
            }
        }

        let mut changed = 0usize;
        let mut last_path: Option<String> = None;
        for (idempotency, path) in to_demote {
            let path_key =
                path.clone().and_then(|path| SubjectKey::path(path).ok());
            let updated = self.update_item_status(
                &idempotency,
                None,
                ScanItemStatus::DeadLettered,
                now,
                path_key,
                Some("no_root_match".to_string()),
            );
            if updated {
                tracing::info!(
                    target: "scan::state",
                    scan = %self.scan_id,
                    library = %self.library_id,
                    idempotency = %idempotency,
                    path = ?path,
                    "demoted folder completion due to zero indexed media"
                );
                changed += 1;
                if let Some(p) = path {
                    last_path = Some(p);
                }
            }
        }

        if changed > 0 {
            self.current_path = last_path.clone();
            self.path_key =
                last_path.and_then(|path| SubjectKey::path(path).ok());
            self.last_activity_at = Some(now);
        }

        changed
    }

    fn build_payload(&mut self) -> ScanProgressEvent {
        self.event_sequence += 1;
        let idempotency_key =
            format!("{}{}", self.idempotency_prefix, self.event_sequence);
        self.last_idempotency_key = idempotency_key.clone();
        ScanProgressEvent {
            version: EVENT_VERSION.to_string(),
            scan_id: self.scan_id,
            library_id: self.library_id,
            status: self.status_string(),
            completed_items: self.completed_items,
            total_items: self.total_items,
            sequence: self.event_sequence,
            current_path: self.current_path.clone(),
            path_key: self.path_key.clone(),
            p95_stage_latencies_ms: DEFAULT_LATENCIES,
            correlation_id: self.correlation_id,
            idempotency_key,
            emitted_at: Utc::now(),
            retrying_items: (self.retrying_items > 0)
                .then_some(self.retrying_items),
            dead_lettered_items: (self.dead_lettered_items > 0)
                .then_some(self.dead_lettered_items),
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
                match status {
                    ScanItemStatus::Completed => self.completed_items += 1,
                    ScanItemStatus::DeadLettered => {
                        self.dead_lettered_items += 1
                    }
                    ScanItemStatus::Retrying => self.retrying_items += 1,
                    _ => {}
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
                if old_status == status {
                    item.last_activity = event_time;
                    if let Some(path) = path_key {
                        item.path_key = Some(path);
                    }
                    if let Some(err) = error {
                        item.last_error = Some(err);
                    } else if matches!(
                        status,
                        ScanItemStatus::Completed | ScanItemStatus::InProgress
                    ) {
                        item.last_error = None;
                    }
                    if let Some(job) = job_id {
                        item.last_job_id = Some(job);
                    }
                    return false;
                }

                match old_status {
                    ScanItemStatus::Completed => {
                        self.completed_items =
                            self.completed_items.saturating_sub(1);
                    }
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

                match status {
                    ScanItemStatus::Completed => self.completed_items += 1,
                    ScanItemStatus::DeadLettered => {
                        self.dead_lettered_items += 1
                    }
                    ScanItemStatus::Retrying => self.retrying_items += 1,
                    _ => {}
                }

                item.status = status;
                item.last_activity = event_time;
                if let Some(path) = path_key {
                    item.path_key = Some(path);
                }
                match error {
                    Some(err) => item.last_error = Some(err),
                    None => {
                        if matches!(
                            status,
                            ScanItemStatus::Completed
                                | ScanItemStatus::InProgress
                        ) {
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

#[derive(Clone)]
struct ScanRunAggregator {
    inner: Arc<ScanRunAggregatorInner>,
}

struct ScanRunAggregatorInner {
    orchestrator: Arc<ScanOrchestrator>,
    runs: RwLock<HashMap<Uuid, Arc<ScanRun>>>,
    quiescence_chrono: ChronoDuration,
    stall_timeout: ChronoDuration,
    media_bus: Arc<MediaEventBus>,
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
        media_bus: Arc<MediaEventBus>,
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
            media_bus,
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

    async fn handle_job_event(&self, event: JobEvent) {
        let run = {
            let guard = self.runs.read().await;
            guard.get(&event.meta.correlation_id).cloned()
        };

        self.observe_series_bundle_job_event(&event).await;

        if let Some(run) = run {
            let completed = match event.payload {
                JobEventPayload::Enqueued { kind, job_id, .. } => {
                    if kind == JobKind::FolderScan {
                        run.record_folder_enqueued(
                            &event.meta.idempotency_key,
                            job_id,
                            event.meta.path_key.clone(),
                        )
                        .await;
                    }
                    false
                }
                JobEventPayload::Completed { kind, job_id, .. } => {
                    if kind == JobKind::FolderScan {
                        run.record_folder_completed(
                            &event.meta.idempotency_key,
                            job_id,
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
                    ..
                } => {
                    if kind == JobKind::FolderScan {
                        run.record_folder_failure(
                            &event.meta.idempotency_key,
                            job_id,
                            None,
                            event.meta.path_key.clone(),
                            retryable,
                        )
                        .await;

                        if !retryable {
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
                JobEventPayload::DeadLettered { kind, job_id, .. } => {
                    if kind == JobKind::FolderScan {
                        run.record_folder_dead_lettered(
                            &event.meta.idempotency_key,
                            job_id,
                            None,
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
                    run.record_folder_lease_renewed(
                        &event.meta.idempotency_key,
                        job_id,
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
        } else {
            self.handle_orphan_event(&event).await;
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

            let event = MediaEvent::SeriesBundleFinalized {
                library_id: finalization.library_id,
                series_id: finalization.series_id,
            };

            let receivers = self.media_bus.receiver_count();
            let frame = self.media_bus.publish(event);

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
                MediaEvent::MovieAdded { movie: *movie }
            }
            (Media::Movie(movie), IndexingChange::Updated) => {
                MediaEvent::MovieUpdated { movie: *movie }
            }
            (Media::Series(series), IndexingChange::Created) => {
                MediaEvent::SeriesAdded { series: *series }
            }
            (Media::Series(series), IndexingChange::Updated) => {
                MediaEvent::SeriesUpdated { series: *series }
            }
            (_, _) => return Ok(()),
        };

        let _ = self.media_bus.publish(event);

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

    async fn on_run_completed(&self, run: Arc<ScanRun>) {
        if run.start_mode() != StartMode::Bulk {
            return;
        }

        let library_id = run.library_id();
        let command = LibraryActorCommand::Start {
            mode: StartMode::Maintenance,
            correlation_id: Some(Uuid::now_v7()),
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
    pub started_at: DateTime<Utc>,
    pub terminal_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSnapshot {
    pub scan_id: Uuid,
    pub library_id: LibraryId,
    pub status: ScanLifecycleStatus,
    pub completed_items: u64,
    pub total_items: u64,
    pub retrying_items: u64,
    pub dead_lettered_items: u64,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub current_path: Option<String>,
    pub started_at: DateTime<Utc>,
    pub terminal_at: Option<DateTime<Utc>>,
    pub sequence: u64,
}

impl From<ScanSnapshot> for ScanSnapshotDto {
    fn from(snapshot: ScanSnapshot) -> Self {
        ScanSnapshotDto {
            scan_id: snapshot.scan_id,
            library_id: snapshot.library_id,
            status: snapshot.status.into(),
            completed_items: snapshot.completed_items,
            total_items: snapshot.total_items,
            retrying_items: snapshot.retrying_items,
            dead_lettered_items: snapshot.dead_lettered_items,
            correlation_id: snapshot.correlation_id,
            idempotency_key: snapshot.idempotency_key,
            current_path: snapshot.current_path,
            started_at: snapshot.started_at,
            terminal_at: snapshot.terminal_at,
            sequence: snapshot.sequence,
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

#[derive(Debug)]
pub enum ScanControlError {
    LibraryNotFound,
    LibraryDisabled,
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
