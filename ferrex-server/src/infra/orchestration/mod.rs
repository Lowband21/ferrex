//! Server-side wiring for the scan orchestrator runtime backed by Postgres.
//!
//! This module binds the concrete queue, cursor repository, and actor system
//! together so the REST server can enqueue work, observe progress, and drive
//! follow-up automation using the same runtime that production nodes execute.

use std::{collections::HashMap, fmt, sync::Arc};

use ferrex_core::QueueService;
use ferrex_core::image_service::ImageService;
use ferrex_core::orchestration::actors::folder::FolderScanActor;
use ferrex_core::orchestration::actors::pipeline::{
    DefaultImageFetchActor, ImageFetchActor, IndexerActor, MediaAnalyzeActor, MetadataActor,
    TmdbMetadataActor,
};
use ferrex_core::{LibraryID, LibraryRootsId};
use ferrex_core::{
    MediaDatabase, MediaError, PostgresCursorRepository, PostgresDatabase, PostgresQueueService,
    Result,
    fs_watch::{FsWatchConfig, FsWatchService, NoopFsWatchObserver},
    orchestration::{
        actors::{
            DefaultFolderScanActor, DefaultIndexerActor, DefaultLibraryActor,
            DefaultMediaAnalyzeActor, LibraryActorCommand, LibraryActorConfig, NoopActorObserver,
        },
        budget::InMemoryBudget,
        config::OrchestratorConfig,
        correlation::CorrelationCache,
        dispatcher::{DefaultJobDispatcher, DispatcherActors, JobDispatcher},
        events::{DomainEvent, JobEvent, JobEventPayload, JobEventPublisher, stable_path_key},
        job::{EnqueueRequest, JobHandle, JobKind, JobPriority, JobValidator},
        lease::{DequeueRequest, JobLease},
        runtime::{
            InProcJobEventBus, LibraryActorHandle, OrchestratorRuntime, OrchestratorRuntimeBuilder,
        },
        scheduler::ReadyCountEntry,
    },
    providers::TmdbApiProvider,
};
use tokio::sync::Mutex;
use tracing::{debug, info, instrument};

pub struct ScanOrchestrator {
    runtime: Arc<OrchestratorRuntime<PostgresQueueService, InProcJobEventBus, InMemoryBudget>>,
    actors: Arc<ActorSystem>,
    validator: Arc<dyn JobValidator>,
    cursors: Arc<PostgresCursorRepository>,
    events: Arc<InProcJobEventBus>,
    watchers: Arc<FsWatchService>,
    correlations: CorrelationCache,
}

impl fmt::Debug for ScanOrchestrator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScanOrchestrator").finish_non_exhaustive()
    }
}

impl ScanOrchestrator {
    pub fn new(
        config: OrchestratorConfig,
        db: Arc<MediaDatabase>,
        tmdb: Arc<TmdbApiProvider>,
        image_service: Arc<ImageService>,
        queue: Arc<PostgresQueueService>,
        cursors: Arc<PostgresCursorRepository>,
        budget: Arc<InMemoryBudget>,
    ) -> Result<Self> {
        let events = Arc::new(InProcJobEventBus::new(256));
        let correlations = CorrelationCache::default();
        let actors = Arc::new(ActorSystem::new(
            Arc::clone(&db),
            Arc::clone(&tmdb),
            Arc::clone(&image_service),
            Arc::clone(&events),
            correlations.clone(),
        ));

        let dispatcher_actors = DispatcherActors::new(
            actors.folder_actor(),
            actors.analyze_actor(),
            actors.metadata_actor(),
            actors.indexer_actor(),
            actors.image_actor(),
        );

        let dispatcher: Arc<dyn JobDispatcher> = Arc::new(DefaultJobDispatcher::new(
            Arc::clone(&queue),
            Arc::clone(&events),
            Arc::clone(&cursors),
            dispatcher_actors,
            correlations.clone(),
        ));

        let watch_cfg = config.watch.clone();

        let runtime = OrchestratorRuntimeBuilder::new(config)
            .with_queue(Arc::clone(&queue))
            .with_events(Arc::clone(&events))
            .with_budget(Arc::clone(&budget))
            .with_dispatcher(dispatcher)
            .with_correlations(correlations.clone())
            .build()?;

        let validator: Arc<dyn JobValidator> = Arc::new(NoopJobValidator);
        let watchers: Arc<FsWatchService> = Arc::new(FsWatchService::new(
            FsWatchConfig::from(watch_cfg),
            Arc::new(NoopFsWatchObserver),
        ));

        Ok(Self {
            runtime: Arc::new(runtime),
            actors,
            validator,
            cursors,
            events,
            watchers,
            correlations,
        })
    }

    pub fn runtime(
        &self,
    ) -> Arc<OrchestratorRuntime<PostgresQueueService, InProcJobEventBus, InMemoryBudget>> {
        Arc::clone(&self.runtime)
    }

