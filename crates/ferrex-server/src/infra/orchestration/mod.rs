//! Server-side wiring for the scan orchestrator runtime backed by Postgres.
//!
//! This module binds the concrete queue, cursor repository, and actor system
//! together so the REST server can enqueue work, observe progress, and drive
//! follow-up automation using the same runtime that production nodes execute.

use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
};

use ferrex_core::api::{IncrementalScanStatusView, ScanQueueDepths};
use ferrex_core::application::unit_of_work::AppUnitOfWork;
use ferrex_core::database::PostgresDatabase;
use ferrex_core::database::repositories::media::PostgresMediaRepository;
use ferrex_core::domain::scan::actors::provider::TmdbMetadataActor;
use ferrex_core::domain::scan::actors::{
    DefaultFolderScanActor, DefaultLibraryActor, LibraryActorCommand,
    LibraryActorConfig, LibraryRootsId, NoopActorObserver,
    analyze::{DefaultMediaAnalyzeActor, MediaAnalyzeActor},
    folder::{FolderScanActor, ScannerFileFilterPolicy},
};
use ferrex_core::domain::scan::image_fetch::{
    DefaultImageFetchActor, ImageFetchActor,
};
use ferrex_core::domain::scan::index::{DefaultIndexerActor, IndexerActor};
use ferrex_core::domain::scan::metadata::MetadataActor;
use ferrex_core::domain::scan::orchestration::{
    budget::InMemoryBudget,
    config::OrchestratorConfig,
    correlation::CorrelationCache,
    dispatcher::{DefaultJobDispatcher, DispatcherActors, JobDispatcher},
    events::{
        JobEvent, JobEventPayload, JobEventPublisher, ScanEvent,
        stable_path_key,
    },
    job::{EnqueueRequest, JobHandle, JobKind, JobPriority},
    lease::{DequeueRequest, JobLease},
    queue::QueueService,
    runtime::{
        InProcJobEventBus, LibraryActorHandle, LibraryCommandExecutor,
        OrchestratorRuntime, OrchestratorRuntimeBuilder,
    },
    scheduler::ReadyCountEntry,
    series::{
        DefaultSeriesResolver, SeriesMetadataProvider, SeriesResolverPort,
    },
    series_state::PostgresSeriesScanStateRepository,
};
use ferrex_core::domain::scan::{
    FileChangeEventBus, FsWatchConfig, FsWatchObserver, FsWatchService,
    PostgresCursorRepository, PostgresFileChangeEventBus, PostgresQueueService,
    SeriesScanStateRepository,
};
use ferrex_core::error::{MediaError, Result};
use ferrex_core::infra::media::{
    image_service::ImageService, providers::TmdbApiProvider,
};
use ferrex_core::types::LibraryId;
use tokio::sync::Mutex;
use tracing::{debug, info, instrument};

mod maintenance;

#[derive(Debug, Default)]
struct ScanWatchObserver {
    error_count: AtomicU64,
    last_error: StdMutex<Option<String>>,
}

impl ScanWatchObserver {
    fn snapshot(&self) -> (u64, Option<String>) {
        let last_error = self
            .last_error
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        (self.error_count.load(Ordering::Relaxed), last_error)
    }
}

impl FsWatchObserver for ScanWatchObserver {
    fn on_error(&self, library_id: LibraryId, error: &str) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut guard) = self.last_error.lock() {
            *guard = Some(format!("library {library_id}: {error}"));
        }
    }
}

#[derive(Debug, Default)]
struct FileWatchHealth {
    replay_pending_events: u64,
    replay_lag_ms: Option<u64>,
    overflow_events: u64,
}

#[derive(Debug, Default)]
struct CursorHealth {
    stale_cursor_libraries: u64,
    stale_cursors: u64,
    oldest_cursor_staleness_ms: Option<u64>,
}

pub struct ScanOrchestrator {
    runtime: Arc<
        OrchestratorRuntime<
            PostgresQueueService,
            InProcJobEventBus,
            InMemoryBudget,
        >,
    >,
    actors: Arc<ActorSystem>,
    cursors: Arc<PostgresCursorRepository>,
    events: Arc<InProcJobEventBus>,
    watchers: Arc<FsWatchService<ScanWatchObserver>>,
    watch_observer: Arc<ScanWatchObserver>,
    correlations: CorrelationCache,
    unit_of_work: Arc<AppUnitOfWork>,
    maintenance: Mutex<Option<maintenance::MaintenanceSchedulerHandle>>,
}

impl fmt::Debug for ScanOrchestrator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScanOrchestrator").finish_non_exhaustive()
    }
}

