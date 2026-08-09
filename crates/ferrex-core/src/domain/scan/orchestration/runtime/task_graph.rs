use std::{collections::HashMap, fmt, future::Future, sync::Arc};

use tokio::sync::{Mutex, RwLock, oneshot};
use tokio_util::sync::CancellationToken;

use super::{JobEventStream, ScanEventStream};
use crate::domain::scan::actors::{
    LibraryActor, LibraryActorCommand, StartMode,
};
use crate::domain::scan::orchestration::context::FolderScanContext;
use crate::domain::scan::orchestration::{
    budget::{WorkloadBudget, WorkloadType},
    config::LeaseConfig,
    correlation::CorrelationCache,
    dispatcher::{DispatchStatus, JobDispatcher},
    enqueuer::PipelineEnqueuer,
    events::{
        JobEvent, JobEventPayload, ScanEvent, ScanEventBus, ScanSeedMode,
        ScanSeedSummary, stable_path_key,
    },
    job::{
        DedupeKey, EnqueueRequest, FolderScanJob, JobId, JobKind, JobPayload,
        JobPriority, JobState, ScanReason,
    },
    lease::{DequeueRequest, JobLease, LeaseRenewal, QueueSelector},
    queue::{
        FailOutcome, LeaseExpiryScanner, QueueService, QueueTransitionOutcome,
    },
    scheduler::{ReadyCountEntry, WeightedFairScheduler},
};
use crate::{
    error::{MediaError, Result},
    types::ids::LibraryId,
};

pub type LibraryActorHandle = Arc<Mutex<Box<dyn LibraryActor>>>;

#[derive(Debug)]
pub enum OrchestratorCommand {
    Library {
        library_id: LibraryId,
        command: LibraryActorCommand,
        completion: Option<oneshot::Sender<Result<()>>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeTaskKind {
    SchedulerObserver,
    DomainEventRouter,
    Worker(JobKind),
    Housekeeper,
    MailboxRunner,
}

struct RuntimeTaskHandle {
    kind: RuntimeTaskKind,
    handle: tokio::task::JoinHandle<()>,
}

#[derive(Debug)]
enum WorkerTransition {
    Completed,
    RetryScheduled {
        error: String,
    },
    TerminalFailed {
        error: String,
    },
    DeadLettered {
        error: String,
    },
    Missing {
        operation: &'static str,
        error: String,
    },
    PersistenceError {
        operation: &'static str,
        error: String,
    },
}

impl WorkerTransition {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::TerminalFailed { .. }
                | Self::DeadLettered { .. }
        )
    }
}

fn worker_transition_from_complete(
    outcome: Result<QueueTransitionOutcome>,
) -> WorkerTransition {
    match outcome {
        Ok(QueueTransitionOutcome::Applied) => WorkerTransition::Completed,
        Ok(QueueTransitionOutcome::Missing) => WorkerTransition::Missing {
            operation: "complete",
            error: "durable completion was not applied because the lease was missing"
                .into(),
        },
        Err(err) => WorkerTransition::PersistenceError {
            operation: "complete",
            error: format!("failed to persist job completion: {err}"),
        },
    }
}

fn worker_transition_from_fail(
    outcome: Result<FailOutcome>,
    execution_error: String,
    operation: &'static str,
) -> WorkerTransition {
    match outcome {
        Ok(FailOutcome::RetryScheduled) => WorkerTransition::RetryScheduled {
            error: execution_error,
        },
        Ok(FailOutcome::Terminal {
            state: JobState::DeadLetter,
        }) => WorkerTransition::DeadLettered {
            error: execution_error,
        },
        Ok(FailOutcome::Terminal {
            state: JobState::Failed,
        }) => WorkerTransition::TerminalFailed {
            error: execution_error,
        },
        Ok(FailOutcome::Terminal {
            state: JobState::Completed,
        }) => WorkerTransition::Completed,
        Ok(FailOutcome::Terminal { state }) => {
            WorkerTransition::PersistenceError {
                operation,
                error: format!(
                    "queue reported non-terminal state {state:?} as terminal; original error: {execution_error}"
                ),
            }
        }
        Ok(FailOutcome::Missing) => WorkerTransition::Missing {
            operation,
            error: format!(
                "durable failure transition was not applied because the lease was missing; original error: {execution_error}"
            ),
        },
        Err(err) => WorkerTransition::PersistenceError {
            operation,
            error: format!(
                "failed to persist job failure: {err}; original error: {execution_error}"
            ),
        },
    }
}

fn worker_transition_from_dead_letter(
    outcome: Result<QueueTransitionOutcome>,
    execution_error: String,
) -> WorkerTransition {
    match outcome {
        Ok(QueueTransitionOutcome::Applied) => WorkerTransition::DeadLettered {
            error: execution_error,
        },
        Ok(QueueTransitionOutcome::Missing) => WorkerTransition::Missing {
            operation: "dead_letter",
            error: format!(
                "durable dead-letter transition was not applied because the lease was missing; original error: {execution_error}"
            ),
        },
        Err(err) => WorkerTransition::PersistenceError {
            operation: "dead_letter",
            error: format!(
                "failed to persist job dead-letter: {err}; original error: {execution_error}"
            ),
        },
    }
}

