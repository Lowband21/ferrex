mod common;

use std::{
    fs::{self, File},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use axum::{
    body::to_bytes,
    extract::{Query, State},
    response::{IntoResponse, Sse},
};
use chrono::Utc;
use ferrex_core::{
    api::types::ScanRunMode,
    application::unit_of_work::AppUnitOfWork,
    database::PostgresDatabase,
    domain::scan::{
        actors::folder::{
            DefaultFolderScanActor, FolderListingPlan, FolderScanActor,
            ScannerFileFilterPolicy,
        },
        orchestration::{
            FolderScanJob, LibraryActorConfig, ScanReason,
            budget::InMemoryBudget,
            config::OrchestratorConfig,
            context::{
                FolderScanContext, MovieFolderScanContext, MovieRootPath,
            },
            job::{EnqueueRequest, JobPayload, JobPriority},
            persistence::{PostgresCursorRepository, PostgresQueueService},
            scan_cursor::{
                ScanCursor, ScanCursorId, ScanCursorRepository, normalize_path,
            },
        },
    },
    infra::{image_service::ImageService, providers::TmdbApiProvider},
    types::{LibraryId, LibraryReference, LibraryType},
};
use ferrex_server::{
    handlers::scan::handle_scan::{
        ProgressQuery, build_scan_progress_stream, latest_progress_handler,
    },
    infra::{
        orchestration::ScanOrchestrator,
        scan::scan_manager::ScanLifecycleStatus, startup::NoopStartupHooks,
    },
};
use sqlx::postgres::PgPoolOptions;
use tempfile::TempDir;
use tokio::time::sleep;
use uuid::Uuid;

use crate::common::build_test_app_with_hooks;

const TEMP_POSTGRES_START_TIMEOUT: Duration = Duration::from_secs(15);
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread")]
async fn persisted_cursor_unchanged_scan_completes_and_replays_terminal_progress()
-> Result<()> {
    let db = TempPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db.database_url)
        .await
        .context("connect to temporary postgres")?;
    ferrex_core::MIGRATOR
        .run(&pool)
        .await
        .context("migrate temporary postgres")?;

    let app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (_router, state, _app_tempdir) = app.into_parts();

    let library_root = TempDir::new().context("create media root")?;
    let root = library_root.path().to_path_buf();
    let stable_dir = root.join("Stable Movie");
    tokio::fs::create_dir_all(&stable_dir).await?;
    tokio::fs::write(stable_dir.join("feature.mkv"), b"unchanged media")
        .await?;

    let library_id = LibraryId(Uuid::now_v7());
    let root_norm = normalize_path(&root)?;
    seed_movie_library(&pool, library_id, &root_norm).await?;

    let folder_job = folder_scan_job(library_id, &root, &stable_dir)?;
    let listing_plan = DefaultFolderScanActor::new()
        .plan_listing(&folder_job)
        .await?;
    let planned_entries = planned_entry_count(&listing_plan);
    let folder_norm = normalize_path(&stable_dir)?;
    let cursors = state.scan_control().orchestrator().cursor_repository();
    cursors
        .upsert(ScanCursor {
            id: ScanCursorId::new(
                library_id,
                &vec![PathBuf::from(&folder_norm)],
            ),
            folder_path_norm: folder_norm.clone(),
            listing_hash: listing_plan.generated_listing_hash,
            entry_count: planned_entries,
            last_scan_at: Utc::now(),
            last_modified_at: None,
            device_id: None,
        })
        .await?;

    let orchestrator = state.scan_control().orchestrator();
    orchestrator
        .register_library(
            LibraryActorConfig {
                library: LibraryReference {
                    id: library_id,
                    name: "Cursor regression movies".to_string(),
                    library_type: LibraryType::Movies,
                    paths: vec![PathBuf::from(&root_norm)],
                },
                root_paths: vec![PathBuf::from(&root_norm)],
                max_outstanding_jobs: 64,
            },
            false,
        )
        .await?;
    orchestrator.start().await?;

    let scan_id = Uuid::now_v7();
    let accepted = state
        .scan_control()
        .start_library_scan(library_id, Some(scan_id), ScanRunMode::Manual)
        .await
        .map_err(|err| anyhow::anyhow!("start scan failed: {err:?}"))?;
    assert_eq!(accepted.scan_id, scan_id);

    let terminal = wait_for_history(state.scan_control(), scan_id).await?;
    assert_eq!(terminal.status, ScanLifecycleStatus::Completed);
    let terminal_json = serde_json::to_value(&terminal)?;
    assert_eq!(terminal_json["failed_items"].as_u64(), Some(0));
    assert_eq!(terminal_json["needs_attention_items"].as_u64(), Some(0));
    assert_eq!(terminal_json["retrying_items"].as_u64(), Some(0));
    assert!(
        terminal_json["known_unchanged_items"]
            .as_u64()
            .unwrap_or_default()
            > 0,
        "unchanged cursor short-circuit should be visible in terminal counters"
    );

    let active = state.scan_control().active_scans().await;
    assert!(
        active.iter().all(|snapshot| snapshot.scan_id != scan_id),
        "terminal scan should be removed from the active run map"
    );

    let latest_response = latest_progress_handler(
        State(state.clone()),
        Query(ProgressQuery { scan_id }),
    )
    .await
    .map_err(|err| anyhow::anyhow!("latest progress failed: {err:?}"))?;
    let axum::Json(envelope) = latest_response;
    let latest = envelope
        .data
        .expect("latest progress response")
        .latest
        .expect("terminal latest progress frame");
    let latest_json = serde_json::to_value(&latest)?;
    assert_eq!(latest_json["status"].as_str(), Some("completed"));
    assert_eq!(latest_json["failed_items"].as_u64(), Some(0));
    assert_eq!(latest_json["needs_attention_items"].as_u64(), Some(0));
    assert!(
        latest_json["known_unchanged_items"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
    assert!(latest_json["terminal_at"].as_str().is_some());

    let frames = state.scan_control().events(&scan_id).await?;
    assert_eq!(
        frames
            .iter()
            .filter(|frame| {
                matches!(
                    frame.event,
                    ferrex_server::infra::scan::scan_manager::ScanEventKind::Completed
                )
            })
            .count(),
        1,
        "terminal broadcast history should contain one completed frame"
    );

    let stream = build_scan_progress_stream(
        Arc::clone(&state.scan_control()),
        scan_id,
        None,
    )
    .await?;
    let response = Sse::new(stream).into_response();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let sse_text = String::from_utf8(body.to_vec())?;
    assert_eq!(
        sse_text.matches("event: scan.completed").count(),
        1,
        "terminal SSE replay should emit exactly one completed event"
    );
    assert!(sse_text.contains("\"status\":\"completed\""));
    assert!(sse_text.contains("\"known_unchanged_items\":"));
    assert!(!sse_text.contains("dead_lettered_items"));

    orchestrator.shutdown().await?;
    pool.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn orchestrator_start_primes_persisted_ready_jobs_once() -> Result<()> {
    let db = TempPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db.database_url)
        .await
        .context("connect to temporary postgres")?;
    ferrex_core::MIGRATOR
        .run(&pool)
        .await
        .context("migrate temporary postgres")?;

    let media_root = TempDir::new().context("create media root")?;
    let root = media_root.path().to_path_buf();
    let movie_folder = root.join("Primed Movie");
    tokio::fs::create_dir_all(&movie_folder).await?;

    let library_id = LibraryId(Uuid::now_v7());
    let root_norm = normalize_path(&root)?;
    seed_movie_library(&pool, library_id, &root_norm).await?;

    let cache_dir = TempDir::new().context("create image cache root")?;
    let image_cache_dir = cache_dir.path().join("images");
    tokio::fs::create_dir_all(&image_cache_dir).await?;

    let postgres = Arc::new(PostgresDatabase::from_pool(pool.clone()));
    let unit_of_work =
        Arc::new(AppUnitOfWork::from_postgres(Arc::clone(&postgres)).map_err(
            |err| anyhow::anyhow!("failed to build unit of work: {err}"),
        )?);
    let image_service = Arc::new(ImageService::new(
        unit_of_work.media_files_read.clone(),
        unit_of_work.images.clone(),
        image_cache_dir,
    ));
    let queue = Arc::new(PostgresQueueService::new(pool.clone()).await?);
    let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));

    let mut config = OrchestratorConfig::default();
    config.queue.max_parallel_scans = 0;
    config.queue.max_parallel_series_resolve = 0;
    config.queue.max_parallel_analyses = 0;
    config.queue.max_parallel_metadata = 0;
    config.queue.max_parallel_index = 0;
    config.queue.max_parallel_image_fetch = 0;
    config.queue.max_parallel_transcript_extract = 0;
    config.maintenance.enabled = false;

    let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));
    let orchestrator = Arc::new(ScanOrchestrator::new(
        config,
        Arc::new(TmdbApiProvider::new()),
        image_service,
        unit_of_work,
        queue,
        cursors,
        budget,
        ScannerFileFilterPolicy::default(),
    )?);

    let request = EnqueueRequest::new(
        JobPriority::P1,
        JobPayload::FolderScan(folder_scan_job(
            library_id,
            &root,
            &movie_folder,
        )?),
    );
    let handle = orchestrator.enqueue(request).await?;
    assert!(handle.accepted, "test setup should persist one ready job");

    orchestrator.start().await?;

    let scheduler = orchestrator.runtime().scheduler();
    let reservation = scheduler
        .reserve()
        .await
        .expect("persisted ready job should be scheduled after startup");
    assert_eq!(reservation.library_id, library_id);
    assert_eq!(reservation.priority, JobPriority::P1);

    let confirmed = scheduler
        .confirm(reservation.id)
        .await
        .expect("reserved job should be confirmable");
    assert_eq!(confirmed.library_id, library_id);
    scheduler.record_completed(library_id).await;

    assert!(
        scheduler.reserve().await.is_none(),
        "startup must prime persisted ready jobs once, without phantom reservations"
    );

    orchestrator.shutdown().await?;
    pool.close().await;
    Ok(())
}

