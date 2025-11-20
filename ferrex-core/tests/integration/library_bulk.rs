use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use tempfile::tempdir;
use uuid::Uuid;

use ferrex_core::scan::orchestration::actors::folder::{DefaultFolderScanActor, FolderScanActor};
use ferrex_core::scan::orchestration::actors::library::*;
use ferrex_core::scan::orchestration::actors::messages::ParentDescriptors;
use ferrex_core::scan::orchestration::actors::pipeline::*;
use ferrex_core::scan::orchestration::correlation::CorrelationCache;
use ferrex_core::scan::orchestration::dispatcher::DefaultJobDispatcher;
use ferrex_core::scan::orchestration::events::EventBus;
use ferrex_core::scan::orchestration::job::{JobKind, JobPayload, JobPriority, MediaAnalyzeJob, MediaFingerprint};
use ferrex_core::scan::orchestration::lease::{DequeueRequest, LeaseRenewal};
use ferrex_core::scan::orchestration::persistence::{PostgresCursorRepository, PostgresQueueService};
use ferrex_core::scan::orchestration::queue::QueueService;
use ferrex_core::scan::orchestration::runtime::InProcJobEventBus;
use ferrex_core::{LibraryID, LibraryReference, LibraryType, Result};

fn norm(path: &Path) -> String {
    ferrex_core::scan::orchestration::scan_cursor::normalize_path(path)
}

fn make_library(root: PathBuf, library_type: LibraryType) -> LibraryReference {
    LibraryReference {
        id: LibraryID(Uuid::now_v7()),
        name: "Bulk Test".into(),
        library_type,
        paths: vec![root],
    }
}

// Simple pipeline stubs to let dispatcher progress without external IO.
struct StubAnalyze;
#[async_trait]
impl MediaAnalyzeActor for StubAnalyze {
    async fn analyze(&self, command: MediaAnalyzeCommand) -> Result<MediaAnalyzed> {
        Ok(MediaAnalyzed {
            library_id: command.job.library_id,
            path_norm: command.job.path_norm,
            fingerprint: command.job.fingerprint,
            analyzed_at: Utc::now(),
            streams_json: serde_json::json!({"ok": true}),
            thumbnails: vec![],
            context: command.job.context,
        })
    }
}

struct StubMetadata;
#[async_trait]
impl MetadataActor for StubMetadata {
    async fn enrich(&self, command: MetadataCommand) -> Result<MediaReadyForIndex> {
        Ok(MediaReadyForIndex {
            library_id: command.job.library_id,
            logical_id: Some(command.job.logical_candidate_id.clone()),
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
            media_id: None,
            change: IndexingChange::Created,
        })
    }
}

struct StubImage;
#[async_trait]
impl ImageFetchActor for StubImage {
    async fn fetch(&self, _command: ImageFetchCommand) -> Result<()> {
        Ok(())
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
    let rows = sqlx::query!(
        r#"
        SELECT payload
        FROM orchestrator_jobs
        WHERE kind = 'scan' AND state = 'ready'
        ORDER BY created_at ASC
        "#
    )
    .fetch_all(&pool)
    .await?;

    let expect1 = norm(&x1);
    let expect2 = norm(&x2);
    let mut seen = Vec::new();
    for row in rows.iter() {
        let payload: serde_json::Value = row.payload.clone();
        let folder = payload["payload"]["folder_path_norm"].as_str().unwrap_or("").to_string();
        seen.push(folder);
    }
    assert!(seen.contains(&expect1), "X1 must be enqueued at depth-1");
    assert!(seen.contains(&expect2), "X2 must be enqueued at depth-1");

    // Dispatcher with real folder actor to recurse and enqueue child scans
    let actors = ferrex_core::scan::orchestration::dispatcher::DispatcherActors::new(
        Arc::new(DefaultFolderScanActor::new()) as Arc<dyn FolderScanActor>,
        Arc::new(StubAnalyze) as Arc<dyn MediaAnalyzeActor>,
        Arc::new(StubMetadata) as Arc<dyn MetadataActor>,
        Arc::new(StubIndexer) as Arc<dyn IndexerActor>,
        Arc::new(StubImage) as Arc<dyn ImageFetchActor>,
    );

    let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));

    let dispatcher = Arc::new(DefaultJobDispatcher::new(
        Arc::clone(&queue),
        Arc::clone(&events),
        Arc::clone(&cursors),
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
    assert!(matches!(status, ferrex_core::scan::orchestration::dispatcher::DispatchStatus::Success));
    // Mark completed
    queue.complete(lease.lease_id).await?;

    // Follow-up should include a child folder scan (X1/child or X2/child)
    let child1 = norm(&x1_child);
    let child2 = norm(&x2_child);
    let children = sqlx::query!(
        r#"
        SELECT payload
        FROM orchestrator_jobs
        WHERE kind = 'scan'
        "#
    )
    .fetch_all(&pool)
    .await?;

    let mut child_seen = false;
    for row in children.iter() {
        let payload: serde_json::Value = row.payload.clone();
        let folder = payload["payload"]["folder_path_norm"].as_str().unwrap_or("");
        if folder == child1 || folder == child2 {
            child_seen = true;
            break;
        }
    }
    assert!(child_seen, "expected a child subfolder scan to be enqueued by dispatcher");

    Ok(())
}