#[allow(clippy::too_many_arguments)]
async fn finalize_worker_transition<E>(
    transition: WorkerTransition,
    lease: &JobLease,
    reserved_library_id: LibraryId,
    scheduler: &WeightedFairScheduler,
    events: &E,
    correlations: &CorrelationCache,
    mailbox_tx: &Arc<
        Mutex<Option<tokio::sync::mpsc::Sender<OrchestratorCommand>>>,
    >,
) where
    E: ScanEventBus + ?Sized,
{
    let job_id = lease.job.id;
    let job_kind = lease.job.payload.kind();
    let job_priority = lease.job.priority;
    let library_id = lease.job.payload.library_id();
    let dedupe_key: DedupeKey = lease.job.payload.dedupe_key();
    let terminal = transition.is_terminal();

    let (payload, command) = match transition {
        WorkerTransition::Completed => (
            JobEventPayload::Completed {
                job_id,
                kind: job_kind,
                priority: job_priority,
            },
            LibraryActorCommand::JobCompleted {
                job_id,
                dedupe_key: dedupe_key.clone(),
            },
        ),
        WorkerTransition::RetryScheduled { error } => (
            JobEventPayload::Failed {
                job_id,
                kind: job_kind,
                priority: job_priority,
                retryable: true,
            },
            LibraryActorCommand::JobFailed {
                job_id,
                dedupe_key: dedupe_key.clone(),
                retryable: true,
                error: Some(error),
            },
        ),
        WorkerTransition::TerminalFailed { error } => (
            JobEventPayload::Failed {
                job_id,
                kind: job_kind,
                priority: job_priority,
                retryable: false,
            },
            LibraryActorCommand::JobFailed {
                job_id,
                dedupe_key: dedupe_key.clone(),
                retryable: false,
                error: Some(error),
            },
        ),
        WorkerTransition::DeadLettered { error } => (
            JobEventPayload::DeadLettered {
                job_id,
                kind: job_kind,
                priority: job_priority,
            },
            LibraryActorCommand::JobFailed {
                job_id,
                dedupe_key: dedupe_key.clone(),
                retryable: false,
                error: Some(error),
            },
        ),
        WorkerTransition::Missing { operation, error } => {
            tracing::warn!(
                job = %job_id.0,
                lease = %lease.lease_id.0,
                operation,
                "durable queue transition was not applied because the lease was missing"
            );
            (
                JobEventPayload::Failed {
                    job_id,
                    kind: job_kind,
                    priority: job_priority,
                    retryable: true,
                },
                LibraryActorCommand::JobFailed {
                    job_id,
                    dedupe_key: dedupe_key.clone(),
                    retryable: true,
                    error: Some(error),
                },
            )
        }
        WorkerTransition::PersistenceError { operation, error } => {
            tracing::error!(
                job = %job_id.0,
                lease = %lease.lease_id.0,
                operation,
                error = %error,
                "durable queue transition failed"
            );
            (
                JobEventPayload::Failed {
                    job_id,
                    kind: job_kind,
                    priority: job_priority,
                    retryable: true,
                },
                LibraryActorCommand::JobFailed {
                    job_id,
                    dedupe_key: dedupe_key.clone(),
                    retryable: true,
                    error: Some(error),
                },
            )
        }
    };

    let correlation_id = if terminal {
        correlations
            .take_persisted_or_generate(job_id, lease.job.correlation_id)
            .await
    } else {
        correlations
            .fetch_persisted_or_generate(job_id, lease.job.correlation_id)
            .await
    };
    let event = JobEvent::from_job(
        Some(correlation_id),
        library_id,
        lease.job.dedupe_key.clone(),
        stable_path_key(&lease.job.payload),
        payload,
    );
    if let Err(err) = events.publish(event).await {
        tracing::error!(job = %job_id.0, "publish worker transition event failed: {err}");
    }

    if terminal {
        scheduler.record_completed(reserved_library_id).await;
    } else {
        scheduler.release(reserved_library_id).await;
    }

    let sender_opt = {
        let guard = mailbox_tx.lock().await;
        guard.clone()
    };
    if let Some(sender) = sender_opt
        && let Err(err) = sender
            .send(OrchestratorCommand::Library {
                library_id,
                command,
                completion: None,
            })
            .await
    {
        tracing::warn!(
            job = %job_id.0,
            "failed to send library actor notification: {err}"
        );
    }
}

impl fmt::Debug for RuntimeTaskHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeTaskHandle")
            .field("kind", &self.kind)
            .field("finished", &self.handle.is_finished())
            .finish()
    }
}

/// Explicit supervision graph for the scan runtime.
///
/// Every long-lived task started by [`OrchestratorRuntime`](super::OrchestratorRuntime)
/// is registered here so shutdown can cancel and await it deterministically.
/// Lease renewal tasks are intentionally lease-scoped rather than long-lived:
/// a worker spawns one renewal child per dequeued lease, cancels it, and awaits
/// it before releasing budget and returning to the worker loop.
pub(super) struct RuntimeTaskGraph {
    shutdown_token: CancellationToken,
    tasks: Mutex<Vec<RuntimeTaskHandle>>,
}

fn folder_scan_request_from_discovery(
    context: &FolderScanContext,
    reason: ScanReason,
    correlation_id: Option<uuid::Uuid>,
    durable_job_id: Option<JobId>,
) -> Option<EnqueueRequest> {
    if durable_job_id.is_some() {
        return None;
    }
    let job = FolderScanJob {
        context: context.clone(),
        scan_reason: reason,
        enqueue_time: chrono::Utc::now(),
        device_id: None,
    };
    let priority = match reason {
        ScanReason::HotChange | ScanReason::WatcherOverflow => JobPriority::P0,
        ScanReason::UserRequested | ScanReason::BulkSeed => JobPriority::P1,
        ScanReason::MaintenanceSweep => JobPriority::P2,
    };
    let mut request =
        EnqueueRequest::new(priority, JobPayload::FolderScan(job));
    request.correlation_id = correlation_id;
    Some(request)
}

