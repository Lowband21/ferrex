use std::any::type_name;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{
    error::Result,
    types::{ids::LibraryID, prelude::LibraryReference},
};

use super::folder::is_media_file_path;
use super::messages::{ActorObserver, IssuedJobRecord, ParentDescriptors};
use crate::orchestration::{
    correlation::CorrelationCache,
    events::JobEventPublisher,
    job::{DedupeKey, JobHandle, JobId, JobPriority, ScanReason},
    queue::QueueService,
    scan_cursor::normalize_path,
};

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct LibraryRootsId(pub u16);

#[derive(Clone, Debug)]
pub struct LibraryRootDescriptor {
    pub root_id: LibraryRootsId,
    pub path_norm: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StartMode {
    Bulk,
    Maintenance,
    Resume,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LibraryRootState {
    pub last_scan_at: Option<DateTime<Utc>>,
    pub is_watching: bool,
}

/// Configuration for a library actor instance.
/// Stable identifier representing a maintenance partition for a library.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct MaintenancePartition(pub u16);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryActorConfig {
    pub library: LibraryReference,
    pub root_paths: Vec<PathBuf>,
    pub max_outstanding_jobs: usize,
}

impl LibraryActorConfig {
    pub fn roots(&self) -> impl Iterator<Item = LibraryRootDescriptor> + '_ {
        self.root_paths
            .iter()
            .enumerate()
            .map(|(idx, path)| LibraryRootDescriptor {
                root_id: LibraryRootsId(idx as u16),
                path_norm: path.to_string_lossy().to_string(),
            })
    }

    pub fn root_path(&self, id: LibraryRootsId) -> Option<PathBuf> {
        self.root_paths.get(id.0 as usize).cloned()
    }
}

/// Messages accepted by the `LibraryActor`.
///
/// Correlation flow overview:
/// - `Start` commands stash the supplied `correlation_id` so bulk seeding reuses it.
/// - Watcher bursts forward their correlation (or fall back to the stored one) into every enqueue.
/// - Each `EnqueueRequest` keeps that value, letting downstream dispatchers surface it on job events.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LibraryActorCommand {
    Start {
        mode: StartMode,
        correlation_id: Option<Uuid>,
    },
    Shutdown,
    Pause,
    Resume,
    FsEvents {
        root: LibraryRootsId,
        events: Vec<FileSystemEvent>,
        correlation_id: Option<Uuid>,
    },
    JobCompleted {
        job_id: JobId,
        dedupe_key: DedupeKey,
    },
    JobFailed {
        job_id: JobId,
        dedupe_key: DedupeKey,
        retryable: bool,
        error: Option<String>,
    },
}

/// Events emitted by the `LibraryActor`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LibraryActorEvent {
    /// Request orchestrator to enqueue a folder scan. Actors must not enqueue directly.
    EnqueueFolderScan {
        folder_path: String,
        priority: JobPriority,
        reason: ScanReason,
        parent: ParentDescriptors,
        correlation_id: Option<Uuid>,
    },
    JobEnqueued(JobHandle),
    JobThrottled {
        dedupe_key: DedupeKey,
    },
}

/// Tracks outstanding jobs and budget tokens per library.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LibraryActorState {
    pub outstanding_jobs: HashMap<DedupeKey, IssuedJobRecord>,
    pub roots: HashMap<LibraryRootsId, LibraryRootState>,
    pub is_paused: bool,
    pub active_folder_scans: HashSet<String>,
    #[serde(default)]
    pub current_correlation: Option<Uuid>,
    #[serde(default)]
    pub is_bulk_scanning: bool,
}

impl LibraryActorState {
    pub fn record_job(&mut self, record: IssuedJobRecord) {
        self.outstanding_jobs
            .insert(record.dedupe_key.clone(), record);
    }

    pub fn release_job(&mut self, dedupe_key: &DedupeKey) -> Option<IssuedJobRecord> {
        self.outstanding_jobs.remove(dedupe_key)
    }

