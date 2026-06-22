use std::{collections::HashMap, fmt, future::Future, sync::Arc};

use tokio::sync::{Mutex, RwLock, oneshot};
use tokio_util::sync::CancellationToken;

use super::{JobEventStream, ScanEventStream};
use crate::domain::scan::actors::{
    LibraryActor, LibraryActorCommand, StartMode,
};
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
        DedupeKey, EnqueueRequest, FolderScanJob, JobKind, JobPayload,
        JobPriority, ScanReason,
    },
    lease::{DequeueRequest, LeaseRenewal, QueueSelector},
    queue::{LeaseExpiryScanner, QueueService},
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
        let counts = queue.ready_counts_grouped().await?;
        scheduler
            .record_ready_bulk(counts.into_iter().map(|count| {
                ReadyCountEntry {
                    library_id: count.library_id,
                    priority: count.priority,
                    count: count.ready,
                }
            }))
            .await;
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

        // Helper mirrors dispatcher priority mapping.
        fn priority_for_reason(reason: &ScanReason) -> JobPriority {
            match reason {
                ScanReason::HotChange | ScanReason::WatcherOverflow => {
                    JobPriority::P0
                }
                ScanReason::UserRequested | ScanReason::BulkSeed => {
                    JobPriority::P1
                }
                ScanReason::MaintenanceSweep => JobPriority::P2,
            }
        }

        self.spawn_task(RuntimeTaskKind::DomainEventRouter, async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        tracing::info!("Domain event router shutting down");
                        break;
                    }
                    evt = domain_rx.recv() => match evt {
                        Ok(ScanEvent::FolderDiscovered { context, reason }) => {
                            let job = FolderScanJob {
                                context: *context.clone(),
                                scan_reason: reason,
                                enqueue_time: chrono::Utc::now(),
                                device_id: None,
                            };
                            let payload = JobPayload::FolderScan(job);
                            let priority = priority_for_reason(&reason);
                            let request = EnqueueRequest::new(priority, payload);

                            if let Err(err) = enqueuer.enqueue(request).await {
                                tracing::warn!(target: "scan::router", error = %err, folder = %context.folder_path_norm(), "failed to enqueue FolderScan from FolderDiscovered");
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

                                    if !batch.is_empty() {
                                        enqueuer.enqueue_many(batch).await.map_err(
                                            |err| {
                                                tracing::warn!(target: "scan::mailbox", error = %err, "failed to enqueue scan batch from actor request");
                                                err
                                            },
                                        )?;
                                    }

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
                                            completed_at: chrono::Utc::now(),
                                        };
                                        if let Err(err) = events
                                            .publish_scan_event(ScanEvent::SeedCompleted(
                                                summary,
                                            ))
                                            .await
                                        {
                                            tracing::warn!(target: "scan::mailbox", error = %err, "failed to publish scan seed completion");
                                        }
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

    pub(super) async fn spawn_scheduler_observer<E>(
        &self,
        events: Arc<E>,
        scheduler: WeightedFairScheduler,
        correlations: CorrelationCache,
    ) where
        E: ScanEventBus + JobEventStream + 'static,
    {
        let mut job_rx = events.subscribe_jobs();
        let shutdown = self.shutdown_token.clone();

        self.spawn_task(RuntimeTaskKind::SchedulerObserver, async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        tracing::info!("Scheduler observer shutting down");
                        break;
                    }
                    event = job_rx.recv() => match event {
                        Ok(event) => {
                            match event.payload {
                                JobEventPayload::Enqueued { job_id, priority, .. } => {
                                    correlations.remember(job_id, event.meta.correlation_id).await;
                                    scheduler
                                        .record_enqueued(event.meta.library_id, priority)
                                        .await;
                                }
                                JobEventPayload::Merged {
                                    existing_job_id,
                                    merged_job_id,
                                    ..
                                } => {
                                    correlations
                                        .remember_if_absent(existing_job_id, event.meta.correlation_id)
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
                                "scheduler observer lagged, skipped {skipped} events"
                            );
                        }
                    }
                }
            }
        }).await;
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

                    let reservation = match scheduler.reserve().await {
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
                                    let _ = q
                                        .fail(
                                            lease_id,
                                            true,
                                            Some(
                                                "budget acquire failed".into(),
                                            ),
                                        )
                                        .await;
                                    scheduler.release(library_id).await;
                                    scheduler
                                        .record_enqueued(
                                            library_id,
                                            job_priority,
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

                            let dispatch_status = d.dispatch(&lease).await;

                            renew_task.stop().await;

                            let dedupe_key: DedupeKey =
                                lease.job.payload.dedupe_key();
                            let library_id = lease.job.payload.library_id();
                            let notify_command = match dispatch_status {
                                DispatchStatus::Success => {
                                    if let Err(err) = q.complete(lease_id).await
                                    {
                                        tracing::error!(
                                            "queue complete error: {err}"
                                        );
                                    }
                                    let correlation_id = correlation_cache
                                        .take_persisted_or_generate(
                                            job_id,
                                            lease.job.correlation_id,
                                        )
                                        .await;
                                    let event = JobEvent::from_job(
                                        Some(correlation_id),
                                        library_id,
                                        lease.job.dedupe_key.clone(),
                                        stable_path_key(&lease.job.payload),
                                        JobEventPayload::Completed {
                                            job_id,
                                            kind: job_kind,
                                            priority: job_priority,
                                        },
                                    );
                                    if let Err(err) = e.publish(event).await {
                                        tracing::error!(
                                            "publish complete event failed: {err}"
                                        );
                                    }
                                    scheduler.record_completed(library_id).await;
                                    Some(LibraryActorCommand::JobCompleted {
                                        job_id,
                                        dedupe_key: dedupe_key.clone(),
                                    })
                                }
                                DispatchStatus::Retry { error } => {
                                    if let Err(err) = q
                                        .fail(
                                            lease_id,
                                            true,
                                            Some(error.clone()),
                                        )
                                        .await
                                    {
                                        tracing::error!(
                                            "queue fail error: {err}"
                                        );
                                    }
                                    let correlation_id = correlation_cache
                                        .fetch_persisted_or_generate(
                                            job_id,
                                            lease.job.correlation_id,
                                        )
                                        .await;
                                    let event = JobEvent::from_job(
                                        Some(correlation_id),
                                        library_id,
                                        lease.job.dedupe_key.clone(),
                                        stable_path_key(&lease.job.payload),
                                        JobEventPayload::Failed {
                                            job_id,
                                            kind: job_kind,
                                            priority: job_priority,
                                            retryable: true,
                                        },
                                    );
                                    if let Err(err) = e.publish(event).await {
                                        tracing::error!(
                                            "publish retry event failed: {err}"
                                        );
                                    }
                                    scheduler.release(library_id).await;
                                    scheduler
                                        .record_enqueued(
                                            library_id,
                                            job_priority,
                                        )
                                        .await;
                                    Some(LibraryActorCommand::JobFailed {
                                        job_id,
                                        dedupe_key: dedupe_key.clone(),
                                        retryable: true,
                                        error: Some(error),
                                    })
                                }
                                DispatchStatus::DeadLetter { error } => {
                                    if let Err(err) = q
                                        .dead_letter(
                                            lease_id,
                                            Some(error.clone()),
                                        )
                                        .await
                                    {
                                        tracing::error!(
                                            "queue dead-letter error: {err}"
                                        );
                                    }
                                    let correlation_id = correlation_cache
                                        .take_persisted_or_generate(
                                            job_id,
                                            lease.job.correlation_id,
                                        )
                                        .await;
                                    let event = JobEvent::from_job(
                                        Some(correlation_id),
                                        library_id,
                                        lease.job.dedupe_key.clone(),
                                        stable_path_key(&lease.job.payload),
                                        JobEventPayload::DeadLettered {
                                            job_id,
                                            kind: job_kind,
                                            priority: job_priority,
                                        },
                                    );
                                    if let Err(err) = e.publish(event).await {
                                        tracing::error!(
                                            "publish dead-letter event failed: {err}"
                                        );
                                    }
                                    scheduler.record_completed(library_id).await;
                                    Some(LibraryActorCommand::JobFailed {
                                        job_id,
                                        dedupe_key: dedupe_key.clone(),
                                        retryable: false,
                                        error: Some(error),
                                    })
                                }
                            };

                            if let Some(command) = notify_command {
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
                                        "failed to send library actor notification: {err}"
                                    );
                                }
                            }

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
        interval: std::time::Duration,
    ) where
        Q: LeaseExpiryScanner + 'static,
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

    async fn stop(self) {
        let _ = self.cancel_tx.try_send(());
        if let Err(err) = self.handle.await {
            tracing::warn!("lease renewal task failed: {err}");
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