async fn reconcile_scheduler_ready<Q>(
    queue: &Q,
    scheduler: &WeightedFairScheduler,
) -> Result<usize>
where
    Q: QueueService + ?Sized,
{
    let counts = queue.ready_counts_grouped().await?;
    let ready_total = counts.iter().map(|count| count.ready).sum();
    scheduler
        .reconcile_ready_absolute(counts.into_iter().map(|count| {
            ReadyCountEntry {
                kind: count.kind,
                library_id: count.library_id,
                priority: count.priority,
                count: count.ready,
                leased: count.leased,
            }
        }))
        .await;
    Ok(ready_total)
}

async fn observe_scheduler_events<Q>(
    queue: Arc<Q>,
    mut job_rx: tokio::sync::broadcast::Receiver<JobEvent>,
    scheduler: WeightedFairScheduler,
    correlations: CorrelationCache,
    shutdown: CancellationToken,
) where
    Q: QueueService + ?Sized,
{
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                tracing::info!("Scheduler observer shutting down");
                break;
            }
            event = job_rx.recv() => match event {
                Ok(event) => {
                    match event.payload {
                        JobEventPayload::Enqueued {
                            job_id,
                            kind,
                            priority,
                        } => {
                            correlations
                                .remember(job_id, event.meta.correlation_id)
                                .await;
                            scheduler
                                .record_enqueued(
                                    kind,
                                    event.meta.library_id,
                                    priority,
                                )
                                .await;
                        }
                        JobEventPayload::Merged {
                            existing_job_id,
                            merged_job_id,
                            ..
                        } => {
                            correlations
                                .remember_if_absent(
                                    existing_job_id,
                                    event.meta.correlation_id,
                                )
                                .await;
                            if merged_job_id != existing_job_id {
                                correlations
                                    .remember_if_absent(
                                        merged_job_id,
                                        event.meta.correlation_id,
                                    )
                                    .await;
                            }
                        }
                        _ => {}
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        "scheduler observer lagged, skipped {skipped} events; reconciling durable ready counts"
                    );
                    match reconcile_scheduler_ready(queue.as_ref(), &scheduler)
                        .await
                    {
                        Ok(ready_total) => tracing::info!(
                            ready_total,
                            "scheduler ready counts reconciled after event lag"
                        ),
                        Err(err) => tracing::error!(
                            error = %err,
                            "scheduler ready reconciliation failed after event lag"
                        ),
                    }
                }
            }
        }
    }
}

impl fmt::Debug for RuntimeTaskGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let task_count = self.try_task_count();
        f.debug_struct("RuntimeTaskGraph")
            .field("task_count", &task_count)
            .field("shutdown_cancelled", &self.shutdown_token.is_cancelled())
            .finish()
    }
}