    pub fn actors(&self) -> Arc<ActorSystem> {
        Arc::clone(&self.actors)
    }

    pub fn subscribe_job_events(&self) -> tokio::sync::broadcast::Receiver<JobEvent> {
        self.events.subscribe()
    }

    pub fn subscribe_domain_events(&self) -> tokio::sync::broadcast::Receiver<DomainEvent> {
        self.events.subscribe_domain()
    }

    pub fn config(&self) -> OrchestratorConfig {
        self.runtime.config().clone()
    }

    pub async fn command_library(
        &self,
        library_id: LibraryID,
        command: LibraryActorCommand,
    ) -> Result<()> {
        self.runtime
            .submit_library_command(library_id, command)
            .await
    }

    pub fn cursor_repository(&self) -> Arc<PostgresCursorRepository> {
        Arc::clone(&self.cursors)
    }

    #[instrument(
        name = "scan_orchestrator.register_library",
        skip(self, config),
        fields(library_id = %config.library.id, root_count = config.root_paths.len()),
        err
    )]
    pub async fn register_library(&self, config: LibraryActorConfig) -> Result<()> {
        let queue = self.runtime.queue();
        let actor = self.actors.make_library_actor(config.clone(), queue);
        self.runtime
            .register_library_actor(config.library.id, Arc::clone(&actor))
            .await?;
        let roots = config
            .root_paths
            .iter()
            .enumerate()
            .map(|(idx, path)| (LibraryRootsId(idx as u16), path.clone()))
            .collect();
        self.watchers
            .register_library(config.library.id, roots, actor)
            .await?;
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        self.prime_ready_jobs().await?;
        self.runtime.start().await
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.watchers.shutdown().await;
        self.runtime.shutdown().await
    }

    pub async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle> {
        self.validator.validate(&request)?;

        let queue = self.runtime.queue();
        let events = self.runtime.events();

        let path_key = stable_path_key(&request.payload);
        let library_id = request.payload.library_id();
        let idempotency_key = request.dedupe_key().to_string();
        let priority = request.priority;
        let correlation_hint = request.correlation_id;

        let handle = queue.enqueue(request).await?;

        let correlation_for_event = if handle.accepted {
            correlation_hint
        } else if let Some(existing) = handle.merged_into {
            self.correlations
                .fetch(&existing)
                .await
                .or(correlation_hint)
        } else {
            correlation_hint
        };

        let payload = if handle.accepted {
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

        let event = JobEvent::from_job(
            correlation_for_event,
            library_id,
            idempotency_key,
            path_key,
            payload,
        );

        if handle.accepted {
            self.correlations
                .remember(handle.job_id, event.meta.correlation_id)
                .await;
        } else {
            self.correlations
                .remember_if_absent(handle.job_id, event.meta.correlation_id)
                .await;
        }

        events.publish(event).await.map_err(|err| {
            MediaError::Internal(format!("failed to publish enqueue event: {err}"))
        })?;

        Ok(handle)
    }

    #[instrument(skip(self), level = "debug", err)]
    async fn prime_ready_jobs(&self) -> Result<()> {
        let queue = self.runtime.queue();
        let scheduler = self.runtime.scheduler();

        let persistent_counts = queue.ready_counts_grouped().await?;
        if persistent_counts.is_empty() {
            debug!("no ready jobs found during scheduler prime");
            return Ok(());
        }

        let mut totals: HashMap<(LibraryID, JobPriority), usize> = HashMap::new();
        let mut ready_total = 0usize;

        for bucket in persistent_counts.iter() {
            if bucket.ready == 0 {
                continue;
            }

            ready_total += bucket.ready;
            totals
                .entry((bucket.library_id, bucket.priority))
                .and_modify(|count| *count += bucket.ready)
                .or_insert(bucket.ready);
        }

        if totals.is_empty() {
            debug!("no ready jobs to apply after filtering zero-count buckets");
            return Ok(());
        }

        let bucket_total = totals.len();
        let ready_entries: Vec<ReadyCountEntry> = totals
            .into_iter()
            .map(|((library_id, priority), count)| ReadyCountEntry {
                library_id,
                priority,
                count,
            })
            .collect();

        scheduler.record_ready_bulk(ready_entries).await;

        info!(
            ready_total,
            bucket_total,
            persistent_buckets = persistent_counts.len(),
            "primed scheduler ready counts from persistence"
        );

        Ok(())
    }

    pub async fn dequeue(&self, request: DequeueRequest) -> Result<Option<JobLease>> {
        let queue = self.runtime.queue();
        let events = self.runtime.events();

        let lease = queue.dequeue(request).await?;
        if let Some(ref lease) = lease {
            let payload = &lease.job.payload;
            let correlation_id = self.correlations.fetch_or_generate(lease.job.id).await;
            let event = JobEvent::from_job(
                Some(correlation_id),
                payload.library_id(),
                lease.job.dedupe_key.clone(),
                stable_path_key(payload),
                JobEventPayload::Dequeued {
                    job_id: lease.job.id,
                    kind: payload.kind(),
                    priority: lease.job.priority,
                    lease_id: lease.lease_id,
                },
            );

            events.publish(event).await.map_err(|err| {
                MediaError::Internal(format!("failed to publish dequeue event: {err}"))
            })?;
        }

        Ok(lease)
    }

    /// Return ready-queue depths for each job kind to aid diagnostics.
    pub async fn queue_depths(&self) -> Result<ferrex_core::api_scan::ScanQueueDepths> {
        let queue = self.runtime.queue();
        Ok(ferrex_core::api_scan::ScanQueueDepths {
            folder_scan: queue.queue_depth(JobKind::FolderScan).await?,
            analyze: queue.queue_depth(JobKind::MediaAnalyze).await?,
            metadata: queue.queue_depth(JobKind::MetadataEnrich).await?,
            index: queue.queue_depth(JobKind::IndexUpsert).await?,
            image_fetch: queue.queue_depth(JobKind::ImageFetch).await?,
        })
    }
}

