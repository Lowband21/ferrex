use std::{
    any::{type_name, type_name_of_val},
    collections::HashMap,
    fmt,
    sync::Arc,
};

use tokio::sync::{Mutex, RwLock, oneshot};

use super::task_graph::{
    LibraryActorHandle, OrchestratorCommand, RuntimeTaskGraph,
};
use crate::domain::scan::actors::LibraryActorCommand;
use crate::domain::scan::orchestration::runtime::JobEventStream;
use crate::domain::scan::orchestration::{
    budget::WorkloadBudget,
    config::OrchestratorConfig,
    correlation::CorrelationCache,
    dispatcher::JobDispatcher,
    enqueuer::PipelineEnqueuer,
    events::ScanEventBus,
    job::JobKind,
    queue::{LeaseExpiryScanner, QueueService},
    scheduler::WeightedFairScheduler,
};
use crate::{
    error::{MediaError, Result},
    types::ids::LibraryId,
};

/// Orchestrator-owned command executor used by producers that should share the
/// normal library actor mailbox and enqueue/event publication path.
#[async_trait::async_trait]
pub trait LibraryCommandExecutor: Send + Sync {
    async fn execute_library_command(
        &self,
        library_id: LibraryId,
        command: LibraryActorCommand,
    ) -> Result<()>;
}

/// Supervises the lifetime of actors and queue workers inside a single
/// process. This is deliberately conservative until we firm up scheduling and
/// persistence behaviour.
pub struct OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus
        + JobEventStream
        + crate::domain::scan::orchestration::runtime::ScanEventStream
        + 'static,
    B: WorkloadBudget + 'static,
{
    config: OrchestratorConfig,
    queue: Arc<Q>,
    events: Arc<E>,
    budget: Arc<B>,
    dispatcher: Arc<dyn JobDispatcher>,
    correlations: CorrelationCache,
    scheduler: WeightedFairScheduler,
    library_actors: Arc<RwLock<HashMap<LibraryId, LibraryActorHandle>>>,
    mailbox_tx:
        Arc<Mutex<Option<tokio::sync::mpsc::Sender<OrchestratorCommand>>>>,
    task_graph: RuntimeTaskGraph,
}

impl<Q, E, B> fmt::Debug for OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus
        + JobEventStream
        + crate::domain::scan::orchestration::runtime::ScanEventStream
        + 'static,
    B: WorkloadBudget + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let queue_type = type_name::<Q>();
        let events_type = type_name::<E>();
        let budget_type = type_name::<B>();
        let dispatcher_type = type_name_of_val(self.dispatcher.as_ref());

        let library_actor_count = self
            .library_actors
            .try_read()
            .map(|guard| guard.len())
            .unwrap_or_default();
        let runtime_task_count = self.task_graph.try_task_count();
        let mailbox_ready = self
            .mailbox_tx
            .try_lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false);

        f.debug_struct("OrchestratorRuntime")
            .field("config", &self.config)
            .field("queue_type", &queue_type)
            .field("events_type", &events_type)
            .field("budget_type", &budget_type)
            .field("dispatcher_type", &dispatcher_type)
            .field("scheduler", &self.scheduler)
            .field("library_actor_count", &library_actor_count)
            .field("runtime_task_count", &runtime_task_count)
            .field("mailbox_ready", &mailbox_ready)
            .field(
                "shutdown_cancelled",
                &self.task_graph.is_shutdown_requested(),
            )
            .finish()
    }
}

