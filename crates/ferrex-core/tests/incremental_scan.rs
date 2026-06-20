#![cfg(all(feature = "database", feature = "scan-runtime"))]

//! DB-backed acceptance coverage for incremental scan orchestration.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ferrex_core::database::repositories::media::PostgresMediaRepository;
use ferrex_core::database::traits::{FileWatchEvent, FileWatchEventType};
use ferrex_core::domain::scan::actors::analyze::{
    AnalysisContext, MediaAnalyzeActor, MediaAnalyzed,
};
use ferrex_core::domain::scan::actors::folder::{
    DefaultFolderScanActor, FolderScanActor,
};
use ferrex_core::domain::scan::actors::image_fetch::ImageFetchActor;
use ferrex_core::domain::scan::actors::index::{
    IndexCommand, IndexerActor, IndexingChange, IndexingOutcome,
};
use ferrex_core::domain::scan::actors::metadata::DefaultMetadataActor;
use ferrex_core::domain::scan::fs_watch::{
    FileChangeEventBus, FsWatchConfig, FsWatchService, NoopFsWatchObserver,
    PostgresFileChangeEventBus,
};
use ferrex_core::domain::scan::orchestration::context::{
    FolderScanContext, MovieFolderScanContext, MovieRootPath, SeriesRootPath,
};
use ferrex_core::domain::scan::orchestration::{
    CorrelationCache, DefaultJobDispatcher, DefaultLibraryActor,
    DispatchStatus, DispatcherActors, FolderScanJob, InMemoryBudget,
    InMemorySeriesScanStateRepository, InProcJobEventBus, JobDispatcher,
    JobKind, JobLease, JobPayload, JobPriority, JobRecord, LibraryActorCommand,
    LibraryActorConfig, LibraryActorHandle, LibraryCommandExecutor,
    LibraryRootsId, MaintenanceLibrary, MaintenancePlanningLimits,
    MediaAnalyzeJob, MediaFingerprint, NoopActorObserver, OrchestratorConfig,
    OrchestratorRuntime, OrchestratorRuntimeBuilder, PostgresCursorRepository,
    PostgresQueueService, QueueService, ScanCursor, ScanCursorId,
    ScanCursorRepository, ScanReason, SeriesResolution, SeriesResolverPort,
    SeriesScanState, SeriesScanStateRepository, WatchStrategy, normalize_path,
    plan_maintenance_sweep,
};
use ferrex_core::error::Result as CoreResult;
use ferrex_core::types::{LibraryId, LibraryReference, LibraryType};
use ferrex_core::types::{MediaID, VideoMediaType};
use notify::Event;
use notify::event::{
    CreateKind, DataChange, EventKind, ModifyKind, RemoveKind, RenameMode,
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Executor, PgPool};
use tempfile::TempDir;
use tokio::time::sleep;
use url::Url;
use uuid::Uuid;

const FALLBACK_ADMIN_DATABASE_URL: &str = "postgresql://postgres@127.0.0.1:55432/postgres?options=-csearch_path%3Dferrex,public";
const PROBE_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const TEMP_POSTGRES_START_TIMEOUT: Duration = Duration::from_secs(15);
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

type TestRuntime = OrchestratorRuntime<
    PostgresQueueService,
    InProcJobEventBus,
    InMemoryBudget,
>;
type TestDispatcher = DefaultJobDispatcher<
    PostgresQueueService,
    InProcJobEventBus,
    PostgresCursorRepository,
>;

