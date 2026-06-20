//! DB-backed scan observability repository tests.

#![cfg(feature = "database")]

use anyhow::{Context, Result, bail};
use chrono::{Duration as ChronoDuration, Utc};
use ferrex_core::{
    database::{
        repositories::scan_observability::PostgresScanObservabilityRepository,
        repository_ports::scan_observability::{
            NewScanRunEvent, ScanObservabilityRepository,
            ScanRunEventPageRequest, ScanRunFailurePageRequest,
            ScanRunFailureSummary, ScanRunPageRequest, ScanRunRecord,
            ScanRunRetentionPolicy, ScanRunSource, ScanRunStatus,
            ScanRunUpdate,
        },
    },
    types::ids::LibraryId,
};
use serde_json::json;
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    fs,
    fs::File,
    net::TcpListener,
    process::{Child, Command, Stdio},
    str::FromStr,
    time::{Duration as StdDuration, Instant},
};
use tempfile::TempDir;
use tokio::time::sleep;
use url::Url;
use uuid::Uuid;

const FALLBACK_ADMIN_DATABASE_URL: &str = "postgresql://postgres@127.0.0.1:55432/postgres?options=-csearch_path%3Dferrex,public";
const PROBE_CONNECT_TIMEOUT: StdDuration = StdDuration::from_millis(300);
const TEMP_POSTGRES_START_TIMEOUT: StdDuration = StdDuration::from_secs(10);

async fn seed_library(pool: &PgPool) -> Result<LibraryId> {
    let library_id = LibraryId(Uuid::now_v7());
    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, $2, 'movies', $3)
        "#,
    )
    .bind(library_id.to_uuid())
    .bind(format!("scan-observability-{}", library_id))
    .bind(vec!["/media/movies".to_string()])
    .execute(pool)
    .await?;

    Ok(library_id)
}

