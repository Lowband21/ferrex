#![cfg(all(feature = "database", feature = "scan-runtime"))]
//! DB-backed end-to-end manifest scanner coverage using temporary media trees.

use std::{collections::BTreeMap, future::Future, path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use ferrex_core::{
    database::repositories::{
        manifest::PostgresManifestRepository, media::PostgresMediaRepository,
    },
    domain::scan::{
        DefaultLibraryActor, FileSystemEvent, FileSystemEventKind,
        LibraryActor, LibraryActorCommand, LibraryActorConfig,
        LibraryActorEvent, LibraryRootsId, ManifestRootId, ManifestRootScope,
        ManifestScanExecutor, ManifestScanJob, ManifestScanTrigger,
        ManifestScope, ManifestWalkLimits, ManifestWalker, NoopActorObserver,
        ScannerLayoutContract,
        orchestration::{
            CorrelationCache, EnqueueRequest, InProcJobEventBus, JobKind,
            JobPayload, PostgresCursorRepository, PostgresQueueService,
            QueueService, ScanReason, scan_cursor::normalize_path,
        },
    },
    types::{LibraryId, LibraryReference, LibraryType},
};
use sqlx::{Executor, PgPool, postgres::PgPoolOptions};
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;

struct TestDatabase {
    admin_database_url: String,
    database_name: String,
    pool: PgPool,
}

impl TestDatabase {
    async fn create_if_configured() -> Result<Option<Self>> {
        let Ok(admin_database_url) = std::env::var("DATABASE_URL") else {
            eprintln!(
                "skipping manifest_scan_e2e DB-backed tests: DATABASE_URL is not set"
            );
            return Ok(None);
        };

        let database_name =
            format!("ferrex_manifest_e2e_{}", Uuid::now_v7().simple());
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_database_url)
            .await
            .with_context(|| {
                format!(
                    "failed to connect to admin database {}",
                    display_database_url(&admin_database_url)
                )
            })?;

        admin_pool
            .execute(
                format!("CREATE DATABASE {}", quote_ident(&database_name))
                    .as_str(),
            )
            .await
            .with_context(|| {
                format!("failed to create test database {database_name}")
            })?;
        admin_pool.close().await;

        let database_url =
            database_url_with_database(&admin_database_url, &database_name)?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .with_context(|| {
                format!(
                    "failed to connect to isolated test database {}",
                    display_database_url(&database_url)
                )
            })?;
        ferrex_core::MIGRATOR
            .run(&pool)
            .await
            .context("failed to migrate isolated test database")?;

        Ok(Some(Self {
            admin_database_url,
            database_name,
            pool,
        }))
    }

    async fn cleanup(self) -> Result<()> {
        self.pool.close().await;
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.admin_database_url)
            .await
            .with_context(|| {
                format!(
                    "failed to reconnect to admin database {} for cleanup",
                    display_database_url(&self.admin_database_url)
                )
            })?;

        let drop_result = admin_pool
            .execute(
                format!(
                    "DROP DATABASE IF EXISTS {} WITH (FORCE)",
                    quote_ident(&self.database_name)
                )
                .as_str(),
            )
            .await
            .with_context(|| {
                format!("failed to drop test database {}", self.database_name)
            });
        admin_pool.close().await;
        drop_result.map(|_| ())
    }
}

async fn with_database<F, Fut>(test: F) -> Result<()>
where
    F: FnOnce(PgPool) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let Some(db) = TestDatabase::create_if_configured().await? else {
        return Ok(());
    };

    let pool = db.pool.clone();
    let test_result = test(pool).await;
    let cleanup_result = db.cleanup().await;
    test_result?;
    cleanup_result
}

