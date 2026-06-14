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
use ferrex_core::domain::scan::actors::library::*;
use ferrex_core::domain::scan::actors::metadata::{
    MediaReadyForIndex, MetadataActor, MetadataCommand,
};
use ferrex_core::domain::scan::orchestration::context::{
    ScanHierarchy, ScanNodeKind, SeriesHint, SeriesLink, SeriesRef,
    SeriesRootPath,
};
use ferrex_core::domain::scan::orchestration::correlation::CorrelationCache;
use ferrex_core::domain::scan::orchestration::dispatcher::{
    DefaultJobDispatcher, DispatchStatus, DispatcherActors,
};
use ferrex_core::domain::scan::orchestration::job::{
    ImageFetchJob, JobKind, MediaAnalyzeJob, MediaFingerprint,
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
async fn bulk_seed_depth1_and_recursive_followups(pool: PgPool) -> Result<()> {
    // Filesystem layout
    // root/
    //   X1/
    //     child/
    //     a.mkv
    //   X2/
    //     child/
    //     b.mkv
    let temp = tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let x1 = root.join("X1");
    let x2 = root.join("X2");
    let x1_child = x1.join("child");
    let x2_child = x2.join("child");
    tokio::fs::create_dir_all(&x1_child).await?;
    tokio::fs::create_dir_all(&x2_child).await?;
    tokio::fs::write(x1.join("a.mkv"), b"test").await?;
    tokio::fs::write(x2.join("b.mkv"), b"test").await?;

    // Wiring
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

    // Start bulk seed => depth-1 folder scan jobs only (X1, X2)
    let _ = actor
        .handle_command(LibraryActorCommand::Start {
            mode: StartMode::Bulk,
            correlation_id: None,
        })
        .await?;

    // Verify persistent queue contains only depth-1 immediate subfolders
    let folder_kind = JobKind::FolderScan as i16;
    let rows = sqlx::query!(
        r#"
        SELECT payload
        FROM orchestrator_jobs
        WHERE kind = $1 AND state = 'ready'
        ORDER BY created_at ASC
        "#,
        folder_kind
    )
    .fetch_all(&pool)
    .await?;

    let expect1 = norm(&x1);
    let expect2 = norm(&x2);
    let mut seen = Vec::new();
    for row in rows.iter() {
        let payload: serde_json::Value = row.payload.clone();
        let folder = payload["payload"]["folder_path_norm"]
            .as_str()
            .unwrap_or("")
            .to_string();
        seen.push(folder);
    }
    assert!(seen.contains(&expect1), "X1 must be enqueued at depth-1");
    assert!(seen.contains(&expect2), "X2 must be enqueued at depth-1");

    // Dispatcher with real folder actor to recurse and enqueue child scans
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

    let dispatcher = Arc::new(DefaultJobDispatcher::new(
        Arc::clone(&queue),
        Arc::clone(&events),
        Arc::clone(&cursors),
        Arc::clone(&series_states),
        series_resolver,
        actors,
        CorrelationCache::default(),
    ));

    // Dequeue and dispatch one folder scan (either X1 or X2)
    let lease = queue
        .dequeue(DequeueRequest {
            kind: JobKind::FolderScan,
            worker_id: "it-test".into(),
            lease_ttl: chrono::Duration::seconds(30),
            selector: None,
        })
        .await?
        .expect("expected a folder scan job to be queued");

    let status = dispatcher.dispatch(&lease).await;
    assert!(matches!(status, DispatchStatus::Success));
    // Mark completed
    queue.complete(lease.lease_id).await?;

    // Follow-up should include a child folder scan (X1/child or X2/child)
    let child1 = norm(&x1_child);
    let child2 = norm(&x2_child);
    let children = sqlx::query!(
        r#"
        SELECT payload
        FROM orchestrator_jobs
        WHERE kind = $1
        "#,
        folder_kind
    )
    .fetch_all(&pool)
    .await?;

    let mut child_seen = false;
    for row in children.iter() {
        let payload: serde_json::Value = row.payload.clone();
        let folder = payload["payload"]["folder_path_norm"]
            .as_str()
            .unwrap_or("");
        if folder == child1 || folder == child2 {
            child_seen = true;
            break;
        }
    }
    assert!(
        child_seen,
        "expected a child subfolder scan to be enqueued by dispatcher"
    );

    Ok(())
}
