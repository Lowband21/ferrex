use std::any::type_name;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::domain::scan::MediaCandidate;
use crate::{
    error::Result,
    types::{ids::LibraryId, prelude::LibraryReference},
};

use super::folder::ScannerFileFilterPolicy;
use super::messages::{ActorObserver, IssuedJobRecord};
use crate::domain::scan::manifest::{
    ManifestPartitionId, ManifestPartitionScope, ManifestRootId,
    ManifestRootScope, ManifestScope,
};
use crate::domain::scan::orchestration::context::FolderScanContext;
use crate::domain::scan::orchestration::{
    correlation::CorrelationCache,
    events::JobEventPublisher,
    job::{
        DedupeKey, JobHandle, JobId, JobPriority, ManifestScanTrigger,
        MetadataEnrichJob, ScanReason, manifest_scope_key,
    },
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
    /// Explicitly end actor bulk mode after a scan run reaches a terminal state.
    ScanRunTerminal {
        correlation_id: Option<Uuid>,
    },
    /// Clear stuck state and enqueue a bounded root-level manifest recovery sweep.
    RecoverStuckScan {
        correlation_id: Option<Uuid>,
    },
}

/// Events emitted by the `LibraryActor`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LibraryActorEvent {
    /// Request orchestrator to enqueue a folder scan. Actors must not enqueue directly.
    EnqueueFolderScan {
        context: Box<FolderScanContext>,
        priority: JobPriority,
        reason: ScanReason,
        correlation_id: Option<Uuid>,
    },
    /// Request orchestrator to enqueue a metadata enrich job (e.g., series pre-seed).
    EnqueueMetadataEnrich {
        job: Box<MetadataEnrichJob>,
        priority: JobPriority,
        correlation_id: Option<Uuid>,
    },
    /// Request orchestrator to enqueue a manifest root/partition scan.
    EnqueueManifestScan {
        scope: Box<ManifestScope>,
        priority: JobPriority,
        reason: ScanReason,
        trigger: ManifestScanTrigger,
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
            if let Ok(folder_norm) = folder_norm
                && seen.insert(folder_norm.clone())
            {
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

    fn manifest_root_scope(
        &self,
        root_id: LibraryRootsId,
        root_path_norm: String,
    ) -> ManifestScope {
        ManifestScope::Root(ManifestRootScope {
            library_id: self.config.library.id,
            library_type: self.config.library.library_type,
            root_id: ManifestRootId(root_id.0),
            root_path_norm,
        })
    }

    fn manifest_partition_scope(
        &self,
        root_id: LibraryRootsId,
        root_path_norm: String,
        prefix_norm: String,
    ) -> ManifestScope {
        let mut hasher = DefaultHasher::new();
        root_id.hash(&mut hasher);
        prefix_norm.hash(&mut hasher);
        let partition_id = (hasher.finish() & u64::from(u16::MAX)) as u16;
        ManifestScope::Partition(ManifestPartitionScope {
            root: ManifestRootScope {
                library_id: self.config.library.id,
                library_type: self.config.library.library_type,
                root_id: ManifestRootId(root_id.0),
                root_path_norm,
            },
            partition_id: ManifestPartitionId(partition_id),
            prefix_norm: Some(prefix_norm),
        })
    }

    async fn enqueue_manifest_scan(
        &mut self,
        scope: ManifestScope,
        priority: JobPriority,
        reason: ScanReason,
        trigger: ManifestScanTrigger,
        correlation_id: Option<Uuid>,
    ) -> Result<Vec<LibraryActorEvent>> {
        let scope_key = manifest_scope_key(&scope);
        debug!(
            target: "scan::manifest",
            library_id = %scope.library_id(),
            scope = %scope_key,
            reason = ?reason,
            trigger = ?trigger,
            priority = ?priority,
            "requesting enqueue for manifest scan"
        );
        Ok(vec![LibraryActorEvent::EnqueueManifestScan {
            scope: Box::new(scope),
            priority,
            reason,
            trigger,
            correlation_id,
        }])
    }

    async fn seed_manifest_roots(
        &mut self,
        reason: ScanReason,
        trigger: ManifestScanTrigger,
        correlation_id: Option<Uuid>,
    ) -> Result<Vec<LibraryActorEvent>> {
        let roots: Vec<LibraryRootDescriptor> = self.config.roots().collect();
        let mut events = Vec::with_capacity(roots.len());
        for root in roots {
            let scope = self.manifest_root_scope(root.root_id, root.path_norm);
            let mut issued = self
                .enqueue_manifest_scan(
                    scope,
                    JobPriority::P1,
                    reason,
                    trigger,
                    correlation_id,
                )
                .await?;
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

        if self.state.is_bulk_scanning {
            debug!(
                target: "scan::events",
                library_id = %self.config.library.id,
                root_id = root.0,
                events = events.len(),
                "routing fs events through manifest scopes while bulk scan is active"
            );
        }

        let state_correlation = self.state.current_correlation;
        let event_hint = events.iter().find_map(|event| event.correlation_id);
        let burst_correlation =
            correlation_id.or(state_correlation).or(event_hint);

        let (overflow, changes): (Vec<_>, Vec<_>) =
            events.into_iter().partition(|event| {
                matches!(event.kind, FileSystemEventKind::Overflow)
            });

        if !overflow.is_empty() {
            let scope = self.manifest_root_scope(root, root_path_norm.clone());
            let mut issued = self
                .enqueue_manifest_scan(
                    scope,
                    JobPriority::P0,
                    ScanReason::WatcherOverflow,
                    ManifestScanTrigger::WatchOverflow,
                    burst_correlation,
                )
                .await?;
            responses.append(&mut issued);
        }

        if !changes.is_empty() {
            let total_changes = changes.len();
            let filtered: Vec<FileSystemEvent> = changes
                .into_iter()
                .filter(|ev| self.should_route_watch_event(ev))
                .collect();
            let dropped = total_changes.saturating_sub(filtered.len());
            if dropped > 0 {
                warn!(
                    target: "scan::events",
                    dropped,
                    "ignored non-media file change events"
                );
            }

            let mut scopes_by_key: HashMap<String, ManifestScope> =
                HashMap::new();
            for ev in &filtered {
                for scope in self.manifest_scopes_for_event(
                    root,
                    &root_path,
                    &root_path_norm,
                    ev,
                ) {
                    scopes_by_key
                        .entry(manifest_scope_key(&scope))
                        .or_insert(scope);
                }
            }

            for scope in scopes_by_key.into_values() {
                let mut issued = self
                    .enqueue_manifest_scan(
                        scope,
                        JobPriority::P0,
                        ScanReason::HotChange,
                        ManifestScanTrigger::WatchChange,
                        burst_correlation,
                    )
                    .await?;
                responses.append(&mut issued);
            }
        }

        Ok(responses)
    }

    fn should_route_watch_event(&self, event: &FileSystemEvent) -> bool {
        let should_route_path = |path: &Path| {
            if self.file_filters.is_ignored_path(path) {
                return false;
            }
            if path.is_dir() || self.file_filters.is_media_file_path(path) {
                return true;
            }
            matches!(
                event.kind,
                FileSystemEventKind::Deleted | FileSystemEventKind::Moved
            ) && path.extension().is_none()
        };

        should_route_path(&event.path)
            || event.old_path.as_deref().is_some_and(should_route_path)
    }

    fn manifest_scopes_for_event(
        &self,
        root_id: LibraryRootsId,
        root_path: &Path,
        root_path_norm: &str,
        event: &FileSystemEvent,
    ) -> Vec<ManifestScope> {
        let mut scopes = Vec::new();
        if let Some(scope) = self.manifest_scope_for_path(
            root_id,
            root_path,
            root_path_norm,
            &event.path,
        ) {
            scopes.push(scope);
        }

        if let Some(old_path) = &event.old_path
            && let Some(scope) = self.manifest_scope_for_path(
                root_id,
                root_path,
                root_path_norm,
                old_path,
            )
        {
            scopes.push(scope);
        }

        scopes
    }

    fn manifest_scope_for_path(
        &self,
        root_id: LibraryRootsId,
        root_path: &Path,
        root_path_norm: &str,
        path: &Path,
    ) -> Option<ManifestScope> {
        if path.as_os_str().is_empty() || !path.starts_with(root_path) {
            return None;
        }

        let rel = path.strip_prefix(root_path).ok()?;
        if rel.components().next().is_none() {
            return Some(
                self.manifest_root_scope(root_id, root_path_norm.to_string()),
            );
        }

        let target = if self.file_filters.is_media_file_path(path) {
            path.parent().unwrap_or(root_path)
        } else {
            path
        };

        if target == root_path {
            return Some(
                self.manifest_root_scope(root_id, root_path_norm.to_string()),
            );
        }

        let prefix_norm = normalize_path(target).ok()?;
        Some(self.manifest_partition_scope(
            root_id,
            root_path_norm.to_string(),
            prefix_norm,
        ))
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
                LibraryActorCommand::ScanRunTerminal { .. } => {
                    self.state.is_bulk_scanning = false;
                    self.state.current_correlation = None;
                    Ok(vec![])
                }
                LibraryActorCommand::RecoverStuckScan { correlation_id } => {
                    self.state.is_paused = false;
                    self.state.is_bulk_scanning = false;
                    self.state.current_correlation = correlation_id;
                    self.seed_manifest_roots(
                        ScanReason::MaintenanceSweep,
                        ManifestScanTrigger::Recovery,
                        correlation_id,
                    )
                    .await
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
                            // Initialize root states and seed manifest root jobs.
                            // Manifest roots cover flat-root media and zero-folder roots,
                            // while partition batches keep downstream reconciliation bounded.
                            for root in self.config.roots() {
                                self.state.roots.insert(
                                    root.root_id,
                                    LibraryRootState {
                                        last_scan_at: None,
                                        is_watching: true,
                                    },
                                );
                            }
                            self.seed_manifest_roots(
                                ScanReason::BulkSeed,
                                ManifestScanTrigger::BulkStart,
                                correlation_id,
                            )
                            .await
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
                        candidate: MediaCandidate { path_norm, .. },
                    } = &dedupe_key
                    {
                        self.state.mark_scan_inactive(path_norm);
                    }
                    Ok(vec![])
                }
                LibraryActorCommand::JobFailed { dedupe_key, .. } => {
                    let _ = self.state.release_job(&dedupe_key);
                    if let DedupeKey::FolderScan {
                        candidate: MediaCandidate { path_norm, .. },
                    } = &dedupe_key
                    {
                        self.state.mark_scan_inactive(path_norm);
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
                LibraryActorCommand::ScanRunTerminal { correlation_id } => {
                    if self.state.current_correlation == correlation_id
                        || correlation_id.is_none()
                    {
                        self.state.is_bulk_scanning = false;
                        self.state.current_correlation = None;
                    }
                    for root_state in self.state.roots.values_mut() {
                        root_state.is_watching = true;
                    }
                    Ok(vec![])
                }
                LibraryActorCommand::RecoverStuckScan { correlation_id } => {
                    self.state.is_bulk_scanning = false;
                    self.state.current_correlation = correlation_id;
                    for root in self.config.roots() {
                        self.state.roots.insert(
                            root.root_id,
                            LibraryRootState {
                                last_scan_at: None,
                                is_watching: true,
                            },
                        );
                    }
                    self.seed_manifest_roots(
                        ScanReason::MaintenanceSweep,
                        ManifestScanTrigger::Recovery,
                        correlation_id,
                    )
                    .await
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
    use std::fmt;

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

        let enqueued = events
            .iter()
            .find_map(|event| {
                if let LibraryActorEvent::EnqueueManifestScan {
                    correlation_id,
                    ..
                } = event
                {
                    Some(*correlation_id)
                } else {
                    None
                }
            })
            .expect("expected a manifest enqueue event");

        assert_eq!(enqueued, Some(correlation));
        assert!(actor.state.is_bulk_scanning);

        Ok(())
    }

    #[tokio::test]
    async fn terminal_and_recovery_commands_clear_bulk_and_reseed_manifest()
    -> Result<()> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let queue = Arc::new(RecordingQueue::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let mut actor =
            make_actor(Arc::clone(&queue), root, Arc::clone(&publisher));
        let correlation = Uuid::now_v7();

        let _ = actor
            .handle_command(LibraryActorCommand::Start {
                mode: StartMode::Bulk,
                correlation_id: Some(correlation),
            })
            .await?;
        assert!(actor.state.is_bulk_scanning);

        let terminal = actor
            .handle_command(LibraryActorCommand::ScanRunTerminal {
                correlation_id: Some(correlation),
            })
            .await?;
        assert!(terminal.is_empty());
        assert!(!actor.state.is_bulk_scanning);

        let recovery_id = Uuid::now_v7();
        let recovery = actor
            .handle_command(LibraryActorCommand::RecoverStuckScan {
                correlation_id: Some(recovery_id),
            })
            .await?;
        assert!(!actor.state.is_bulk_scanning);
        assert!(recovery.iter().any(|event| matches!(
            event,
            LibraryActorEvent::EnqueueManifestScan {
                trigger: ManifestScanTrigger::Recovery,
                correlation_id: Some(id),
                ..
            } if *id == recovery_id
        )));

        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_events_route_manifest_scopes_during_bulk_scan()
    -> Result<()> {
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

        assert!(
            responses.iter().any(|event| matches!(
                event,
                LibraryActorEvent::EnqueueManifestScan { .. }
            )),
            "fs events during bulk should enqueue a manifest recovery scope"
        );

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

        let enqueued = responses
            .iter()
            .find_map(|event| {
                if let LibraryActorEvent::EnqueueManifestScan {
                    correlation_id: observed,
                    ..
                } = event
                {
                    Some(*observed)
                } else {
                    None
                }
            })
            .expect("maintenance watcher should enqueue manifest scan");

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

        let enqueued = responses
            .iter()
            .find_map(|event| {
                if let LibraryActorEvent::EnqueueManifestScan {
                    correlation_id,
                    ..
                } = event
                {
                    Some(*correlation_id)
                } else {
                    None
                }
            })
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