impl RuntimeTaskGraph {
    pub(super) fn new() -> Self {
        Self {
            shutdown_token: CancellationToken::new(),
            tasks: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn is_shutdown_requested(&self) -> bool {
        self.shutdown_token.is_cancelled()
    }

    pub(super) fn try_task_count(&self) -> usize {
        self.tasks
            .try_lock()
            .map(|tasks| tasks.len())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(super) async fn task_count(&self) -> usize {
        self.tasks.lock().await.len()
    }

    async fn spawn_task<F>(&self, kind: RuntimeTaskKind, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::spawn(future);
        let mut tasks = self.tasks.lock().await;
        tasks.push(RuntimeTaskHandle { kind, handle });
    }

    pub(super) async fn prime_scheduler_from_persistence<Q>(
        &self,
        queue: Arc<Q>,
        scheduler: WeightedFairScheduler,
    ) -> Result<()>
    where
        Q: QueueService + ?Sized,
    {
        reconcile_scheduler_ready(queue.as_ref(), &scheduler).await?;
        Ok(())
    }

    pub(super) async fn spawn_domain_event_router<Q, E>(
        &self,
        events: Arc<E>,
        enqueuer: PipelineEnqueuer<Q, E>,
    ) where
        Q: QueueService + 'static,
        E: ScanEventBus + ScanEventStream + 'static,
    {
        let mut domain_rx = events.subscribe_scan();
        let shutdown = self.shutdown_token.clone();

        self.spawn_task(RuntimeTaskKind::DomainEventRouter, async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        tracing::info!("Domain event router shutting down");
                        break;
                    }
                    evt = domain_rx.recv() => match evt {
                        Ok(ScanEvent::FolderDiscovered {
                            context,
                            reason,
                            correlation_id,
                            durable_job_id,
                        }) => {
                            let request = folder_scan_request_from_discovery(
                                &context,
                                reason,
                                correlation_id,
                                durable_job_id,
                            );

                            if let Some(request) = request
                                && let Err(err) = enqueuer.enqueue(request).await
                            {
                                tracing::warn!(target: "scan::router", error = %err, folder = %context.folder_path_norm(), "failed to enqueue legacy FolderScan from FolderDiscovered");
                            }
                        }
                        Ok(_) => { /* ignore other domain events */ }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!("domain event router lagged, skipped {skipped} events");
                        }
                    }
                }
            }
        }).await;
    }

    pub(super) async fn start_mailbox_runner<Q, E>(
        &self,
        mailbox_tx: Arc<
            Mutex<Option<tokio::sync::mpsc::Sender<OrchestratorCommand>>>,
        >,
        library_actors: Arc<RwLock<HashMap<LibraryId, LibraryActorHandle>>>,
        enqueuer: PipelineEnqueuer<Q, E>,
        events: Arc<E>,
    ) -> Result<()>
    where
        Q: QueueService + 'static,
        E: ScanEventBus + 'static,
    {
        let (tx, mut rx) =
            tokio::sync::mpsc::channel::<OrchestratorCommand>(1024);
        {
            let mut guard = mailbox_tx.lock().await;
            if guard.is_some() {
                return Ok(());
            }
            *guard = Some(tx);
        }

        let handle = OrchestratorRuntimeHandle { library_actors };
        let shutdown = self.shutdown_token.clone();
        self.spawn_task(RuntimeTaskKind::MailboxRunner, async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        tracing::info!("Mailbox runner shutting down");
                        break;
                    }
                    msg = rx.recv() => {
                        let Some(msg) = msg else { break; };
                        match msg {
                            OrchestratorCommand::Library {
                                library_id,
                                command,
                                completion,
                            } => {
                                let result: Result<()> = async {
                                    let actor_handle = {
                                        let guard = handle.library_actors.read().await;
                                        guard.get(&library_id).cloned()
                                    }
                                    .ok_or_else(|| {
                                        MediaError::Internal(format!(
                                            "library actor not registered for {library_id}"
                                        ))
                                    })?;

                                    let mut actor = actor_handle.lock().await;
                                    let actor_events = actor
                                        .handle_command(command.clone())
                                        .await
                                        .map_err(|err| {
                                            tracing::warn!(
                                                "library actor command failed: {err}"
                                            );
                                            err
                                        })?;
                                    drop(actor);

                                    // Process actor-emitted events (e.g., enqueue requests).
                                    // Batch enqueue requests for transactional queue implementations.
                                    let mut batch: Vec<EnqueueRequest> = Vec::new();
                                    for evt in actor_events {
                                        match evt {
                                            crate::domain::scan::actors::LibraryActorEvent::EnqueueFolderScan { request } => {
                                                batch.push(*request);
                                            }
                                            crate::domain::scan::actors::LibraryActorEvent::EnqueueMetadataEnrich { job, priority, correlation_id } => {
                                                let payload = JobPayload::MetadataEnrich(*job);
                                                let mut request = EnqueueRequest::new(priority, payload);
                                                request.correlation_id = correlation_id;
                                                batch.push(request);
                                            }
                                            _ => {}
                                        }
                                    }

                                    let queued_folders = batch
                                        .iter()
                                        .filter(|request| {
                                            matches!(request.payload, JobPayload::FolderScan(_))
                                        })
                                        .count();

                                    // LibraryActor admission marks folder paths
                                    // active before persistence is attempted. If
                                    // enqueue fails, release those provisional
                                    // records so the retained watcher batch (or a
                                    // retried seed command) can be admitted again.
                                    let provisional_folder_jobs: Vec<_> = batch
                                        .iter()
                                        .filter(|request| {
                                            matches!(request.payload, JobPayload::FolderScan(_))
                                        })
                                        .map(|request| request.dedupe_key())
                                        .collect();

                                    let handles = if batch.is_empty() {
                                        Vec::new()
                                    } else {
                                        match enqueuer.enqueue_many(batch).await {
                                            Ok(handles) => handles,
                                            Err(err) => {
                                                tracing::warn!(target: "scan::mailbox", error = %err, "failed to enqueue scan batch from actor request; rolling back provisional actor state");
                                                let mut actor = actor_handle.lock().await;
                                                for dedupe_key in provisional_folder_jobs {
                                                    if let Err(rollback_err) = actor
                                                        .handle_command(LibraryActorCommand::JobFailed {
                                                            job_id: JobId::new(),
                                                            dedupe_key,
                                                            retryable: true,
                                                            error: Some("durable enqueue failed".to_string()),
                                                        })
                                                        .await
                                                    {
                                                        tracing::warn!(target: "scan::mailbox", error = %rollback_err, "failed to roll back provisional library actor state");
                                                    }
                                                }
                                                return Err(err);
                                            }
                                        }
                                    };
                                    let mut enrolled_job_ids: Vec<_> = handles
                                        .iter()
                                        .map(|handle| {
                                            handle
                                                .merged_into
                                                .unwrap_or(handle.job_id)
                                        })
                                        .collect();
                                    enrolled_job_ids.sort_unstable_by_key(
                                        |job_id| job_id.0,
                                    );
                                    enrolled_job_ids.dedup();

                                    if let LibraryActorCommand::Start {
                                        mode,
                                        correlation_id,
                                    } = &command
                                    {
                                        let mode = match mode {
                                            StartMode::Bulk => ScanSeedMode::Bulk,
                                            StartMode::Maintenance => {
                                                ScanSeedMode::Maintenance
                                            }
                                            StartMode::Resume => ScanSeedMode::Resume,
                                        };
                                        let summary = ScanSeedSummary {
                                            library_id,
                                            correlation_id: *correlation_id,
                                            mode,
                                            queued_folders,
                                            enrolled_job_ids,
                                            completed_at: chrono::Utc::now(),
                                        };
                                        events
                                            .publish_scan_event(
                                                ScanEvent::SeedCompleted(summary),
                                            )
                                            .await
                                            .map_err(|err| {
                                                tracing::warn!(target: "scan::mailbox", error = %err, "failed to publish scan seed completion");
                                                err
                                            })?;
                                    }

                                    Ok(())
                                }
                                .await;

                                if let Err(err) = &result {
                                    tracing::warn!(target: "scan::mailbox", error = %err, "library actor command did not complete");
                                }

                                if let Some(completion) = completion {
                                    let _ = completion.send(result);
                                }
                            }
                        }
                    }
                }
            }
        }).await;
        Ok(())
    }

    pub(super) async fn spawn_scheduler_observer<Q, E>(
        &self,
        queue: Arc<Q>,
        events: Arc<E>,
        scheduler: WeightedFairScheduler,
        correlations: CorrelationCache,
    ) where
        Q: QueueService + 'static,
        E: ScanEventBus + JobEventStream + 'static,
    {
        let job_rx = events.subscribe_jobs();
        let shutdown = self.shutdown_token.clone();

        self.spawn_task(
            RuntimeTaskKind::SchedulerObserver,
            observe_scheduler_events(
                queue,
                job_rx,
                scheduler,
                correlations,
                shutdown,
            ),
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn spawn_worker_pool<Q, E, B>(
        &self,
        kind: JobKind,
        parallelism: usize,
        lease_cfg: LeaseConfig,
        queue: Arc<Q>,
        events: Arc<E>,
        budget: Arc<B>,
        dispatcher: Arc<dyn JobDispatcher>,
        mailbox: Arc<
            Mutex<Option<tokio::sync::mpsc::Sender<OrchestratorCommand>>>,
        >,
        correlations: CorrelationCache,
        scheduler: WeightedFairScheduler,
    ) where
        Q: QueueService + 'static,
        E: ScanEventBus + 'static,
        B: WorkloadBudget + 'static,
    {
        let worker_group = format!("{}-{}", kind, std::process::id());

        for i in 0..parallelism {
            let worker_id = format!("{}-w{}", worker_group, i);
            let q = Arc::clone(&queue);
            let e = Arc::clone(&events);
            let b = Arc::clone(&budget);
            let d = Arc::clone(&dispatcher);
            let mailbox_tx = Arc::clone(&mailbox);
            let correlation_cache = correlations.clone();
            let shutdown = self.shutdown_token.clone();
            let scheduler = scheduler.clone();
            let worker_kind = kind;

            self.spawn_task(RuntimeTaskKind::Worker(worker_kind), async move {
                loop {
                    if shutdown.is_cancelled() {
                        tracing::info!("Worker {} shutting down", worker_id);
                        break;
                    }

                    // Preflight global budget: avoid growing inflight leases when at cap.
                    if let Ok(false) =
                        b.has_budget(workload_for(worker_kind)).await
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            50,
                        ))
                        .await;
                        continue;
                    }

                    let reservation = match scheduler.reserve(worker_kind).await {
                        Some(reservation) => reservation,
                        None => {
                            tokio::time::sleep(
                                std::time::Duration::from_millis(50),
                            )
                            .await;
                            continue;
                        }
                    };

                    tracing::trace!(
                        worker = %worker_id,
                        kind = ?worker_kind,
                        library = %reservation.library_id,
                        priority = ?reservation.priority,
                        reservation = %reservation.id,
                        "scheduler reservation granted"
                    );

                    let dequeue = DequeueRequest {
                        kind: worker_kind,
                        worker_id: worker_id.clone(),
                        lease_ttl: chrono::Duration::seconds(
                            lease_cfg.lease_ttl_secs,
                        ),
                        selector: Some(QueueSelector {
                            library_id: reservation.library_id,
                            priority: reservation.priority,
                        }),
                    };

                    match q.dequeue(dequeue).await {
                        Ok(Some(lease)) => {
                            let _ = scheduler.confirm(reservation.id).await;

                            tracing::trace!(
                                worker = %worker_id,
                                kind = ?worker_kind,
                                library = %reservation.library_id,
                                priority = ?reservation.priority,
                                reservation = %reservation.id,
                                job = %lease.job.id.0,
                                "scheduler reservation confirmed"
                            );

                            let job_id = lease.job.id;
                            let job_kind = lease.job.payload.kind();
                            let job_priority = lease.job.priority;
                            let lease_id = lease.lease_id;
                            let library_id = lease.job.payload.library_id();
                            let current_expires_at = lease.expires_at;

                            let correlation_id = correlation_cache
                                .fetch_persisted_or_generate(
                                    job_id,
                                    lease.job.correlation_id,
                                )
                                .await;

                            let dequeue_event = JobEvent::from_job(
                                Some(correlation_id),
                                library_id,
                                lease.job.dedupe_key.clone(),
                                stable_path_key(&lease.job.payload),
                                JobEventPayload::Dequeued {
                                    job_id,
                                    kind: job_kind,
                                    priority: job_priority,
                                    lease_id,
                                },
                            );
                            let _ = e.publish(dequeue_event).await;

                            let token = match b
                                .acquire(workload_for(worker_kind), library_id)
                                .await
                            {
                                Ok(t) => t,
                                Err(err) => {
                                    tracing::error!(
                                        "budget acquire error: {err}"
                                    );
                                    let execution_error = format!(
                                        "budget acquire failed: {err}"
                                    );
                                    let transition = worker_transition_from_fail(
                                        q.fail_with_outcome(
                                            lease_id,
                                            true,
                                            Some(execution_error.clone()),
                                        )
                                        .await,
                                        execution_error,
                                        "fail_after_budget_acquire",
                                    );
                                    finalize_worker_transition(
                                        transition,
                                        &lease,
                                        reservation.library_id,
                                        &scheduler,
                                        e.as_ref(),
                                        &correlation_cache,
                                        &mailbox_tx,
                                    )
                                    .await;
                                    continue;
                                }
                            };

                            let renew_task = LeaseRenewalTask::spawn(
                                LeaseRenewalParams {
                                    queue: Arc::clone(&q),
                                    events: Arc::clone(&e),
                                    correlations: correlation_cache.clone(),
                                    lease_id,
                                    worker_id: worker_id.clone(),
                                    ttl: chrono::Duration::seconds(
                                        lease_cfg.lease_ttl_secs,
                                    ),
                                    renew_margin: std::time::Duration::from_millis(
                                        lease_cfg.renew_min_margin_ms,
                                    ),
                                    renew_fraction: lease_cfg.renew_at_fraction,
                                    initial_expires_at: current_expires_at,
                                },
                            );

                            let dispatch_timeout =
                                std::time::Duration::from_millis(
                                    lease_cfg.dispatch_timeout_ms.max(1),
                                );
                            let dispatch_status = match tokio::time::timeout(
                                dispatch_timeout,
                                d.dispatch(&lease),
                            )
                            .await
                            {
                                Ok(status) => status,
                                Err(_) => {
                                    let error = format!(
                                        "job dispatch timed out after {} ms",
                                        dispatch_timeout.as_millis()
                                    );
                                    tracing::error!(
                                        job = %job_id.0,
                                        kind = ?job_kind,
                                        library = %library_id,
                                        timeout_ms = dispatch_timeout.as_millis(),
                                        "{error}"
                                    );
                                    DispatchStatus::Retry { error }
                                }
                            };

                            renew_task.stop().await;

                            let transition = match dispatch_status {
                                DispatchStatus::Success => {
                                    worker_transition_from_complete(
                                        q.complete_with_outcome(lease_id).await,
                                    )
                                }
                                DispatchStatus::Retry { error } => {
                                    worker_transition_from_fail(
                                        q.fail_with_outcome(
                                            lease_id,
                                            true,
                                            Some(error.clone()),
                                        )
                                        .await,
                                        error,
                                        "fail",
                                    )
                                }
                                DispatchStatus::DeadLetter { error } => {
                                    worker_transition_from_dead_letter(
                                        q.dead_letter_with_outcome(
                                            lease_id,
                                            Some(error.clone()),
                                        )
                                        .await,
                                        error,
                                    )
                                }
                            };

                            finalize_worker_transition(
                                transition,
                                &lease,
                                reservation.library_id,
                                &scheduler,
                                e.as_ref(),
                                &correlation_cache,
                                &mailbox_tx,
                            )
                            .await;

                            let _ = b.release(token).await;
                        }
                        Ok(None) => {
                            scheduler.discard_stale(reservation.id).await;
                            tracing::trace!(
                                worker = %worker_id,
                                kind = ?worker_kind,
                                library = %reservation.library_id,
                                priority = ?reservation.priority,
                                reservation = %reservation.id,
                                "scheduler reservation discarded (ready count was stale)"
                            );
                            tokio::time::sleep(
                                std::time::Duration::from_millis(100),
                            )
                            .await;
                            continue;
                        }
                        Err(err) => {
                            scheduler.cancel(reservation.id).await;
                            tracing::trace!(
                                worker = %worker_id,
                                kind = ?worker_kind,
                                library = %reservation.library_id,
                                priority = ?reservation.priority,
                                reservation = %reservation.id,
                                error = %err,
                                "scheduler reservation cancelled (dequeue error)"
                            );
                            tracing::error!("dequeue error: {err}");
                            tokio::time::sleep(
                                std::time::Duration::from_millis(250),
                            )
                            .await;
                            continue;
                        }
                    }
                }
            }).await;
        }
    }

    pub(super) async fn spawn_housekeeper<Q>(
        &self,
        queue: Arc<Q>,
        scheduler: WeightedFairScheduler,
        interval: std::time::Duration,
    ) where
        Q: QueueService + LeaseExpiryScanner + 'static,
    {
        let shutdown = self.shutdown_token.clone();
        self.spawn_task(RuntimeTaskKind::Housekeeper, async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        tracing::info!("Housekeeper shutting down");
                        break;
                    }
                    _ = tokio::time::sleep(interval) => {
                        if let Err(err) = queue.scan_expired_leases().await {
                            tracing::warn!("housekeeper scan_expired_leases error: {err}");
                        }
                        match reconcile_scheduler_ready(
                            queue.as_ref(),
                            &scheduler,
                        )
                        .await
                        {
                            Ok(ready_total) => tracing::trace!(
                                ready_total,
                                "scheduler ready counts reconciled"
                            ),
                            Err(err) => tracing::warn!(
                                error = %err,
                                "periodic scheduler ready reconciliation failed"
                            ),
                        }
                    }
                }
            }
        }).await;
    }

    pub(super) async fn shutdown(
        &self,
        mailbox_tx: Arc<
            Mutex<Option<tokio::sync::mpsc::Sender<OrchestratorCommand>>>,
        >,
        library_actors: Arc<RwLock<HashMap<LibraryId, LibraryActorHandle>>>,
    ) -> Result<()> {
        tracing::info!("Initiating graceful shutdown of orchestrator runtime");

        self.shutdown_token.cancel();

        {
            let mut guard = mailbox_tx.lock().await;
            *guard = None;
        }

        let handles = {
            let mut guard = self.tasks.lock().await;
            std::mem::take(&mut *guard)
        };

        for task in handles {
            let RuntimeTaskHandle { kind, mut handle } = task;
            match tokio::time::timeout(
                std::time::Duration::from_secs(30),
                &mut handle,
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!(
                    task_kind = ?kind,
                    "runtime task failed: {:?}",
                    e
                ),
                Err(_) => {
                    tracing::warn!(
                        task_kind = ?kind,
                        "runtime task timed out during shutdown; aborting"
                    );
                    handle.abort();
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        handle,
                    )
                    .await;
                }
            }
        }

        let actors = {
            let guard = library_actors.read().await;
            guard.keys().cloned().collect::<Vec<_>>()
        };

        for library_id in actors {
            let actor = {
                let guard = library_actors.read().await;
                guard.get(&library_id).cloned()
            };
            if let Some(actor) = actor {
                let mut actor_guard = actor.lock().await;
                let _ = actor_guard
                    .handle_command(LibraryActorCommand::Shutdown)
                    .await;
            }
        }

        tracing::info!("Orchestrator runtime shutdown complete");
        Ok(())
    }
}