impl ScanOrchestrator {
    pub fn new(
        config: OrchestratorConfig,
        tmdb: Arc<TmdbApiProvider>,
        image_service: Arc<ImageService>,
        unit_of_work: Arc<AppUnitOfWork>,
        queue: Arc<PostgresQueueService>,
        cursors: Arc<PostgresCursorRepository>,
        budget: Arc<InMemoryBudget>,
        file_filters: ScannerFileFilterPolicy,
    ) -> Result<Self> {
        let events = Arc::new(InProcJobEventBus::new(256));
        let correlations = CorrelationCache::default();
        let actors = Arc::new(ActorSystem::new(
            Arc::clone(&tmdb),
            Arc::clone(&image_service),
            Arc::clone(&unit_of_work),
            Arc::clone(&events),
            correlations.clone(),
            file_filters,
        ));

        let dispatcher_actors = DispatcherActors::new(
            actors.folder_actor(),
            actors.analyze_actor(),
            actors.metadata_actor(),
            actors.indexer_actor(),
            actors.image_actor(),
        );

        let series_states: Arc<Box<dyn SeriesScanStateRepository>> =
            Arc::new(Box::new(PostgresSeriesScanStateRepository::new(
                queue.pool().clone(),
            )));
        let series_resolver: Arc<dyn SeriesResolverPort> =
            Arc::new(DefaultSeriesResolver::new(
                actors.series_provider(),
                Arc::clone(&series_states),
            ));

        let delta_repo =
            Arc::new(PostgresMediaRepository::new(queue.pool().clone()));
        let dispatcher: Arc<dyn JobDispatcher> = Arc::new(
            DefaultJobDispatcher::new(
                Arc::clone(&queue),
                Arc::clone(&events),
                Arc::clone(&cursors),
                Arc::clone(&series_states),
                Arc::clone(&series_resolver),
                dispatcher_actors,
                correlations.clone(),
            )
            .with_delta_repository(delta_repo)
            .with_intelligence_repository(unit_of_work.intelligence.clone()),
        );

        let watch_cfg = config.watch.clone();

        let runtime = Arc::new(
            OrchestratorRuntimeBuilder::new(config)
                .with_queue(Arc::clone(&queue))
                .with_events(Arc::clone(&events))
                .with_budget(Arc::clone(&budget))
                .with_dispatcher(dispatcher)
                .with_correlations(correlations.clone())
                .build()?,
        );

        let command_executor: Arc<dyn LibraryCommandExecutor> = runtime.clone();
        let file_change_bus: Arc<dyn FileChangeEventBus> =
            Arc::new(PostgresFileChangeEventBus::new(queue.pool().clone()));
        let watch_observer = Arc::new(ScanWatchObserver::default());
        let watchers: Arc<FsWatchService<ScanWatchObserver>> =
            Arc::new(FsWatchService::with_event_bus(
                FsWatchConfig::from(watch_cfg),
                Arc::clone(&watch_observer),
                command_executor,
                file_change_bus,
            ));

        Ok(Self {
            runtime,
            actors,
            cursors,
            events,
            watchers,
            watch_observer,
            correlations,
            unit_of_work,
            maintenance: Mutex::new(None),
        })
    }

    pub fn runtime(
        &self,
    ) -> Arc<
        OrchestratorRuntime<
            PostgresQueueService,
            InProcJobEventBus,
            InMemoryBudget,
        >,
    > {
        Arc::clone(&self.runtime)
    }

    pub fn actors(&self) -> Arc<ActorSystem> {
        Arc::clone(&self.actors)
    }

    pub fn subscribe_job_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<JobEvent> {
        self.events.subscribe()
    }

    pub fn subscribe_scan_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<ScanEvent> {
        self.events.subscribe_scan()
    }

    pub fn config(&self) -> OrchestratorConfig {
        self.runtime.config().clone()
    }