#[tokio::test]
async fn manifest_scan_e2e_discovers_layouts_and_persists_diagnostics()
-> Result<()> {
    with_database(|pool| async move {
        let movie_root = TempDir::new()?;
        let series_root = TempDir::new()?;

        let flat_movie = movie_root.path().join("Alien.mkv");
        let folder_movie_dir = movie_root.path().join("Blade Runner (1982)");
        let folder_movie = folder_movie_dir.join("Blade Runner.mkv");
        let nested_movie_dir =
            movie_root.path().join("Nested Movie").join("Disc 1");
        let nested_movie = nested_movie_dir.join("Nested Movie.mkv");
        let extras_dir = folder_movie_dir.join("Extras");
        let movie_extra = extras_dir.join("Trailer.mkv");
        tokio::fs::create_dir_all(&folder_movie_dir).await?;
        tokio::fs::create_dir_all(&nested_movie_dir).await?;
        tokio::fs::create_dir_all(&extras_dir).await?;
        tokio::fs::write(&flat_movie, b"flat").await?;
        tokio::fs::write(&folder_movie, b"folder").await?;
        tokio::fs::write(&nested_movie, b"nested").await?;
        tokio::fs::write(&movie_extra, b"extra").await?;

        let series_dir = series_root.path().join("Fringe");
        let season_dir = series_dir.join("Season 01");
        let specials_dir = series_dir.join("Specials");
        let season_episode = season_dir.join("S01E01 - Pilot.mkv");
        let special_episode = specials_dir.join("S00E01 - Special.mkv");
        let direct_episode = series_dir.join("S01E02 - Same Old Story.mkv");
        let unparseable_episode = season_dir.join("Episode Without Number.mkv");
        tokio::fs::create_dir_all(&season_dir).await?;
        tokio::fs::create_dir_all(&specials_dir).await?;
        tokio::fs::write(&season_episode, b"episode").await?;
        tokio::fs::write(&special_episode, b"special").await?;
        tokio::fs::write(&direct_episode, b"direct").await?;
        tokio::fs::write(&unparseable_episode, b"bad").await?;

        let movie_library = insert_library(
            &pool,
            "Manifest E2E Movies",
            LibraryType::Movies,
            movie_root.path(),
        )
        .await?;
        let series_library = insert_library(
            &pool,
            "Manifest E2E Series",
            LibraryType::Series,
            series_root.path(),
        )
        .await?;

        let movie_summary = run_manifest_scan(
            &pool,
            movie_library,
            LibraryType::Movies,
            movie_root.path(),
            ScanReason::BulkSeed,
            ManifestScanTrigger::BulkStart,
        )
        .await?;
        let series_summary = run_manifest_scan(
            &pool,
            series_library,
            LibraryType::Series,
            series_root.path(),
            ScanReason::BulkSeed,
            ManifestScanTrigger::BulkStart,
        )
        .await?;

        assert_eq!(movie_summary.supported_media_seen, 2);
        assert_eq!(series_summary.supported_media_seen, 3);
        assert!(movie_summary.diagnostics_seen >= 2);
        assert!(series_summary.diagnostics_seen >= 1);

        let movie_supported_files =
            count_manifest_entries(&pool, movie_library, "file", "supported")
                .await?;
        let series_supported_files =
            count_manifest_entries(&pool, series_library, "file", "supported")
                .await?;
        assert_eq!(movie_supported_files, 2);
        assert_eq!(series_supported_files, 3);

        let diagnostic_counts = diagnostic_counts_by_code(&pool).await?;
        assert!(
            diagnostic_counts
                .get("scanner.layout.movie_nested_folder_unsupported")
                .copied()
                .unwrap_or_default()
                >= 1
        );
        assert!(
            diagnostic_counts
                .get("scanner.layout.movie_extras_unsupported")
                .copied()
                .unwrap_or_default()
                >= 1
        );
        assert!(
            diagnostic_counts
                .get("scanner.layout.series_episode_parse_failed")
                .copied()
                .unwrap_or_default()
                >= 1
        );

        let analyze_jobs =
            count_ready_jobs(&pool, JobKind::MediaAnalyze).await?;
        assert_eq!(analyze_jobs, 5);

        Ok(())
    })
    .await
}