struct LeaseRenewalParams<Q, E> {
    queue: Arc<Q>,
    events: Arc<E>,
    correlations: CorrelationCache,
    lease_id: crate::domain::scan::orchestration::lease::LeaseId,
    worker_id: String,
    ttl: chrono::Duration,
    renew_margin: std::time::Duration,
    renew_fraction: f32,
    initial_expires_at: chrono::DateTime<chrono::Utc>,
}

struct LeaseRenewalTask {
    cancel_tx: tokio::sync::mpsc::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}

impl LeaseRenewalTask {
    fn spawn<Q, E>(params: LeaseRenewalParams<Q, E>) -> Self
    where
        Q: QueueService + 'static,
        E: ScanEventBus + 'static,
    {
        let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::channel::<()>(1);
        let handle = tokio::spawn(async move {
            let mut local_expires_at = params.initial_expires_at;
            loop {
                let now = chrono::Utc::now();
                let mut sleep_dur = std::time::Duration::from_millis(500);
                if local_expires_at > now {
                    let ttl_total = params
                        .ttl
                        .to_std()
                        .unwrap_or(std::time::Duration::from_secs(30));
                    let target = ttl_total.mul_f32(1.0 - params.renew_fraction);
                    let remaining = (local_expires_at - now)
                        .to_std()
                        .unwrap_or(std::time::Duration::from_millis(0));
                    sleep_dur = if remaining > target {
                        remaining - target
                    } else if remaining > params.renew_margin {
                        remaining - params.renew_margin
                    } else {
                        std::time::Duration::from_millis(0)
                    };
                }

                tokio::select! {
                    _ = tokio::time::sleep(sleep_dur) => {},
                    _ = cancel_rx.recv() => { break; }
                }

                match params
                    .queue
                    .renew(LeaseRenewal {
                        lease_id: params.lease_id,
                        worker_id: params.worker_id.clone(),
                        extend_by: params.ttl,
                    })
                    .await
                {
                    Ok(updated) => {
                        local_expires_at = updated.expires_at;
                        let correlation_id = params
                            .correlations
                            .fetch_persisted_or_generate(
                                updated.job.id,
                                updated.job.correlation_id,
                            )
                            .await;
                        let renew_event = JobEvent::from_job(
                            Some(correlation_id),
                            updated.job.payload.library_id(),
                            updated.job.dedupe_key.clone(),
                            stable_path_key(&updated.job.payload),
                            JobEventPayload::LeaseRenewed {
                                job_id: updated.job.id,
                                lease_id: params.lease_id,
                                renewals: updated.renewals,
                            },
                        );
                        let _ = params.events.publish(renew_event).await;
                    }
                    Err(MediaError::NotFound(_)) => {
                        tracing::trace!(
                            lease = ?params.lease_id,
                            "lease renew skipped (completed or released)"
                        );
                        break;
                    }
                    Err(err) => {
                        tracing::warn!("lease renew failed: {err}");
                    }
                }
            }
        });

        Self { cancel_tx, handle }
    }

