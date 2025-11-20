use axum::http::StatusCode;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use ferrex_core::api_types::{ScanLifecycleStatus as ApiScanLifecycleStatus, ScanSnapshotDto};
use ferrex_core::orchestration::actors::pipeline::{IndexingChange, IndexingOutcome};
use ferrex_core::types::ids::{EpisodeID, MovieID, SeasonID, SeriesID};
use ferrex_core::{
    JobEvent, LibraryActorCommand, LibraryID, Media, MediaDatabase, MediaError, MediaEvent,
    PostgresCursorRepository, ScanEventMetadata, ScanProgressEvent, ScanStageLatencySummary,
    StartMode,
    database::traits::MediaDatabaseTrait,
    orchestration::{
        events::{DomainEvent, JobEventPayload},
        job::{JobId, JobKind},
        scan_cursor::{ScanCursor, ScanCursorId, ScanCursorRepository},
    },
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::{
    collections::{HashMap, HashSet, VecDeque, hash_map::Entry},
    fmt,
    hash::{Hash, Hasher},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tokio::{
    spawn,
    sync::{Mutex, RwLock, broadcast},
    time::interval,
};
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::infra::orchestration::ScanOrchestrator;

const EVENT_VERSION: &str = "1";
const HISTORY_CAPACITY: usize = 256;
const EVENT_HISTORY_CAPACITY: usize = 512;
const DEFAULT_LATENCIES: ScanStageLatencySummary = ScanStageLatencySummary {
    scan: 12,
    analyze: 210,
    index: 44,
};
const DEFAULT_QUIESCENCE: Duration = Duration::from_secs(3);
const STALLED_SCAN_TIMEOUT_MULTIPLIER: u32 = 5;

/// Command dispatcher + read model for scan orchestration state.
#[derive(Clone)]
pub struct ScanControlPlane {
    inner: Arc<ScanControlPlaneInner>,
}

struct ScanControlPlaneInner {
    db: Arc<MediaDatabase>,
    orchestrator: Arc<ScanOrchestrator>,
    active: RwLock<HashMap<Uuid, Arc<ScanRun>>>,
    history: RwLock<VecDeque<ScanHistoryEntry>>,
    media_tx: broadcast::Sender<MediaEvent>,
    aggregator: ScanRunAggregator,
}

impl ScanControlPlane {
    pub fn new(db: Arc<MediaDatabase>, orchestrator: Arc<ScanOrchestrator>) -> Self {
        Self::with_quiescence_window(db, orchestrator, DEFAULT_QUIESCENCE)
    }

    pub fn with_quiescence_window(
        db: Arc<MediaDatabase>,
        orchestrator: Arc<ScanOrchestrator>,
        quiescence: Duration,
    ) -> Self {
        let (media_tx, _rx) = broadcast::channel(512);
        let aggregator = ScanRunAggregator::new(
            Arc::clone(&orchestrator),
            orchestrator.cursor_repository(),
            quiescence,
            media_tx.clone(),
            Arc::clone(&db),
        );

        Self {
            inner: Arc::new(ScanControlPlaneInner {
                db,
                orchestrator,
                active: RwLock::new(HashMap::new()),
                history: RwLock::new(VecDeque::with_capacity(HISTORY_CAPACITY)),
                media_tx,
                aggregator,
            }),
        }
    }

    pub fn orchestrator(&self) -> Arc<ScanOrchestrator> {
        Arc::clone(&self.inner.orchestrator)
    }

    pub fn subscribe_media_events(&self) -> broadcast::Receiver<MediaEvent> {
        self.inner.media_tx.subscribe()
    }

    pub fn publish_media_event(&self, event: MediaEvent) {
        let _ = self.inner.media_tx.send(event);
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
        library_id: LibraryID,
        correlation_id: Option<Uuid>,
    ) -> Result<ScanCommandAccepted, ScanControlError> {
        let library = self
            .inner
            .db
            .backend()
            .get_library(&library_id)
            .await
            .map_err(|err| ScanControlError::internal(err.to_string()))?
            .ok_or(ScanControlError::LibraryNotFound)?;

        if !library.enabled {
            return Err(ScanControlError::LibraryDisabled);
        }

        let correlation_id = correlation_id.unwrap_or_else(Uuid::now_v7);
        let scan_id = correlation_id;
        let cursor_repository = self.inner.orchestrator.cursor_repository();
        let run = ScanRun::new(
            Arc::clone(&self.inner),
            scan_id,
            library_id,
            correlation_id,
            StartMode::Bulk,
            cursor_repository,
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
            match run.snapshot().await {
                Ok(snapshot) => Some(snapshot),
                Err(_) => None,
            }
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
        self.aggregator.register(run).await;
    }

    async fn finalize_run(&self, scan_id: Uuid, correlation_id: Uuid, snapshot: ScanHistoryEntry) {
        {
            let mut guard = self.active.write().await;
            guard.remove(&scan_id);
        }
        self.aggregator.drop(&correlation_id).await;

        let mut history = self.history.write().await;
        if history.len() == HISTORY_CAPACITY {
            history.pop_front();
        }
        history.push_back(snapshot);
    }

    async fn lookup(&self, scan_id: &Uuid) -> Result<Arc<ScanRun>, ScanControlError> {
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

struct ScanRun {
    scan_id: Uuid,
    library_id: LibraryID,
    correlation_id: Uuid,
    state: Mutex<ScanRunState>,
    tx: broadcast::Sender<ScanBroadcastFrame>,
    inner: Weak<ScanControlPlaneInner>,
    events: Mutex<VecDeque<ScanBroadcastFrame>>,
    start_mode: StartMode,
    cursor_repository: Arc<PostgresCursorRepository>,
    log: Mutex<ScanLogWatermark>,
}

#[derive(Debug)]
struct ScanRunState {
    scan_id: Uuid,
    library_id: LibraryID,
    phase: ScanPhase,
    status: ScanLifecycleStatus,
    completed_items: u64,
    total_items: u64,
    dead_lettered_items: u64,
    retrying_items: u64,
    current_path: Option<String>,
    path_key: Option<String>,
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
    path_key: Option<String>,
    last_error: Option<String>,
    last_job_id: Option<JobId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanItemStatus {
    Pending,
    InProgress,
    Retrying,
    Completed,
    DeadLettered,
}

impl ScanItemStatus {
    fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::InProgress | Self::Retrying)
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
        library_id: LibraryID,
        correlation_id: Uuid,
        mode: StartMode,
        cursor_repository: Arc<PostgresCursorRepository>,
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
            }),
            tx,
            inner: Arc::downgrade(&inner),
            events: Mutex::new(VecDeque::with_capacity(EVENT_HISTORY_CAPACITY)),
            start_mode: mode,
            cursor_repository,
            log: Mutex::new(ScanLogWatermark::default()),
        })
    }

    fn scan_id(&self) -> Uuid {
        self.scan_id
    }

    fn correlation_id(&self) -> Uuid {
        self.correlation_id
    }

    fn library_id(&self) -> LibraryID {
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
        let cursors = match self
            .cursor_repository
            .list_by_library(self.library_id)
            .await
        {
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

    async fn persist_completed_cursor(&self, path_norm: &str, event_time: DateTime<Utc>) {
        if let Some(cursor) = self.make_cursor(path_norm, event_time) {
            if let Err(err) = self.cursor_repository.upsert(cursor).await {
                warn!(
                    library = %self.library_id,
                    scan = %self.scan_id,
                    path = %path_norm,
                    error = %err,
                    "failed to persist completed cursor"
                );
            }
        }
    }

    fn make_cursor(&self, path_norm: &str, last_scan_at: DateTime<Utc>) -> Option<ScanCursor> {
        if path_norm.is_empty() {
            return None;
        }

        Some(build_cursor(
            self.library_id,
            path_norm,
            last_scan_at,
            self.scan_id,
        ))
    }

    async fn pause(&self, correlation_id: Uuid) -> Result<(), ScanControlError> {
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
                | ScanLifecycleStatus::Canceled => return Err(ScanControlError::ScanTerminal),
                ScanLifecycleStatus::Pending => return Err(ScanControlError::ScanNotRunning),
            }
        };
        self.emit_frame(ScanEventKind::Progress, payload).await;
        Ok(())
    }

    async fn resume(&self, correlation_id: Uuid) -> Result<(), ScanControlError> {
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
                | ScanLifecycleStatus::Canceled => return Err(ScanControlError::ScanTerminal),
                ScanLifecycleStatus::Pending => return Err(ScanControlError::ScanNotRunning),
            }
        };
        self.emit_frame(ScanEventKind::Progress, payload).await;
        Ok(())
    }

    async fn cancel(&self, correlation_id: Uuid) -> Result<(), ScanControlError> {
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
        path_key: Option<String>,
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
                    if let Some(item) = state.item_states.get_mut(idempotency_key) {
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
                    state.current_path = path.clone();
                    state.path_key = path.clone();

                    let mut frames = Vec::new();
                    if let Some(frame) =
                        state.handle_state_event(ScanStateEvent::NewItemFound, event_time)
                    {
                        frames.push(frame);
                    }

                    let reopened = matches!(previous_phase, ScanPhase::Quiescing)
                        && matches!(state.phase, ScanPhase::Processing);

                    if let Some(payload) = state.build_payload_if(changed || reopened) {
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
        path_key: Option<String>,
    ) {
        let event_time = Utc::now();
        let path = path_key.clone();
        let (frames, persist_path) = {
            let mut state = self.state.lock().await;
            if state.is_terminal() {
                (Vec::new(), None)
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
                    state.current_path = path.clone();
                    state.path_key = path.clone();
                    state.last_activity_at = Some(event_time);
                    let progress = state.build_payload();
                    frames.push(QueuedFrame {
                        event: ScanEventKind::Progress,
                        payload: progress,
                    });

                    if state.can_enter_quiescing() {
                        if let Some(frame) =
                            state.handle_state_event(ScanStateEvent::AllItemsProcessed, event_time)
                        {
                            frames.push(frame);
                        }
                    }
                }
                let persist = if changed { path.clone() } else { None };
                (frames, persist)
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
        path_key: Option<String>,
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
            if let Some(last) = item.last_job_id {
                if last != job_id {
                    // Ignore renewals from a stale job
                    return;
                }
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
                item.path_key = Some(path_value.clone());
                state.current_path = Some(path_value.clone());
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
        path_key: Option<String>,
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
                    state.current_path = path.clone();
                    state.path_key = path.clone();
                    state.last_activity_at = Some(event_time);
                    let progress = state.build_payload();
                    frames.push(QueuedFrame {
                        event: ScanEventKind::Progress,
                        payload: progress,
                    });

                    if !retryable && state.can_enter_quiescing() {
                        if let Some(frame) =
                            state.handle_state_event(ScanStateEvent::AllItemsProcessed, event_time)
                        {
                            frames.push(frame);
                        }
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
        path_key: Option<String>,
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
                    state.current_path = path.clone();
                    state.path_key = path.clone();
                    state.last_activity_at = Some(event_time);
                    let progress = state.build_payload();
                    frames.push(QueuedFrame {
                        event: ScanEventKind::Progress,
                        payload: progress,
                    });

                    if state.can_enter_quiescing() {
                        if let Some(frame) =
                            state.handle_state_event(ScanStateEvent::AllItemsProcessed, event_time)
                        {
                            frames.push(frame);
                        }
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

                if matches!(state.phase, ScanPhase::Processing | ScanPhase::Discovering)
                    && state.can_enter_quiescing()
                {
                    frame = state.handle_state_event(ScanStateEvent::AllItemsProcessed, now);
                }

                if frame.is_none() && matches!(state.phase, ScanPhase::Quiescing) {
                    let quiesced = state
                        .quiescence_started_at
                        .map(|ts| now - ts >= completion_quiescence)
                        .unwrap_or(false);

                    if quiesced && state.can_enter_quiescing() {
                        // Only complete if no new activity has occurred since quiescing began.
                        let no_new_activity =
                            match (state.quiescence_started_at, state.last_activity_at) {
                                (Some(qts), Some(last)) => last <= qts,
                                (Some(_), None) => true,
                                _ => true,
                            };
                        if no_new_activity {
                            frame =
                                state.handle_state_event(ScanStateEvent::QuiescenceComplete, now);
                        }
                    }
                }

                if frame.is_none()
                    && matches!(state.phase, ScanPhase::Processing | ScanPhase::Discovering)
                    && state.outstanding_items_stalled(stall_timeout, now)
                {
                    frame = state.handle_state_event(
                        ScanStateEvent::Stalled {
                            reason: "quiescence_timeout".to_string(),
                        },
                        now,
                    );
                }

                let finalize = frame.as_ref().and_then(|queued| match queued.event {
                    ScanEventKind::Completed => Some(ScanLifecycleStatus::Completed),
                    ScanEventKind::Failed => Some(ScanLifecycleStatus::Failed),
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

    async fn emit_frame(&self, event: ScanEventKind, payload: ScanProgressEvent) {
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

    async fn maybe_log_summary(&self, event: &ScanEventKind, payload: &ScanProgressEvent) {
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
        let terminal_copy = terminal.clone();
        let snapshot = {
            let state = self.state.lock().await;
            ScanHistoryEntry {
                scan_id: state.scan_id,
                library_id: state.library_id,
                status: terminal.clone(),
                completed_items: state.completed_items,
                total_items: state.total_items,
                started_at: state.started_at,
                terminal_at: state.terminal_at.unwrap_or_else(|| Utc::now()),
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
            let _ = inner.media_tx.send(message);
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

fn build_cursor(
    library_id: LibraryID,
    path_norm: &str,
    last_scan_at: DateTime<Utc>,
    salt: Uuid,
) -> ScanCursor {
    let mut hasher = DefaultHasher::new();
    path_norm.hash(&mut hasher);
    let path_hash = hasher.finish();
    let listing_hash = format!("scan:{}:{:x}", salt, path_hash);

    ScanCursor {
        id: ScanCursorId {
            library_id,
            path_hash,
        },
        folder_path_norm: path_norm.to_string(),
        listing_hash,
        entry_count: 0,
        last_scan_at,
        last_modified_at: None,
        device_id: None,
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
            let idempotency_key = format!("scan:{}:{}", self.library_id, cursor.folder_path_norm);
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
                    path_key: Some(cursor.folder_path_norm.clone()),
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
            ScanPhase::Discovering => matches!(self.phase, ScanPhase::Initializing),
            ScanPhase::Processing => {
                matches!(self.phase, ScanPhase::Discovering | ScanPhase::Quiescing)
            }
            ScanPhase::Quiescing => {
                matches!(self.phase, ScanPhase::Processing | ScanPhase::Discovering)
                    && self.can_enter_quiescing()
            }
            ScanPhase::Completed => {
                matches!(self.phase, ScanPhase::Quiescing)
                    && self.completed_items + self.dead_lettered_items == self.total_items
            }
            ScanPhase::Failed | ScanPhase::Canceled => !self.phase.is_terminal(),
        }
    }

    fn transition(&mut self, next: ScanPhase, now: DateTime<Utc>) -> Option<QueuedFrame> {
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

    fn build_payload(&mut self) -> ScanProgressEvent {
        self.event_sequence += 1;
        let idempotency_key = format!("{}{}", self.idempotency_prefix, self.event_sequence);
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
            retrying_items: (self.retrying_items > 0).then_some(self.retrying_items),
            dead_lettered_items: (self.dead_lettered_items > 0).then_some(self.dead_lettered_items),
        }
    }

    fn build_payload_if(&mut self, condition: bool) -> Option<ScanProgressEvent> {
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
        path_key: Option<String>,
        error: Option<String>,
    ) -> bool {
        match self.item_states.entry(idempotency_key.to_string()) {
            Entry::Vacant(slot) => {
                self.total_items += 1;
                match status {
                    ScanItemStatus::Completed => self.completed_items += 1,
                    ScanItemStatus::DeadLettered => self.dead_lettered_items += 1,
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
                        self.completed_items = self.completed_items.saturating_sub(1);
                    }
                    ScanItemStatus::DeadLettered => {
                        self.dead_lettered_items = self.dead_lettered_items.saturating_sub(1);
                    }
                    ScanItemStatus::Retrying => {
                        self.retrying_items = self.retrying_items.saturating_sub(1);
                    }
                    _ => {}
                }

                match status {
                    ScanItemStatus::Completed => self.completed_items += 1,
                    ScanItemStatus::DeadLettered => self.dead_lettered_items += 1,
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
                            ScanItemStatus::Completed | ScanItemStatus::InProgress
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
        self.total_items > 0 && self.completed_items + self.dead_lettered_items == self.total_items
    }

    fn has_outstanding_items(&self) -> bool {
        !self.can_enter_quiescing()
    }

    fn outstanding_items_stalled(&self, stall_timeout: ChronoDuration, now: DateTime<Utc>) -> bool {
        if self.retrying_items > 0 {
            return false;
        }

        let mut saw_active = false;
        for item in self.item_states.values() {
            if !item.status.is_active() {
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
    cursor_repository: Arc<PostgresCursorRepository>,
    runs: RwLock<HashMap<Uuid, Arc<ScanRun>>>,
    quiescence_chrono: ChronoDuration,
    stall_timeout: ChronoDuration,
    media_tx: broadcast::Sender<MediaEvent>,
    db: Arc<MediaDatabase>,
    seen_media: Mutex<HashSet<Uuid>>,
}

impl ScanRunAggregator {
    fn new(
        orchestrator: Arc<ScanOrchestrator>,
        cursor_repository: Arc<PostgresCursorRepository>,
        quiescence: Duration,
        media_tx: broadcast::Sender<MediaEvent>,
        db: Arc<MediaDatabase>,
    ) -> Self {
        let chrono_window =
            ChronoDuration::from_std(quiescence).unwrap_or_else(|_| ChronoDuration::seconds(3));
        let stall_std = quiescence
            .checked_mul(STALLED_SCAN_TIMEOUT_MULTIPLIER)
            .unwrap_or(Duration::from_secs(60));
        let stall_window =
            ChronoDuration::from_std(stall_std).unwrap_or_else(|_| ChronoDuration::seconds(60));
        let inner = Arc::new(ScanRunAggregatorInner {
            orchestrator,
            cursor_repository,
            runs: RwLock::new(HashMap::new()),
            quiescence_chrono: chrono_window,
            stall_timeout: stall_window,
            media_tx,
            db,
            seen_media: Mutex::new(HashSet::new()),
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
        let mut domain_rx = self.orchestrator.subscribe_domain_events();
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
                        Ok(event) => self.handle_domain_event(event).await,
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
    }

    async fn handle_job_event(&self, event: JobEvent) {
        let run = {
            let guard = self.runs.read().await;
            guard.get(&event.meta.correlation_id).cloned()
        };

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
                        run.try_complete(self.quiescence_chrono, self.stall_timeout)
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
                            run.try_complete(self.quiescence_chrono, self.stall_timeout)
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
                        run.try_complete(self.quiescence_chrono, self.stall_timeout)
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

    async fn handle_domain_event(&self, event: DomainEvent) {
        match event {
            DomainEvent::Indexed(outcome) => {
                if let Err(err) = self.handle_indexed_outcome(outcome).await {
                    warn!("failed to process indexed outcome: {err}");
                }
            }
            _ => {}
        }
    }

    async fn handle_indexed_outcome(&self, mut outcome: IndexingOutcome) -> Result<(), String> {
        let mut media = outcome.media.clone();
        let mut media_id = outcome
            .media_id
            .or_else(|| media.as_ref().map(Self::media_uuid));

        if media.is_none() {
            if let Some(candidate) = media_id {
                media = self.load_media(candidate).await;
            }
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

        let media_id = media_id.unwrap_or_else(|| Self::media_uuid(&media));

        let mut seen = self.seen_media.lock().await;
        let first_seen = seen.insert(media_id);
        drop(seen);

        let change = match outcome.change {
            IndexingChange::Created if first_seen => IndexingChange::Created,
            _ => IndexingChange::Updated,
        };

        let event = match (media, change) {
            (Media::Movie(movie), IndexingChange::Created) => MediaEvent::MovieAdded { movie },
            (Media::Movie(movie), IndexingChange::Updated) => MediaEvent::MovieUpdated { movie },
            (Media::Series(series), IndexingChange::Created) => MediaEvent::SeriesAdded { series },
            (Media::Series(series), IndexingChange::Updated) => {
                MediaEvent::SeriesUpdated { series }
            }
            (Media::Season(season), IndexingChange::Created) => MediaEvent::SeasonAdded { season },
            (Media::Season(season), IndexingChange::Updated) => {
                MediaEvent::SeasonUpdated { season }
            }
            (Media::Episode(episode), IndexingChange::Created) => {
                MediaEvent::EpisodeAdded { episode }
            }
            (Media::Episode(episode), IndexingChange::Updated) => {
                MediaEvent::EpisodeUpdated { episode }
            }
        };

        if let Err(err) = self.media_tx.send(event) {
            return Err(format!("broadcast error: {err}"));
        }

        Ok(())
    }

    async fn load_media(&self, uuid: Uuid) -> Option<Media> {
        let backend = self.db.backend();

        match backend.get_movie_reference(&MovieID(uuid)).await {
            Ok(movie) => return Some(Media::Movie(movie)),
            Err(MediaError::NotFound(_)) => {}
            Err(err) => {
                warn!("failed to hydrate movie reference {uuid}: {err}");
                return None;
            }
        }

        match backend.get_series_reference(&SeriesID(uuid)).await {
            Ok(series) => return Some(Media::Series(series)),
            Err(MediaError::NotFound(_)) => {}
            Err(err) => {
                warn!("failed to hydrate series reference {uuid}: {err}");
                return None;
            }
        }

        match backend.get_season_reference(&SeasonID(uuid)).await {
            Ok(season) => return Some(Media::Season(season)),
            Err(MediaError::NotFound(_)) => {}
            Err(err) => {
                warn!("failed to hydrate season reference {uuid}: {err}");
                return None;
            }
        }

        match backend.get_episode_reference(&EpisodeID(uuid)).await {
            Ok(episode) => return Some(Media::Episode(episode)),
            Err(MediaError::NotFound(_)) => {}
            Err(err) => {
                warn!("failed to hydrate episode reference {uuid}: {err}");
                return None;
            }
        }

        None
    }

    fn media_uuid(media: &Media) -> Uuid {
        match media {
            Media::Movie(movie) => movie.id.0,
            Media::Series(series) => series.id.0,
            Media::Season(season) => season.id.0,
            Media::Episode(episode) => episode.id.0,
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
        use ferrex_core::orchestration::job::JobKind::FolderScan;

        let path_norm = match event.meta.path_key.as_deref() {
            Some(value) if !value.is_empty() => value,
            _ => return,
        };

        let should_persist = match event.payload {
            JobEventPayload::Completed { kind, .. } if matches!(kind, FolderScan) => true,
            JobEventPayload::DeadLettered { kind, .. } if matches!(kind, FolderScan) => true,
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
                    Some(path_owned.clone()),
                )
                .await;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanHistoryEntry {
    pub scan_id: Uuid,
    pub library_id: LibraryID,
    pub status: ScanLifecycleStatus,
    pub completed_items: u64,
    pub total_items: u64,
    pub started_at: DateTime<Utc>,
    pub terminal_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSnapshot {
    pub scan_id: Uuid,
    pub library_id: LibraryID,
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