#[tokio::test]
async fn local_library_events_queue_jobs_and_reconcile_read_models()
-> Result<()> {
    let db = TestDatabase::create().await?;
    let temp = TempDir::new().context("create library tempdir")?;
    let root = temp.path().to_path_buf();
    let library_id = LibraryId(Uuid::now_v7());
    seed_movie_library(&db.pool, library_id, &root).await?;

    let created_dir = root.join("Created Movie");
    let modified_dir = root.join("Modified Movie");
    let moved_dir = root.join("Moved Movie");
    let deleted_dir = root.join("Deleted Movie");
    for dir in [&created_dir, &modified_dir, &moved_dir, &deleted_dir] {
        tokio::fs::create_dir_all(dir).await?;
    }

    let created_file = created_dir.join("fresh.mkv");
    tokio::fs::write(&created_file, b"created media").await?;

    let modified_file = modified_dir.join("feature.mkv");
    tokio::fs::write(&modified_file, b"old").await?;
    let old_modified_fp = fingerprint_for_path(&modified_file)?;
    seed_media_file(&db.pool, library_id, &modified_file, &old_modified_fp)
        .await?;
    tokio::fs::write(&modified_file, b"new contents with a different size")
        .await?;

    let moved_old_file = moved_dir.join("old-name.mkv");
    let moved_new_file = moved_dir.join("new-name.mkv");
    tokio::fs::write(&moved_new_file, b"same bytes after rename").await?;
    let moved_fp = fingerprint_for_path(&moved_new_file)?;
    seed_media_file_with_path(
        &db.pool,
        library_id,
        &moved_old_file,
        &moved_fp,
        "old-name.mkv",
    )
    .await?;

    let deleted_file = deleted_dir.join("gone.mkv");
    tokio::fs::write(&deleted_file, b"removed media").await?;
    let deleted_fp = fingerprint_for_path(&deleted_file)?;
    seed_media_file(&db.pool, library_id, &deleted_file, &deleted_fp).await?;
    tokio::fs::remove_file(&deleted_file).await?;

    let harness = build_harness(&db.pool, library_id, &root).await?;
    let watch_bus = Arc::new(PostgresFileChangeEventBus::new(db.pool.clone()));
    let event_bus: Arc<dyn FileChangeEventBus> = watch_bus.clone();
    let command_executor: Arc<dyn LibraryCommandExecutor> =
        harness.runtime.clone();
    let watcher: FsWatchService = FsWatchService::with_event_bus(
        quiet_watch_config(),
        Arc::new(NoopFsWatchObserver),
        command_executor,
        event_bus,
    );
    watcher
        .register_library(library_id, vec![(LibraryRootsId(0), root.clone())])
        .await?;

    watcher
        .inject_notify_event_for_test(
            library_id,
            Event::new(EventKind::Create(CreateKind::File))
                .add_path(created_file.clone()),
        )
        .await?;
    watcher
        .inject_notify_event_for_test(
            library_id,
            Event::new(EventKind::Modify(ModifyKind::Data(
                DataChange::Content,
            )))
            .add_path(modified_file.clone()),
        )
        .await?;
    watcher
        .inject_notify_event_for_test(
            library_id,
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                .add_path(moved_old_file.clone())
                .add_path(moved_new_file.clone()),
        )
        .await?;
    watcher
        .inject_notify_event_for_test(
            library_id,
            Event::new(EventKind::Remove(RemoveKind::File))
                .add_path(deleted_file.clone()),
        )
        .await?;

    let expected_folders = BTreeSet::from([
        normalize_path(&created_dir)?,
        normalize_path(&modified_dir)?,
        normalize_path(&moved_dir)?,
        normalize_path(&deleted_dir)?,
    ]);
    let queued_folders = wait_for_folder_jobs(&db.pool, library_id, 4).await?;
    assert_eq!(queued_folders, expected_folders);

    let durable_types =
        wait_for_file_watch_types(&db.pool, library_id, 4).await?;
    assert_eq!(
        durable_types,
        BTreeSet::from([
            "created".to_owned(),
            "deleted".to_owned(),
            "modified".to_owned(),
            "moved".to_owned(),
        ])
    );
    let unprocessed = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::bigint AS "count!" FROM file_watch_events WHERE library_id = $1 AND processed = false"#,
        library_id.0
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(unprocessed, 0, "watch events must be acked after handoff");

    let folder_count = dispatch_ready_jobs(
        &harness.queue,
        &harness.dispatcher,
        JobKind::FolderScan,
        8,
    )
    .await?;
    assert_eq!(folder_count, 4);

    let analyze_paths =
        ready_job_paths(&db.pool, library_id, JobKind::MediaAnalyze).await?;
    assert_eq!(
        analyze_paths,
        BTreeSet::from([
            normalize_path(&created_file)?,
            normalize_path(&modified_file)?,
        ]),
        "create and fingerprint-changing modify should enter the media pipeline; move/delete should reconcile safely"
    );

    let analyze_count = dispatch_ready_jobs(
        &harness.queue,
        &harness.dispatcher,
        JobKind::MediaAnalyze,
        8,
    )
    .await?;
    assert_eq!(analyze_count, 2);
    let metadata_count = dispatch_ready_jobs(
        &harness.queue,
        &harness.dispatcher,
        JobKind::MetadataEnrich,
        8,
    )
    .await?;
    assert_eq!(metadata_count, 2);
    let index_paths =
        ready_job_paths(&db.pool, library_id, JobKind::IndexUpsert).await?;
    assert_eq!(
        index_paths,
        BTreeSet::from([
            normalize_path(&created_file)?,
            normalize_path(&modified_file)?,
        ])
    );

    let moved_row =
        media_file_row(&db.pool, library_id, &moved_new_file).await?;
    assert_eq!(moved_row.is_available, Some(true));
    let old_moved_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::bigint AS "count!" FROM media_files WHERE library_id = $1 AND file_path = $2"#,
        library_id.0,
        normalize_path(&moved_old_file)?
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        old_moved_count, 0,
        "move should update the existing row path"
    );

    let deleted_row =
        media_file_row(&db.pool, library_id, &deleted_file).await?;
    assert_eq!(deleted_row.is_available, Some(false));
    assert_eq!(
        deleted_row.tombstone_reason.as_deref(),
        Some("folder_delta_file_missing")
    );

    let cursors = harness.cursors.list_by_library(library_id).await?;
    let cursor_paths: BTreeSet<_> = cursors
        .into_iter()
        .map(|cursor| cursor.folder_path_norm)
        .collect();
    assert!(expected_folders.is_subset(&cursor_paths));

    watcher.unregister_library(library_id).await;
    db.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn durable_replay_delivers_unacked_events_once() -> Result<()> {
    let db = TestDatabase::create().await?;
    let temp = TempDir::new().context("create replay tempdir")?;
    let root = temp.path().to_path_buf();
    let library_id = LibraryId(Uuid::now_v7());
    seed_movie_library(&db.pool, library_id, &root).await?;

    let movie_dir = root.join("Replay Movie");
    tokio::fs::create_dir_all(&movie_dir).await?;
    let media_path = movie_dir.join("feature.mkv");
    tokio::fs::write(&media_path, b"durable replay").await?;

    let harness = build_harness(&db.pool, library_id, &root).await?;
    let bus = Arc::new(PostgresFileChangeEventBus::new(db.pool.clone()));
    let event = durable_event(
        library_id,
        &root,
        &media_path,
        FileWatchEventType::Created,
        "replay-created",
    )?;
    assert!(bus.publish(event.clone()).await?);

    let command_executor: Arc<dyn LibraryCommandExecutor> =
        harness.runtime.clone();
    let watcher: FsWatchService = FsWatchService::with_event_bus(
        quiet_watch_config(),
        Arc::new(NoopFsWatchObserver),
        command_executor,
        bus.clone(),
    );
    watcher
        .register_library(library_id, vec![(LibraryRootsId(0), root.clone())])
        .await?;

    let queued = wait_for_folder_jobs(&db.pool, library_id, 1).await?;
    assert_eq!(queued, BTreeSet::from([normalize_path(&movie_dir)?]));
    let processed = sqlx::query_scalar!(
        "SELECT processed FROM file_watch_events WHERE id = $1",
        event.id
    )
    .fetch_one(&db.pool)
    .await?;
    assert!(processed, "replayed event should be acked");
    assert_eq!(
        bus.get_cursor("fs-watch-service", library_id)
            .await?
            .and_then(|cursor| cursor.last_event_id),
        Some(event.id)
    );

    watcher.unregister_library(library_id).await;
    let command_executor: Arc<dyn LibraryCommandExecutor> =
        harness.runtime.clone();
    let second_watcher: FsWatchService = FsWatchService::with_event_bus(
        quiet_watch_config(),
        Arc::new(NoopFsWatchObserver),
        command_executor,
        bus,
    );
    second_watcher
        .register_library(library_id, vec![(LibraryRootsId(0), root)])
        .await?;
    sleep(Duration::from_millis(100)).await;

    let job_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::bigint AS "count!" FROM orchestrator_jobs WHERE library_id = $1 AND kind = $2"#,
        library_id.0,
        JobKind::FolderScan as i16
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        job_count, 1,
        "processed replay event must not be double-applied"
    );

    second_watcher.unregister_library(library_id).await;
    db.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn forced_poll_strategy_discovers_changes_without_native_notify()