    async fn stop(mut self) {
        let _ = self.cancel_tx.try_send(());
        match tokio::time::timeout(
            std::time::Duration::from_secs(1),
            &mut self.handle,
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                tracing::warn!("lease renewal task failed: {err}");
            }
            Err(_) => {
                tracing::warn!(
                    "lease renewal task did not stop within one second; aborting"
                );
                self.handle.abort();
            }
        }
    }
}

/// Lightweight handle for mailbox runner internals.
pub struct OrchestratorRuntimeHandle {
    library_actors: Arc<RwLock<HashMap<LibraryId, LibraryActorHandle>>>,
}

impl fmt::Debug for OrchestratorRuntimeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let library_actor_count = self
            .library_actors
            .try_read()
            .map(|guard| guard.len())
            .unwrap_or_default();

        f.debug_struct("OrchestratorRuntimeHandle")
            .field("library_actor_count", &library_actor_count)
            .finish()
    }
}

fn workload_for(kind: JobKind) -> WorkloadType {
    match kind {
        JobKind::FolderScan => WorkloadType::LibraryScan,
        JobKind::SeriesResolve => WorkloadType::MetadataEnrichment,
        JobKind::MediaAnalyze => WorkloadType::MediaAnalysis,
        JobKind::MetadataEnrich => WorkloadType::MetadataEnrichment,
        JobKind::EpisodeMatch => WorkloadType::MetadataEnrichment,
        JobKind::IndexUpsert => WorkloadType::Indexing,
        JobKind::ImageFetch => WorkloadType::ImageFetch,
        JobKind::TranscriptExtract => WorkloadType::TranscriptExtraction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::context::{MovieFolderScanContext, MovieRootPath};
    use crate::domain::scan::orchestration::events::{
        EventMeta, JobEventPublisher,
    };
    use crate::domain::scan::orchestration::queue::ReadyQueueCount;
    use crate::domain::scan::orchestration::runtime::InProcJobEventBus;
    use async_trait::async_trait;
    use std::time::Duration;
    use uuid::Uuid;

    struct ReadyCountQueue {
        counts: Vec<ReadyQueueCount>,
    }

    #[async_trait]
    impl QueueService for ReadyCountQueue {
        async fn enqueue(
            &self,
            _request: EnqueueRequest,
        ) -> Result<crate::domain::scan::orchestration::job::JobHandle>
        {
            panic!("enqueue is not used by scheduler reconciliation tests")
        }

        async fn dequeue(
            &self,
            _request: DequeueRequest,
        ) -> Result<Option<crate::domain::scan::orchestration::lease::JobLease>>
        {
            panic!("dequeue is not used by scheduler reconciliation tests")
        }

        async fn renew(
            &self,
            _renewal: LeaseRenewal,
        ) -> Result<crate::domain::scan::orchestration::lease::JobLease>
        {
            panic!("renew is not used by scheduler reconciliation tests")
        }

        async fn complete(
            &self,
            _lease_id: crate::domain::scan::orchestration::lease::LeaseId,
        ) -> Result<()> {
            panic!("complete is not used by scheduler reconciliation tests")
        }

        async fn fail(
            &self,
            _lease_id: crate::domain::scan::orchestration::lease::LeaseId,
            _retryable: bool,
            _error: Option<String>,
        ) -> Result<()> {
            panic!("fail is not used by scheduler reconciliation tests")
        }

        async fn dead_letter(
            &self,
            _lease_id: crate::domain::scan::orchestration::lease::LeaseId,
            _error: Option<String>,
        ) -> Result<()> {
            panic!("dead_letter is not used by reconciliation tests")
        }

        async fn cancel_job(&self, _job_id: JobId) -> Result<()> {
            panic!("cancel_job is not used by reconciliation tests")
        }

        async fn queue_depth(&self, _kind: JobKind) -> Result<usize> {
            panic!("queue_depth is not used by reconciliation tests")
        }

        async fn release_dependency(
            &self,
            _library_id: LibraryId,
            _dependency_key: &crate::domain::scan::orchestration::job::DependencyKey,
        ) -> Result<u64> {
            panic!("release_dependency is not used by reconciliation tests")
        }

        async fn ready_counts_grouped(&self) -> Result<Vec<ReadyQueueCount>> {
            Ok(self.counts.clone())
        }
    }

    #[test]
    fn folder_discovery_request_preserves_parent_correlation() {
        let library_id =
            LibraryId(Uuid::from_u128(0x61100000000000000000000000000001));
        let correlation_id =
            Uuid::from_u128(0x6110000000000000000000000000c001);
        let context = FolderScanContext::Movie(MovieFolderScanContext {
            library_id,
            movie_root_path: MovieRootPath::try_new_under_library_root(
                "/library",
                "/library/Child Movie",
            )
            .unwrap(),
        });

        let request = folder_scan_request_from_discovery(
            &context,
            ScanReason::BulkSeed,
            Some(correlation_id),
            None,
        )
        .expect("legacy discovery requires enqueue");

        assert_eq!(request.correlation_id, Some(correlation_id));
        assert_eq!(request.priority, JobPriority::P1);
        let JobPayload::FolderScan(job) = request.payload else {
            panic!("expected child FolderScan request");
        };
        assert_eq!(job.context.library_id(), context.library_id());
        assert_eq!(job.context.folder_path_norm(), context.folder_path_norm());
        assert_eq!(job.scan_reason, ScanReason::BulkSeed);

        assert!(
            folder_scan_request_from_discovery(
                &context,
                ScanReason::BulkSeed,
                Some(correlation_id),
                Some(JobId::new()),
            )
            .is_none(),
            "a discovery event with a durable child must remain observational"
        );
    }

    #[tokio::test]
    async fn event_lag_above_256_recovers_ready_counts_from_durable_queue() {
        let library_id = LibraryId::new();
        let queue = Arc::new(ReadyCountQueue {
            counts: vec![ReadyQueueCount {
                kind: JobKind::FolderScan,
                library_id,
                priority: JobPriority::P1,
                ready: 1,
                leased: 0,
            }],
        });
        let scheduler = WeightedFairScheduler::new(
            &crate::domain::scan::orchestration::config::QueueConfig::default(),
            crate::domain::scan::orchestration::config::PriorityWeights::default(),
        );
        let events = Arc::new(InProcJobEventBus::new(256));
        let receiver = events.subscribe_jobs();

        for sequence in 0..300 {
            events
                .publish(JobEvent {
                    meta: EventMeta::new(
                        None,
                        library_id,
                        format!("lag-regression:{sequence}"),
                        None,
                    ),
                    payload: JobEventPayload::ThroughputTick {
                        queue_depths: Vec::new(),
                        sampled_at: chrono::Utc::now(),
                    },
                })
                .await
                .expect("event publish succeeds");
        }

        let shutdown = CancellationToken::new();
        let observer = tokio::spawn(observe_scheduler_events(
            queue,
            receiver,
            scheduler.clone(),
            CorrelationCache::default(),
            shutdown.clone(),
        ));

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if scheduler
                    .snapshot()
                    .await
                    .get(&library_id)
                    .is_some_and(|(_, ready)| *ready == 1)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("lag reconciliation restores the durable ready count");

        shutdown.cancel();
        observer.await.expect("observer exits cleanly");
    }
}