    pub fn is_scan_active(&self, folder: &str) -> bool {
        self.active_folder_scans.contains(folder)
    }

    pub fn mark_scan_active(&mut self, folder: &str) {
        self.active_folder_scans.insert(folder.to_owned());
    }

    pub fn mark_scan_inactive(&mut self, folder: &str) {
        self.active_folder_scans.remove(folder);
    }

    pub fn coalesce_events(&self, events: &[FileSystemEvent]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut folders = Vec::new();

        for event in events {
            let candidate = if event.path.is_dir() {
                event.path.clone()
            } else {
                event
                    .path
                    .parent()
                    .map(|parent| parent.to_path_buf())
                    .unwrap_or_else(|| event.path.clone())
            };

            let folder_norm = normalize_path(&candidate);
            if seen.insert(folder_norm.clone()) {
                folders.push(folder_norm);
            }
        }

        folders
    }
}

/// Simplified representation of filesystem change events delivered to a library actor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FileSystemEventKind {
    Created,
    Modified,
    Deleted,
    Moved,
    Overflow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileSystemEvent {
    pub version: u16,
    pub correlation_id: Option<Uuid>,
    pub idempotency_key: String,
    pub library_id: LibraryID,
    pub path_key: String,
    pub fingerprint: Option<String>,
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub kind: FileSystemEventKind,
    pub occurred_at: DateTime<Utc>,
}

/// Trait describing the behaviours expected from a library actor implementation.
#[async_trait]
pub trait LibraryActor: Send + Sync {
    fn config(&self) -> &LibraryActorConfig;
    fn state(&self) -> &LibraryActorState;
    fn state_mut(&mut self) -> &mut LibraryActorState;

    async fn handle_command(
        &mut self,
        command: LibraryActorCommand,
    ) -> Result<Vec<LibraryActorEvent>>;
}

/// Library actor that directly enqueues jobs via QueueService and manages budget tokens.
pub struct DefaultLibraryActor<Q, O, E>
where
    Q: QueueService + Send + Sync,
    O: ActorObserver,
    E: JobEventPublisher,
{
    config: LibraryActorConfig,
    state: LibraryActorState,
    queue: Arc<Q>,
    observer: Arc<O>,
    events: Arc<E>,
    correlations: CorrelationCache,
}

impl<Q, O, E> fmt::Debug for DefaultLibraryActor<Q, O, E>
where
    Q: QueueService + Send + Sync,
    O: ActorObserver,
    E: JobEventPublisher,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultLibraryActor")
            .field("library_id", &self.config.library.id)
            .field("library_name", &self.config.library.name)
            .field("queue_type", &type_name::<Q>())
            .field("observer_type", &type_name::<O>())
            .field("event_bus_type", &type_name::<E>())
            .field("outstanding_jobs", &self.state.outstanding_jobs.len())
            .field("active_scans", &self.state.active_folder_scans.len())
            .field("is_paused", &self.state.is_paused)
            .finish()
    }
}

