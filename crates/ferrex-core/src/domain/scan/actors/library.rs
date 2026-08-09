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

use crate::domain::scan::MediaCandidate;
use crate::{
    error::Result,
    types::{ids::LibraryId, prelude::LibraryReference},
};

use super::folder::ScannerFileFilterPolicy;
use super::messages::{ActorObserver, IssuedJobRecord};
use crate::domain::scan::orchestration::{
    correlation::CorrelationCache,
    events::JobEventPublisher,
    job::{
        DedupeKey, EnqueueRequest, JobHandle, JobId, JobPayload, JobPriority,
        MetadataEnrichJob,
    },
    queue::QueueService,
    scan_cursor::normalize_path,
    work_planning::{
        FsEventPlanningInput, LibraryStartPlanningInput, ScanFilesystemEvent,
        ScanFilesystemEventKind, ScanPlanningLimits, ScanPlanningRoot,
        ScanStartPlanningMode, plan_fs_event_burst, plan_library_start,
    },
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
        self.root_paths.iter().enumerate().map(|(idx, path)| {
            LibraryRootDescriptor {
                root_id: LibraryRootsId(idx as u16),
                path_norm: path.to_string_lossy().to_string(),
            }
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
    /// Request orchestrator to enqueue a planned folder scan. Actors must not enqueue directly.
    EnqueueFolderScan {
        request: Box<EnqueueRequest>,
    },
    /// Request orchestrator to enqueue a metadata enrich job (e.g., series pre-seed).
    EnqueueMetadataEnrich {
        job: Box<MetadataEnrichJob>,
        priority: JobPriority,
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

    pub fn release_job(
        &mut self,
        dedupe_key: &DedupeKey,
    ) -> Option<IssuedJobRecord> {
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
    pub library_id: LibraryId,
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
    _queue: Arc<Q>,
    _observer: Arc<O>,
    _events: Arc<E>,
    _correlations: CorrelationCache,
    file_filters: ScannerFileFilterPolicy,
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
        Self::with_file_filter_policy(
            config,
            queue,
            observer,
            events,
            correlations,
            ScannerFileFilterPolicy::default(),
        )
    }

    pub fn with_file_filter_policy(
        config: LibraryActorConfig,
        queue: Arc<Q>,
        observer: Arc<O>,
        events: Arc<E>,
        correlations: CorrelationCache,
        file_filters: ScannerFileFilterPolicy,
    ) -> Self {
        Self {
            config,
            state: LibraryActorState::default(),
            _queue: queue,
            _observer: observer,
            _events: events,
            _correlations: correlations,
            file_filters,
        }
    }

    fn planning_roots(&self) -> Vec<ScanPlanningRoot> {
        self.config
            .roots()
            .map(|root| {
                ScanPlanningRoot::with_path_norm(
                    root.root_id.0,
                    PathBuf::from(&root.path_norm),
                    root.path_norm,
                )
            })
            .collect()
    }

    fn planning_start_mode(mode: StartMode) -> ScanStartPlanningMode {
        match mode {
            StartMode::Bulk => ScanStartPlanningMode::Bulk,
            StartMode::Maintenance => ScanStartPlanningMode::Maintenance,
            StartMode::Resume => ScanStartPlanningMode::Resume,
        }
    }

    fn planning_event_kind(
        kind: FileSystemEventKind,
    ) -> ScanFilesystemEventKind {
        match kind {
            FileSystemEventKind::Created => ScanFilesystemEventKind::Created,
            FileSystemEventKind::Modified => ScanFilesystemEventKind::Modified,
            FileSystemEventKind::Deleted => ScanFilesystemEventKind::Deleted,
            FileSystemEventKind::Moved => ScanFilesystemEventKind::Moved,
            FileSystemEventKind::Overflow => ScanFilesystemEventKind::Overflow,
        }
    }

    fn release_tracked_job(&mut self, dedupe_key: &DedupeKey) {
        let _ = self.state.release_job(dedupe_key);
        if let DedupeKey::FolderScan {
            candidate: MediaCandidate { path_norm, .. },
        } = dedupe_key
        {
            self.state.mark_scan_inactive(path_norm);
        }
    }

    async fn enqueue_folder_scan(
        &mut self,
        request: EnqueueRequest,
    ) -> Result<Vec<LibraryActorEvent>> {
        let library_id = self.config.library.id;
        let (context_library_id, folder_path, reason) = match &request.payload {
            JobPayload::FolderScan(job) => (
                job.context.library_id(),
                job.context.folder_path_norm().to_string(),
                job.scan_reason,
            ),
            _ => {
                return Err(crate::error::MediaError::Internal(
                    "library actor can only admit folder scan requests".into(),
                ));
            }
        };

        if context_library_id != library_id {
            return Err(crate::error::MediaError::Internal(format!(
                "enqueue folder scan library_id mismatch (actor={}, context={}, folder={})",
                library_id, context_library_id, folder_path
            )));
        }

        let dedupe_key = request.dedupe_key();

        if self.state.is_scan_active(&folder_path) {
            return Ok(vec![LibraryActorEvent::JobThrottled { dedupe_key }]);
        }

        // Record outstanding and mark active; orchestrator will enqueue from the returned event.
        self.state.record_job(IssuedJobRecord {
            dedupe_key: dedupe_key.clone(),
            job_id: None,
            issued_at: request.requested_at,
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
            priority = ?request.priority,
            "requesting enqueue for folder scan (via orchestrator)"
        );
        Ok(vec![LibraryActorEvent::EnqueueFolderScan {
            request: Box::new(request),
        }])
    }

    async fn seed_bulk_folders(
        &mut self,
        mode: StartMode,
        correlation_id: Option<Uuid>,
    ) -> Result<Vec<LibraryActorEvent>> {
        let mut events = Vec::new();

        let roots = self.planning_roots();
        let root_paths: Vec<String> =
            roots.iter().map(|root| root.path_norm.clone()).collect();
        let preview: Vec<&str> =
            root_paths.iter().take(5).map(|s| s.as_str()).collect();
        info!(
            target: "scan::seed",
            library_id = %self.config.library.id,
            roots = root_paths.len(),
            max_outstanding = self.config.max_outstanding_jobs,
            preview = ?preview,
            "preparing bulk folder scan seed (depth=1)"
        );

        let plan = plan_library_start(LibraryStartPlanningInput {
            library: &self.config.library,
            roots: &roots,
            mode: Self::planning_start_mode(mode),
            correlation_id,
            limits: ScanPlanningLimits::unbounded(),
            now: Utc::now(),
        })
        .await?;

        for error in &plan.errors {
            warn!(
                target: "scan::seed",
                library_id = %self.config.library.id,
                error = %error,
                "bulk seed planner reported an enumeration error"
            );
        }

        info!(
            target: "scan::seed",
            library_id = %self.config.library.id,
            folders = plan.requests.len(),
            skipped = plan.skipped_entries,
            "bulk seed enumerated root child folders"
        );

        for request in plan.requests {
            // For bulk seeding we bypass outstanding throttles; persistence dedupe ensures safety.
            let mut issued = self.enqueue_folder_scan(request).await?;
            events.append(&mut issued);
        }

        Ok(events)
    }

    async fn handle_fs_events(
        &mut self,
        root: LibraryRootsId,
        events: Vec<FileSystemEvent>,
        correlation_id: Option<Uuid>,
    ) -> Result<Vec<LibraryActorEvent>> {
        let mut responses = Vec::new();

        let Some(root_path) = self.config.root_path(root) else {
            warn!(
                target: "scan::events",
                root_id = root.0,
                "fs events received for unknown root id"
            );
            return Ok(responses);
        };
        let root_path_norm = normalize_path(&root_path)?;
        let planning_root = ScanPlanningRoot::with_path_norm(
            root.0,
            root_path.clone(),
            root_path_norm,
        );
        let planning_events = events
            .into_iter()
            .map(|event| ScanFilesystemEvent {
                correlation_id: event.correlation_id,
                path: event.path,
                kind: Self::planning_event_kind(event.kind),
            })
            .collect();

        let mut plan = plan_fs_event_burst(FsEventPlanningInput {
            library: &self.config.library,
            root: planning_root,
            events: planning_events,
            command_correlation_id: correlation_id,
            state_correlation_id: self.state.current_correlation,
            file_filters: &self.file_filters,
            limits: ScanPlanningLimits::unbounded(),
            now: Utc::now(),
        })
        .await?;

        // Overflow planning and ordinary change planning can identify the same
        // folder in one burst. Keep admission atomic and let the durable queue
        // dedupe across separate commands.
        let mut planned_folders = HashSet::new();
        plan.requests.retain(|request| match &request.payload {
            JobPayload::FolderScan(job) => planned_folders
                .insert(job.context.folder_path_norm().to_string()),
            _ => true,
        });

        // A watcher batch is acknowledged as a unit. Check every target before
        // recording any of them so an active folder makes the whole batch
        // retryable instead of silently dropping only the conflicting path.
        let active_folders: Vec<_> = plan
            .requests
            .iter()
            .filter_map(|request| match &request.payload {
                JobPayload::FolderScan(job) => {
                    let folder = job.context.folder_path_norm();
                    self.state.is_scan_active(folder).then(|| folder.to_owned())
                }
                _ => None,
            })
            .collect();
        if !active_folders.is_empty() {
            return Err(crate::error::MediaError::ConcurrencyLimit(format!(
                "filesystem event targets already being scanned for library {}: {}",
                self.config.library.id,
                active_folders.join(", ")
            )));
        }

        if plan.dropped_events > 0 {
            warn!(
                target: "scan::events",
                dropped = plan.dropped_events,
                "ignored non-media file change events"
            );
        }
        for error in &plan.errors {
            warn!(
                target: "scan::events",
                root_id = root.0,
                error = %error,
                "filesystem event planner reported an enumeration error"
            );
        }

        for request in plan.requests {
            let mut issued = self.enqueue_folder_scan(request).await?;
            responses.append(&mut issued);
        }

        Ok(responses)
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
                LibraryActorCommand::Start {
                    mode: StartMode::Maintenance,
                    correlation_id,
                } => {
                    if self.state.is_bulk_scanning
                        && self.state.current_correlation != correlation_id
                    {
                        warn!(
                            library_id = %self.config.library.id,
                            current_correlation = ?self.state.current_correlation,
                            maintenance_correlation = ?correlation_id,
                            "ignoring stale maintenance transition for newer bulk scan"
                        );
                        return Ok(vec![]);
                    }
                    // Scan finalization uses Maintenance as the actor's durable
                    // reset transition. It must work while paused so a
                    // pause/cancel sequence cannot leave stale active folders
                    // suppressing every later bulk seed.
                    self.state.is_paused = false;
                    self.state.is_bulk_scanning = false;
                    self.state.current_correlation = correlation_id;
                    self.state.outstanding_jobs.clear();
                    self.state.active_folder_scans.clear();
                    self.state.roots.clear();
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
                LibraryActorCommand::Resume => {
                    self.state.is_paused = false;
                    for root_state in self.state.roots.values_mut() {
                        root_state.is_watching = true;
                    }
                    Ok(vec![])
                }
                LibraryActorCommand::JobCompleted { dedupe_key, .. }
                | LibraryActorCommand::JobFailed { dedupe_key, .. } => {
                    // Pausing admission does not pause already leased workers.
                    // Consume their terminal notifications so Resume cannot
                    // retain phantom active-folder conflicts.
                    self.release_tracked_job(&dedupe_key);
                    Ok(vec![])
                }
                LibraryActorCommand::Shutdown => {
                    self.state.outstanding_jobs.clear();
                    self.state.active_folder_scans.clear();
                    self.state.current_correlation = None;
                    Ok(vec![])
                }
                LibraryActorCommand::FsEvents { .. } => {
                    Err(crate::error::MediaError::ConcurrencyLimit(format!(
                        "filesystem event admission paused for library {}",
                        self.config.library.id
                    )))
                }
                LibraryActorCommand::Start {
                    mode: StartMode::Bulk | StartMode::Resume,
                    ..
                } => Err(crate::error::MediaError::ConcurrencyLimit(format!(
                    "scan admission paused for library {}",
                    self.config.library.id
                ))),
                LibraryActorCommand::Pause => Ok(vec![]),
            }
        } else {
            match command {
                LibraryActorCommand::Start {
                    mode,
                    correlation_id,
                } => match mode {
                    StartMode::Bulk => {
                        self.state.current_correlation = correlation_id;
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
                        self.seed_bulk_folders(mode, correlation_id).await
                    }
                    StartMode::Maintenance => {
                        if self.state.is_bulk_scanning
                            && self.state.current_correlation != correlation_id
                        {
                            warn!(
                                library_id = %self.config.library.id,
                                current_correlation = ?self.state.current_correlation,
                                maintenance_correlation = ?correlation_id,
                                "ignoring stale maintenance transition for newer bulk scan"
                            );
                            return Ok(vec![]);
                        }

                        let reset_bulk_state = self.state.is_bulk_scanning;
                        self.state.current_correlation = correlation_id;
                        self.state.is_bulk_scanning = false;
                        if reset_bulk_state {
                            self.state.outstanding_jobs.clear();
                            self.state.active_folder_scans.clear();
                        }
                        // Initialize roots for watching only.
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
                    StartMode::Resume => {
                        self.state.current_correlation = correlation_id;
                        self.state.is_bulk_scanning = false;
                        // Initialize roots for watching only.
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
                },
                LibraryActorCommand::FsEvents {
                    root,
                    events,
                    correlation_id,
                } => self.handle_fs_events(root, events, correlation_id).await,
                LibraryActorCommand::JobCompleted { dedupe_key, .. } => {
                    self.release_tracked_job(&dedupe_key);
                    Ok(vec![])
                }
                LibraryActorCommand::JobFailed { dedupe_key, .. } => {
                    self.release_tracked_job(&dedupe_key);
                    Ok(vec![])
                }
                LibraryActorCommand::Pause => {
                    self.state.is_paused = true;
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
    use std::{path::Path, sync::Arc};
    use tokio::sync::Mutex as AsyncMutex;

    use crate::domain::scan::orchestration::{
        FolderScanJob, NoopActorObserver,
        correlation::CorrelationCache,
        events::{JobEvent, JobEventPublisher},
        job::{DependencyKey, EnqueueRequest, JobHandle, JobKind, JobPayload},
        lease::{DequeueRequest, JobLease, LeaseId, LeaseRenewal},
    };
    use crate::{error::MediaError, types::LibraryType};
    use std::{collections::HashSet, fmt};

    #[derive(Clone, Debug)]
    struct RecordedJob {
        // Fields are recorded for potential future assertions; currently unused in these tests.
        _job: FolderScanJob,
        _correlation: Option<Uuid>,
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
                    _job: job.clone(),
                    _correlation: request.correlation_id,
                });
            }
            Ok(JobHandle::accepted(
                JobId::new(),
                &request.payload,
                request.priority,
            ))
        }

        async fn dequeue(
            &self,
            _request: DequeueRequest,
        ) -> Result<Option<JobLease>> {
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

        async fn dead_letter(
            &self,
            _lease_id: LeaseId,
            _error: Option<String>,
        ) -> Result<()> {
            Ok(())
        }

        async fn cancel_job(&self, _job_id: JobId) -> Result<()> {
            Ok(())
        }

        async fn queue_depth(&self, _kind: JobKind) -> Result<usize> {
            Ok(0)
        }

        async fn release_dependency(
            &self,
            _library_id: LibraryId,
            _dependency_key: &DependencyKey,
        ) -> Result<u64> {
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
        path: &Path,
        kind: FileSystemEventKind,
        library_id: LibraryId,
        correlation: Option<Uuid>,
    ) -> Result<FileSystemEvent> {
        let path_key = normalize_path(path)?;
        Ok(FileSystemEvent {
            version: 1,
            correlation_id: correlation,
            idempotency_key: hash_parts(&[
                "fs-test",
                &library_id.to_string(),
                &path_key,
            ]),
            library_id,
            path_key,
            fingerprint: None,
            path: path.to_path_buf(),
            old_path: None,
            kind,
            occurred_at: Utc::now(),
        })
    }

    fn enqueued_folder_paths(events: &[LibraryActorEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| {
                if let LibraryActorEvent::EnqueueFolderScan { request } = event
                    && let JobPayload::FolderScan(job) = &request.payload
                {
                    Some(job.context.folder_path_norm().to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    fn enqueued_correlations(
        events: &[LibraryActorEvent],
    ) -> Vec<Option<Uuid>> {
        events
            .iter()
            .filter_map(|event| {
                if let LibraryActorEvent::EnqueueFolderScan { request } = event
                {
                    Some(request.correlation_id)
                } else {
                    None
                }
            })
            .collect()
    }

    fn make_actor(
        queue: Arc<RecordingQueue>,
        root: PathBuf,
        publisher: Arc<RecordingPublisher>,
    ) -> DefaultLibraryActor<
        RecordingQueue,
        NoopActorObserver,
        RecordingPublisher,
    > {
        let library_id = LibraryId::new();
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
        let mut actor = make_actor(
            Arc::clone(&queue),
            root.clone(),
            Arc::clone(&publisher),
        );
        let correlation = Uuid::now_v7();

        std::fs::create_dir_all(root.join("seed")).unwrap();

        let events = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(correlation),
            })
            .await?;

        let enqueued = enqueued_correlations(&events)
            .into_iter()
            .next()
            .expect("expected an enqueue event");

        assert_eq!(enqueued, Some(correlation));
        assert_eq!(actor.state.outstanding_jobs.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_events_added_during_bulk_scan_are_enqueued() -> Result<()>
    {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor = make_actor(
            Arc::clone(&queue),
            root.clone(),
            Arc::clone(&publisher),
        );
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
            None,
        )?];

        let responses = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events,
                correlation_id: None,
            })
            .await?;

        assert_eq!(
            enqueued_folder_paths(&responses),
            vec![normalize_path(&folder)?],
            "folders created after bulk enumeration must still be scanned"
        );

        Ok(())
    }

    #[tokio::test]
    async fn watcher_burst_for_active_folder_is_retryable_without_state_change()
    -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let seeded_folder = root.join("seeded-movie");
        std::fs::create_dir_all(&seeded_folder).unwrap();
        let media_file = seeded_folder.join("feature.mkv");
        std::fs::write(&media_file, b"fixture").unwrap();
        let seeded_folder_norm = normalize_path(&seeded_folder)?;

        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor = make_actor(
            Arc::clone(&queue),
            root.clone(),
            Arc::clone(&publisher),
        );
        let library_id = actor.config.library.id;
        let scan_id = Uuid::now_v7();

        let start_events = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(scan_id),
            })
            .await?;

        assert_eq!(
            enqueued_folder_paths(&start_events),
            vec![seeded_folder_norm.clone()],
            "bulk seed should enqueue the folder once"
        );
        assert!(actor.state.is_bulk_scanning);
        assert_eq!(actor.state.active_folder_scans.len(), 1);
        assert!(actor.state.is_scan_active(&seeded_folder_norm));

        let burst = vec![
            make_event(
                &media_file,
                FileSystemEventKind::Created,
                library_id,
                None,
            )?,
            make_event(
                &media_file,
                FileSystemEventKind::Modified,
                library_id,
                None,
            )?,
            make_event(
                &seeded_folder,
                FileSystemEventKind::Modified,
                library_id,
                None,
            )?,
        ];

        let before_outstanding: HashSet<_> =
            actor.state.outstanding_jobs.keys().cloned().collect();
        let before_active = actor.state.active_folder_scans.clone();
        let error = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events: burst,
                correlation_id: Some(scan_id),
            })
            .await
            .expect_err(
                "an active folder must make the watcher burst retryable",
            );

        assert!(
            matches!(error, MediaError::ConcurrencyLimit(_)),
            "active folder conflicts should use the retryable concurrency error"
        );
        assert_eq!(
            actor
                .state
                .outstanding_jobs
                .keys()
                .cloned()
                .collect::<HashSet<_>>(),
            before_outstanding
        );
        assert_eq!(actor.state.active_folder_scans, before_active);

        Ok(())
    }

    #[tokio::test]
    async fn watcher_overflow_bypasses_actor_outstanding_limit() -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let folder_count = 40usize;
        for index in 0..folder_count {
            std::fs::create_dir_all(root.join(format!("movie-{index:03}")))
                .unwrap();
        }

        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor = make_actor(
            Arc::clone(&queue),
            root.clone(),
            Arc::clone(&publisher),
        );
        assert!(actor.config.max_outstanding_jobs < folder_count);
        let library_id = actor.config.library.id;

        let responses = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events: vec![make_event(
                    &root,
                    FileSystemEventKind::Overflow,
                    library_id,
                    None,
                )?],
                correlation_id: None,
            })
            .await?;

        assert_eq!(enqueued_folder_paths(&responses).len(), folder_count);
        assert_eq!(actor.state.outstanding_jobs.len(), folder_count);
        assert_eq!(actor.state.active_folder_scans.len(), folder_count);

        Ok(())
    }

    #[tokio::test]
    async fn rehydrate_resume_does_not_reseed_existing_active_folder_jobs()
    -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let seeded_folder = root.join("rehydrated-movie");
        std::fs::create_dir_all(&seeded_folder).unwrap();
        let seeded_folder_norm = normalize_path(&seeded_folder)?;

        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor = make_actor(
            Arc::clone(&queue),
            root.clone(),
            Arc::clone(&publisher),
        );

        let start_events = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(Uuid::now_v7()),
            })
            .await?;
        assert_eq!(
            enqueued_folder_paths(&start_events),
            vec![seeded_folder_norm.clone()]
        );

        let resume_events = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Resume,
                correlation_id: Some(Uuid::now_v7()),
            })
            .await?;

        assert!(
            enqueued_folder_paths(&resume_events).is_empty(),
            "rehydration/resume registration must not reseed folder jobs"
        );
        assert!(!actor.state.is_bulk_scanning);
        assert_eq!(actor.state.outstanding_jobs.len(), 1);
        assert_eq!(actor.state.active_folder_scans.len(), 1);
        assert!(actor.state.is_scan_active(&seeded_folder_norm));

        Ok(())
    }

    #[tokio::test]
    async fn maintenance_resets_paused_actor_for_subsequent_bulk_scan()
    -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let seeded_folder = root.join("cancelled-movie");
        std::fs::create_dir_all(&seeded_folder).unwrap();
        let seeded_folder_norm = normalize_path(&seeded_folder)?;

        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor =
            make_actor(Arc::clone(&queue), root, Arc::clone(&publisher));
        let bulk_correlation = Uuid::now_v7();

        let first_bulk = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(bulk_correlation),
            })
            .await?;
        assert_eq!(
            enqueued_folder_paths(&first_bulk),
            vec![seeded_folder_norm.clone()]
        );

        actor.handle_command(LibraryActorCommand::Pause).await?;
        assert!(actor.state.is_paused);
        assert!(actor.state.is_scan_active(&seeded_folder_norm));

        actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Maintenance,
                correlation_id: Some(bulk_correlation),
            })
            .await?;
        assert!(!actor.state.is_paused);
        assert!(!actor.state.is_bulk_scanning);
        assert!(actor.state.outstanding_jobs.is_empty());
        assert!(actor.state.active_folder_scans.is_empty());
        assert!(actor.state.roots.values().all(|root| root.is_watching));

        let second_bulk = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(Uuid::now_v7()),
            })
            .await?;
        assert_eq!(
            enqueued_folder_paths(&second_bulk),
            vec![seeded_folder_norm],
            "bulk admission must recover after paused scan finalization"
        );

        Ok(())
    }

    #[tokio::test]
    async fn paused_actor_consumes_terminal_jobs_before_resume() -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let seeded_folder = root.join("paused-worker-movie");
        std::fs::create_dir_all(&seeded_folder).unwrap();
        let media_file = seeded_folder.join("feature.mkv");
        std::fs::write(&media_file, b"fixture").unwrap();
        let seeded_folder_norm = normalize_path(&seeded_folder)?;

        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor =
            make_actor(Arc::clone(&queue), root, Arc::clone(&publisher));
        let library_id = actor.config.library.id;

        let _ = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(Uuid::now_v7()),
            })
            .await?;
        actor.handle_command(LibraryActorCommand::Pause).await?;
        actor
            .handle_command(LibraryActorCommand::JobCompleted {
                job_id: JobId::new(),
                dedupe_key: DedupeKey::FolderScan {
                    candidate: MediaCandidate::new(
                        library_id,
                        seeded_folder_norm.clone(),
                    ),
                },
            })
            .await?;

        assert!(actor.state.is_paused);
        assert!(actor.state.outstanding_jobs.is_empty());
        assert!(actor.state.active_folder_scans.is_empty());

        actor.handle_command(LibraryActorCommand::Resume).await?;
        let watcher_events = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events: vec![make_event(
                    &media_file,
                    FileSystemEventKind::Modified,
                    library_id,
                    None,
                )?],
                correlation_id: None,
            })
            .await?;
        assert_eq!(
            enqueued_folder_paths(&watcher_events),
            vec![seeded_folder_norm]
        );

        Ok(())
    }

    #[tokio::test]
    async fn stale_maintenance_does_not_clobber_newer_bulk_scan() -> Result<()>
    {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let seeded_folder = root.join("newer-bulk-movie");
        std::fs::create_dir_all(&seeded_folder).unwrap();
        let seeded_folder_norm = normalize_path(&seeded_folder)?;

        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor =
            make_actor(Arc::clone(&queue), root, Arc::clone(&publisher));
        let library_id = actor.config.library.id;
        let old_bulk = Uuid::now_v7();
        let new_bulk = Uuid::now_v7();

        let _ = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(old_bulk),
            })
            .await?;
        actor
            .handle_command(LibraryActorCommand::JobCompleted {
                job_id: JobId::new(),
                dedupe_key: DedupeKey::FolderScan {
                    candidate: MediaCandidate::new(
                        library_id,
                        seeded_folder_norm.clone(),
                    ),
                },
            })
            .await?;

        let newer_events = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(new_bulk),
            })
            .await?;
        assert_eq!(
            enqueued_folder_paths(&newer_events),
            vec![seeded_folder_norm.clone()]
        );

        actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Maintenance,
                correlation_id: Some(old_bulk),
            })
            .await?;

        assert!(actor.state.is_bulk_scanning);
        assert_eq!(actor.state.current_correlation, Some(new_bulk));
        assert!(actor.state.is_scan_active(&seeded_folder_norm));
        assert_eq!(actor.state.outstanding_jobs.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn post_bulk_maintenance_start_does_not_duplicate_folder_jobs()
    -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let seeded_folder = root.join("maintenance-movie");
        std::fs::create_dir_all(&seeded_folder).unwrap();
        let media_file = seeded_folder.join("feature.mkv");
        std::fs::write(&media_file, b"fixture").unwrap();
        let seeded_folder_norm = normalize_path(&seeded_folder)?;

        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor = make_actor(
            Arc::clone(&queue),
            root.clone(),
            Arc::clone(&publisher),
        );
        let library_id = actor.config.library.id;
        let bulk_correlation = Uuid::now_v7();

        let start_events = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(bulk_correlation),
            })
            .await?;
        assert_eq!(
            enqueued_folder_paths(&start_events),
            vec![seeded_folder_norm.clone()]
        );

        actor
            .handle_command(LibraryActorCommand::JobCompleted {
                job_id: JobId::new(),
                dedupe_key: DedupeKey::FolderScan {
                    candidate: MediaCandidate::new(
                        library_id,
                        seeded_folder_norm.clone(),
                    ),
                },
            })
            .await?;
        assert!(actor.state.outstanding_jobs.is_empty());
        assert!(actor.state.active_folder_scans.is_empty());

        let first_maintenance = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Maintenance,
                correlation_id: Some(bulk_correlation),
            })
            .await?;
        let duplicate_maintenance = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Maintenance,
                correlation_id: Some(Uuid::now_v7()),
            })
            .await?;
        assert!(enqueued_folder_paths(&first_maintenance).is_empty());
        assert!(enqueued_folder_paths(&duplicate_maintenance).is_empty());
        assert!(!actor.state.is_bulk_scanning);
        assert!(actor.state.active_folder_scans.is_empty());

        let burst = vec![
            make_event(
                &media_file,
                FileSystemEventKind::Created,
                library_id,
                None,
            )?,
            make_event(
                &media_file,
                FileSystemEventKind::Modified,
                library_id,
                None,
            )?,
        ];
        let responses = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events: burst.clone(),
                correlation_id: None,
            })
            .await?;
        assert_eq!(
            enqueued_folder_paths(&responses),
            vec![seeded_folder_norm.clone()],
            "maintenance watcher burst should coalesce to one folder job"
        );
        assert_eq!(actor.state.active_folder_scans.len(), 1);

        let duplicate_error = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events: burst,
                correlation_id: None,
            })
            .await
            .expect_err("active folder should defer the whole watcher burst");
        assert!(matches!(duplicate_error, MediaError::ConcurrencyLimit(_)));
        let active_paths: HashSet<_> =
            actor.state.active_folder_scans.iter().cloned().collect();
        assert_eq!(active_paths, HashSet::from([seeded_folder_norm]));

        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_is_correlated_during_maintenance_scan() -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor = make_actor(
            Arc::clone(&queue),
            root.clone(),
            Arc::clone(&publisher),
        );
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
            None,
        )?];

        let responses = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events,
                correlation_id: None,
            })
            .await?;

        let enqueued = enqueued_correlations(&responses)
            .into_iter()
            .next()
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
        let mut actor = make_actor(
            Arc::clone(&queue),
            root.clone(),
            Arc::clone(&publisher),
        );
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
            None,
        )?];

        let responses = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events,
                correlation_id: None,
            })
            .await?;

        let enqueued = enqueued_correlations(&responses)
            .into_iter()
            .next()
            .expect("expected enqueue response");

        assert_eq!(enqueued, None);

        Ok(())
    }

    // #[tokio::test]
    // async fn burst_of_events_enqueues_single_scan() -> Result<()> {
    //     let temp = tempfile::tempdir().unwrap();
    //     let root = temp.path().to_path_buf();
    //     let queue = Arc::new(RecordingQueue::default());
    //     let mut actor = make_actor(Arc::clone(&queue), root.clone());
    //     let library_id = actor.config.library.id;

    //     let _ = actor
    //         .handle_command(LibraryActorCommand::Start {
    //             mode: StartMode::Resume,
    //             correlation_id: None,
    //         })
    //         .await?;

    //     let folder = root.join("movies");
    //     std::fs::create_dir_all(&folder).unwrap();
    //     let events = vec![
    //         make_event(
    //             &folder.join("a.mkv"),
    //             FileSystemEventKind::Created,
    //             library_id,
    //         ),
    //         make_event(
    //             &folder.join("b.mkv"),
    //             FileSystemEventKind::Modified,
    //             library_id,
    //         ),
    //         make_event(
    //             &folder.join("c.mkv"),
    //             FileSystemEventKind::Deleted,
    //             library_id,
    //         ),
    //     ];

    //     let responses = actor
    //         .handle_command(LibraryActorCommand::FsEvents {
    //             root: LibraryRootsId(0),
    //             events,
    //             correlation_id: None,
    //         })
    //         .await?;

    //     let enqueued = responses
    //         .iter()
    //         .find_map(|event| {
    //             if let LibraryActorEvent::EnqueueFolderScan {
    //                 folder_path,
    //                 reason,
    //                 ..
    //             } = event
    //             {
    //                 Some((folder_path.clone(), reason))
    //             } else {
    //                 None
    //             }
    //         })
    //         .expect("expected enqueue response");

    //     assert!(matches!(enqueued.1, ScanReason::HotChange));
    //     assert!(enqueued.0.ends_with("movies"));

    //     Ok(())
    // }

    // #[tokio::test]
    // async fn overflow_triggers_rescan() -> Result<()> {
    //     let temp = tempfile::tempdir().unwrap();
    //     let root = temp.path().to_path_buf();
    //     let queue = Arc::new(RecordingQueue::default());
    //     let mut actor = make_actor(Arc::clone(&queue), root.clone());
    //     let library_id = actor.config.library.id;

    //     let _ = actor
    //         .handle_command(LibraryActorCommand::Start {
    //             mode: StartMode::Resume,
    //             correlation_id: None,
    //         })
    //         .await?;

    //     let event = FileSystemEvent {
    //         version: 1,
    //         correlation_id: None,
    //         idempotency_key: hash_parts(&["overflow", &library_id.to_string()]),
    //         library_id,
    //         path_key: normalize_path(&root),
    //         fingerprint: None,
    //         path: root.clone(),
    //         old_path: None,
    //         kind: FileSystemEventKind::Overflow,
    //         occurred_at: Utc::now(),
    //     };

    //     let responses = actor
    //         .handle_command(LibraryActorCommand::FsEvents {
    //             root: LibraryRootsId(0),
    //             events: vec![event],
    //             correlation_id: None,
    //         })
    //         .await?;

    //     let enqueued = responses
    //         .iter()
    //         .find_map(|event| {
    //             if let LibraryActorEvent::EnqueueFolderScan {
    //                 folder_path,
    //                 reason,
    //                 ..
    //             } = event
    //             {
    //                 Some((folder_path.clone(), *reason))
    //             } else {
    //                 None
    //             }
    //         })
    //         .expect("expected enqueue response");

    //     assert!(matches!(enqueued.1, ScanReason::WatcherOverflow));
    //     assert_eq!(enqueued.0, normalize_path(&root));

    //     Ok(())
    // }
}