async fn seed_movie_library(
    pool: &sqlx::PgPool,
    library_id: LibraryId,
    root_norm: &str,
) -> Result<()> {
    let paths = vec![root_norm.to_string()];
    sqlx::query(
        r#"
        INSERT INTO libraries (
            id, name, library_type, paths,
            enabled, auto_scan, watch_for_changes, analyze_on_scan
        )
        VALUES ($1, $2, 'movies', $3, true, false, false, false)
        "#,
    )
    .bind(library_id.0)
    .bind("Cursor regression movies")
    .bind(&paths)
    .execute(pool)
    .await?;
    Ok(())
}

fn planned_entry_count(plan: &FolderListingPlan) -> usize {
    plan.directories.len() + plan.media_files.len() + plan.ancillary_files.len()
}

fn folder_scan_job(
    library_id: LibraryId,
    root: &Path,
    folder: &Path,
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
        scan_reason: ScanReason::BulkSeed,
        enqueue_time: Utc::now(),
        device_id: None,
    })
}

async fn wait_for_history(
    scan_control: Arc<
        ferrex_server::infra::scan::scan_manager::ScanControlPlane,
    >,
    scan_id: Uuid,
) -> Result<ferrex_server::infra::scan::scan_manager::ScanHistoryEntry> {
    let started = Instant::now();
    loop {
        if let Some(entry) = scan_control
            .history(25)
            .await
            .into_iter()
            .find(|entry| entry.scan_id == scan_id)
        {
            return Ok(entry);
        }

        if started.elapsed() > WAIT_TIMEOUT {
            bail!("scan {scan_id} did not reach terminal history");
        }

        sleep(Duration::from_millis(50)).await;
    }
}

struct TempPostgres {
    child: Child,
    database_url: String,
    log_path: PathBuf,
    _temp_dir: TempDir,
}

impl TempPostgres {
    async fn start() -> Result<Self> {
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
            .arg("postgres")
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

        let database_url = format!(
            "postgresql://postgres@127.0.0.1:{port}/postgres?options=-csearch_path%3Dferrex,public"
        );
        let mut server = Self {
            child,
            database_url,
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

            if let Ok(pool) = PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_millis(250))
                .connect(&self.database_url)
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

fn free_local_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .context("failed to bind a local port for temporary PostgreSQL")?;
    Ok(listener.local_addr()?.port())
}