impl<Q, O, E> DefaultLibraryActor<Q, O, E>
where
    Q: QueueService + Send + Sync,
    O: ActorObserver,
    E: JobEventPublisher,
{
    pub fn new(
        config: LibraryActorConfig,
        queue: Arc<Q>,
        observer: Arc<O>,
        events: Arc<E>,
        correlations: CorrelationCache,
    ) -> Self {
        Self {
            config,
            state: LibraryActorState::default(),
            queue,
            observer,
            events,
            correlations,
        }
    }

    async fn enqueue_folder_scan(
        &mut self,
        folder_path: String,
        priority: JobPriority,
        reason: ScanReason,
        parent: Option<ParentDescriptors>,
        correlation_id: Option<Uuid>,
    ) -> Result<Vec<LibraryActorEvent>> {
        let library_id = self.config.library.id;
        let dedupe_key = DedupeKey::FolderScan {
            library_id,
            folder_path_norm: folder_path.clone(),
        };

        if self.state.is_scan_active(&folder_path) {
            return Ok(vec![LibraryActorEvent::JobThrottled { dedupe_key }]);
        }

        // For bulk seeding we bypass the in-memory outstanding throttle so we can
        // enqueue all folders into the persistent queue up-front. The scheduler
        // will regulate actual execution concurrency.
        if !matches!(reason, ScanReason::BulkSeed) {
            let outstanding_limit_reached = self.state.outstanding_jobs.len()
                >= self.config.max_outstanding_jobs
                && !self.state.outstanding_jobs.contains_key(&dedupe_key);
            if outstanding_limit_reached {
                return Ok(vec![LibraryActorEvent::JobThrottled { dedupe_key }]);
            }
        }

        // Record outstanding and mark active; orchestrator will enqueue from the returned event
        self.state.record_job(IssuedJobRecord {
            dedupe_key: dedupe_key.clone(),
            job_id: None,
            issued_at: Utc::now(),
            pending_children: vec![],
        });
        self.state.mark_scan_active(&folder_path);
        let queued_total = self.state.outstanding_jobs.len();
        debug!(
            target: "scan::queue",
            library_id = %library_id,
            folder = %folder_path,
            queued_total,
            reason = ?reason,
            priority = ?priority,
            "requesting enqueue for folder scan (via orchestrator)"
        );
        Ok(vec![LibraryActorEvent::EnqueueFolderScan {
            folder_path,
            priority,
            reason,
            parent: parent.unwrap_or_default(),
            correlation_id,
        }])
    }

    async fn seed_bulk_folders(
        &mut self,
        correlation_id: Option<Uuid>,
    ) -> Result<Vec<LibraryActorEvent>> {
        let mut events = Vec::new();

        let root_paths: Vec<String> = self.config.roots().map(|r| r.path_norm).collect();
        let preview: Vec<&str> = root_paths.iter().take(5).map(|s| s.as_str()).collect();
        info!(
            target: "scan::seed",
            library_id = %self.config.library.id,
            roots = root_paths.len(),
            max_outstanding = self.config.max_outstanding_jobs,
            preview = ?preview,
            "preparing bulk folder scan seed (depth=1)"
        );

        // Depth-1 enumeration only; let FolderScan jobs recurse
        let (folders, skipped) = Self::enumerate_first_level_folders(&root_paths).await;

        info!(
            target: "scan::seed",
            library_id = %self.config.library.id,
            folders = folders.len(),
            skipped,
            "bulk seed enumerated root child folders"
        );

        for path in folders {
            // For bulk seeding we bypass outstanding throttles; persistence dedupe ensures safety.
            let mut issued = self
                .enqueue_folder_scan(
                    path,
                    JobPriority::P1,
                    ScanReason::BulkSeed,
                    Some(ParentDescriptors {
                        resolved_type: Some(self.config.library.library_type),
                        ..ParentDescriptors::default()
                    }),
                    correlation_id,
                )
                .await?;
            events.append(&mut issued);
        }

        Ok(events)
    }

    // Enumerate immediate child folders for each root (depth=1).
    // Continues on errors; returns (folders, skipped_count).
    async fn enumerate_first_level_folders(root_paths: &[String]) -> (Vec<String>, usize) {
        use tokio::fs;
        let mut folders: Vec<String> = Vec::new();
        let mut skipped: usize = 0;

        for root in root_paths {
            match fs::read_dir(root).await {
                Ok(mut rd) => {
                    while let Ok(Some(entry)) = rd.next_entry().await {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        if name_str.starts_with('.') {
                            continue;
                        }
                        match entry.metadata().await {
                            Ok(meta) => {
                                if meta.is_dir() {
                                    let path = entry.path();
                                    let norm = normalize_path(&path);
                                    folders.push(norm);
                                }
                            }
                            Err(err) => {
                                skipped += 1;
                                warn!(
                                    target: "scan::seed",
                                    path = %root,
                                    error = %err,
                                    "skipping entry due to metadata error"
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    skipped += 1;
                    warn!(
                        target: "scan::seed",
                        path = %root,
                        error = %err,
                        "skipping directory due to read_dir error"
                    );
                }
            }
        }
        (folders, skipped)
    }

    // Removed recursive enumerator in favor of depth-1 seed + per-folder recursion

    async fn handle_fs_events(
        &mut self,
        root: LibraryRootsId,
        events: Vec<FileSystemEvent>,
        correlation_id: Option<Uuid>,
    ) -> Result<Vec<LibraryActorEvent>> {
        if self.state.is_bulk_scanning {
            return Ok(vec![]);
        }
        let mut responses = Vec::new();

        let state_correlation = self.state.current_correlation;
        let event_hint = events.iter().find_map(|event| event.correlation_id);
        let burst_correlation = correlation_id.or(state_correlation).or(event_hint);

        let (overflow, changes): (Vec<_>, Vec<_>) = events
            .into_iter()
            .partition(|event| matches!(event.kind, FileSystemEventKind::Overflow));

        if !overflow.is_empty() {
            let mut targets = HashSet::new();
            for event in &overflow {
                if let Some(path) = self.event_scan_target(event) {
                    targets.insert(path);
                }
            }

            if targets.is_empty()
                && let Some(root_path) = self.config.root_path(root)
            {
                targets.insert(normalize_path(&root_path));
            }

            for folder in targets {
                let mut issued = self
                    .enqueue_folder_scan(
                        folder,
                        JobPriority::P0,
                        ScanReason::WatcherOverflow,
                        Some(ParentDescriptors {
                            resolved_type: Some(self.config.library.library_type),
                            ..ParentDescriptors::default()
                        }),
                        burst_correlation,
                    )
                    .await?;
                responses.append(&mut issued);
            }
        }

        if !changes.is_empty() {
            // Filter out non-media file changes to avoid starving bulk scans
            // with HotChange re-scans caused by our own image writes, etc.
            let total_changes = changes.len();
            let filtered: Vec<FileSystemEvent> = changes
                .into_iter()
                .filter(|ev| {
                    if ev.path.is_dir() {
                        return true;
                    }
                    is_media_file_path(&ev.path)
                })
                .collect();
            let dropped = total_changes.saturating_sub(filtered.len());
            if dropped > 0 {
                warn!(
                    target: "scan::events",
                    dropped,
                    "ignored non-media file change events"
                );
            }
            let folders = self.state.coalesce_events(&filtered);
            for folder in folders {
                let mut issued = self
                    .enqueue_folder_scan(
                        folder,
                        JobPriority::P0,
                        ScanReason::HotChange,
                        Some(ParentDescriptors {
                            resolved_type: Some(self.config.library.library_type),
                            ..ParentDescriptors::default()
                        }),
                        burst_correlation,
                    )
                    .await?;
                responses.append(&mut issued);
            }
        }

        Ok(responses)
    }

    fn event_scan_target(&self, event: &FileSystemEvent) -> Option<String> {
        if event.path.as_os_str().is_empty() {
            return None;
        }

        let folder = if event.path.is_dir() {
            event.path.clone()
        } else {
            event
                .path
                .parent()
                .map(|parent| parent.to_path_buf())
                .unwrap_or_else(|| event.path.clone())
        };

        Some(normalize_path(&folder))
    }
}

#[async_trait]
impl<Q, O, E> LibraryActor for DefaultLibraryActor<Q, O, E>
where
    Q: QueueService + Send + Sync,
    O: ActorObserver,
    E: JobEventPublisher,
{
    fn config(&self) -> &LibraryActorConfig {
        &self.config
    }

    fn state(&self) -> &LibraryActorState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut LibraryActorState {
        &mut self.state
    }

    async fn handle_command(
        &mut self,
        command: LibraryActorCommand,
    ) -> Result<Vec<LibraryActorEvent>> {
        if self.state.is_paused {
            match command {
                LibraryActorCommand::Resume => {
                    self.state.is_paused = false;
                    Ok(vec![])
                }
                LibraryActorCommand::Shutdown => {
                    self.state.outstanding_jobs.clear();
                    self.state.active_folder_scans.clear();
                    self.state.current_correlation = None;
                    Ok(vec![])
                }
                _ => Ok(vec![]), // Ignore other commands when paused
            }
        } else {
            match command {
                LibraryActorCommand::Start {
                    mode,
                    correlation_id,
                } => {
                    self.state.current_correlation = correlation_id;
                    match mode {
                        StartMode::Bulk => {
                            self.state.is_bulk_scanning = true;
                            // Initialize root states and seed bulk folders
                            for root in self.config.roots() {
                                self.state.roots.insert(
                                    root.root_id,
                                    LibraryRootState {
                                        last_scan_at: None,
                                        is_watching: true,
                                    },
                                );
                            }
                            self.seed_bulk_folders(correlation_id).await
                        }
                        StartMode::Maintenance | StartMode::Resume => {
                            self.state.is_bulk_scanning = false;
                            // Initialize roots for watching only
                            for root in self.config.roots() {
                                self.state.roots.insert(
                                    root.root_id,
                                    LibraryRootState {
                                        last_scan_at: None,
                                        is_watching: true,
                                    },
                                );
                            }
                            Ok(vec![])
                        }
                    }
                }
                LibraryActorCommand::FsEvents {
                    root,
                    events,
                    correlation_id,
                } => self.handle_fs_events(root, events, correlation_id).await,
                LibraryActorCommand::JobCompleted { dedupe_key, .. } => {
                    let _ = self.state.release_job(&dedupe_key);
                    if let DedupeKey::FolderScan {
                        folder_path_norm, ..
                    } = &dedupe_key
                    {
                        self.state.mark_scan_inactive(folder_path_norm);
                    }
                    Ok(vec![])
                }
                LibraryActorCommand::JobFailed { dedupe_key, .. } => {
                    let _ = self.state.release_job(&dedupe_key);
                    if let DedupeKey::FolderScan {
                        folder_path_norm, ..
                    } = &dedupe_key
                    {
                        self.state.mark_scan_inactive(folder_path_norm);
                    }
                    Ok(vec![])
                }
                LibraryActorCommand::Pause => {
                    self.state.is_paused = true;
                    self.state.current_correlation = None;
                    for root_state in self.state.roots.values_mut() {
                        root_state.is_watching = false;
                    }
                    Ok(vec![])
                }
                LibraryActorCommand::Resume => {
                    self.state.is_paused = false;
                    for root_state in self.state.roots.values_mut() {
                        root_state.is_watching = true;
                    }
                    Ok(vec![])
                }
                LibraryActorCommand::Shutdown => {
                    // Clear outstanding job tracking and exit
                    self.state.outstanding_jobs.clear();
                    self.state.active_folder_scans.clear();
                    self.state.current_correlation = None;
                    Ok(vec![])
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use sha2::{Digest, Sha256};
    use std::sync::Arc;
    use tokio::sync::Mutex as AsyncMutex;

    use crate::{
        error::MediaError,
        orchestration::{
            FolderScanJob, NoopActorObserver,
            correlation::CorrelationCache,
            events::{JobEvent, JobEventPublisher},
            job::{EnqueueRequest, JobHandle, JobKind, JobPayload},
            lease::{DequeueRequest, JobLease, LeaseId, LeaseRenewal},
        },
        types::LibraryType,
    };
    use std::fmt;

    #[derive(Clone, Debug)]
    struct RecordedJob {
        job: FolderScanJob,
        correlation: Option<Uuid>,
    }

    #[derive(Clone, Default)]
    struct RecordingQueue {
        jobs: Arc<AsyncMutex<Vec<RecordedJob>>>,
    }

    impl fmt::Debug for RecordingQueue {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.jobs.try_lock() {
                Ok(guard) => f
                    .debug_struct("RecordingQueue")
                    .field("queued_jobs", &guard.len())
                    .finish(),
                Err(_) => f
                    .debug_struct("RecordingQueue")
                    .field("queued_jobs", &"<locked>")
                    .finish(),
            }
        }
    }

    #[derive(Debug, Clone, Default)]
    struct NoopPublisher;

    #[async_trait]
    impl JobEventPublisher for NoopPublisher {
        async fn publish(&self, _event: JobEvent) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct RecordingPublisher {
        events: Arc<AsyncMutex<Vec<JobEvent>>>,
    }

    impl fmt::Debug for RecordingPublisher {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.events.try_lock() {
                Ok(guard) => f
                    .debug_struct("RecordingPublisher")
                    .field("event_count", &guard.len())
                    .finish(),
                Err(_) => f
                    .debug_struct("RecordingPublisher")
                    .field("event_count", &"<locked>")
                    .finish(),
            }
        }
    }

    #[async_trait]
    impl JobEventPublisher for RecordingPublisher {
        async fn publish(&self, event: JobEvent) -> Result<()> {
            self.events.lock().await.push(event);
            Ok(())
        }
    }

    #[async_trait]
    impl QueueService for RecordingQueue {
        async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle> {
            if let JobPayload::FolderScan(job) = &request.payload {
                self.jobs.lock().await.push(RecordedJob {
                    job: job.clone(),
                    correlation: request.correlation_id,
                });
            }
            Ok(JobHandle::accepted(
                JobId::new(),
                &request.payload,
                request.priority,
            ))
        }

        async fn dequeue(&self, _request: DequeueRequest) -> Result<Option<JobLease>> {
            Ok(None)
        }

        async fn renew(&self, _renewal: LeaseRenewal) -> Result<JobLease> {
            Err(MediaError::Internal(
                "renew not implemented in RecordingQueue".into(),
            ))
        }

        async fn complete(&self, _lease_id: LeaseId) -> Result<()> {
            Ok(())
        }

        async fn fail(
            &self,
            _lease_id: LeaseId,
            _retryable: bool,
            _error: Option<String>,
        ) -> Result<()> {
            Ok(())
        }

        async fn dead_letter(&self, _lease_id: LeaseId, _error: Option<String>) -> Result<()> {
            Ok(())
        }

        async fn cancel_job(&self, _job_id: JobId) -> Result<()> {
            Ok(())
        }

        async fn queue_depth(&self, _kind: JobKind) -> Result<usize> {
            Ok(0)
        }
    }

    fn hash_parts(parts: &[&str]) -> String {
        let mut hasher = Sha256::new();
        for part in parts {
            hasher.update(part.as_bytes());
        }
        let digest = hasher.finalize();
        URL_SAFE_NO_PAD.encode(&digest[..16])
    }

    fn make_event(
        path: &PathBuf,
        kind: FileSystemEventKind,
        library_id: LibraryID,
    ) -> FileSystemEvent {
        make_event_with_correlation(path, kind, library_id, None)
    }

    fn make_event_with_correlation(
        path: &PathBuf,
        kind: FileSystemEventKind,
        library_id: LibraryID,
        correlation: Option<Uuid>,
    ) -> FileSystemEvent {
        let path_key = normalize_path(path);
        FileSystemEvent {
            version: 1,
            correlation_id: correlation,
            idempotency_key: hash_parts(&["fs-test", &library_id.to_string(), &path_key]),
            library_id,
            path_key,
            fingerprint: None,
            path: path.clone(),
            old_path: None,
            kind,
            occurred_at: Utc::now(),
        }
    }

    fn make_actor(
        queue: Arc<RecordingQueue>,
        root: PathBuf,
    ) -> DefaultLibraryActor<RecordingQueue, NoopActorObserver, NoopPublisher> {
        let library_id = LibraryID::new();
        let reference = LibraryReference {
            id: library_id,
            name: "Test".into(),
            library_type: LibraryType::Movies,
            paths: vec![root.clone()],
        };
        let config = LibraryActorConfig {
            library: reference,
            root_paths: vec![root],
            max_outstanding_jobs: 8,
        };
        DefaultLibraryActor::new(
            config,
            queue,
            Arc::new(NoopActorObserver),
            Arc::new(NoopPublisher::default()),
            CorrelationCache::default(),
        )
    }

    fn make_actor_with_publisher(
        queue: Arc<RecordingQueue>,
        root: PathBuf,
        publisher: Arc<RecordingPublisher>,
    ) -> DefaultLibraryActor<RecordingQueue, NoopActorObserver, RecordingPublisher> {
        let library_id = LibraryID::new();
        let reference = LibraryReference {
            id: library_id,
            name: "Test".into(),
            library_type: LibraryType::Movies,
            paths: vec![root.clone()],
        };
        let config = LibraryActorConfig {
            library: reference,
            root_paths: vec![root],
            max_outstanding_jobs: 8,
        };
        DefaultLibraryActor::new(
            config,
            queue,
            Arc::new(NoopActorObserver),
            publisher,
            CorrelationCache::default(),
        )
    }

    #[tokio::test]
    async fn bulk_scan_is_correlated() -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        std::fs::create_dir_all(root.join("seed")).unwrap();
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor =
            make_actor_with_publisher(Arc::clone(&queue), root.clone(), Arc::clone(&publisher));
        let correlation = Uuid::now_v7();

        std::fs::create_dir_all(root.join("seed")).unwrap();

        let events = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(correlation),
            })
            .await?;

        let enqueued = events
            .iter()
            .find_map(|event| {
                if let LibraryActorEvent::EnqueueFolderScan { correlation_id, .. } = event {
                    Some(correlation_id.clone())
                } else {
                    None
                }
            })
            .expect("expected an enqueue event");

        assert_eq!(enqueued, Some(correlation));
        assert_eq!(actor.state.outstanding_jobs.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_events_are_ignored_during_bulk_scan() -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor =
            make_actor_with_publisher(Arc::clone(&queue), root.clone(), Arc::clone(&publisher));
        let library_id = actor.config.library.id;
        let scan_id = Uuid::now_v7();

        let _ = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(scan_id),
            })
            .await?;

        let folder = root.join("watch-folders");
        std::fs::create_dir_all(&folder).unwrap();
        let events = vec![make_event(
            &folder.join("fresh.mkv"),
            FileSystemEventKind::Created,
            library_id,
        )];

        let responses = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events,
                correlation_id: None,
            })
            .await?;

        // While a bulk scan is underway watcher bursts are ignored—the bulk seed
        // has already enqueued every folder, so any queued work here would be
        // redundant and could reintroduce the incomplete-state bug these guards
        // were meant to avoid.
        assert!(
            responses
                .iter()
                .all(|event| !matches!(event, LibraryActorEvent::EnqueueFolderScan { .. })),
            "fs events during bulk should not enqueue additional scans"
        );

        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_is_correlated_during_maintenance_scan() -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor =
            make_actor_with_publisher(Arc::clone(&queue), root.clone(), Arc::clone(&publisher));
        let library_id = actor.config.library.id;
        let correlation = Uuid::now_v7();

        let _ = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Maintenance,
                correlation_id: Some(correlation),
            })
            .await?;

        let folder = root.join("watch-maintenance");
        std::fs::create_dir_all(&folder).unwrap();
        let events = vec![make_event(
            &folder.join("fresh.mkv"),
            FileSystemEventKind::Created,
            library_id,
        )];

        let responses = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events,
                correlation_id: None,
            })
            .await?;

        let enqueued = responses
            .iter()
            .find_map(|event| {
                if let LibraryActorEvent::EnqueueFolderScan {
                    correlation_id: observed,
                    ..
                } = event
                {
                    Some(observed.clone())
                } else {
                    None
                }
            })
            .expect("maintenance watcher should enqueue folder scan");

        assert_eq!(enqueued, Some(correlation));

        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_without_scan_can_be_uncorrelated() -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor =
            make_actor_with_publisher(Arc::clone(&queue), root.clone(), Arc::clone(&publisher));
        let library_id = actor.config.library.id;

        let _ = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Resume,
                correlation_id: None,
            })
            .await?;

        let folder = root.join("watch-uncorrelated");
        std::fs::create_dir_all(&folder).unwrap();
        let events = vec![make_event(
            &folder.join("clip.mkv"),
            FileSystemEventKind::Created,
            library_id,
        )];

        let responses = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events,
                correlation_id: None,
            })
            .await?;

        let enqueued = responses
            .iter()
            .find_map(|event| {
                if let LibraryActorEvent::EnqueueFolderScan { correlation_id, .. } = event {
                    Some(correlation_id.clone())
                } else {
                    None
                }
            })
            .expect("expected enqueue response");

        assert_eq!(enqueued, None);

        Ok(())
    }

    #[tokio::test]
    async fn burst_of_events_enqueues_single_scan() -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let queue = Arc::new(RecordingQueue::default());
        let mut actor = make_actor(Arc::clone(&queue), root.clone());
        let library_id = actor.config.library.id;

        let _ = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Resume,
                correlation_id: None,
            })
            .await?;

        let folder = root.join("movies");
        std::fs::create_dir_all(&folder).unwrap();
        let events = vec![
            make_event(
                &folder.join("a.mkv"),
                FileSystemEventKind::Created,
                library_id,
            ),
            make_event(
                &folder.join("b.mkv"),
                FileSystemEventKind::Modified,
                library_id,
            ),
            make_event(
                &folder.join("c.mkv"),
                FileSystemEventKind::Deleted,
                library_id,
            ),
        ];

        let responses = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events,
                correlation_id: None,
            })
            .await?;

        let enqueued = responses
            .iter()
            .find_map(|event| {
                if let LibraryActorEvent::EnqueueFolderScan {
                    folder_path,
                    reason,
                    ..
                } = event
                {
                    Some((folder_path.clone(), reason.clone()))
                } else {
                    None
                }
            })
            .expect("expected enqueue response");

        assert!(matches!(enqueued.1, ScanReason::HotChange));
        assert!(enqueued.0.ends_with("movies"));

        Ok(())
    }

    #[tokio::test]
    async fn overflow_triggers_rescan() -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let queue = Arc::new(RecordingQueue::default());
        let mut actor = make_actor(Arc::clone(&queue), root.clone());
        let library_id = actor.config.library.id;

        let _ = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Resume,
                correlation_id: None,
            })
            .await?;

        let event = FileSystemEvent {
            version: 1,
            correlation_id: None,
            idempotency_key: hash_parts(&["overflow", &library_id.to_string()]),
            library_id,
            path_key: normalize_path(&root),
            fingerprint: None,
            path: root.clone(),
            old_path: None,
            kind: FileSystemEventKind::Overflow,
            occurred_at: Utc::now(),
        };

        let responses = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events: vec![event],
                correlation_id: None,
            })
            .await?;

        let enqueued = responses
            .iter()
            .find_map(|event| {
                if let LibraryActorEvent::EnqueueFolderScan {
                    folder_path,
                    reason,
                    ..
                } = event
                {
                    Some((folder_path.clone(), reason.clone()))
                } else {
                    None
                }
            })
            .expect("expected enqueue response");

        assert!(matches!(enqueued.1, ScanReason::WatcherOverflow));
        assert_eq!(enqueued.0, normalize_path(&root));

        Ok(())
    }
}