-> Result<()> {
    let db = TestDatabase::create().await?;
    let temp = TempDir::new().context("create poll tempdir")?;
    let root = temp.path().to_path_buf();
    let library_id = LibraryId(Uuid::now_v7());
    seed_movie_library(&db.pool, library_id, &root).await?;
    let harness = build_harness(&db.pool, library_id, &root).await?;

    let watch_bus = Arc::new(PostgresFileChangeEventBus::new(db.pool.clone()));
    let event_bus: Arc<dyn FileChangeEventBus> = watch_bus;
    let command_executor: Arc<dyn LibraryCommandExecutor> =
        harness.runtime.clone();
    let watcher: FsWatchService = FsWatchService::with_event_bus(
        FsWatchConfig {
            debounce_window: Duration::from_millis(25),
            max_batch_events: 32,
            strategy: WatchStrategy::Poll,
            poll_interval: Duration::from_millis(20),
            poll_backoff_max: Duration::from_secs(1),
        },
        Arc::new(NoopFsWatchObserver),
        command_executor,
        event_bus,
    );
    watcher
        .register_library(library_id, vec![(LibraryRootsId(0), root.clone())])
        .await?;
    sleep(Duration::from_millis(200)).await;

    let movie_dir = root.join("Poll Movie");
    tokio::fs::create_dir_all(&movie_dir).await?;
    let media_path = movie_dir.join("network-change.mkv");
    tokio::fs::write(&media_path, b"polling fallback").await?;

    let queued = wait_for_folder_jobs(&db.pool, library_id, 1).await?;
    assert!(
        queued.contains(&normalize_path(&movie_dir)?),
        "forced polling watcher should enqueue the changed top-level folder without native notify"
    );
    let events_seen = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::bigint AS "count!" FROM file_watch_events WHERE library_id = $1"#,
        library_id.0
    )
    .fetch_one(&db.pool)
    .await?;
    assert!(
        events_seen >= 1,
        "polling watcher should persist durable events"
    );

    watcher.unregister_library(library_id).await;
    db.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn maintenance_uses_db_cursors_and_unchanged_folders_short_circuit()