    pub async fn command_library(
        &self,
        library_id: LibraryId,
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
    pub async fn register_library(
        &self,
        config: LibraryActorConfig,
        watch_for_changes: bool,
    ) -> Result<()> {
        let queue = self.runtime.queue();
        let actor = self.actors.make_library_actor(config.clone(), queue);
        self.runtime
            .register_library_actor(config.library.id, Arc::clone(&actor))
            .await?;
        if watch_for_changes {
            let roots = config
                .root_paths
                .iter()
                .enumerate()
                .map(|(idx, path)| (LibraryRootsId(idx as u16), path.clone()))
                .collect();
            self.runtime.start_mailbox_runner().await?;
            self.watchers
                .register_library(config.library.id, roots)
                .await?;
        } else {
            debug!(library_id = %config.library.id, "skipping watcher registration (disabled)");
        }
        Ok(())
    }

    pub async fn unregister_library_watch(&self, library_id: LibraryId) {
        self.watchers.unregister_library(library_id).await;
    }

    pub async fn incremental_status(
        &self,
    ) -> Result<IncrementalScanStatusView> {
        let libraries = self.unit_of_work.libraries.list_libraries().await?;
        let enabled_libraries = libraries.iter().filter(|l| l.enabled).count();
        let auto_scan_libraries = libraries
            .iter()
            .filter(|l| l.enabled && l.auto_scan)
            .count();
        let watch_enabled_libraries = libraries
            .iter()
            .filter(|l| l.enabled && l.watch_for_changes)
            .count();

        let watcher_runtime = self.watchers.runtime_snapshot().await;
        let (watcher_error_count, last_watcher_error) =
            self.watch_observer.snapshot();
        let file_watch = self.file_watch_health().await?;
        let cursor = self.cursor_health().await?;

        Ok(IncrementalScanStatusView {
            enabled_libraries,
            auto_scan_libraries,
            watch_enabled_libraries,
            registered_watch_libraries: watcher_runtime.registered_libraries,
            active_watch_libraries: watcher_runtime.active_libraries,
            initializing_watch_libraries: watcher_runtime
                .initializing_libraries,
            registered_watch_roots: watcher_runtime.registered_roots,
            active_watch_roots: watcher_runtime.active_roots,
            watcher_error_count,
            last_watcher_error,
            replay_pending_events: file_watch.replay_pending_events,
            replay_lag_ms: file_watch.replay_lag_ms,
            overflow_events: file_watch.overflow_events,
            stale_cursor_libraries: cursor.stale_cursor_libraries,
            stale_cursors: cursor.stale_cursors,
            oldest_cursor_staleness_ms: cursor.oldest_cursor_staleness_ms,
        })
    }

    async fn file_watch_health(&self) -> Result<FileWatchHealth> {
        let queue = self.runtime.queue();
        let row = sqlx::query!(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE processed = false)::bigint AS "replay_pending_events!",
                COUNT(*) FILTER (WHERE event_type = 'overflow')::bigint AS "overflow_events!",
                (EXTRACT(EPOCH FROM (
                    NOW() - (MIN(detected_at) FILTER (WHERE processed = false))
                )) * 1000)::bigint AS replay_lag_ms
            FROM file_watch_events
            "#
        )
        .fetch_one(queue.pool())
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "file watch health query failed: {err}"
            ))
        })?;

        Ok(FileWatchHealth {
            replay_pending_events: row.replay_pending_events.max(0) as u64,
            replay_lag_ms: row.replay_lag_ms.map(|value| value.max(0) as u64),
            overflow_events: row.overflow_events.max(0) as u64,
        })
    }

    async fn cursor_health(&self) -> Result<CursorHealth> {
        let queue = self.runtime.queue();
        let row = sqlx::query!(
            r#"
            WITH stale AS (
                SELECT sc.library_id, sc.last_scan_at
                FROM scan_cursors sc
                JOIN libraries l ON l.id = sc.library_id
                WHERE l.enabled = true
                  AND l.auto_scan = true
                  AND sc.last_scan_at < NOW() - (
                      GREATEST(l.scan_interval_minutes, 1)::text || ' minutes'
                  )::interval
            )
            SELECT
                COUNT(*)::bigint AS "stale_cursors!",
                COUNT(DISTINCT library_id)::bigint AS "stale_cursor_libraries!",
                (EXTRACT(EPOCH FROM (NOW() - MIN(last_scan_at))) * 1000)::bigint AS oldest_cursor_staleness_ms
            FROM stale
            "#
        )
        .fetch_one(queue.pool())
        .await
        .map_err(|err| {
            MediaError::Internal(format!("cursor health query failed: {err}"))
        })?;

        Ok(CursorHealth {
            stale_cursor_libraries: row.stale_cursor_libraries.max(0) as u64,
            stale_cursors: row.stale_cursors.max(0) as u64,
            oldest_cursor_staleness_ms: row
                .oldest_cursor_staleness_ms
                .map(|value| value.max(0) as u64),
        })
    }

    pub async fn start(self: &Arc<Self>) -> Result<()> {
        self.prime_ready_jobs().await?;
        self.runtime.start().await?;
        self.start_maintenance_scheduler().await;
        Ok(())
    }

    async fn start_maintenance_scheduler(self: &Arc<Self>) {
        let config = self.runtime.config().maintenance.clone();
        if !config.enabled {
            debug!("incremental maintenance scheduler disabled");
            return;
        }

        let mut guard = self.maintenance.lock().await;
        if guard.is_some() {
            return;
        }

        *guard = Some(maintenance::spawn_maintenance_scheduler(
            Arc::clone(self),
            config,
        ));
        info!("incremental maintenance scheduler started");
    }

    pub async fn shutdown(&self) -> Result<()> {
        if let Some(handle) = self.maintenance.lock().await.take() {
            handle.shutdown().await;
        }
        self.watchers.shutdown().await;
        self.runtime.shutdown().await
    }

    pub async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle> {
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
            MediaError::Internal(format!(
                "failed to publish enqueue event: {err}"
            ))
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

        let mut totals: HashMap<(LibraryId, JobPriority), usize> =
            HashMap::new();
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

    pub async fn dequeue(
        &self,
        request: DequeueRequest,
    ) -> Result<Option<JobLease>> {
        let queue = self.runtime.queue();
        let events = self.runtime.events();

        let lease = queue.dequeue(request).await?;
        if let Some(ref lease) = lease {
            let payload = &lease.job.payload;
            let correlation_id = self
                .correlations
                .fetch_persisted_or_generate(
                    lease.job.id,
                    lease.job.correlation_id,
                )
                .await;
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
                MediaError::Internal(format!(
                    "failed to publish dequeue event: {err}"
                ))
            })?;
        }

        Ok(lease)
    }

    /// Return ready-queue depths for each job kind to aid diagnostics.
    pub async fn queue_depths(&self) -> Result<ScanQueueDepths> {
        let queue = self.runtime.queue();
        Ok(ferrex_core::api::scan::ScanQueueDepths {
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
        postgres: Arc<PostgresDatabase>,
        tmdb: Arc<TmdbApiProvider>,
        image_service: Arc<ImageService>,
        unit_of_work: Arc<AppUnitOfWork>,
        file_filters: ScannerFileFilterPolicy,
    ) -> Result<Self> {
        let pool = postgres.pool().clone();
        let queue = Arc::new(
            PostgresQueueService::new_with_retry(pool.clone(), config.retry)
                .await?,
        );
        let cursors = Arc::new(PostgresCursorRepository::new(pool));
        let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));

        Self::new(
            config,
            tmdb,
            image_service,
            unit_of_work,
            queue,
            cursors,
            budget,
            file_filters,
        )
    }
}