impl ScanOrchestrator {
    pub async fn postgres(
        config: OrchestratorConfig,
        db: Arc<MediaDatabase>,
        tmdb: Arc<TmdbApiProvider>,
        image_service: Arc<ImageService>,
    ) -> Result<Self> {
        let backend = db
            .backend()
            .as_any()
            .downcast_ref::<PostgresDatabase>()
            .ok_or_else(|| {
                MediaError::Internal("Media database backend must be Postgres".into())
            })?;

        let pool = backend.pool().clone();
        let queue =
            Arc::new(PostgresQueueService::new_with_retry(pool.clone(), config.retry).await?);
        let cursors = Arc::new(PostgresCursorRepository::new(pool));
        let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));

        Self::new(config, db, tmdb, image_service, queue, cursors, budget)
    }
}

pub struct ActorSystem {
    observer: Arc<NoopActorObserver>,
    folder_actor: Arc<dyn FolderScanActor>,
    analyze_actor: Arc<dyn MediaAnalyzeActor>,
    metadata_actor: Arc<dyn MetadataActor>,
    indexer_actor: Arc<dyn IndexerActor>,
    image_actor: Arc<dyn ImageFetchActor>,
    events: Arc<InProcJobEventBus>,
    correlations: CorrelationCache,
}

impl fmt::Debug for ActorSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActorSystem").finish_non_exhaustive()
    }
}

impl ActorSystem {
    pub fn new(
        db: Arc<MediaDatabase>,
        tmdb: Arc<TmdbApiProvider>,
        image_service: Arc<ImageService>,
        events: Arc<InProcJobEventBus>,
        correlations: CorrelationCache,
    ) -> Self {
        let image_actor: Arc<dyn ImageFetchActor> =
            Arc::new(DefaultImageFetchActor::new(Arc::clone(&image_service)));
        let metadata_actor: Arc<dyn MetadataActor> =
            Arc::new(TmdbMetadataActor::new(Arc::clone(&db), tmdb, image_service));

        Self {
            observer: Arc::new(NoopActorObserver),
            folder_actor: Arc::new(DefaultFolderScanActor::new()),
            analyze_actor: Arc::new(DefaultMediaAnalyzeActor::new()),
            metadata_actor,
            indexer_actor: Arc::new(DefaultIndexerActor::new(Arc::clone(&db))),
            image_actor,
            events,
            correlations,
        }
    }

    pub fn make_library_actor(
        &self,
        config: LibraryActorConfig,
        queue: Arc<PostgresQueueService>,
    ) -> LibraryActorHandle {
        Arc::new(Mutex::new(Box::new(DefaultLibraryActor::new(
            config,
            queue,
            Arc::clone(&self.observer),
            Arc::clone(&self.events),
            self.correlations.clone(),
        ))))
    }

    pub fn folder_actor(&self) -> Arc<dyn FolderScanActor> {
        Arc::clone(&self.folder_actor)
    }

    pub fn analyze_actor(&self) -> Arc<dyn MediaAnalyzeActor> {
        Arc::clone(&self.analyze_actor)
    }

    pub fn metadata_actor(&self) -> Arc<dyn MetadataActor> {
        Arc::clone(&self.metadata_actor)
    }

    pub fn indexer_actor(&self) -> Arc<dyn IndexerActor> {
        Arc::clone(&self.indexer_actor)
    }

    pub fn image_actor(&self) -> Arc<dyn ImageFetchActor> {
        Arc::clone(&self.image_actor)
    }
}

#[derive(Clone)]
struct NoopJobValidator;

impl JobValidator for NoopJobValidator {}