-> Result<()> {
    let db = TestDatabase::create().await?;
    let temp = TempDir::new().context("create maintenance tempdir")?;
    let root = temp.path().to_path_buf();
    let library_id = LibraryId(Uuid::now_v7());
    seed_movie_library(&db.pool, library_id, &root).await?;
    let harness = build_harness(&db.pool, library_id, &root).await?;

    let stable_dir = root.join("Stable Movie");
    tokio::fs::create_dir_all(&stable_dir).await?;
    tokio::fs::write(stable_dir.join("same.mkv"), b"same bytes").await?;
    let stable_job =
        folder_scan_job(library_id, &root, &stable_dir, ScanReason::HotChange)?;
    let status = harness
        .dispatcher
        .dispatch(&lease_for_payload(JobPayload::FolderScan(
            stable_job.clone(),
        )))
        .await;
    assert!(matches!(status, DispatchStatus::Success));
    assert!(
        harness
            .cursors
            .get(&cursor_id(library_id, &stable_dir)?)
            .await?
            .is_some(),
        "first scan should write the cursor"
    );

    clear_jobs_for_library(&db.pool, library_id).await?;
    let status = harness
        .dispatcher
        .dispatch(&lease_for_payload(JobPayload::FolderScan(stable_job)))
        .await;
    assert!(matches!(status, DispatchStatus::Success));
    for kind in [
        JobKind::MediaAnalyze,
        JobKind::MetadataEnrich,
        JobKind::IndexUpsert,
    ] {
        let count = count_jobs_by_kind(&db.pool, library_id, kind).await?;
        assert_eq!(
            count, 0,
            "unchanged cursor short-circuit should not enqueue {:?}",
            kind
        );
    }

    let now = Utc::now();
    let stale_dir = root.join("Stale Movie");
    let fresh_dir = root.join("Fresh Movie");
    let new_dir = root.join("New Maintenance Movie");
    for dir in [&stale_dir, &fresh_dir, &new_dir] {
        tokio::fs::create_dir_all(dir).await?;
    }
    upsert_cursor(
        &harness.cursors,
        library_id,
        &stale_dir,
        now - chrono::Duration::minutes(90),
        "stale-hash",
    )
    .await?;
    upsert_cursor(
        &harness.cursors,
        library_id,
        &fresh_dir,
        now - chrono::Duration::minutes(5),
        "fresh-hash",
    )
    .await?;

    let library = MaintenanceLibrary {
        id: library_id,
        name: "Maintenance acceptance".into(),
        library_type: LibraryType::Movies,
        paths: vec![root.clone()],
        scan_interval_minutes: 60,
        last_scan: Some(now - chrono::Duration::minutes(61)),
        enabled: true,
        auto_scan: true,
        watch_for_changes: false,
    };
    let plan = plan_maintenance_sweep(
        &library,
        harness.cursors.as_ref(),
        MaintenancePlanningLimits::new(16, 64),
        now,
    )
    .await?;
    let planned_paths: BTreeSet<_> = plan
        .requests
        .iter()
        .filter_map(|request| match &request.payload {
            JobPayload::FolderScan(job) => {
                Some(job.context.folder_path_norm().to_owned())
            }
            _ => None,
        })
        .collect();
    assert!(planned_paths.contains(&normalize_path(&stale_dir)?));
    assert!(planned_paths.contains(&normalize_path(&new_dir)?));
    assert!(!planned_paths.contains(&normalize_path(&fresh_dir)?));

    harness.queue.enqueue_many(plan.requests).await?;
    let maintenance_jobs =
        ready_job_paths(&db.pool, library_id, JobKind::FolderScan).await?;
    assert!(maintenance_jobs.contains(&normalize_path(&stale_dir)?));
    assert!(maintenance_jobs.contains(&normalize_path(&new_dir)?));

    db.cleanup().await?;
    Ok(())
}

async fn build_harness(
    pool: &PgPool,
    library_id: LibraryId,
    root: &Path,
) -> Result<AcceptanceHarness> {
    let queue = Arc::new(PostgresQueueService::new(pool.clone()).await?);
    let events = Arc::new(InProcJobEventBus::new(256));
    let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));
    let media_repo = Arc::new(PostgresMediaRepository::new(pool.clone()));

    let actors = DispatcherActors::new(
        Arc::new(DefaultFolderScanActor::new()) as Arc<dyn FolderScanActor>,
        Arc::new(StubAnalyze) as Arc<dyn MediaAnalyzeActor>,
        Arc::new(DefaultMetadataActor::new()),
        Arc::new(StubIndexer) as Arc<dyn IndexerActor>,
        Arc::new(StubImage) as Arc<dyn ImageFetchActor>,
    );
    let correlations = CorrelationCache::default();
    let series_states: Arc<Box<dyn SeriesScanStateRepository>> =
        Arc::new(Box::new(InMemorySeriesScanStateRepository::default()));
    let dispatcher = DefaultJobDispatcher::new(
        Arc::clone(&queue),
        Arc::clone(&events),
        Arc::clone(&cursors),
        Arc::clone(&series_states),
        Arc::new(NoopSeriesResolver),
        actors,
        correlations.clone(),
    )
    .with_delta_repository(media_repo);

    let config = OrchestratorConfig::default();
    let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));
    let runtime = Arc::new(
        OrchestratorRuntimeBuilder::new(config)
            .with_queue(Arc::clone(&queue))
            .with_events(Arc::clone(&events))
            .with_budget(budget)
            .with_dispatcher(Arc::new(NoopDispatcher) as Arc<dyn JobDispatcher>)
            .with_correlations(correlations)
            .build()?,
    );

    let root_norm = normalize_path(root)?;
    let actor_config = LibraryActorConfig {
        library: LibraryReference {
            id: library_id,
            name: format!("Incremental acceptance {library_id}"),
            library_type: LibraryType::Movies,
            paths: vec![PathBuf::from(&root_norm)],
        },
        root_paths: vec![PathBuf::from(&root_norm)],
        max_outstanding_jobs: 256,
    };
    let actor = DefaultLibraryActor::new(
        actor_config,
        Arc::clone(&queue),
        Arc::new(NoopActorObserver),
        Arc::clone(&events),
        CorrelationCache::default(),
    );
    let actor: LibraryActorHandle =
        Arc::new(tokio::sync::Mutex::new(Box::new(actor)));
    runtime.register_library_actor(library_id, actor).await?;
    runtime.start_mailbox_runner().await?;
    runtime
        .submit_library_command(
            library_id,
            LibraryActorCommand::Start {
                mode:
                    ferrex_core::domain::scan::orchestration::StartMode::Resume,
                correlation_id: None,
            },
        )
        .await?;

    Ok(AcceptanceHarness {
        queue,
        dispatcher,
        cursors,
        runtime,
    })
}