impl<Q, E, B> OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus
        + JobEventStream
        + crate::domain::scan::orchestration::runtime::ScanEventStream
        + 'static,
    B: WorkloadBudget + 'static,
{
    pub fn new(
        config: OrchestratorConfig,
        queue: Arc<Q>,
        events: Arc<E>,
        budget: Arc<B>,
        dispatcher: Arc<dyn JobDispatcher>,
        correlations: CorrelationCache,
    ) -> Self {
        let scheduler =
            WeightedFairScheduler::new(&config.queue, config.priority_weights);

        Self {
            config,
            queue,
            events,
            budget,
            dispatcher,
            correlations,
            scheduler,
            library_actors: Arc::new(RwLock::new(HashMap::new())),
            mailbox_tx: Arc::new(Mutex::new(None)),
            task_graph: RuntimeTaskGraph::new(),
        }
    }

    pub fn config(&self) -> &OrchestratorConfig {
        &self.config
    }

    pub fn queue(&self) -> Arc<Q> {
        Arc::clone(&self.queue)
    }

    pub fn events(&self) -> Arc<E> {
        Arc::clone(&self.events)
    }

    pub fn budget(&self) -> Arc<B> {
        Arc::clone(&self.budget)
    }

    pub fn dispatcher(&self) -> Arc<dyn JobDispatcher> {
        Arc::clone(&self.dispatcher)
    }

    pub fn correlations(&self) -> CorrelationCache {
        self.correlations.clone()
    }

    pub fn enqueuer(&self) -> PipelineEnqueuer<Q, E> {
        PipelineEnqueuer::new(
            self.queue(),
            self.events(),
            self.correlations.clone(),
        )
    }

    pub fn scheduler(&self) -> WeightedFairScheduler {
        self.scheduler.clone()
    }

    pub async fn register_library_actor(
        &self,
        library_id: LibraryId,
        actor: LibraryActorHandle,
    ) -> Result<()> {
        let mut guard = self.library_actors.write().await;
        guard.insert(library_id, actor);
        Ok(())
    }

    pub async fn library_actor(
        &self,
        library_id: LibraryId,
    ) -> Option<LibraryActorHandle> {
        let guard = self.library_actors.read().await;
        guard.get(&library_id).cloned()
    }

    pub async fn library_ids(&self) -> Vec<LibraryId> {
        let guard = self.library_actors.read().await;
        guard.keys().cloned().collect()
    }

    pub async fn start(&self) -> Result<()> {
        self.task_graph
            .prime_scheduler_from_persistence(self.queue(), self.scheduler())
            .await?;

        self.task_graph
            .spawn_scheduler_observer(
                self.events(),
                self.scheduler(),
                self.correlations.clone(),
            )
            .await;

        self.task_graph
            .spawn_domain_event_router(self.events(), self.enqueuer())
            .await;

        self.spawn_worker_pool(
            JobKind::FolderScan,
            self.config.queue.max_parallel_scans,
        )
        .await;
        self.spawn_worker_pool(
            JobKind::SeriesResolve,
            self.config.queue.max_parallel_series_resolve,
        )
        .await;
        self.spawn_worker_pool(
            JobKind::MediaAnalyze,
            self.config.queue.max_parallel_analyses,
        )
        .await;
        self.spawn_worker_pool(
            JobKind::MetadataEnrich,
            self.config.queue.max_parallel_metadata,
        )
        .await;
        self.spawn_worker_pool(
            JobKind::EpisodeMatch,
            self.config.queue.max_parallel_metadata,
        )
        .await;
        self.spawn_worker_pool(
            JobKind::IndexUpsert,
            self.config.queue.max_parallel_index,
        )
        .await;
        self.spawn_worker_pool(
            JobKind::ImageFetch,
            self.config.queue.max_parallel_image_fetch,
        )
        .await;

        self.task_graph
            .spawn_housekeeper(
                self.queue(),
                std::time::Duration::from_millis(
                    self.config.lease.housekeeper_interval_ms,
                ),
            )
            .await;

        self.start_mailbox_runner().await?;

        Ok(())
    }
}

impl<Q, E, B> OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus
        + JobEventStream
        + crate::domain::scan::orchestration::runtime::ScanEventStream
        + 'static,
    B: WorkloadBudget + 'static,
{
    async fn spawn_worker_pool(&self, kind: JobKind, parallelism: usize) {
        self.task_graph
            .spawn_worker_pool(
                kind,
                parallelism,
                self.config.lease,
                self.queue(),
                self.events(),
                self.budget(),
                self.dispatcher(),
                Arc::clone(&self.mailbox_tx),
                self.correlations.clone(),
                self.scheduler(),
            )
            .await;
    }

    pub async fn start_mailbox_runner(&self) -> Result<()> {
        self.task_graph
            .start_mailbox_runner(
                Arc::clone(&self.mailbox_tx),
                Arc::clone(&self.library_actors),
                self.enqueuer(),
                self.events(),
            )
            .await
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.task_graph
            .shutdown(
                Arc::clone(&self.mailbox_tx),
                Arc::clone(&self.library_actors),
            )
            .await
    }

    #[cfg(test)]
    async fn runtime_task_count(&self) -> usize {
        self.task_graph.task_count().await
    }
}