#[tokio::test]
async fn manifest_scan_e2e_reconciles_moves_and_deletes() -> Result<()> {
    with_database(|pool| async move {
        let movie_root = TempDir::new()?;
        let original = movie_root.path().join("Original.mkv");
        let renamed = movie_root.path().join("Renamed.mkv");
        tokio::fs::write(&original, b"same bytes").await?;

        let library_id = insert_library(
            &pool,
            "Manifest E2E Reconcile",
            LibraryType::Movies,
            movie_root.path(),
        )
        .await?;
        let first = run_manifest_scan(
            &pool,
            library_id,
            LibraryType::Movies,
            movie_root.path(),
            ScanReason::BulkSeed,
            ManifestScanTrigger::BulkStart,
        )
        .await?;
        assert_eq!(first.supported_media_seen, 1);

        let original_norm = norm(&original)?;
        let renamed_norm = norm(&renamed)?;
        seed_media_file_from_manifest(&pool, library_id, &original_norm)
            .await?;

        tokio::fs::rename(&original, &renamed).await?;
        let moved = run_manifest_scan(
            &pool,
            library_id,
            LibraryType::Movies,
            movie_root.path(),
            ScanReason::MaintenanceSweep,
            ManifestScanTrigger::Maintenance,
        )
        .await?;
        assert_eq!(moved.media_moved, 1);
        assert_eq!(
            current_media_path(&pool, library_id).await?,
            Some(renamed_norm.clone())
        );

        tokio::fs::remove_file(&renamed).await?;
        let deleted = run_manifest_scan(
            &pool,
            library_id,
            LibraryType::Movies,
            movie_root.path(),
            ScanReason::MaintenanceSweep,
            ManifestScanTrigger::Maintenance,
        )
        .await?;
        assert!(deleted.manifest_entries_marked_missing >= 1);
        assert!(deleted.media_tombstoned >= 1);
        assert_eq!(
            media_available(&pool, library_id, &renamed_norm).await?,
            Some(false)
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn manifest_scan_e2e_routes_active_watch_events_and_overflow()
-> Result<()> {
    with_database(|pool| async move {
        let root = TempDir::new()?;
        let library_id = insert_library(
            &pool,
            "Manifest E2E Watch",
            LibraryType::Movies,
            root.path(),
        )
        .await?;
        let library = LibraryReference {
            id: library_id,
            name: "Manifest E2E Watch".to_string(),
            library_type: LibraryType::Movies,
            paths: vec![root.path().to_path_buf()],
        };
        let queue = Arc::new(PostgresQueueService::new(pool.clone()).await?);
        let events = Arc::new(InProcJobEventBus::new(128));
        let observer = Arc::new(NoopActorObserver);
        let mut actor = DefaultLibraryActor::new(
            LibraryActorConfig {
                library,
                root_paths: vec![root.path().to_path_buf()],
                max_outstanding_jobs: 256,
            },
            Arc::clone(&queue),
            observer,
            Arc::clone(&events),
            CorrelationCache::default(),
        );

        let correlation_id = Uuid::now_v7();
        let start_events = actor
            .handle_command(LibraryActorCommand::Start {
                mode: ferrex_core::domain::scan::StartMode::Bulk,
                correlation_id: Some(correlation_id),
            })
            .await?;
        enqueue_manifest_events(&queue, start_events).await?;

        let changed_dir = root.path().join("Incoming");
        let changed_file = changed_dir.join("Fresh.mkv");
        tokio::fs::create_dir_all(&changed_dir).await?;
        tokio::fs::write(&changed_file, b"fresh").await?;

        let changed_dir_norm = norm(&changed_dir)?;
        let watch_events = vec![
            watch_event(
                library_id,
                &changed_file,
                FileSystemEventKind::Created,
            )?,
            watch_event(
                library_id,
                root.path(),
                FileSystemEventKind::Overflow,
            )?,
        ];
        let routed = actor
            .handle_command(LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events: watch_events,
                correlation_id: None,
            })
            .await?;

        let mut saw_hot_partition = false;
        let mut saw_overflow_root = false;
        for event in &routed {
            if let LibraryActorEvent::EnqueueManifestScan {
                scope,
                priority,
                reason,
                trigger,
                correlation_id: observed_correlation,
            } = event
            {
                assert_eq!(
                    *priority,
                    ferrex_core::domain::scan::JobPriority::P0
                );
                assert_eq!(*observed_correlation, Some(correlation_id));
                match (scope.as_ref(), reason, trigger) {
                    (
                        ManifestScope::Partition(partition),
                        ScanReason::HotChange,
                        ManifestScanTrigger::WatchChange,
                    ) => {
                        assert_eq!(
                            partition.prefix_norm.as_deref(),
                            Some(changed_dir_norm.as_str())
                        );
                        saw_hot_partition = true;
                    }
                    (
                        ManifestScope::Root(_),
                        ScanReason::WatcherOverflow,
                        ManifestScanTrigger::WatchOverflow,
                    ) => {
                        saw_overflow_root = true;
                    }
                    _ => {}
                }
            }
        }
        assert!(
            saw_hot_partition,
            "active file event should route to a manifest partition scan"
        );
        assert!(
            saw_overflow_root,
            "watch overflow should route to a root manifest scan"
        );

        enqueue_manifest_events(&queue, routed).await?;
        assert_eq!(queue.queue_depth(JobKind::ManifestScan).await?, 3);

        Ok(())
    })
    .await
}

async fn run_manifest_scan(
    pool: &PgPool,
    library_id: LibraryId,
    library_type: LibraryType,
    root: &Path,
    reason: ScanReason,
    trigger: ManifestScanTrigger,
) -> Result<ferrex_core::domain::scan::ManifestReconciliationSummary> {
    let queue = Arc::new(PostgresQueueService::new(pool.clone()).await?);
    let events = Arc::new(InProcJobEventBus::new(128));
    let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));
    let manifest_repo = Arc::new(PostgresManifestRepository::new(pool.clone()));
    let manifest_media = Arc::new(PostgresMediaRepository::new(pool.clone()));
    let executor = ferrex_core::domain::scan::DefaultManifestScanExecutor::new(
        ManifestWalker::new(
            ScannerLayoutContract::default(),
            ManifestWalkLimits::default(),
        ),
        manifest_repo,
        manifest_media,
        queue,
        events,
        cursors,
    );

    let root_path_norm = norm(root)?;
    executor
        .execute(&ManifestScanJob {
            scope: ManifestScope::Root(ManifestRootScope {
                library_id,
                library_type,
                root_id: ManifestRootId(0),
                root_path_norm,
            }),
            scan_reason: reason,
            enqueue_time: Utc::now(),
            trigger,
        })
        .await
        .map_err(Into::into)
}

async fn enqueue_manifest_events(
    queue: &Arc<PostgresQueueService>,
    events: Vec<LibraryActorEvent>,
) -> Result<()> {
    for event in events {
        if let LibraryActorEvent::EnqueueManifestScan {
            scope,
            priority,
            reason,
            trigger,
            correlation_id,
        } = event
        {
            let mut request = EnqueueRequest::new(
                priority,
                JobPayload::ManifestScan(ManifestScanJob {
                    scope: *scope,
                    scan_reason: reason,
                    enqueue_time: Utc::now(),
                    trigger,
                }),
            );
            request.correlation_id = correlation_id;
            queue.enqueue(request).await?;
        }
    }
    Ok(())
}

async fn insert_library(
    pool: &PgPool,
    name: &str,
    library_type: LibraryType,
    root: &Path,
) -> Result<LibraryId> {
    let id = Uuid::now_v7();
    let root_norm = norm(root)?;
    let library_type = match library_type {
        LibraryType::Movies => "movies",
        LibraryType::Series => "tvshows",
    };
    let paths = vec![root_norm];
    sqlx::query!(
        r#"
        INSERT INTO libraries (
            id, name, library_type, paths, scan_interval_minutes, last_scan,
            enabled, auto_scan, watch_for_changes, analyze_on_scan
        )
        VALUES ($1, $2, $3, $4, 60, NULL, true, true, true, false)
        "#,
        id,
        name,
        library_type,
        &paths[..]
    )
    .execute(pool)
    .await?;
    Ok(LibraryId(id))
}

async fn count_manifest_entries(
    pool: &PgPool,
    library_id: LibraryId,
    entry_kind: &str,
    classification_status: &str,
) -> Result<i64> {
    sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::bigint AS "count!"
        FROM manifest_entries
        WHERE library_id = $1
          AND entry_kind = $2
          AND classification_status = $3
        "#,
        library_id.0,
        entry_kind,
        classification_status
    )
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

async fn diagnostic_counts_by_code(
    pool: &PgPool,
) -> Result<BTreeMap<String, i64>> {
    let rows = sqlx::query!(
        r#"
        SELECT code AS "code!", COUNT(*)::bigint AS "count!"
        FROM manifest_diagnostics
        GROUP BY code
        "#
    )
    .fetch_all(pool)
    .await?;
    let mut counts = BTreeMap::new();
    for row in rows {
        counts.insert(row.code, row.count);
    }
    Ok(counts)
}

async fn count_ready_jobs(pool: &PgPool, kind: JobKind) -> Result<i64> {
    sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::bigint AS "count!"
        FROM orchestrator_jobs
        WHERE kind = $1 AND state = 'ready'
        "#,
        kind as i16
    )
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

async fn seed_media_file_from_manifest(
    pool: &PgPool,
    library_id: LibraryId,
    path_norm: &str,
) -> Result<()> {
    let inserted = sqlx::query!(
        r#"
        INSERT INTO media_files (
            library_id, media_id, media_type, file_path, filename, file_size,
            is_available, fingerprint_device_id, fingerprint_inode,
            fingerprint_size, fingerprint_mtime_ms, fingerprint_weak_hash
        )
        SELECT
            library_id,
            uuidv7(),
            'movie'::media_type,
            path_norm,
            split_part(path_norm, '/', array_length(string_to_array(path_norm, '/'), 1)),
            fingerprint_size,
            true,
            fingerprint_device_id,
            fingerprint_inode::bigint,
            fingerprint_size,
            fingerprint_mtime_ms,
            fingerprint_weak_hash
        FROM manifest_entries
        WHERE library_id = $1 AND path_norm = $2
        "#,
        library_id.0,
        path_norm
    )
    .execute(pool)
    .await?
    .rows_affected();
    if inserted != 1 {
        bail!(
            "expected to seed one media_files row from manifest entry, inserted {inserted}"
        );
    }
    Ok(())
}

async fn current_media_path(
    pool: &PgPool,
    library_id: LibraryId,
) -> Result<Option<String>> {
    sqlx::query_scalar!(
        r#"
        SELECT file_path
        FROM media_files
        WHERE library_id = $1
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
        library_id.0
    )
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

async fn media_available(
    pool: &PgPool,
    library_id: LibraryId,
    path_norm: &str,
) -> Result<Option<bool>> {
    sqlx::query_scalar!(
        r#"
        SELECT is_available
        FROM media_files
        WHERE library_id = $1 AND file_path = $2
        "#,
        library_id.0,
        path_norm
    )
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

fn watch_event(
    library_id: LibraryId,
    path: &Path,
    kind: FileSystemEventKind,
) -> Result<FileSystemEvent> {
    let path_norm = norm(path)?;
    Ok(FileSystemEvent {
        version: 1,
        correlation_id: None,
        idempotency_key: format!("watch:{kind:?}:{path_norm}"),
        library_id,
        path_key: path_norm.clone(),
        fingerprint: None,
        path: path.to_path_buf(),
        old_path: None,
        kind,
        occurred_at: Utc::now(),
    })
}

fn norm(path: &Path) -> Result<String> {
    normalize_path(path).map_err(Into::into)
}

fn database_url_with_database(
    admin_url: &str,
    database_name: &str,
) -> Result<String> {
    let mut url = Url::parse(admin_url)
        .with_context(|| format!("invalid DATABASE_URL: {admin_url}"))?;
    url.set_path(&format!("/{database_name}"));
    Ok(url.to_string())
}

fn quote_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn display_database_url(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(mut url) => {
            if url.password().is_some() {
                let _ = url.set_password(Some("***"));
            }
            url.to_string()
        }
        Err(_) => "<invalid database url>".to_string(),
    }
}