struct AcceptanceHarness {
    queue: Arc<PostgresQueueService>,
    dispatcher: TestDispatcher,
    cursors: Arc<PostgresCursorRepository>,
    runtime: Arc<TestRuntime>,
}

#[derive(Debug)]
struct NoopDispatcher;

#[async_trait]
impl JobDispatcher for NoopDispatcher {
    async fn dispatch(&self, _lease: &JobLease) -> DispatchStatus {
        DispatchStatus::Success
    }
}

#[derive(Debug)]
struct NoopSeriesResolver;

#[async_trait]
impl SeriesResolverPort for NoopSeriesResolver {
    async fn resolve(
        &self,
        _job: &ferrex_core::domain::scan::orchestration::SeriesResolveJob,
    ) -> CoreResult<SeriesResolution> {
        Err(ferrex_core::error::MediaError::Internal(
            "series resolution is unused by movie acceptance tests".into(),
        ))
    }

    async fn mark_failed(
        &self,
        _library_id: LibraryId,
        _series_root_path: SeriesRootPath,
        _reason: String,
    ) -> CoreResult<()> {
        Ok(())
    }

    async fn get_state(
        &self,
        _library_id: LibraryId,
        _series_root_path: &SeriesRootPath,
    ) -> CoreResult<Option<SeriesScanState>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct StubAnalyze;

#[async_trait]
impl MediaAnalyzeActor for StubAnalyze {
    async fn analyze(
        &self,
        command: MediaAnalyzeJob,
    ) -> CoreResult<MediaAnalyzed> {
        Ok(MediaAnalyzed {
            library_id: command.library_id,
            media_id: command.media_id,
            variant: command.variant,
            hierarchy: command.hierarchy,
            node: command.node,
            path_norm: command.path_norm,
            fingerprint: command.fingerprint,
            analyzed_at: Utc::now(),
            analysis: AnalysisContext::default(),
            thumbnails: Vec::new(),
        })
    }
}

#[derive(Debug)]
struct StubIndexer;

#[async_trait]
impl IndexerActor for StubIndexer {
    async fn index(
        &self,
        command: IndexCommand,
    ) -> CoreResult<IndexingOutcome> {
        Ok(IndexingOutcome {
            library_id: command.job.library_id,
            path_norm: command.job.path_norm,
            media_id: command.ready.media_id,
            hierarchy: command.job.hierarchy,
            indexed_at: Utc::now(),
            upserted: true,
            media: None,
            change: IndexingChange::Created,
        })
    }
}

#[derive(Debug)]
struct StubImage;

#[async_trait]
impl ImageFetchActor for StubImage {
    async fn fetch(
        &self,
        _job: &ferrex_core::domain::scan::ImageFetchJob,
    ) -> CoreResult<()> {
        Ok(())
    }
}

fn quiet_watch_config() -> FsWatchConfig {
    FsWatchConfig {
        debounce_window: Duration::from_millis(25),
        max_batch_events: 32,
        strategy: WatchStrategy::Poll,
        poll_interval: Duration::from_secs(60 * 60),
        poll_backoff_max: Duration::from_secs(60 * 60),
    }
}

async fn dispatch_ready_jobs(
    queue: &PostgresQueueService,
    dispatcher: &dyn JobDispatcher,
    kind: JobKind,
    limit: usize,
) -> Result<usize> {
    let mut count = 0;
    for idx in 0..limit {
        let Some(lease) = queue
            .dequeue(ferrex_core::domain::scan::orchestration::DequeueRequest {
                kind,
                worker_id: format!("incremental-acceptance-{kind:?}-{idx}"),
                lease_ttl: chrono::Duration::seconds(30),
                selector: None,
            })
            .await?
        else {
            break;
        };
        let status = dispatcher.dispatch(&lease).await;
        assert!(
            matches!(status, DispatchStatus::Success),
            "dispatch for {:?} returned {:?}",
            kind,
            status
        );
        queue.complete(lease.lease_id).await?;
        count += 1;
    }
    Ok(count)
}

async fn wait_for_folder_jobs(
    pool: &PgPool,
    library_id: LibraryId,
    expected_count: usize,
) -> Result<BTreeSet<String>> {
    wait_until(WAIT_TIMEOUT, || async {
        let paths =
            ready_job_paths(pool, library_id, JobKind::FolderScan).await?;
        Ok((paths.len() >= expected_count).then_some(paths))
    })
    .await
}

async fn wait_for_file_watch_types(
    pool: &PgPool,
    library_id: LibraryId,
    expected_count: usize,
) -> Result<BTreeSet<String>> {
    wait_until(WAIT_TIMEOUT, || async {
        let rows = sqlx::query!(
            "SELECT event_type FROM file_watch_events WHERE library_id = $1",
            library_id.0
        )
        .fetch_all(pool)
        .await?;
        if rows.len() < expected_count {
            return Ok(None);
        }
        Ok(Some(rows.into_iter().map(|row| row.event_type).collect()))
    })
    .await
}

async fn wait_until<T, F, Fut>(timeout: Duration, mut f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Option<T>>>,
{
    let started = Instant::now();
    loop {
        if let Some(value) = f().await? {
            return Ok(value);
        }
        if started.elapsed() > timeout {
            bail!("condition was not met within {timeout:?}");
        }
        sleep(Duration::from_millis(25)).await;
    }
}

async fn ready_job_paths(
    pool: &PgPool,
    library_id: LibraryId,
    kind: JobKind,
) -> Result<BTreeSet<String>> {
    let rows = sqlx::query!(
        r#"
        SELECT payload
        FROM orchestrator_jobs
        WHERE library_id = $1 AND kind = $2 AND state = 'ready'
        ORDER BY created_at ASC
        "#,
        library_id.0,
        kind as i16
    )
    .fetch_all(pool)
    .await?;

    let mut paths = BTreeSet::new();
    for row in rows {
        let payload: JobPayload = serde_json::from_value(row.payload)?;
        match payload {
            JobPayload::FolderScan(job) => {
                paths.insert(job.context.folder_path_norm().to_owned());
            }
            JobPayload::MediaAnalyze(job) => {
                paths.insert(job.path_norm);
            }
            JobPayload::MetadataEnrich(job) => {
                paths.insert(job.path_norm);
            }
            JobPayload::IndexUpsert(job) => {
                paths.insert(job.path_norm);
            }
            JobPayload::ImageFetch(_)
            | JobPayload::SeriesResolve(_)
            | JobPayload::EpisodeMatch(_)
            | JobPayload::ManifestScan(_) => {}
        }
    }
    Ok(paths)
}

async fn count_jobs_by_kind(
    pool: &PgPool,
    library_id: LibraryId,
    kind: JobKind,
) -> Result<i64> {
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::bigint AS "count!" FROM orchestrator_jobs WHERE library_id = $1 AND kind = $2"#,
        library_id.0,
        kind as i16
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}

async fn clear_jobs_for_library(
    pool: &PgPool,
    library_id: LibraryId,
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM orchestrator_jobs WHERE library_id = $1",
        library_id.0
    )
    .execute(pool)
    .await?;
    Ok(())
}

fn folder_scan_job(
    library_id: LibraryId,
    root: &Path,
    folder: &Path,
    reason: ScanReason,
) -> Result<FolderScanJob> {
    let root_norm = normalize_path(root)?;
    let folder_norm = normalize_path(folder)?;
    let movie_root_path =
        MovieRootPath::try_new_under_library_root(&root_norm, folder_norm)?;
    Ok(FolderScanJob {
        context: FolderScanContext::Movie(MovieFolderScanContext {
            library_id,
            movie_root_path,
        }),
        scan_reason: reason,
        enqueue_time: Utc::now(),
        device_id: None,
    })
}

fn cursor_id(library_id: LibraryId, folder: &Path) -> Result<ScanCursorId> {
    Ok(ScanCursorId::new(
        library_id,
        &vec![PathBuf::from(normalize_path(folder)?)],
    ))
}

fn lease_for_payload(payload: JobPayload) -> JobLease {
    let record = JobRecord::new(payload, JobPriority::P1);
    JobLease::new(
        record,
        "incremental-acceptance-direct".into(),
        chrono::Duration::seconds(30),
    )
}

async fn upsert_cursor(
    cursors: &PostgresCursorRepository,
    library_id: LibraryId,
    folder: &Path,
    last_scan_at: DateTime<Utc>,
    listing_hash: &str,
) -> Result<()> {
    let folder_path_norm = normalize_path(folder)?;
    cursors
        .upsert(ScanCursor {
            id: ScanCursorId::new(
                library_id,
                &vec![PathBuf::from(&folder_path_norm)],
            ),
            folder_path_norm,
            listing_hash: listing_hash.into(),
            entry_count: 0,
            last_scan_at,
            last_modified_at: None,
            device_id: None,
        })
        .await?;
    Ok(())
}

fn fingerprint_for_path(path: &Path) -> Result<MediaFingerprint> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("read metadata for {}", path.display()))?;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default();
    Ok(MediaFingerprint {
        device_id: None,
        inode: None,
        size: metadata.len(),
        mtime: modified_ms,
        weak_hash: None,
    })
}