#[tokio::test]
async fn terminal_runs_events_and_failures_survive_reconstruction() -> Result<()>
{
    let db = TestDatabase::create().await?;
    let result = run_reconstruction_assertions(db.pool.clone()).await;
    let cleanup = db.cleanup().await;
    result?;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn active_scan_run_identity_survives_reload_and_retries_safely()
-> Result<()> {
    let db = TestDatabase::create().await?;
    let result = run_reload_and_retry_assertions(db.pool.clone()).await;
    let cleanup = db.cleanup().await;
    result?;
    cleanup?;
    Ok(())
}

async fn run_reconstruction_assertions(pool: PgPool) -> Result<()> {
    let library_id = seed_library(&pool).await?;
    let repo = PostgresScanObservabilityRepository::new(pool.clone());
    let run_id = Uuid::now_v7();
    let started_at = Utc::now();

    let run = ScanRunRecord {
        id: run_id,
        library_id,
        source: ScanRunSource::Manual,
        status: ScanRunStatus::Running,
        correlation_id: run_id,
        idempotency_key: "manual:start".to_string(),
        sequence: 0,
        started_at,
        last_event_at: started_at,
        terminal_at: None,
        current_path: None,
        completed_items: 0,
        total_items: 0,
        retrying_items: 0,
        dead_lettered_items: 0,
        terminal_summary: json!({}),
    };
    assert!(repo.create_run(&run).await?);

    let first = repo
        .append_event(&NewScanRunEvent {
            run_id,
            library_id,
            event_kind: "started".to_string(),
            status: "discovering".to_string(),
            correlation_id: run_id,
            idempotency_key: "manual:start".to_string(),
            subject_key: None,
            current_path: None,
            occurred_at: started_at,
            completed_items: 0,
            total_items: 1,
            retrying_items: 0,
            dead_lettered_items: 0,
            payload: json!({"sequence": 1, "status": "discovering"}),
        })
        .await?;

    let failure_time = started_at + ChronoDuration::seconds(5);
    repo.update_run(&ScanRunUpdate {
        id: run_id,
        status: ScanRunStatus::Failed,
        idempotency_key: "folder:failed".to_string(),
        last_event_at: failure_time,
        terminal_at: Some(failure_time),
        current_path: Some("/media/movies/Broken".to_string()),
        completed_items: 0,
        total_items: 1,
        retrying_items: 0,
        dead_lettered_items: 1,
        terminal_summary: json!({
            "message_code": "scan.folder_permission_denied",
            "dead_lettered_items": 1,
        }),
    })
    .await?;

    let second = repo
        .append_event(&NewScanRunEvent {
            run_id,
            library_id,
            event_kind: "failed".to_string(),
            status: "failed".to_string(),
            correlation_id: run_id,
            idempotency_key: "folder:failed".to_string(),
            subject_key: Some("/media/movies/Broken".to_string()),
            current_path: Some("/media/movies/Broken".to_string()),
            occurred_at: failure_time,
            completed_items: 0,
            total_items: 1,
            retrying_items: 0,
            dead_lettered_items: 1,
            payload: json!({"sequence": 2, "status": "failed"}),
        })
        .await?;

    assert_eq!(second.sequence, first.sequence + 1);

    repo.upsert_failure_summary(&ScanRunFailureSummary {
        run_id,
        library_id,
        subject_key: "/media/movies/Broken".to_string(),
        category: "filesystem_permission".to_string(),
        message_code: "scan.folder_permission_denied".to_string(),
        raw_debug_details: json!({"raw_error": "permission denied: /media/movies/Broken"}),
        last_error: Some("permission denied".to_string()),
        occurrences: 1,
        first_seen_at: failure_time,
        last_seen_at: failure_time,
        retryable: false,
        job_id: Some(Uuid::now_v7()),
        idempotency_key: "folder:failed".to_string(),
    })
    .await?;

    let reconstructed = PostgresScanObservabilityRepository::new(pool.clone());
    assert!(reconstructed.active_runs(library_id).await?.is_empty());

    let recent = reconstructed.recent_runs(Some(library_id), 10).await?;
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, run_id);
    assert_eq!(recent[0].status, ScanRunStatus::Failed);
    assert_eq!(recent[0].dead_lettered_items, 1);

    let looked_up = reconstructed
        .get_run(run_id)
        .await?
        .context("scan run should be retained")?;
    assert_eq!(looked_up.id, run_id);
    assert_eq!(looked_up.status, ScanRunStatus::Failed);

    let run_page = reconstructed
        .runs_page(ScanRunPageRequest {
            library_id: Some(library_id),
            status: Some(ScanRunStatus::Failed),
            limit: 1,
            offset: 0,
        })
        .await?;
    assert_eq!(run_page.total, 1);
    assert_eq!(run_page.runs.len(), 1);
    assert_eq!(run_page.runs[0].id, run_id);

    let events = reconstructed.events_for_run(run_id).await?;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_kind, "started");
    assert_eq!(events[1].event_kind, "failed");
    assert_eq!(events[1].sequence, events[0].sequence + 1);

    let bounds = reconstructed.event_sequence_bounds(run_id).await?;
    assert_eq!(bounds.min_sequence, Some(events[0].sequence));
    assert_eq!(bounds.max_sequence, Some(events[1].sequence));

    let replay_page = reconstructed
        .events_page_for_run(ScanRunEventPageRequest {
            run_id,
            after_sequence: Some(events[0].sequence),
            limit: 10,
        })
        .await?;
    assert_eq!(replay_page.len(), 1);
    assert_eq!(replay_page[0].event_kind, "failed");

    let failures = reconstructed.failure_summaries_for_run(run_id).await?;
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].category, "filesystem_permission");
    assert_eq!(failures[0].message_code, "scan.folder_permission_denied");
    assert_eq!(
        failures[0].raw_debug_details["raw_error"],
        "permission denied: /media/movies/Broken"
    );

    let failure_page = reconstructed
        .failure_summaries_page_for_run(ScanRunFailurePageRequest {
            run_id,
            limit: 1,
            offset: 0,
        })
        .await?;
    assert_eq!(failure_page.total, 1);
    assert_eq!(failure_page.failures.len(), 1);

    let pruned = reconstructed
        .prune(ScanRunRetentionPolicy {
            terminal_before: failure_time + ChronoDuration::seconds(1),
        })
        .await?;
    assert_eq!(pruned, 1);
    assert!(
        reconstructed
            .recent_runs(Some(library_id), 10)
            .await?
            .is_empty()
    );

    Ok(())
}

