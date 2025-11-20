use std::{
    any::{type_name, type_name_of_val},
    collections::HashMap,
    fmt,
    sync::Arc,
};

use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::orchestration::{
    actors::LibraryActor,
    budget::{WorkloadBudget, WorkloadType},
    config::OrchestratorConfig,
    correlation::CorrelationCache,
    dispatcher::{DispatchStatus, JobDispatcher},
    events::{JobEvent, JobEventPayload, ScanEvent, ScanEventBus, stable_path_key},
    job::{DedupeKey, EnqueueRequest, FolderScanJob, JobKind, JobPayload, JobPriority, ScanReason},
    lease::{DequeueRequest, LeaseRenewal, QueueSelector},
    queue::{LeaseExpiryScanner, QueueService},
    scheduler::WeightedFairScheduler,
};
use crate::{
    error::{MediaError, Result},
    types::ids::LibraryID,
};

use crate::orchestration::actors::LibraryActorCommand;
use crate::orchestration::runtime::JobEventStream;

pub type LibraryActorHandle = Arc<Mutex<Box<dyn LibraryActor>>>;

/// Supervises the lifetime of actors and queue workers inside a single
/// process. This is deliberately conservative until we firm up scheduling and
/// persistence behaviour.
pub struct OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus + JobEventStream + crate::orchestration::runtime::ScanEventStream + 'static,
    B: WorkloadBudget + 'static,
{
    config: OrchestratorConfig,
    queue: Arc<Q>,
    events: Arc<E>,
    budget: Arc<B>,
    dispatcher: Arc<dyn JobDispatcher>,
    correlations: CorrelationCache,
    scheduler: WeightedFairScheduler,
    library_actors: Arc<RwLock<HashMap<LibraryID, LibraryActorHandle>>>,
    mailbox_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<OrchestratorCommand>>>>,
    // Runtime supervision
    shutdown_token: CancellationToken,
    worker_handles: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl<Q, E, B> fmt::Debug for OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus + JobEventStream + crate::orchestration::runtime::ScanEventStream + 'static,
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
        let worker_handle_count = self
            .worker_handles
            .try_lock()
            .map(|handles| handles.len())
            .unwrap_or_default();
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
            .field("worker_handle_count", &worker_handle_count)
            .field("mailbox_ready", &mailbox_ready)
            .field("shutdown_cancelled", &self.shutdown_token.is_cancelled())
            .finish()
    }
}