async fn seed_movie_library(
    pool: &PgPool,
    library_id: LibraryId,
    root: &Path,
) -> Result<()> {
    let library_name = format!("Incremental Scan Acceptance {library_id}");
    let paths = vec![normalize_path(root)?];
    sqlx::query!(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, $2, 'movies', $3)
        "#,
        library_id.0,
        library_name,
        &paths
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_media_file(
    pool: &PgPool,
    library_id: LibraryId,
    path: &Path,
    fingerprint: &MediaFingerprint,
) -> Result<()> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("media.mkv");
    seed_media_file_with_path(pool, library_id, path, fingerprint, filename)
        .await
}

async fn seed_media_file_with_path(
    pool: &PgPool,
    library_id: LibraryId,
    path: &Path,
    fingerprint: &MediaFingerprint,
    filename: &str,
) -> Result<()> {
    let media_id = MediaID::new(VideoMediaType::Movie);
    let media_uuid = media_id.as_uuid();
    let path_norm = normalize_path(path)?;
    sqlx::query!(
        r#"
        INSERT INTO media_files (
            id, library_id, media_id, media_type, file_path, filename, file_size,
            fingerprint_device_id, fingerprint_inode, fingerprint_size,
            fingerprint_mtime_ms, fingerprint_weak_hash
        )
        VALUES ($1, $2, $3, 'movie', $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
        Uuid::now_v7(),
        library_id.0,
        media_uuid,
        path_norm,
        filename,
        fingerprint.size as i64,
        fingerprint.device_id.as_deref(),
        fingerprint.inode.map(|value| value as i64),
        fingerprint.size as i64,
        fingerprint.mtime,
        fingerprint.weak_hash.as_deref()
    )
    .execute(pool)
    .await?;
    Ok(())
}

struct MediaFileRow {
    is_available: Option<bool>,
    tombstone_reason: Option<String>,
}

async fn media_file_row(
    pool: &PgPool,
    library_id: LibraryId,
    path: &Path,
) -> Result<MediaFileRow> {
    let row = sqlx::query!(
        "SELECT is_available, tombstone_reason FROM media_files WHERE library_id = $1 AND file_path = $2",
        library_id.0,
        normalize_path(path)?
    )
    .fetch_one(pool)
    .await?;
    Ok(MediaFileRow {
        is_available: Some(row.is_available),
        tombstone_reason: row.tombstone_reason,
    })
}

fn durable_event(
    library_id: LibraryId,
    root: &Path,
    path: &Path,
    event_type: FileWatchEventType,
    suffix: &str,
) -> Result<FileWatchEvent> {
    Ok(FileWatchEvent {
        id: Uuid::now_v7(),
        event_version: 1,
        library_id,
        library_root_id: 0,
        root_path: normalize_path(root)?,
        event_type,
        file_path: normalize_path(path)?,
        path_key: normalize_path(path)?,
        old_path: None,
        fingerprint: None,
        file_size: Some(
            fs::metadata(path)
                .map(|metadata| metadata.len() as i64)
                .unwrap_or(0),
        ),
        file_modified_at: Some(Utc::now()),
        correlation_id: None,
        idempotency_key: format!(
            "incremental-acceptance:{library_id}:{suffix}"
        ),
        detected_at: Utc::now(),
        processed: false,
        processed_at: None,
        processing_attempts: 0,
        last_error: None,
    })
}

struct TestDatabase {
    admin_database_url: String,
    database_name: String,
    pool: PgPool,
    _server: Option<TempPostgres>,
}

impl TestDatabase {
    async fn create() -> Result<Self> {
        let requested_admin_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| FALLBACK_ADMIN_DATABASE_URL.to_owned());
        match connect_pool(&requested_admin_url, 1, PROBE_CONNECT_TIMEOUT).await
        {
            Ok(pool) => {
                pool.close().await;
                Self::create_on_admin_url(requested_admin_url, None).await
            }
            Err(connect_error) => {
                let temp_postgres = TempPostgres::start(&requested_admin_url)
                    .await
                    .with_context(|| {
                        format!(
                            "{} was unreachable ({connect_error:#}); failed to start temporary PostgreSQL",
                            display_database_url(&requested_admin_url)
                        )
                    })?;
                let admin_database_url =
                    temp_postgres.admin_database_url.clone();
                Self::create_on_admin_url(
                    admin_database_url,
                    Some(temp_postgres),
                )
                .await
            }
        }
    }

    async fn create_on_admin_url(
        admin_database_url: String,
        server: Option<TempPostgres>,
    ) -> Result<Self> {
        let database_name = unique_test_database_name();
        let admin_pool =
            connect_pool(&admin_database_url, 1, Duration::from_secs(5))
                .await
                .with_context(|| {
                    format!(
                        "failed to connect to setup test database {}",
                        display_database_url(&admin_database_url)
                    )
                })?;
        let create_database_sql =
            format!("CREATE DATABASE {}", quote_ident(&database_name));
        admin_pool
            .execute(create_database_sql.as_str())
            .await
            .with_context(|| {
                format!("failed to create test database {database_name}")
            })?;
        admin_pool.close().await;

        let database_url =
            database_url_with_database(&admin_database_url, &database_name)?;
        let pool = connect_pool(&database_url, 5, Duration::from_secs(5))
            .await
            .with_context(|| {
                format!(
                    "failed to connect to isolated test database {}",
                    display_database_url(&database_url)
                )
            })?;
        ferrex_core::MIGRATOR.run(&pool).await.with_context(|| {
            "failed to migrate isolated test database; ensure pg_uuidv7 is available"
        })?;

        Ok(Self {
            admin_database_url,
            database_name,
            pool,
            _server: server,
        })
    }

    async fn cleanup(self) -> Result<()> {
        self.pool.close().await;
        let admin_pool = connect_pool(
            &self.admin_database_url,
            1,
            Duration::from_secs(5),
        )
        .await
        .with_context(|| {
            format!(
                "failed to reconnect to setup test database {} for cleanup",
                display_database_url(&self.admin_database_url)
            )
        })?;
        let drop_database_sql = format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            quote_ident(&self.database_name)
        );
        admin_pool
            .execute(drop_database_sql.as_str())
            .await
            .with_context(|| {
                format!("failed to drop test database {}", self.database_name)
            })?;
        admin_pool.close().await;
        Ok(())
    }
}

