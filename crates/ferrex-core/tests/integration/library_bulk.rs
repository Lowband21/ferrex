use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use tempfile::tempdir;
use uuid::Uuid;

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
use ferrex_core::database::repositories::{
    manifest::PostgresManifestRepository, media::PostgresMediaRepository,
};
use ferrex_core::domain::scan::actors::library::*;
use ferrex_core::domain::scan::{
    ManifestScope, ManifestWalkLimits, ManifestWalker, ScannerLayoutContract,
};
use ferrex_core::domain::scan::actors::metadata::{
    MediaReadyForIndex, MetadataActor, MetadataCommand,
};
use ferrex_core::domain::scan::orchestration::context::{
    ScanHierarchy, ScanNodeKind, SeriesHint, SeriesLink, SeriesRef,
    SeriesRootPath,
};
use ferrex_core::domain::scan::orchestration::correlation::CorrelationCache;
use ferrex_core::domain::scan::orchestration::dispatcher::{
    DefaultJobDispatcher, DefaultManifestScanExecutor, DispatchStatus,
    DispatcherActors, ManifestScanExecutor,
};
use ferrex_core::domain::scan::orchestration::job::{
    EnqueueRequest, ImageFetchJob, JobKind, JobPayload, ManifestScanJob,
    ManifestScanTrigger, MediaAnalyzeJob, MediaFingerprint, ScanReason,
};
use ferrex_core::domain::scan::orchestration::lease::DequeueRequest;
use ferrex_core::domain::scan::orchestration::persistence::{
    PostgresCursorRepository, PostgresQueueService,
};
use ferrex_core::domain::scan::orchestration::queue::QueueService;
use ferrex_core::domain::scan::orchestration::runtime::InProcJobEventBus;
use ferrex_core::domain::scan::orchestration::scan_cursor::normalize_path;
use ferrex_core::domain::scan::orchestration::series::{
    DefaultSeriesResolver, SeriesMetadataProvider, SeriesResolution,
};
use ferrex_core::domain::scan::orchestration::series_state::{
    InMemorySeriesScanStateRepository, SeriesScanStateRepository,
};
use ferrex_core::error::Result;
use ferrex_core::types::{
    LibraryId, LibraryReference, LibraryType, MediaID, SeriesID, VideoMediaType,
};

fn norm(path: &Path) -> String {
    normalize_path(path)
}

fn make_library(root: PathBuf, library_type: LibraryType) -> LibraryReference {
    LibraryReference {
        id: LibraryId(Uuid::now_v7()),
        name: "Bulk Test".into(),
        library_type,
        paths: vec![root],
    }
}

// Simple provider stubs to let dispatcher progress without external IO.
struct StubAnalyze;
#[async_trait]
impl MediaAnalyzeActor for StubAnalyze {
    async fn analyze(&self, command: MediaAnalyzeJob) -> Result<MediaAnalyzed> {
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
            thumbnails: vec![],
        })
    }
}

struct StubMetadata;
#[async_trait]
impl MetadataActor for StubMetadata {
    async fn enrich(
        &self,
        command: MetadataCommand,
    ) -> Result<MediaReadyForIndex> {
        Ok(MediaReadyForIndex {
            library_id: command.job.library_id,
            media_id: command.job.media_id,
            variant: command.job.variant,
            hierarchy: command.job.hierarchy.clone(),
            node: command.job.node.clone(),
            normalized_title: None,
            analyzed: command.analyzed,
            prepared_at: Utc::now(),
            image_jobs: Vec::new(),
        })
    }
}

struct StubIndexer;
#[async_trait]
impl IndexerActor for StubIndexer {
    async fn index(&self, command: IndexCommand) -> Result<IndexingOutcome> {
        Ok(IndexingOutcome {
            library_id: command.job.library_id,
            path_norm: command.job.path_norm,
            indexed_at: Utc::now(),
            upserted: true,
            media: None,
            media_id: command.ready.media_id,
            hierarchy: command.job.hierarchy,
            change: IndexingChange::Created,
        })
    }
}