pub struct ActorSystem {
    observer: Arc<NoopActorObserver>,
    folder_actor: Arc<dyn FolderScanActor>,
    analyze_actor: Arc<dyn MediaAnalyzeActor>,
    metadata_actor: Arc<dyn MetadataActor>,
    series_provider: Arc<dyn SeriesMetadataProvider>,
    indexer_actor: Arc<dyn IndexerActor>,
    image_actor: Arc<dyn ImageFetchActor>,
    events: Arc<InProcJobEventBus>,
    correlations: CorrelationCache,
    file_filters: ScannerFileFilterPolicy,
}

impl fmt::Debug for ActorSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActorSystem").finish_non_exhaustive()
    }
}

impl ActorSystem {
    pub fn new(
        tmdb: Arc<TmdbApiProvider>,
        image_service: Arc<ImageService>,
        unit_of_work: Arc<AppUnitOfWork>,
        events: Arc<InProcJobEventBus>,
        correlations: CorrelationCache,
        file_filters: ScannerFileFilterPolicy,
    ) -> Self {
        let image_actor: Arc<dyn ImageFetchActor> =
            Arc::new(DefaultImageFetchActor::new(Arc::clone(&image_service)));
        let tmdb_actor = Arc::new(TmdbMetadataActor::new(
            unit_of_work.media_refs.clone(),
            unit_of_work.media_files_write.clone(),
            tmdb,
            Arc::clone(&image_service),
        ));
        let metadata_actor: Arc<dyn MetadataActor> = tmdb_actor.clone();
        let series_provider: Arc<dyn SeriesMetadataProvider> =
            tmdb_actor.clone();

        Self {
            observer: Arc::new(NoopActorObserver),
            folder_actor: Arc::new(DefaultFolderScanActor::with_filter_policy(
                file_filters.clone(),
            )),
            analyze_actor: Arc::new(DefaultMediaAnalyzeActor::new()),
            metadata_actor,
            series_provider,
            indexer_actor: Arc::new(DefaultIndexerActor::new(
                unit_of_work.media_refs.clone(),
            )),
            image_actor,
            events,
            correlations,
            file_filters,
        }
    }

    pub fn make_library_actor(
        &self,
        config: LibraryActorConfig,
        queue: Arc<PostgresQueueService>,
    ) -> LibraryActorHandle {
        Arc::new(Mutex::new(Box::new(
            DefaultLibraryActor::with_file_filter_policy(
                config,
                queue,
                Arc::clone(&self.observer),
                Arc::clone(&self.events),
                self.correlations.clone(),
                self.file_filters.clone(),
            ),
        )))
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

    pub fn series_provider(&self) -> Arc<dyn SeriesMetadataProvider> {
        Arc::clone(&self.series_provider)
    }

    pub fn indexer_actor(&self) -> Arc<dyn IndexerActor> {
        Arc::clone(&self.indexer_actor)
    }

    pub fn image_actor(&self) -> Arc<dyn ImageFetchActor> {
        Arc::clone(&self.image_actor)
    }
}