struct TempPostgres {
    child: Child,
    admin_database_url: String,
    log_path: PathBuf,
    _temp_dir: TempDir,
}

impl TempPostgres {
    async fn start(requested_admin_url: &str) -> Result<Self> {
        let requested_url =
            Url::parse(requested_admin_url).with_context(|| {
                format!("invalid DATABASE_URL: {requested_admin_url}")
            })?;
        if !is_loopback_postgres_url(&requested_url) {
            bail!(
                "DATABASE_URL {} is unreachable and does not point at localhost; refusing to start a temporary replacement server",
                display_database_url(requested_admin_url)
            );
        }

        let username = if requested_url.username().is_empty() {
            "postgres"
        } else {
            requested_url.username()
        };
        let temp_dir = TempDir::new()
            .context("failed to create temporary PostgreSQL directory")?;
        let data_dir = temp_dir.path().join("data");
        let socket_dir = temp_dir.path().join("socket");
        fs::create_dir(&socket_dir)
            .context("failed to create temporary PostgreSQL socket dir")?;

        let initdb = Command::new("initdb")
            .arg("-D")
            .arg(&data_dir)
            .arg("--username")
            .arg(username)
            .arg("--auth=trust")
            .arg("--encoding=UTF8")
            .arg("--no-locale")
            .output()
            .context("failed to execute initdb; run tests inside `nix develop .#ferrex-player`")?;
        if !initdb.status.success() {
            bail!(
                "initdb failed with status {}\nstdout:\n{}\nstderr:\n{}",
                initdb.status,
                String::from_utf8_lossy(&initdb.stdout),
                String::from_utf8_lossy(&initdb.stderr)
            );
        }

        let port = free_local_port()?;
        let log_path = temp_dir.path().join("postgres.log");
        let log = File::create(&log_path)
            .context("failed to create temporary PostgreSQL log")?;
        let child = Command::new("postgres")
            .arg("-D")
            .arg(&data_dir)
            .arg("-h")
            .arg("127.0.0.1")
            .arg("-p")
            .arg(port.to_string())
            .arg("-k")
            .arg(&socket_dir)
            .arg("-c")
            .arg("listen_addresses=127.0.0.1")
            .arg("-c")
            .arg("fsync=off")
            .arg("-c")
            .arg("synchronous_commit=off")
            .arg("-c")
            .arg("full_page_writes=off")
            .stdout(Stdio::from(
                log.try_clone()
                    .context("failed to clone temporary PostgreSQL log handle")?,
            ))
            .stderr(Stdio::from(log))
            .spawn()
            .context("failed to execute postgres; run tests inside `nix develop .#ferrex-player`")?;

        let admin_database_url = temp_admin_database_url(&requested_url, port)?;
        let mut server = Self {
            child,
            admin_database_url,
            log_path,
            _temp_dir: temp_dir,
        };
        server.wait_until_ready().await?;
        Ok(server)
    }