async fn run_reload_and_retry_assertions(pool: PgPool) -> Result<()> {
    let library_id = seed_library(&pool).await?;
    let repo = PostgresScanObservabilityRepository::new(pool.clone());
    let run_id = Uuid::now_v7();
    let correlation_id = Uuid::now_v7();
    let started_at = Utc::now();

    let active_run = ScanRunRecord {
        id: run_id,
        library_id,
        source: ScanRunSource::Manual,
        status: ScanRunStatus::Running,
        correlation_id,
        idempotency_key: "manual:start".to_string(),
        sequence: 0,
        started_at,
        last_event_at: started_at,
        terminal_at: None,
        current_path: None,
        completed_items: 0,
        total_items: 10,
        retrying_items: 0,
        dead_lettered_items: 0,
        terminal_summary: json!({}),
    };
    assert!(repo.create_run(&active_run).await?);
    assert!(!repo.create_run(&active_run).await?);

    let progress_at = started_at + ChronoDuration::seconds(5);
    repo.append_event(&NewScanRunEvent {
        run_id,
        library_id,
        event_kind: "progress".to_string(),
        status: "running".to_string(),
        correlation_id,
        idempotency_key: "manual:progress".to_string(),
        subject_key: Some("/media/movies/RetryMe.mkv".to_string()),
        current_path: Some("/media/movies/RetryMe.mkv".to_string()),
        occurred_at: progress_at,
        completed_items: 4,
        total_items: 10,
        retrying_items: 1,
        dead_lettered_items: 0,
        payload: json!({"sequence": 1, "status": "running"}),
    })
    .await?;
    repo.update_run(&ScanRunUpdate {
        id: run_id,
        status: ScanRunStatus::Running,
        idempotency_key: "manual:progress".to_string(),
        last_event_at: progress_at,
        terminal_at: None,
        current_path: Some("/media/movies/RetryMe.mkv".to_string()),
        completed_items: 4,
        total_items: 10,
        retrying_items: 1,
        dead_lettered_items: 0,
        terminal_summary: json!({}),
    })
    .await?;

    let reloaded = PostgresScanObservabilityRepository::new(pool.clone());
    let active = reloaded.active_runs(library_id).await?;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, run_id);
    assert_eq!(active[0].correlation_id, correlation_id);
    assert_eq!(active[0].sequence, 1);
    assert_eq!(
        active[0].current_path.as_deref(),
        Some("/media/movies/RetryMe.mkv")
    );

    let failed_at = progress_at + ChronoDuration::seconds(5);
    reloaded
        .update_run(&ScanRunUpdate {
            id: run_id,
            status: ScanRunStatus::Failed,
            idempotency_key: "manual:failed".to_string(),
            last_event_at: failed_at,
            terminal_at: Some(failed_at),
            current_path: Some("/media/movies/RetryMe.mkv".to_string()),
            completed_items: 4,
            total_items: 10,
            retrying_items: 1,
            dead_lettered_items: 1,
            terminal_summary: json!({
                "message_code": "scan.folder_permission_denied",
                "needs_attention": true,
            }),
        })
        .await?;
    let terminal = reloaded
        .get_run(run_id)
        .await?
        .context("terminal scan run should survive reload")?;
    assert_eq!(terminal.status, ScanRunStatus::Failed);
    let terminal_at = terminal
        .terminal_at
        .context("terminal timestamp should be set")?;
    assert_eq!(terminal_at.timestamp_micros(), failed_at.timestamp_micros());
    assert!(reloaded.active_runs(library_id).await?.is_empty());

    let retry_id = Uuid::now_v7();
    let retry_correlation_id = Uuid::now_v7();
    let retry_started_at = failed_at + ChronoDuration::seconds(1);
    let retry_run = ScanRunRecord {
        id: retry_id,
        library_id,
        source: ScanRunSource::Retry,
        status: ScanRunStatus::Running,
        correlation_id: retry_correlation_id,
        idempotency_key: format!("retry:{run_id}"),
        sequence: 0,
        started_at: retry_started_at,
        last_event_at: retry_started_at,
        terminal_at: None,
        current_path: Some("/media/movies/RetryMe.mkv".to_string()),
        completed_items: 0,
        total_items: 1,
        retrying_items: 0,
        dead_lettered_items: 0,
        terminal_summary: json!({}),
    };
    assert!(reloaded.create_run(&retry_run).await?);

    let active_retry = reloaded.active_runs(library_id).await?;
    assert_eq!(active_retry.len(), 1);
    assert_eq!(active_retry[0].id, retry_id);
    assert_ne!(active_retry[0].id, run_id);
    assert_eq!(active_retry[0].source, ScanRunSource::Retry);
    assert_eq!(active_retry[0].correlation_id, retry_correlation_id);

    Ok(())
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
            connect_pool(&admin_database_url, 1, StdDuration::from_secs(5))
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
        let pool = connect_pool(&database_url, 5, StdDuration::from_secs(5))
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
            StdDuration::from_secs(5),
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
    log_path: std::path::PathBuf,
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
            .stdout(Stdio::from(log.try_clone().context(
                "failed to clone temporary PostgreSQL log handle",
            )?))
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
                StdDuration::from_millis(250),
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
            sleep(StdDuration::from_millis(100)).await;
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
    timeout: StdDuration,
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