struct StubImage;
#[async_trait]
impl ImageFetchActor for StubImage {
    async fn fetch(&self, _job: &ImageFetchJob) -> Result<()> {
        Ok(())
    }
}

struct StubSeriesProvider;

#[async_trait]
impl SeriesMetadataProvider for StubSeriesProvider {
    async fn resolve_series(
        &self,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
        hint: &SeriesHint,
        _folder_name: &str,
    ) -> Result<SeriesResolution> {
        let series_id = SeriesID(Uuid::now_v7());
        let series_ref = SeriesRef {
            id: series_id,
            slug: hint.slug.clone(),
            title: Some(hint.title.clone()),
        };
        let hierarchy = ScanHierarchy {
            library_type: Some(LibraryType::Series),
            series: Some(SeriesLink::Resolved(series_ref.clone())),
            series_root_path: Some(series_root_path.clone()),
            ..ScanHierarchy::default()
        };
        let analyzed = MediaAnalyzed {
            library_id,
            media_id: MediaID::Series(series_id),
            variant: VideoMediaType::Series,
            hierarchy: hierarchy.clone(),
            node: ScanNodeKind::SeriesRoot,
            path_norm: series_root_path.as_str().to_string(),
            fingerprint: MediaFingerprint::default(),
            analyzed_at: Utc::now(),
            analysis: AnalysisContext::default(),
            thumbnails: vec![],
        };
        let ready = MediaReadyForIndex {
            library_id,
            media_id: analyzed.media_id,
            variant: analyzed.variant,
            hierarchy: hierarchy.clone(),
            node: analyzed.node.clone(),
            normalized_title: Some(hint.title.clone()),
            analyzed,
            prepared_at: Utc::now(),
            image_jobs: Vec::new(),
        };
        Ok(SeriesResolution { series_ref, ready })
    }
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn bulk_seed_manifest_root_reconciles_media_followups(
    pool: PgPool,
) -> Result<()> {
    // Filesystem layout covers both folder and flat-root media:
    // root/
    //   flat.mkv
    //   X1/a.mkv
    //   X2/b.mkv
    let temp = tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let x1 = root.join("X1");
    let x2 = root.join("X2");
    tokio::fs::create_dir_all(&x1).await?;
    tokio::fs::create_dir_all(&x2).await?;
    let flat = root.join("flat.mkv");
    let x1_media = x1.join("a.mkv");
    let x2_media = x2.join("b.mkv");
    tokio::fs::write(&flat, b"flat").await?;
    tokio::fs::write(&x1_media, b"test").await?;
    tokio::fs::write(&x2_media, b"test").await?;

    let queue = Arc::new(PostgresQueueService::new(pool.clone()).await?);
    let events = Arc::new(InProcJobEventBus::new(128));
    let observer = Arc::new(NoopActorObserver);
    let correlations = CorrelationCache::default();

    let library = make_library(root.clone(), LibraryType::Movies);
    let config = LibraryActorConfig {
        library: library.clone(),
        root_paths: vec![root.clone()],
        max_outstanding_jobs: 10_000,
    };
    let mut actor = DefaultLibraryActor::new(
        config,
        Arc::clone(&queue),
        observer,
        Arc::clone(&events),
        correlations.clone(),
    );

    let correlation_id = Uuid::now_v7();
    let actor_events = actor
        .handle_command(LibraryActorCommand::Start {
            mode: StartMode::Bulk,
            correlation_id: Some(correlation_id),
        })
        .await?;

    let manifest_events = actor_events
        .into_iter()
        .filter_map(|event| match event {
            LibraryActorEvent::EnqueueManifestScan {
                scope,
                priority,
                reason,
                trigger,
                correlation_id,
            } => Some((scope, priority, reason, trigger, correlation_id)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(manifest_events.len(), 1);
    let (scope, priority, reason, trigger, observed_correlation) =
        manifest_events.into_iter().next().unwrap();
    assert_eq!(priority, JobPriority::P1);
    assert_eq!(reason, ScanReason::BulkSeed);
    assert_eq!(trigger, ManifestScanTrigger::BulkStart);
    assert_eq!(observed_correlation, Some(correlation_id));
    let ManifestScope::Root(root_scope) = scope.as_ref() else {
        panic!("bulk start should enqueue a manifest root scan")
    };
    assert_eq!(root_scope.root_path_norm, norm(&root));

    let payload = JobPayload::ManifestScan(ManifestScanJob {
        scope: *scope,
        scan_reason: reason,
        enqueue_time: Utc::now(),
        trigger,
    });
    let mut request = EnqueueRequest::new(priority, payload);
    request.correlation_id = observed_correlation;
    queue.enqueue(request).await?;

    let actors = DispatcherActors::new(
        Arc::new(DefaultFolderScanActor::new()) as Arc<dyn FolderScanActor>,
        Arc::new(StubAnalyze) as Arc<dyn MediaAnalyzeActor>,
        Arc::new(StubMetadata) as Arc<dyn MetadataActor>,
        Arc::new(StubIndexer) as Arc<dyn IndexerActor>,
        Arc::new(StubImage) as Arc<dyn ImageFetchActor>,
    );

    let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));
    let series_states = Arc::new(InMemorySeriesScanStateRepository::default());
    let series_resolver = Arc::new(DefaultSeriesResolver::new(
        Arc::new(StubSeriesProvider) as Arc<dyn SeriesMetadataProvider>,
        Arc::clone(&series_states) as Arc<dyn SeriesScanStateRepository>,
    ));
    let manifest_repo =
        Arc::new(PostgresManifestRepository::new(pool.clone()));
    let manifest_media =
        Arc::new(PostgresMediaRepository::new(pool.clone()));
    let manifest_executor: Arc<dyn ManifestScanExecutor> = Arc::new(
        DefaultManifestScanExecutor::new(
            ManifestWalker::new(
                ScannerLayoutContract::default(),
                ManifestWalkLimits::default(),
            ),
            manifest_repo,
            manifest_media,
            Arc::clone(&queue),
            Arc::clone(&events),
            Arc::clone(&cursors),
        ),
    );

    let dispatcher = Arc::new(
        DefaultJobDispatcher::new(
            Arc::clone(&queue),
            Arc::clone(&events),
            Arc::clone(&cursors),
            Arc::clone(&series_states),
            series_resolver,
            actors,
            CorrelationCache::default(),
        )
        .with_manifest_executor(manifest_executor),
    );

    let lease = queue
        .dequeue(DequeueRequest {
            kind: JobKind::ManifestScan,
            worker_id: "it-test".into(),
            lease_ttl: chrono::Duration::seconds(30),
            selector: None,
        })
        .await?
        .expect("expected a manifest scan job to be queued");

    let status = dispatcher.dispatch(&lease).await;
    assert!(matches!(status, DispatchStatus::Success));
    queue.complete(lease.lease_id).await?;

    let analyze_rows = sqlx::query!(
        r#"
        SELECT payload
        FROM orchestrator_jobs
        WHERE library_id = $1 AND kind = $2 AND state = 'ready'
        ORDER BY created_at ASC
        "#,
        library.id.0,
        JobKind::MediaAnalyze as i16,
    )
    .fetch_all(&pool)
    .await?;

    let mut analyze_paths = BTreeSet::new();
    for row in analyze_rows {
        let payload: JobPayload = serde_json::from_value(row.payload)?;
        if let JobPayload::MediaAnalyze(job) = payload {
            analyze_paths.insert(job.path_norm);
        }
    }

    assert_eq!(analyze_paths.len(), 3);
    assert!(analyze_paths.contains(&norm(&flat)));
    assert!(analyze_paths.contains(&norm(&x1_media)));
    assert!(analyze_paths.contains(&norm(&x2_media)));

    Ok(())
}