    async fn wait_until_ready(&mut self) -> Result<()> {
        let started_at = Instant::now();
        loop {
            if let Some(status) = self
                .child
                .try_wait()
                .context("failed to poll temporary PostgreSQL process")?
            {
                bail!(
                    "temporary PostgreSQL exited during startup with {status}\n{}",
                    self.formatted_log()
                );
            }

            if let Ok(pool) = connect_pool(
                &self.admin_database_url,
                1,
                Duration::from_millis(250),
            )
            .await
            {
                pool.close().await;
                return Ok(());
            }

            if started_at.elapsed() > TEMP_POSTGRES_START_TIMEOUT {
                bail!(
                    "timed out waiting for temporary PostgreSQL to accept connections\n{}",
                    self.formatted_log()
                );
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    fn formatted_log(&self) -> String {
        match fs::read_to_string(&self.log_path) {
            Ok(log) if !log.trim().is_empty() => {
                format!("postgres log:\n{log}")
            }
            Ok(_) => "postgres log was empty".to_owned(),
            Err(error) => format!("failed to read postgres log: {error}"),
        }
    }
}

impl Drop for TempPostgres {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn connect_pool(
    database_url: &str,
    max_connections: u32,
    timeout: Duration,
) -> Result<PgPool> {
    let connect_options = PgConnectOptions::from_str(database_url)
        .with_context(|| format!("invalid PostgreSQL URL: {database_url}"))?;

    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(timeout)
        .connect_with(connect_options)
        .await
        .with_context(|| {
            format!(
                "failed to connect to {}",
                display_database_url(database_url)
            )
        })
}

fn database_url_with_database(
    admin_database_url: &str,
    database_name: &str,
) -> Result<String> {
    let mut url = Url::parse(admin_database_url).with_context(|| {
        format!("invalid PostgreSQL URL: {admin_database_url}")
    })?;
    url.set_path(&format!("/{database_name}"));
    Ok(url.to_string())
}

fn temp_admin_database_url(requested_url: &Url, port: u16) -> Result<String> {
    let mut url = requested_url.clone();
    url.set_host(Some("127.0.0.1"))
        .context("failed to set temporary PostgreSQL host")?;
    url.set_port(Some(port)).map_err(|_| {
        anyhow::anyhow!("failed to set temporary PostgreSQL port")
    })?;
    url.set_path("/postgres");
    Ok(url.to_string())
}

fn unique_test_database_name() -> String {
    format!(
        "ferrex_test_{}_{}",
        std::process::id(),
        Uuid::new_v4().simple()
    )
}

fn quote_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn is_loopback_postgres_url(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some("127.0.0.1" | "localhost" | "::1") | None
    )
}

fn free_local_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .context("failed to reserve a temporary PostgreSQL port")?;
    let port = listener
        .local_addr()
        .context("failed to read temporary PostgreSQL listener address")?
        .port();
    drop(listener);
    Ok(port)
}

fn display_database_url(database_url: &str) -> String {
    match Url::parse(database_url) {
        Ok(mut url) => {
            if url.password().is_some() {
                let _ = url.set_password(Some("****"));
            }
            url.to_string()
        }
        Err(_) => database_url.to_owned(),
    }
}