impl<Q, E, B> OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus + JobEventStream + crate::orchestration::runtime::ScanEventStream + 'static,
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
        let scheduler = WeightedFairScheduler::new(&config.queue, config.priority_weights);

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
            shutdown_token: CancellationToken::new(),
            worker_handles: Mutex::new(Vec::new()),
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

    pub fn scheduler(&self) -> WeightedFairScheduler {
        self.scheduler.clone()
    }

    pub async fn register_library_actor(
        &self,
        library_id: LibraryID,
        actor: LibraryActorHandle,
    ) -> Result<()> {
        let mut guard = self.library_actors.write().await;
        guard.insert(library_id, actor);
        Ok(())
    }

    pub async fn library_actor(&self, library_id: LibraryID) -> Option<LibraryActorHandle> {
        let guard = self.library_actors.read().await;
        guard.get(&library_id).cloned()
    }

    pub async fn library_ids(&self) -> Vec<LibraryID> {
        let guard = self.library_actors.read().await;
        guard.keys().cloned().collect()
    }

    pub async fn start(&self) -> Result<()> {
        self.spawn_scheduler_observer();

        // Route domain events -> orchestrator actions (e.g., enqueue folder scans)
        self.spawn_domain_event_router();

        // Spawn worker pools for each queue kind according to config limits
        self.spawn_worker_pool(JobKind::FolderScan, self.config.queue.max_parallel_scans)
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
        self.spawn_worker_pool(JobKind::IndexUpsert, self.config.queue.max_parallel_index)
            .await;
        self.spawn_worker_pool(
            JobKind::ImageFetch,
            self.config.queue.max_parallel_image_fetch,
        )
        .await;

        // Spawn housekeeping loop for lease expiry scanning (no-op for in-memory queue)
        self.spawn_housekeeper();

        // Start mailbox runner for one-shot library actor commands
        self.start_mailbox_runner().await?;

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum OrchestratorCommand {
    Library {
        library_id: LibraryID,
        command: LibraryActorCommand,
    },
}

impl<Q, E, B> OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus + JobEventStream + crate::orchestration::runtime::ScanEventStream + 'static,
    B: WorkloadBudget + 'static,
{
    fn spawn_domain_event_router(&self) {
        let mut domain_rx = self.events().subscribe_scan();
        let queue = self.queue();
        let events = self.events();
        let correlations = self.correlations.clone();
        let shutdown = self.shutdown_token.clone();

        // Helper mirrors dispatcher priority mapping
        fn priority_for_reason(reason: &ScanReason) -> JobPriority {
            match reason {
                ScanReason::HotChange | ScanReason::WatcherOverflow => JobPriority::P0,
                ScanReason::UserRequested | ScanReason::BulkSeed => JobPriority::P1,
                ScanReason::MaintenanceSweep => JobPriority::P2,
            }
        }

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        tracing::info!("Domain event router shutting down");
                        break;
                    }
                    evt = domain_rx.recv() => match evt {
                        Ok(ScanEvent::FolderDiscovered { library_id, folder_path, parent, reason }) => {
                            // Build FolderScan job from event and enqueue
                            let encoded_parent = match serde_json::to_string(&parent) {
                                Ok(s) => s,
                                Err(err) => {
                                    tracing::warn!(target: "scan::router", error = %err, folder = %folder_path, "skipping FolderDiscovered due to parent encode error");
                                    continue;
                                }
                            };

                            let job = FolderScanJob {
                                library_id,
                                folder_path_norm: folder_path.clone(),
                                parent_context: Some(encoded_parent),
                                scan_reason: reason.clone(),
                                enqueue_time: chrono::Utc::now(),
                                device_id: None,
                            };
                            let payload = JobPayload::FolderScan(job);
                            let priority = priority_for_reason(&reason);
                            let request = EnqueueRequest::new(priority, payload.clone());

                            match queue.enqueue(request).await {
                                Ok(handle) => {
                                    // Publish JobEvent::Enqueued / Merged mirroring dispatcher
                                    let path_key = stable_path_key(&payload);
                                    let event_payload = if handle.accepted {
                                        JobEventPayload::Enqueued { job_id: handle.job_id, kind: handle.kind, priority: handle.priority }
                                    } else if let Some(existing_job_id) = handle.merged_into {
                                        JobEventPayload::Merged { existing_job_id, merged_job_id: handle.job_id, kind: handle.kind, priority: handle.priority }
                                    } else {
                                        JobEventPayload::Enqueued { job_id: handle.job_id, kind: handle.kind, priority: handle.priority }
                                    };

                                    // No correlation hint for domain events currently
                                    let event = JobEvent::from_handle(&handle, None, event_payload, path_key);
                                    correlations.remember_if_absent(handle.job_id, event.meta.correlation_id).await;
                                    if let Err(err) = events.publish(event).await {
                                        tracing::warn!(target: "scan::router", error = %err, "failed to publish job enqueue event for FolderDiscovered");
                                    }
                                }
                                Err(err) => {
                                    tracing::warn!(target: "scan::router", error = %err, folder = %folder_path, "failed to enqueue FolderScan from FolderDiscovered");
                                }
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
        });
    }
    pub async fn start_mailbox_runner(&self) -> Result<()> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<OrchestratorCommand>(1024);
        {
            let mut guard = self.mailbox_tx.lock().await;
            *guard = Some(tx);
        }

        let q = self.queue();
        let e = self.events();
        let correlations = self.correlations.clone();

        let handle = OrchestratorRuntimeHandle {
            library_actors: Arc::clone(&self.library_actors),
        };
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                match msg {
                    OrchestratorCommand::Library {
                        library_id,
                        command,
                    } => {
                        let actor_handle_opt = {
                            let guard = handle.library_actors.read().await;
                            guard.get(&library_id).cloned()
                        };
                        if let Some(actor_handle) = actor_handle_opt {
                            let mut actor = actor_handle.lock().await;
                            match actor.handle_command(command.clone()).await {
                                Ok(events) => {
                                    drop(actor);
                                    // Process actor-emitted events (e.g., enqueue requests)
                                    // Batch EnqueueFolderScan events for transactional enqueue
                                    let mut batch: Vec<(JobPayload, EnqueueRequest)> = Vec::new();
                                    for evt in events {
                                        if let crate::orchestration::actors::LibraryActorEvent::EnqueueFolderScan { folder_path, priority, reason, parent, correlation_id } = evt {
                                            let encoded_parent = match serde_json::to_string(&parent) {
                                                Ok(s) => s,
                                                Err(err) => {
                                                    tracing::warn!(target: "scan::mailbox", error = %err, folder = %folder_path, "skipping enqueue due to parent encode error");
                                                    continue;
                                                }
                                            };
                                            let job = FolderScanJob {
                                                library_id,
                                                folder_path_norm: folder_path.clone(),
                                                parent_context: Some(encoded_parent),
                                                scan_reason: reason.clone(),
                                                enqueue_time: chrono::Utc::now(),
                                                device_id: None,
                                            };
                                            let payload = JobPayload::FolderScan(job);
                                            let mut request = EnqueueRequest::new(priority, payload.clone());
                                            request.correlation_id = correlation_id;
                                            batch.push((payload, request));
                                        }
                                    }

                                    if !batch.is_empty() {
                                        // Preserve correlation_ids for event publication
                                        let payloads: Vec<JobPayload> =
                                            batch.iter().map(|(p, _)| p.clone()).collect();
                                        let corrs: Vec<Option<uuid::Uuid>> =
                                            batch.iter().map(|(_, r)| r.correlation_id).collect();
                                        let requests: Vec<EnqueueRequest> =
                                            batch.into_iter().map(|(_, r)| r).collect();
                                        match q.enqueue_many(requests).await {
                                            Ok(handles) => {
                                                for (idx, handle) in handles.into_iter().enumerate()
                                                {
                                                    let payload = &payloads[idx];
                                                    let path_key = stable_path_key(payload);
                                                    let correlation_id = corrs[idx];

                                                    let event_payload = if handle.accepted {
                                                        JobEventPayload::Enqueued {
                                                            job_id: handle.job_id,
                                                            kind: handle.kind,
                                                            priority: handle.priority,
                                                        }
                                                    } else if let Some(existing_job_id) =
                                                        handle.merged_into
                                                    {
                                                        JobEventPayload::Merged {
                                                            existing_job_id,
                                                            merged_job_id: handle.job_id,
                                                            kind: handle.kind,
                                                            priority: handle.priority,
                                                        }
                                                    } else {
                                                        JobEventPayload::Enqueued {
                                                            job_id: handle.job_id,
                                                            kind: handle.kind,
                                                            priority: handle.priority,
                                                        }
                                                    };

                                                    let event = JobEvent::from_handle(
                                                        &handle,
                                                        correlation_id,
                                                        event_payload,
                                                        path_key,
                                                    );
                                                    // Remember correlation for job. If it's None here, correlator will backfill when first seen elsewhere.
                                                    if handle.accepted {
                                                        correlations
                                                            .remember(
                                                                handle.job_id,
                                                                event.meta.correlation_id,
                                                            )
                                                            .await;
                                                    } else {
                                                        correlations
                                                            .remember_if_absent(
                                                                handle.job_id,
                                                                event.meta.correlation_id,
                                                            )
                                                            .await;
                                                    }

                                                    if let Err(err) = e.publish(event).await {
                                                        tracing::warn!(target: "scan::mailbox", error = %err, "failed to publish enqueue event");
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                tracing::warn!(target: "scan::mailbox", error = %err, "failed to enqueue folder scan batch from actor request");
                                            }
                                        }
                                    }
                                    continue;
                                }
                                Err(err) => {
                                    tracing::warn!("library actor command failed: {err}");
                                }
                            }
                        } else {
                            tracing::warn!("library actor not registered for {:?}", library_id);
                        }
                    }
                }
            }
        });
        Ok(())
    }

    fn spawn_scheduler_observer(&self) {
        let mut job_rx = self.events().subscribe_jobs();
        let scheduler = self.scheduler.clone();
        let correlations = self.correlations.clone();
        let shutdown = self.shutdown_token.clone();

        tokio::spawn(async move {
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
        });
    }

    async fn spawn_worker_pool(&self, kind: JobKind, parallelism: usize) {
        let worker_group = format!("{}-{}", kind, std::process::id());
        let lease_cfg = self.config.lease;
        let queue = self.queue();
        let events = self.events();
        let budget = self.budget();
        let dispatcher = self.dispatcher();
        let mailbox = Arc::clone(&self.mailbox_tx);
        let correlations = self.correlations.clone();
        let scheduler = self.scheduler.clone();

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

            let handle = tokio::spawn(async move {
                loop {
                    // Check for shutdown signal
                    if shutdown.is_cancelled() {
                        tracing::info!("Worker {} shutting down", worker_id);
                        break;
                    }

                    // Preflight global budget: avoid growing inflight leases when at cap
                    if let Ok(false) = b.has_budget(workload_for(worker_kind)).await {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }

                    let reservation = match scheduler.reserve().await {
                        Some(reservation) => reservation,
                        None => {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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
                        lease_ttl: chrono::Duration::seconds(lease_cfg.lease_ttl_secs),
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

                            // Capture static info we need after renew loop
                            let job_id = lease.job.id;
                            let job_kind = lease.job.payload.kind();
                            let job_priority = lease.job.priority;
                            let lease_id = lease.lease_id;
                            let library_id = lease.job.payload.library_id();
                            let current_expires_at = lease.expires_at;

                            let correlation_id = correlation_cache.fetch_or_generate(job_id).await;

                            // Publish dequeue event
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

                            // Acquire budget token for the specific library & workload
                            let token = match b.acquire(workload_for(worker_kind), library_id).await
                            {
                                Ok(t) => t,
                                Err(err) => {
                                    tracing::error!("budget acquire error: {err}");
                                    // Return lease as retryable failure to avoid starvation
                                    let _ = q
                                        .fail(lease_id, true, Some("budget acquire failed".into()))
                                        .await;
                                    scheduler.release(library_id).await;
                                    scheduler.record_enqueued(library_id, job_priority).await;
                                    continue;
                                }
                            };

                            // Start renewal loop
                            let renewer_q = Arc::clone(&q);
                            let renewer_e = Arc::clone(&e);
                            let worker_id_clone = worker_id.clone();
                            let ttl = chrono::Duration::seconds(lease_cfg.lease_ttl_secs);
                            let renew_margin =
                                std::time::Duration::from_millis(lease_cfg.renew_min_margin_ms);
                            let renew_fraction = lease_cfg.renew_at_fraction;

                            let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::channel::<()>(1);

                            let mut local_expires_at = current_expires_at;
                            let renew_correlations = correlation_cache.clone();
                            let renew_handle = tokio::spawn(async move {
                                loop {
                                    // Compute next sleep based on current expiry (best-effort using local expiry)
                                    let now = chrono::Utc::now();
                                    let mut sleep_dur = std::time::Duration::from_millis(500);
                                    if local_expires_at > now {
                                        let ttl_total = ttl
                                            .to_std()
                                            .unwrap_or(std::time::Duration::from_secs(30));
                                        let target = ttl_total.mul_f32(1.0 - renew_fraction);
                                        let remaining = (local_expires_at - now)
                                            .to_std()
                                            .unwrap_or(std::time::Duration::from_millis(0));
                                        sleep_dur = if remaining > target {
                                            remaining - target
                                        } else if remaining > renew_margin {
                                            remaining - renew_margin
                                        } else {
                                            std::time::Duration::from_millis(0)
                                        };
                                    }

                                    tokio::select! {
                                        _ = tokio::time::sleep(sleep_dur) => {},
                                        _ = cancel_rx.recv() => { break; }
                                    }

                                    // Attempt renew
                                    match renewer_q
                                        .renew(LeaseRenewal {
                                            lease_id,
                                            worker_id: worker_id_clone.clone(),
                                            extend_by: ttl,
                                        })
                                        .await
                                    {
                                        Ok(updated) => {
                                            local_expires_at = updated.expires_at;
                                            let correlation_id = renew_correlations
                                                .fetch_or_generate(updated.job.id)
                                                .await;
                                            let renew_event = JobEvent::from_job(
                                                Some(correlation_id),
                                                updated.job.payload.library_id(),
                                                updated.job.dedupe_key.clone(),
                                                stable_path_key(&updated.job.payload),
                                                JobEventPayload::LeaseRenewed {
                                                    job_id: updated.job.id,
                                                    lease_id,
                                                    renewals: updated.renewals,
                                                },
                                            );
                                            let _ = renewer_e.publish(renew_event).await;
                                        }
                                        Err(MediaError::NotFound(_)) => {
                                            tracing::trace!(
                                                lease = ?lease_id,
                                                "lease renew skipped (completed or released)"
                                            );
                                            break;
                                        }
                                        Err(err) => {
                                            tracing::warn!("lease renew failed: {err}");
                                            // Continue; housekeeping may reclaim
                                        }
                                    }
                                }
                            });

                            let dispatch_status = d.dispatch(&lease).await;

                            // Stop renewer
                            let _ = cancel_tx.try_send(());
                            let _ = renew_handle.await;

                            let dedupe_key: DedupeKey = lease.job.payload.dedupe_key();
                            let library_id = lease.job.payload.library_id();
                            let notify_command = match dispatch_status {
                                DispatchStatus::Success => {
                                    if let Err(err) = q.complete(lease_id).await {
                                        tracing::error!("queue complete error: {err}");
                                    }
                                    let correlation_id =
                                        correlation_cache.take_or_generate(job_id).await;
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
                                        tracing::error!("publish complete event failed: {err}");
                                    }
                                    scheduler.record_completed(library_id).await;
                                    Some(LibraryActorCommand::JobCompleted {
                                        job_id,
                                        dedupe_key: dedupe_key.clone(),
                                    })
                                }
                                DispatchStatus::Retry { error } => {
                                    if let Err(err) =
                                        q.fail(lease_id, true, Some(error.clone())).await
                                    {
                                        tracing::error!("queue fail error: {err}");
                                    }
                                    let correlation_id =
                                        correlation_cache.fetch_or_generate(job_id).await;
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
                                        tracing::error!("publish retry event failed: {err}");
                                    }
                                    scheduler.release(library_id).await;
                                    scheduler.record_enqueued(library_id, job_priority).await;
                                    Some(LibraryActorCommand::JobFailed {
                                        job_id,
                                        dedupe_key: dedupe_key.clone(),
                                        retryable: true,
                                        error: Some(error),
                                    })
                                }
                                DispatchStatus::DeadLetter { error } => {
                                    if let Err(err) =
                                        q.dead_letter(lease_id, Some(error.clone())).await
                                    {
                                        tracing::error!("queue dead-letter error: {err}");
                                    }
                                    let correlation_id =
                                        correlation_cache.take_or_generate(job_id).await;
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
                                        tracing::error!("publish dead-letter event failed: {err}");
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
                                        })
                                        .await
                                {
                                    tracing::warn!(
                                        "failed to send library actor notification: {err}"
                                    );
                                }
                            }

                            // Release budget
                            let _ = b.release(token).await;
                        }
                        Ok(None) => {
                            scheduler.cancel(reservation.id).await;
                            tracing::trace!(
                                worker = %worker_id,
                                kind = ?worker_kind,
                                library = %reservation.library_id,
                                priority = ?reservation.priority,
                                reservation = %reservation.id,
                                "scheduler reservation cancelled (no job ready)"
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
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
                            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                            continue;
                        }
                    }
                }
            });

            let mut handles = self.worker_handles.lock().await;
            handles.push(handle);
        }
    }

    fn spawn_housekeeper(&self) {
        let q = self.queue();
        let interval = std::time::Duration::from_millis(self.config.lease.housekeeper_interval_ms);
        let shutdown = self.shutdown_token.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        tracing::info!("Housekeeper shutting down");
                        break;
                    }
                    _ = tokio::time::sleep(interval) => {
                        if let Err(err) = q.scan_expired_leases().await {
                            tracing::warn!("housekeeper scan_expired_leases error: {err}");
                        }
                    }
                }
            }
        });

        // Note: We can't store the handle here without cloning self
        // For now, the housekeeper will just run until cancellation token fires
    }

    pub async fn shutdown(&self) -> Result<()> {
        tracing::info!("Initiating graceful shutdown of orchestrator runtime");

        // Signal all workers to stop
        self.shutdown_token.cancel();

        // Close mailbox channel
        {
            let mut guard = self.mailbox_tx.lock().await;
            *guard = None;
        }

        // Wait for all worker tasks to complete
        let handles = {
            let mut guard = self.worker_handles.lock().await;
            std::mem::take(&mut *guard)
        };

        for handle in handles {
            match tokio::time::timeout(std::time::Duration::from_secs(30), handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!("Worker task failed: {:?}", e),
                Err(_) => tracing::warn!("Worker task timed out during shutdown"),
            }
        }

        // Shutdown library actors
        let actors = self.library_ids().await;

        for library_id in actors {
            if let Some(actor) = self.library_actor(library_id).await {
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

impl<Q, E, B> OrchestratorRuntime<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus + JobEventStream + crate::orchestration::runtime::ScanEventStream + 'static,
    B: WorkloadBudget + 'static,
{
    pub async fn submit_library_command(
        &self,
        library_id: LibraryID,
        command: LibraryActorCommand,
    ) -> Result<()> {
        let tx = {
            let guard = self.mailbox_tx.lock().await;
            guard
                .clone()
                .ok_or_else(|| MediaError::Internal("mailbox not started".into()))?
        };
        tx.send(OrchestratorCommand::Library {
            library_id,
            command,
        })
        .await
        .map_err(|e| MediaError::Internal(format!("mailbox send failed: {e}")))
    }
}

/// Lightweight handle for mailbox runner internals.
pub struct OrchestratorRuntimeHandle {
    library_actors: Arc<RwLock<HashMap<LibraryID, LibraryActorHandle>>>,
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

/// Helper for constructing a runtime with explicit dependencies.
pub struct OrchestratorRuntimeBuilder<Q, E, B>
where
    Q: QueueService + LeaseExpiryScanner + 'static,
    E: ScanEventBus + JobEventStream + crate::orchestration::runtime::ScanEventStream + 'static,
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
    E: ScanEventBus + JobEventStream + crate::orchestration::runtime::ScanEventStream + 'static,
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
    E: ScanEventBus + JobEventStream + crate::orchestration::runtime::ScanEventStream + 'static,
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

    pub fn with_dispatcher(mut self, dispatcher: Arc<dyn JobDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    pub fn with_correlations(mut self, correlations: CorrelationCache) -> Self {
        self.correlations = Some(correlations);
        self
    }

    pub fn build(self) -> Result<OrchestratorRuntime<Q, E, B>> {
        let queue = self
            .queue
            .ok_or_else(|| MediaError::Internal("queue dependency missing".into()))?;
        let events = self
            .events
            .ok_or_else(|| MediaError::Internal("event publisher dependency missing".into()))?;
        let budget = self
            .budget
            .ok_or_else(|| MediaError::Internal("budget manager dependency missing".into()))?;
        let dispatcher = self
            .dispatcher
            .ok_or_else(|| MediaError::Internal("dispatcher dependency missing".into()))?;
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

fn workload_for(kind: JobKind) -> WorkloadType {
    match kind {
        JobKind::FolderScan => WorkloadType::LibraryScan,
        JobKind::MediaAnalyze => WorkloadType::MediaAnalysis,
        JobKind::MetadataEnrich => WorkloadType::MetadataEnrichment,
        JobKind::IndexUpsert => WorkloadType::Indexing,
        JobKind::ImageFetch => WorkloadType::ImageFetch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::budget::InMemoryBudget;
    use crate::orchestration::config::{LibraryQueuePolicy, PriorityWeights};
    use crate::orchestration::dispatcher::DispatchStatus;
    use crate::orchestration::events::{
        EventMeta, JobEvent, JobEventPayload, JobEventPublisher, stable_path_key,
    };
    use crate::orchestration::job::{
        EnqueueRequest, FolderScanJob, JobId, JobPayload, JobPriority, ScanReason,
    };
    use crate::orchestration::lease::JobLease;
    use crate::orchestration::persistence::PostgresQueueService;
    use crate::orchestration::runtime::InProcJobEventBus;
    use crate::types::ids::LibraryID;
    use async_trait::async_trait;
    use chrono::Utc;
    use sqlx::PgPool;
    use std::collections::HashMap;
    use std::fmt;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex as TokioMutex;
    use tokio::time;

    #[derive(Default)]
    struct DispatcherState {
        active: HashMap<LibraryID, usize>,
        max_seen: HashMap<LibraryID, usize>,
        completions: Vec<(LibraryID, JobPriority)>,
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

    async fn ensure_library(pool: &PgPool, id: LibraryID, name: &str) {
        let paths = vec!["/tmp".to_string()];
        sqlx::query!(
            "INSERT INTO libraries (id, name, library_type, paths) VALUES ($1, $2, $3, $4)",
            id.as_uuid(),
            name,
            "movies",
            &paths
        )
        .execute(pool)
        .await
        .expect("insert library fixture");
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

        async fn wait_for_completions(&self, expected: usize) {
            loop {
                {
                    let state = self.state.lock().await;
                    if state.completions.len() >= expected {
                        break;
                    }
                }
                time::sleep(Duration::from_millis(10)).await;
            }
        }

        async fn snapshot(&self) -> DispatcherSnapshot {
            let state = self.state.lock().await;
            DispatcherSnapshot {
                max_inflight: state.max_seen.clone(),
                completions: state.completions.clone(),
            }
        }
    }

    struct DispatcherSnapshot {
        max_inflight: HashMap<LibraryID, usize>,
        completions: Vec<(LibraryID, JobPriority)>,
    }

    impl fmt::Debug for DispatcherSnapshot {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("DispatcherSnapshot")
                .field("max_inflight", &self.max_inflight)
                .field("completions", &self.completions)
                .finish()
        }
    }

    #[async_trait]
    impl JobDispatcher for TestDispatcher {
        async fn dispatch(&self, lease: &JobLease) -> DispatchStatus {
            let library_id = lease.job.payload.library_id();
            let priority = lease.job.priority;

            {
                let mut state = self.state.lock().await;
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

            time::sleep(self.delay).await;

            {
                let mut state = self.state.lock().await;
                if let Some(counter) = state.active.get_mut(&library_id) {
                    *counter = counter.saturating_sub(1);
                }
                state.completions.push((library_id, priority));
            }

            DispatchStatus::Success
        }
    }

    async fn enqueue_job(
        queue: Arc<PostgresQueueService>,
        events: Arc<InProcJobEventBus>,
        request: EnqueueRequest,
    ) {
        let payload = request.payload.clone();
        let priority = request.priority;
        let handle = queue.enqueue(request).await.expect("enqueue");
        let event_payload = if handle.accepted {
            JobEventPayload::Enqueued {
                job_id: handle.job_id,
                kind: handle.kind,
                priority,
            }
        } else if let Some(existing_job_id) = handle.merged_into {
            JobEventPayload::Merged {
                existing_job_id,
                merged_job_id: handle.job_id,
                kind: handle.kind,
                priority,
            }
        } else {
            JobEventPayload::Enqueued {
                job_id: handle.job_id,
                kind: handle.kind,
                priority,
            }
        };

        let event = JobEvent::from_handle(&handle, None, event_payload, stable_path_key(&payload));
        events.publish(event).await.expect("publish enqueue event");
    }

    fn make_scan_request(
        library_id: LibraryID,
        sequence: usize,
        priority: JobPriority,
    ) -> EnqueueRequest {
        let payload = JobPayload::FolderScan(FolderScanJob {
            library_id,
            folder_path_norm: format!("/library-{library_id}-job-{sequence}"),
            parent_context: None,
            scan_reason: ScanReason::UserRequested,
            enqueue_time: Utc::now(),
            device_id: None,
        });

        EnqueueRequest {
            priority,
            payload,
            allow_merge: false,
            requested_at: Utc::now(),
            correlation_id: None,
        }
    }

    fn max_consecutive_library(events: &[(LibraryID, JobPriority)], library: LibraryID) -> usize {
        let mut max_run = 0usize;
        let mut current = 0usize;
        for (lib, _) in events {
            if *lib == library {
                current += 1;
                max_run = max_run.max(current);
            } else {
                current = 0;
            }
        }
        max_run
    }

    #[sqlx::test]
    async fn scheduler_observer_skips_ready_for_merged_events(pool: PgPool) {
        let library_id = LibraryID::new();

        let mut config = OrchestratorConfig::default();
        config.queue.max_parallel_scans = 0;
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
        let dispatcher = Arc::new(TestDispatcher::new(Duration::from_millis(0)));

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
            meta: EventMeta::new(Some(merged_correlation), library_id, idempotency_key, None),
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
                if correlations.fetch(&merged_job).await == Some(merged_correlation) {
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

    #[sqlx::test]
    async fn scheduler_respects_caps_and_prevents_starvation(pool: PgPool) {
        let lib_a = LibraryID::new();
        let lib_b = LibraryID::new();

        ensure_library(&pool, lib_a, "scheduler_caps_a").await;
        ensure_library(&pool, lib_b, "scheduler_caps_b").await;

        let mut config = OrchestratorConfig::default();
        config.queue.max_parallel_scans = 4;
        config.queue.max_parallel_analyses = 0;
        config.queue.max_parallel_metadata = 0;
        config.queue.max_parallel_index = 0;
        config.queue.library_overrides.insert(
            lib_a,
            LibraryQueuePolicy {
                max_inflight: Some(1),
                weight: Some(1),
            },
        );
        config.queue.library_overrides.insert(
            lib_b,
            LibraryQueuePolicy {
                max_inflight: Some(4),
                weight: Some(1),
            },
        );
        config.budget.library_scan_limit = 8;

        let queue = Arc::new(
            PostgresQueueService::new(pool.clone())
                .await
                .expect("queue init"),
        );
        let events = Arc::new(InProcJobEventBus::new(256));
        let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));
        let dispatcher = Arc::new(TestDispatcher::new(Duration::from_millis(5)));

        let runtime = OrchestratorRuntimeBuilder::new(config)
            .with_queue(queue.clone())
            .with_events(events.clone())
            .with_budget(budget)
            .with_dispatcher(dispatcher.clone())
            .with_correlations(CorrelationCache::default())
            .build()
            .expect("runtime build");

        runtime.start().await.expect("runtime start");
        time::sleep(Duration::from_millis(20)).await;

        let priorities = [JobPriority::P0, JobPriority::P1];
        let per_priority = 5usize;
        let total_jobs = priorities.len() * per_priority * 2;

        for idx in 0..per_priority {
            for &priority in &priorities {
                enqueue_job(
                    queue.clone(),
                    events.clone(),
                    make_scan_request(lib_a, idx * priorities.len() + priority as usize, priority),
                )
                .await;
                enqueue_job(
                    queue.clone(),
                    events.clone(),
                    make_scan_request(lib_b, idx * priorities.len() + priority as usize, priority),
                )
                .await;
            }
        }

        time::timeout(
            Duration::from_secs(5),
            dispatcher.wait_for_completions(total_jobs),
        )
        .await
        .expect("jobs completed");

        let snapshot = dispatcher.snapshot().await;
        assert_eq!(snapshot.completions.len(), total_jobs);

        let l1_max = snapshot
            .max_inflight
            .get(&lib_a)
            .copied()
            .unwrap_or_default();
        assert!(l1_max <= 1, "library A inflight cap violated: {l1_max}");

        let max_run_l2 = max_consecutive_library(&snapshot.completions, lib_b);
        assert!(
            max_run_l2 <= 4,
            "observed {max_run_l2} consecutive jobs for library B"
        );

        let count_a = snapshot
            .completions
            .iter()
            .filter(|(lib, _)| *lib == lib_a)
            .count();
        let count_b = snapshot
            .completions
            .iter()
            .filter(|(lib, _)| *lib == lib_b)
            .count();
        assert_eq!(count_a, priorities.len() * per_priority);
        assert_eq!(count_b, priorities.len() * per_priority);

        runtime.shutdown().await.expect("runtime shutdown");
    }

    #[sqlx::test]
    async fn scheduler_selector_balances_by_library_and_priority(pool: PgPool) {
        let lib_a = LibraryID::new();
        let lib_b = LibraryID::new();

        ensure_library(&pool, lib_a, "scheduler_balance_a").await;
        ensure_library(&pool, lib_b, "scheduler_balance_b").await;

        let mut config = OrchestratorConfig::default();
        config.queue.max_parallel_scans = 2;
        config.queue.max_parallel_analyses = 0;
        config.queue.max_parallel_metadata = 0;
        config.queue.max_parallel_index = 0;
        config.queue.library_overrides.insert(
            lib_a,
            LibraryQueuePolicy {
                max_inflight: Some(4),
                weight: Some(1),
            },
        );
        config.queue.library_overrides.insert(
            lib_b,
            LibraryQueuePolicy {
                max_inflight: Some(4),
                weight: Some(1),
            },
        );
        config.priority_weights = PriorityWeights {
            p0: 1,
            p1: 1,
            p2: 1,
            p3: 1,
        };
        config.budget.library_scan_limit = 4;

        let queue = Arc::new(
            PostgresQueueService::new(pool.clone())
                .await
                .expect("queue init"),
        );
        let events = Arc::new(InProcJobEventBus::new(512));
        let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));
        let dispatcher = Arc::new(TestDispatcher::new(Duration::from_millis(5)));

        let runtime = OrchestratorRuntimeBuilder::new(config)
            .with_queue(queue.clone())
            .with_events(events.clone())
            .with_budget(budget)
            .with_dispatcher(dispatcher.clone())
            .with_correlations(CorrelationCache::default())
            .build()
            .expect("runtime build");

        let mut job_events = events.subscribe();
        let collected = Arc::new(TokioMutex::new(Vec::new()));
        let total_jobs = 100usize;
        let collector = {
            let collected = Arc::clone(&collected);
            tokio::spawn(async move {
                loop {
                    match job_events.recv().await {
                        Ok(event) => {
                            if let JobEventPayload::Dequeued { priority, .. } = event.payload {
                                let mut guard = collected.lock().await;
                                guard.push((event.meta.library_id, priority));
                                if guard.len() >= total_jobs {
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    }
                }
            })
        };

        runtime.start().await.expect("runtime start");
        time::sleep(Duration::from_millis(20)).await;

        for idx in 0..50 {
            enqueue_job(
                queue.clone(),
                events.clone(),
                make_scan_request(lib_a, idx, JobPriority::P0),
            )
            .await;
        }

        time::sleep(Duration::from_millis(50)).await;

        for idx in 0..50 {
            enqueue_job(
                queue.clone(),
                events.clone(),
                make_scan_request(lib_b, idx + 100, JobPriority::P1),
            )
            .await;
        }

        time::timeout(
            Duration::from_secs(10),
            dispatcher.wait_for_completions(total_jobs),
        )
        .await
        .expect("jobs completed");

        collector.await.expect("collector finished");

        let dequeued = collected.lock().await.clone();
        assert_eq!(dequeued.len(), total_jobs);

        let first_l2 = dequeued
            .iter()
            .position(|(lib, _)| *lib == lib_b)
            .expect("library B dequeued");
        assert!(
            first_l2 < 10,
            "library B should appear early, observed index {first_l2}"
        );

        let max_run_a = max_consecutive_library(&dequeued, lib_a);
        let max_run_b = max_consecutive_library(&dequeued, lib_b);
        // With multiple workers the scheduler can legitimately hand out short bursts when
        // queues momentarily drain, but it should still cap to a small window before rotating.
        let max_allowed_run = 8;
        assert!(
            max_run_a <= max_allowed_run,
            "max consecutive for A: {max_run_a}"
        );
        assert!(
            max_run_b <= max_allowed_run,
            "max consecutive for B: {max_run_b}"
        );

        let p0_count = dequeued
            .iter()
            .filter(|(_, priority)| *priority == JobPriority::P0)
            .count();
        let p1_count = dequeued
            .iter()
            .filter(|(_, priority)| *priority == JobPriority::P1)
            .count();
        assert_eq!(p0_count, 50);
        assert_eq!(p1_count, 50);

        runtime.shutdown().await.expect("runtime shutdown");
    }
}