impl<Q, E, B> OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus
        + JobEventStream
        + crate::domain::scan::orchestration::runtime::ScanEventStream
        + 'static,
    B: WorkloadBudget + 'static,
{
    pub async fn submit_library_command(
        &self,
        library_id: LibraryId,
        command: LibraryActorCommand,
    ) -> Result<()> {
        let tx = {
            let guard = self.mailbox_tx.lock().await;
            guard.clone().ok_or_else(|| {
                MediaError::Internal("mailbox not started".into())
            })?
        };
        tx.send(OrchestratorCommand::Library {
            library_id,
            command,
            completion: None,
        })
        .await
        .map_err(|e| MediaError::Internal(format!("mailbox send failed: {e}")))
    }

    pub async fn submit_library_command_and_wait(
        &self,
        library_id: LibraryId,
        command: LibraryActorCommand,
    ) -> Result<()> {
        let tx = {
            let guard = self.mailbox_tx.lock().await;
            guard.clone().ok_or_else(|| {
                MediaError::Internal("mailbox not started".into())
            })?
        };
        let (completion_tx, completion_rx) = oneshot::channel();
        tx.send(OrchestratorCommand::Library {
            library_id,
            command,
            completion: Some(completion_tx),
        })
        .await
        .map_err(|e| {
            MediaError::Internal(format!("mailbox send failed: {e}"))
        })?;

        completion_rx.await.map_err(|err| {
            MediaError::Internal(format!("mailbox response dropped: {err}"))
        })?
    }
}

#[async_trait::async_trait]
impl<Q, E, B> LibraryCommandExecutor for OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus
        + JobEventStream
        + crate::domain::scan::orchestration::runtime::ScanEventStream
        + 'static,
    B: WorkloadBudget + 'static,
{
    async fn execute_library_command(
        &self,
        library_id: LibraryId,
        command: LibraryActorCommand,
    ) -> Result<()> {
        self.submit_library_command_and_wait(library_id, command)
            .await
    }
}

/// Helper for constructing a runtime with explicit dependencies.
pub struct OrchestratorRuntimeBuilder<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus
        + JobEventStream
        + crate::domain::scan::orchestration::runtime::ScanEventStream
        + 'static,
    B: WorkloadBudget + 'static,
{
    config: OrchestratorConfig,
    queue: Option<Arc<Q>>,
    events: Option<Arc<E>>,
    budget: Option<Arc<B>>,
    dispatcher: Option<Arc<dyn JobDispatcher>>,
    correlations: Option<CorrelationCache>,
}

impl<Q, E, B> fmt::Debug for OrchestratorRuntimeBuilder<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus
        + JobEventStream
        + crate::domain::scan::orchestration::runtime::ScanEventStream
        + 'static,
    B: WorkloadBudget + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("OrchestratorRuntimeBuilder");
        debug.field("config", &self.config);
        debug.field("queue_set", &self.queue.is_some());
        debug.field("events_set", &self.events.is_some());
        debug.field("budget_set", &self.budget.is_some());
        debug.field("dispatcher_set", &self.dispatcher.is_some());
        debug.field("correlations_set", &self.correlations.is_some());

        if self.queue.is_some() {
            debug.field("queue_type", &type_name::<Q>());
        }
        if self.events.is_some() {
            debug.field("events_type", &type_name::<E>());
        }
        if self.budget.is_some() {
            debug.field("budget_type", &type_name::<B>());
        }

        debug.finish()
    }
}

impl<Q, E, B> OrchestratorRuntimeBuilder<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus
        + JobEventStream
        + crate::domain::scan::orchestration::runtime::ScanEventStream
        + 'static,
    B: WorkloadBudget + 'static,
{
    pub fn new(config: OrchestratorConfig) -> Self {
        Self {
            config,
            queue: None,
            events: None,
            budget: None,
            dispatcher: None,
            correlations: None,
        }
    }

    pub fn with_queue(mut self, queue: Arc<Q>) -> Self {
        self.queue = Some(queue);
        self
    }

    pub fn with_events(mut self, events: Arc<E>) -> Self {
        self.events = Some(events);
        self
    }

    pub fn with_budget(mut self, budget: Arc<B>) -> Self {
        self.budget = Some(budget);
        self
    }

    pub fn with_dispatcher(
        mut self,
        dispatcher: Arc<dyn JobDispatcher>,
    ) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    pub fn with_correlations(mut self, correlations: CorrelationCache) -> Self {
        self.correlations = Some(correlations);
        self
    }

    pub fn build(self) -> Result<OrchestratorRuntime<Q, E, B>> {
        let queue = self.queue.ok_or_else(|| {
            MediaError::Internal("queue dependency missing".into())
        })?;
        let events = self.events.ok_or_else(|| {
            MediaError::Internal("event publisher dependency missing".into())
        })?;
        let budget = self.budget.ok_or_else(|| {
            MediaError::Internal("budget manager dependency missing".into())
        })?;
        let dispatcher = self.dispatcher.ok_or_else(|| {
            MediaError::Internal("dispatcher dependency missing".into())
        })?;
        let correlations = self.correlations.unwrap_or_default();

        Ok(OrchestratorRuntime::new(
            self.config,
            queue,
            events,
            budget,
            dispatcher,
            correlations,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::orchestration::budget::{
        InMemoryBudget, WorkloadType,
    };
    use crate::domain::scan::orchestration::context::{
        FolderScanContext, MovieFolderScanContext, MovieRootPath,
    };
    use crate::domain::scan::orchestration::dispatcher::DispatchStatus;
    use crate::domain::scan::orchestration::events::{
        EventMeta, JobEvent, JobEventPayload, JobEventPublisher,
    };
    use crate::domain::scan::orchestration::job::{
        EnqueueRequest, FolderScanJob, JobHandle, JobId, JobPayload,
        JobPriority, JobRecord, JobState, ScanReason,
    };
    use crate::domain::scan::orchestration::lease::{
        DequeueRequest, JobLease, LeaseId, LeaseRenewal, QueueSelector,
    };
    use crate::domain::scan::orchestration::persistence::PostgresQueueService;
    use crate::domain::scan::orchestration::queue::ReadyQueueCount;
    use crate::domain::scan::orchestration::runtime::InProcJobEventBus;
    use crate::types::ids::LibraryId;
    use async_trait::async_trait;
    use sqlx::PgPool;
    use std::collections::{HashMap, VecDeque};
    use std::fmt;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex as TokioMutex;
    use tokio::time;
    use uuid::Uuid;

    #[derive(Default)]
    struct DispatcherState {
        active: HashMap<LibraryId, usize>,
        max_seen: HashMap<LibraryId, usize>,
        completions: Vec<(LibraryId, JobPriority)>,
    }

    impl fmt::Debug for DispatcherState {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("DispatcherState")
                .field("active_libraries", &self.active.len())
                .field("max_seen_entries", &self.max_seen.len())
                .field("completion_count", &self.completions.len())
                .finish()
        }
    }

    struct TestDispatcher {
        delay: Duration,
        state: TokioMutex<DispatcherState>,
    }

    impl TestDispatcher {
        fn new(delay: Duration) -> Self {
            Self {
                delay,
                state: TokioMutex::new(DispatcherState::default()),
            }
        }
    }

    #[async_trait]
    impl JobDispatcher for TestDispatcher {
        async fn dispatch(&self, lease: &JobLease) -> DispatchStatus {
            record_dispatch_start(&self.state, lease).await;
            time::sleep(self.delay).await;
            record_dispatch_finish(&self.state, lease).await;

            DispatchStatus::Success
        }
    }

    struct ScriptedDispatcher {
        delay: Duration,
        statuses: TokioMutex<VecDeque<DispatchStatus>>,
        state: TokioMutex<DispatcherState>,
    }

    impl ScriptedDispatcher {
        fn new(delay: Duration, statuses: Vec<DispatchStatus>) -> Self {
            Self {
                delay,
                statuses: TokioMutex::new(VecDeque::from(statuses)),
                state: TokioMutex::new(DispatcherState::default()),
            }
        }

        async fn max_seen(&self, library_id: LibraryId) -> usize {
            self.state
                .lock()
                .await
                .max_seen
                .get(&library_id)
                .copied()
                .unwrap_or_default()
        }
    }

    #[async_trait]
    impl JobDispatcher for ScriptedDispatcher {
        async fn dispatch(&self, lease: &JobLease) -> DispatchStatus {
            record_dispatch_start(&self.state, lease).await;
            time::sleep(self.delay).await;
            record_dispatch_finish(&self.state, lease).await;

            self.statuses
                .lock()
                .await
                .pop_front()
                .unwrap_or(DispatchStatus::Success)
        }
    }

    async fn record_dispatch_start(
        state: &TokioMutex<DispatcherState>,
        lease: &JobLease,
    ) {
        let library_id = lease.job.payload.library_id();
        let mut state = state.lock().await;
        let current = {
            let counter = state.active.entry(library_id).or_insert(0);
            *counter += 1;
            *counter
        };

        let max_entry = state.max_seen.entry(library_id).or_insert(current);
        if current > *max_entry {
            *max_entry = current;
        }
    }

    async fn record_dispatch_finish(
        state: &TokioMutex<DispatcherState>,
        lease: &JobLease,
    ) {
        let library_id = lease.job.payload.library_id();
        let priority = lease.job.priority;
        let mut state = state.lock().await;
        if let Some(counter) = state.active.get_mut(&library_id) {
            *counter = counter.saturating_sub(1);
        }
        state.completions.push((library_id, priority));
    }

    #[derive(Default)]
    struct RecordingQueue {
        state: TokioMutex<RecordingQueueState>,
    }

    #[derive(Default)]
    struct RecordingQueueState {
        ready: VecDeque<JobRecord>,
        leased: HashMap<LeaseId, JobLease>,
        selectors: Vec<QueueSelector>,
        completed: usize,
        failures: usize,
        dead_letters: usize,
        renewals: usize,
        expired_scans: usize,
    }

    #[derive(Clone, Copy, Debug, Default)]
    struct RecordingQueueStats {
        completed: usize,
        failures: usize,
        dead_letters: usize,
        renewals: usize,
    }

    impl RecordingQueue {
        fn with_ready(jobs: Vec<JobRecord>) -> Self {
            Self {
                state: TokioMutex::new(RecordingQueueState {
                    ready: VecDeque::from(jobs),
                    ..RecordingQueueState::default()
                }),
            }
        }

        async fn selectors(&self) -> Vec<QueueSelector> {
            self.state.lock().await.selectors.clone()
        }

        async fn stats(&self) -> RecordingQueueStats {
            let state = self.state.lock().await;
            RecordingQueueStats {
                completed: state.completed,
                failures: state.failures,
                dead_letters: state.dead_letters,
                renewals: state.renewals,
            }
        }
    }

    #[async_trait]
    impl QueueService for RecordingQueue {
        async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle> {
            let job = JobRecord::new(request.payload.clone(), request.priority);
            let handle =
                JobHandle::accepted(job.id, &job.payload, job.priority);
            self.state.lock().await.ready.push_back(job);
            Ok(handle)
        }

        async fn dequeue(
            &self,
            request: DequeueRequest,
        ) -> Result<Option<JobLease>> {
            let mut state = self.state.lock().await;
            if let Some(selector) = request.selector {
                state.selectors.push(selector);
            }

            let position = state.ready.iter().position(|job| {
                if job.payload.kind() != request.kind {
                    return false;
                }
                match request.selector {
                    Some(selector) => {
                        job.payload.library_id() == selector.library_id
                            && job.priority == selector.priority
                    }
                    None => true,
                }
            });

            let Some(position) = position else {
                return Ok(None);
            };

            let mut job = state
                .ready
                .remove(position)
                .expect("position came from ready queue");
            job.state = JobState::Leased;
            job.lease_owner = Some(request.worker_id.clone());
            let lease =
                JobLease::new(job, request.worker_id, request.lease_ttl);
            state.leased.insert(lease.lease_id, lease.clone());
            Ok(Some(lease))
        }

        async fn renew(&self, renewal: LeaseRenewal) -> Result<JobLease> {
            let mut state = self.state.lock().await;
            let lease = {
                let lease = state
                    .leased
                    .get_mut(&renewal.lease_id)
                    .ok_or_else(|| {
                        MediaError::NotFound(format!(
                            "lease {} not found",
                            renewal.lease_id.0
                        ))
                    })?;
                lease.renewals += 1;
                lease.expires_at = chrono::Utc::now() + renewal.extend_by;
                lease.clone()
            };
            state.renewals += 1;
            Ok(lease)
        }

        async fn complete(&self, lease_id: LeaseId) -> Result<()> {
            let mut state = self.state.lock().await;
            state.leased.remove(&lease_id).ok_or_else(|| {
                MediaError::NotFound(format!("lease {} not found", lease_id.0))
            })?;
            state.completed += 1;
            Ok(())
        }

        async fn fail(
            &self,
            lease_id: LeaseId,
            retryable: bool,
            _error: Option<String>,
        ) -> Result<()> {
            let mut state = self.state.lock().await;
            let mut lease =
                state.leased.remove(&lease_id).ok_or_else(|| {
                    MediaError::NotFound(format!(
                        "lease {} not found",
                        lease_id.0
                    ))
                })?;
            state.failures += 1;
            if retryable {
                lease.job.state = JobState::Ready;
                lease.job.lease_owner = None;
                lease.job.lease_expires_at = None;
                state.ready.push_back(lease.job);
            }
            Ok(())
        }

        async fn dead_letter(
            &self,
            lease_id: LeaseId,
            _error: Option<String>,
        ) -> Result<()> {
            let mut state = self.state.lock().await;
            state.leased.remove(&lease_id).ok_or_else(|| {
                MediaError::NotFound(format!("lease {} not found", lease_id.0))
            })?;
            state.dead_letters += 1;
            Ok(())
        }

        async fn cancel_job(&self, _job_id: JobId) -> Result<()> {
            Ok(())
        }

        async fn queue_depth(&self, kind: JobKind) -> Result<usize> {
            Ok(self
                .state
                .lock()
                .await
                .ready
                .iter()
                .filter(|job| job.payload.kind() == kind)
                .count())
        }

        async fn release_dependency(
            &self,
            _library_id: LibraryId,
            _dependency_key: &crate::domain::scan::orchestration::job::DependencyKey,
        ) -> Result<u64> {
            Ok(0)
        }

        async fn ready_counts_grouped(&self) -> Result<Vec<ReadyQueueCount>> {
            let state = self.state.lock().await;
            let mut grouped: HashMap<(JobKind, LibraryId, JobPriority), usize> =
                HashMap::new();
            for job in &state.ready {
                *grouped
                    .entry((
                        job.payload.kind(),
                        job.payload.library_id(),
                        job.priority,
                    ))
                    .or_default() += 1;
            }

            Ok(grouped
                .into_iter()
                .map(|((kind, library_id, priority), ready)| ReadyQueueCount {
                    kind,
                    library_id,
                    priority,
                    ready,
                })
                .collect())
        }
    }

    #[async_trait]
    impl LeaseExpiryScanner for RecordingQueue {
        async fn scan_expired_leases(&self) -> Result<u64> {
            self.state.lock().await.expired_scans += 1;
            Ok(0)
        }
    }

    fn folder_scan_record(
        library_id: LibraryId,
        path: &str,
        priority: JobPriority,
    ) -> JobRecord {
        let context = FolderScanContext::Movie(MovieFolderScanContext {
            library_id,
            movie_root_path: MovieRootPath::try_new(path)
                .expect("valid test movie root"),
        });
        let payload = JobPayload::FolderScan(FolderScanJob {
            context,
            scan_reason: ScanReason::BulkSeed,
            enqueue_time: chrono::Utc::now(),
            device_id: None,
        });
        JobRecord::new(payload, priority)
    }

    #[tokio::test]
    async fn task_graph_tracks_startup_tasks_and_shutdown_drains_them() {
        let mut config = OrchestratorConfig::default();
        config.queue.max_parallel_scans = 0;
        config.queue.max_parallel_series_resolve = 0;
        config.queue.max_parallel_analyses = 0;
        config.queue.max_parallel_metadata = 0;
        config.queue.max_parallel_index = 0;
        config.queue.max_parallel_image_fetch = 0;

        let queue = Arc::new(RecordingQueue::default());
        let events = Arc::new(InProcJobEventBus::new(32));
        let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));
        let dispatcher =
            Arc::new(TestDispatcher::new(Duration::from_millis(0)));

        let runtime = OrchestratorRuntimeBuilder::new(config)
            .with_queue(queue)
            .with_events(events)
            .with_budget(budget)
            .with_dispatcher(dispatcher)
            .build()
            .expect("runtime build");

        runtime.start().await.expect("runtime start");

        assert_eq!(
            runtime.runtime_task_count().await,
            4,
            "scheduler observer, domain router, housekeeper, and mailbox runner are tracked"
        );

        runtime.shutdown().await.expect("runtime shutdown succeeds");
        assert_eq!(
            runtime.runtime_task_count().await,
            0,
            "shutdown drains all tracked runtime tasks"
        );
    }

    #[tokio::test]
    async fn worker_pool_uses_scheduler_selectors_and_balances_lifecycle_accounting()
     {
        let library_id = LibraryId::new();
        let mut config = OrchestratorConfig::default();
        config.queue.max_parallel_scans = 2;
        config.queue.max_parallel_series_resolve = 0;
        config.queue.max_parallel_analyses = 0;
        config.queue.max_parallel_metadata = 0;
        config.queue.max_parallel_index = 0;
        config.queue.max_parallel_image_fetch = 0;
        config.queue.default_library_cap = 1;
        config.budget.library_scan_limit = 2;
        config.lease.lease_ttl_secs = 1;
        config.lease.renew_at_fraction = 0.0;
        config.lease.renew_min_margin_ms = 950;
        config.lease.housekeeper_interval_ms = 10_000;

        let queue = Arc::new(RecordingQueue::with_ready(vec![
            folder_scan_record(library_id, "/library/movie-a", JobPriority::P1),
            folder_scan_record(library_id, "/library/movie-b", JobPriority::P1),
        ]));
        let events = Arc::new(InProcJobEventBus::new(64));
        let mut job_events = events.subscribe();
        let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));
        let dispatcher = Arc::new(ScriptedDispatcher::new(
            Duration::from_millis(120),
            vec![
                DispatchStatus::Retry {
                    error: "transient".into(),
                },
                DispatchStatus::DeadLetter {
                    error: "terminal".into(),
                },
                DispatchStatus::Success,
            ],
        ));

        let runtime = OrchestratorRuntimeBuilder::new(config)
            .with_queue(queue.clone())
            .with_events(events)
            .with_budget(budget.clone())
            .with_dispatcher(dispatcher.clone())
            .build()
            .expect("runtime build");

        runtime.start().await.expect("runtime start");

        time::timeout(Duration::from_secs(5), async {
            loop {
                let stats = queue.stats().await;
                if stats.completed == 1
                    && stats.failures == 1
                    && stats.dead_letters == 1
                {
                    break;
                }
                time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("worker lifecycle reached terminal outcomes");

        let selectors = queue.selectors().await;
        assert!(
            selectors.len() >= 3,
            "retry causes the requeued job to be selected again"
        );
        assert!(
            selectors
                .iter()
                .all(|selector| selector.library_id == library_id
                    && selector.priority == JobPriority::P1),
            "workers pass scheduler reservations through as queue selectors"
        );

        assert_eq!(
            dispatcher.max_seen(library_id).await,
            1,
            "per-library scheduler cap prevents concurrent dispatches"
        );

        let stats = queue.stats().await;
        assert!(
            stats.renewals > 0,
            "lease renewal task renews active dispatches"
        );

        let mut payloads = Vec::new();
        while let Ok(event) = job_events.try_recv() {
            payloads.push(event.payload);
        }

        assert_eq!(
            payloads
                .iter()
                .filter(|payload| matches!(
                    payload,
                    JobEventPayload::Dequeued { .. }
                ))
                .count(),
            3
        );
        assert!(payloads.iter().any(|payload| matches!(
            payload,
            JobEventPayload::LeaseRenewed { .. }
        )));
        assert_eq!(
            payloads
                .iter()
                .filter(|payload| matches!(
                    payload,
                    JobEventPayload::Failed {
                        retryable: true,
                        ..
                    }
                ))
                .count(),
            1
        );
        assert_eq!(
            payloads
                .iter()
                .filter(|payload| matches!(
                    payload,
                    JobEventPayload::DeadLettered { .. }
                ))
                .count(),
            1
        );
        assert_eq!(
            payloads
                .iter()
                .filter(|payload| matches!(
                    payload,
                    JobEventPayload::Completed { .. }
                ))
                .count(),
            1
        );

        let (in_use, _) = budget
            .utilization(WorkloadType::LibraryScan)
            .await
            .expect("budget utilization");
        assert_eq!(in_use, 0, "worker releases budget after dispatch");

        let scheduler_snapshot = runtime.scheduler().snapshot().await;
        assert_eq!(
            scheduler_snapshot
                .get(&library_id)
                .copied()
                .unwrap_or_default(),
            (0, 0),
            "scheduler inflight and ready accounting returns to zero"
        );

        runtime.shutdown().await.expect("runtime shutdown succeeds");
    }

    #[tokio::test]
    async fn scheduler_observer_skips_ready_for_merged_events() {
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

        let library_id = LibraryId::new();

        let mut config = OrchestratorConfig::default();
        config.queue.max_parallel_scans = 0;
        config.queue.max_parallel_series_resolve = 0;
        config.queue.max_parallel_analyses = 0;
        config.queue.max_parallel_metadata = 0;
        config.queue.max_parallel_index = 0;
        config.budget.library_scan_limit = 1;

        let queue = Arc::new(
            PostgresQueueService::new(pool.clone())
                .await
                .expect("queue init"),
        );
        let events = Arc::new(InProcJobEventBus::new(32));
        let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));
        let dispatcher =
            Arc::new(TestDispatcher::new(Duration::from_millis(0)));

        let runtime = OrchestratorRuntimeBuilder::new(config)
            .with_queue(queue.clone())
            .with_events(events.clone())
            .with_budget(budget)
            .with_dispatcher(dispatcher)
            .with_correlations(CorrelationCache::default())
            .build()
            .expect("runtime build");

        runtime.start().await.expect("runtime start");

        let scheduler = runtime.scheduler();
        let correlations = runtime.correlations();

        let existing_job = JobId::new();
        let initial_correlation = Uuid::now_v7();
        let idempotency_key = format!("dedupe-{}", existing_job.0);

        let enqueued_event = JobEvent {
            meta: EventMeta::new(
                Some(initial_correlation),
                library_id,
                idempotency_key.clone(),
                None,
            ),
            payload: JobEventPayload::Enqueued {
                job_id: existing_job,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        };

        events
            .publish(enqueued_event)
            .await
            .expect("publish enqueued event");

        time::timeout(Duration::from_secs(1), async {
            loop {
                let snapshot = scheduler.snapshot().await;
                if matches!(snapshot.get(&library_id), Some((_, ready)) if *ready == 1) {
                    break;
                }
                time::sleep(Duration::from_millis(10)).await;
            }
        })
            .await
            .expect("ready count reflected enqueued event");

        assert_eq!(
            correlations.fetch(&existing_job).await,
            Some(initial_correlation),
            "enqueued event should seed correlation cache",
        );

        if let Some(reservation) = scheduler.reserve().await {
            scheduler.confirm(reservation.id).await;
            scheduler.record_completed(library_id).await;
        }

        time::timeout(Duration::from_secs(1), async {
            loop {
                let snapshot = scheduler.snapshot().await;
                if matches!(snapshot.get(&library_id), Some((_, ready)) if *ready == 0) {
                    break;
                }
                time::sleep(Duration::from_millis(10)).await;
            }
        })
            .await
            .expect("ready count drained after completion");

        let merged_job = JobId::new();
        let merged_correlation = Uuid::now_v7();

        let merged_event = JobEvent {
            meta: EventMeta::new(
                Some(merged_correlation),
                library_id,
                idempotency_key,
                None,
            ),
            payload: JobEventPayload::Merged {
                existing_job_id: existing_job,
                merged_job_id: merged_job,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        };

        events
            .publish(merged_event)
            .await
            .expect("publish merged event");

        time::timeout(Duration::from_secs(1), async {
            loop {
                if correlations.fetch(&merged_job).await
                    == Some(merged_correlation)
                {
                    break;
                }
                time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("merged correlation recorded");

        let snapshot = scheduler.snapshot().await;
        assert_eq!(
            snapshot
                .get(&library_id)
                .map(|(_, ready)| *ready)
                .unwrap_or_default(),
            0,
            "merged events must not inflate ready counts",
        );

        assert_eq!(
            correlations.fetch(&existing_job).await,
            Some(initial_correlation),
            "existing correlation should remain stable after merge",
        );

        runtime.shutdown().await.expect("runtime shutdown succeeds");
    }
}
