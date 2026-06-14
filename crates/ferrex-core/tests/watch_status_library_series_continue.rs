//! DB-backed library-scoped series continue-watching repository coverage.

use std::{
    collections::HashSet,
    fs::{self, File},
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    str::FromStr,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use ferrex_core::{
    database::{
        repositories::watch_status::PostgresWatchStatusRepository,
        repository_ports::watch_status::WatchStatusRepository,
    },
    domain::watch::ContinueWatchingActionHint,
};
use ferrex_model::LibraryId;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;

const FALLBACK_ADMIN_DATABASE_URL: &str = "postgresql://postgres@127.0.0.1:55432/postgres?options=-csearch_path%3Dferrex,public";
const PROBE_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const TEMP_POSTGRES_START_TIMEOUT: Duration = Duration::from_secs(15);

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

        let probe_result =
            connect_pool(&requested_admin_url, 1, PROBE_CONNECT_TIMEOUT).await;

        match probe_result {
            Ok(pool) => {
                pool.close().await;
                Self::create_on_admin_url(requested_admin_url, None).await
            }
            Err(connect_error) => {
                let temp_postgres = TempPostgres::start(&requested_admin_url).await.with_context(
                    || {
                        format!(
                            "{} was unreachable ({connect_error:#}); failed to start temporary PostgreSQL",
                            display_database_url(&requested_admin_url)
                        )
                    },
                )?;
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

        sqlx::query(&format!(
            "CREATE DATABASE {}",
            quote_ident(&database_name)
        ))
        .execute(&admin_pool)
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
            "failed to migrate isolated test database; ensure the PostgreSQL server has the pg_uuidv7 extension available"
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
        let cleanup_result = sqlx::query(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            quote_ident(&self.database_name)
        ))
        .execute(&admin_pool)
        .await
        .with_context(|| {
            format!("failed to drop test database {}", self.database_name)
        });
        admin_pool.close().await;

        cleanup_result.map(|_| ())
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

            tokio::time::sleep(Duration::from_millis(100)).await;
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

async fn seed_library(pool: &PgPool, id: Uuid, name: &str) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, $2, 'tvshows', ARRAY['/tmp'])
        "#,
    )
    .bind(id)
    .bind(name)
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_series(
    pool: &PgPool,
    library_id: Uuid,
    series_id: Uuid,
    tmdb_id: i64,
    title: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO series (id, library_id, tmdb_id, title)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(series_id)
    .bind(library_id)
    .bind(tmdb_id)
    .bind(title)
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_season(
    pool: &PgPool,
    library_id: Uuid,
    series_id: Uuid,
    season_id: Uuid,
    tmdb_series_id: i64,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO season_references (
            id, series_id, season_number, tmdb_series_id, library_id
        )
        VALUES ($1, $2, 1, $3, $4)
        "#,
    )
    .bind(season_id)
    .bind(series_id)
    .bind(tmdb_series_id)
    .bind(library_id)
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_episode(
    pool: &PgPool,
    library_id: Uuid,
    series_id: Uuid,
    season_id: Uuid,
    episode_id: Uuid,
    file_id: Uuid,
    tmdb_series_id: i64,
    episode_number: i16,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO media_files (
            id, library_id, media_id, media_type, file_path, filename, file_size
        )
        VALUES ($1, $2, $3, 'episode', $4, $5, 100)
        "#,
    )
    .bind(file_id)
    .bind(library_id)
    .bind(episode_id)
    .bind(format!("/tmp/{library_id}/{episode_id}.mkv"))
    .bind(format!("{episode_id}.mkv"))
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO episode_references (
            id, series_id, season_id, file_id,
            season_number, episode_number, tmdb_series_id
        )
        VALUES ($1, $2, $3, $4, 1, $5, $6)
        "#,
    )
    .bind(episode_id)
    .bind(series_id)
    .bind(season_id)
    .bind(file_id)
    .bind(episode_number)
    .bind(tmdb_series_id)
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_series_with_episodes(
    pool: &PgPool,
    library_id: Uuid,
    series_id: Uuid,
    season_id: Uuid,
    tmdb_series_id: i64,
    title: &str,
    episode_ids: &[Uuid],
) -> Result<()> {
    seed_series(pool, library_id, series_id, tmdb_series_id, title).await?;
    seed_season(pool, library_id, series_id, season_id, tmdb_series_id).await?;

    for (index, episode_id) in episode_ids.iter().copied().enumerate() {
        seed_episode(
            pool,
            library_id,
            series_id,
            season_id,
            episode_id,
            Uuid::from_u128(100_000 + episode_id.as_u128()),
            tmdb_series_id,
            (index + 1) as i16,
        )
        .await?;
    }

    Ok(())
}

async fn seed_episode_state(
    pool: &PgPool,
    user_id: Uuid,
    tmdb_series_id: i64,
    episode_number: i16,
    position: f32,
    duration: f32,
    last_watched: i64,
    is_completed: bool,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO user_episode_state (
            user_id, tmdb_series_id, season_number, episode_number,
            position, duration, last_watched, is_completed
        )
        VALUES ($1, $2, 1, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(user_id)
    .bind(tmdb_series_id)
    .bind(episode_number)
    .bind(position)
    .bind(duration)
    .bind(last_watched)
    .bind(is_completed)
    .execute(pool)
    .await?;

    Ok(())
}

#[tokio::test]
async fn library_series_continue_watching_scopes_targets_and_exclusions()
-> Result<()> {
    let database = TestDatabase::create().await?;
    let pool = database.pool.clone();

    let user_id = Uuid::from_u128(1);
    let library_id = Uuid::from_u128(10);
    let other_library_id = Uuid::from_u128(11);
    seed_library(&pool, library_id, "series-library").await?;
    seed_library(&pool, other_library_id, "other-series-library").await?;

    let resume_series_id = Uuid::from_u128(100);
    let resume_episode_id = Uuid::from_u128(101);
    seed_series_with_episodes(
        &pool,
        library_id,
        resume_series_id,
        Uuid::from_u128(102),
        10_000,
        "Resume Show",
        &[resume_episode_id, Uuid::from_u128(103)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 10_000, 1, 120.0, 1_200.0, 1_000, false)
        .await?;

    let next_series_id = Uuid::from_u128(200);
    let next_episode_id = Uuid::from_u128(202);
    seed_series_with_episodes(
        &pool,
        library_id,
        next_series_id,
        Uuid::from_u128(203),
        20_000,
        "Next Show",
        &[Uuid::from_u128(201), next_episode_id],
    )
    .await?;
    seed_episode_state(&pool, user_id, 20_000, 1, 1.0, 1.0, 900, true).await?;

    let done_series_id = Uuid::from_u128(300);
    seed_series_with_episodes(
        &pool,
        library_id,
        done_series_id,
        Uuid::from_u128(302),
        30_000,
        "Done Show",
        &[Uuid::from_u128(301)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 30_000, 1, 1.0, 1.0, 800, true).await?;

    let noise_series_id = Uuid::from_u128(350);
    seed_series_with_episodes(
        &pool,
        library_id,
        noise_series_id,
        Uuid::from_u128(352),
        35_000,
        "Noise Show",
        &[Uuid::from_u128(351), Uuid::from_u128(353)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 35_000, 1, 5.0, 1_200.0, 1_200, false)
        .await?;

    let outside_series_id = Uuid::from_u128(400);
    seed_series_with_episodes(
        &pool,
        other_library_id,
        outside_series_id,
        Uuid::from_u128(402),
        40_000,
        "Outside Show",
        &[Uuid::from_u128(401)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 40_000, 1, 120.0, 1_200.0, 1_100, false)
        .await?;

    let repo = PostgresWatchStatusRepository::new(pool);
    let items = repo
        .get_library_series_continue_watching(
            user_id,
            LibraryId(library_id),
            10,
        )
        .await?;

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].series_id, resume_series_id);
    assert_eq!(items[0].library_id, LibraryId(library_id));
    assert_eq!(items[0].action_episode_id, Some(resume_episode_id));
    assert_eq!(items[0].action_hint, ContinueWatchingActionHint::Resume);
    assert_eq!(items[0].last_watched, 1_000);

    assert_eq!(items[1].series_id, next_series_id);
    assert_eq!(items[1].library_id, LibraryId(library_id));
    assert_eq!(items[1].action_episode_id, Some(next_episode_id));
    assert_eq!(
        items[1].action_hint,
        ContinueWatchingActionHint::NextEpisode
    );
    assert_eq!(items[1].last_watched, 900);

    assert!(items.iter().all(|item| item.series_id != done_series_id));
    assert!(items.iter().all(|item| item.series_id != noise_series_id));
    assert!(items.iter().all(|item| item.series_id != outside_series_id));

    drop(repo);
    database.cleanup().await?;

    Ok(())
}

#[tokio::test]
async fn library_series_continue_watching_orders_by_activity_title_and_id()
-> Result<()> {
    let database = TestDatabase::create().await?;
    let pool = database.pool.clone();

    let user_id = Uuid::from_u128(2);
    let library_id = Uuid::from_u128(20);
    seed_library(&pool, library_id, "series-library-sorting").await?;

    let alpha_low_id = Uuid::from_u128(501);
    seed_series_with_episodes(
        &pool,
        library_id,
        alpha_low_id,
        Uuid::from_u128(502),
        50_100,
        "Alpha",
        &[Uuid::from_u128(503)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 50_100, 1, 120.0, 1_200.0, 1_000, false)
        .await?;

    let alpha_high_id = Uuid::from_u128(601);
    seed_series_with_episodes(
        &pool,
        library_id,
        alpha_high_id,
        Uuid::from_u128(602),
        60_100,
        "Alpha",
        &[Uuid::from_u128(603)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 60_100, 1, 120.0, 1_200.0, 1_000, false)
        .await?;

    let beta_id = Uuid::from_u128(701);
    seed_series_with_episodes(
        &pool,
        library_id,
        beta_id,
        Uuid::from_u128(702),
        70_100,
        "Beta",
        &[Uuid::from_u128(703)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 70_100, 1, 120.0, 1_200.0, 1_000, false)
        .await?;

    let latest_id = Uuid::from_u128(801);
    seed_series_with_episodes(
        &pool,
        library_id,
        latest_id,
        Uuid::from_u128(802),
        80_100,
        "Later Activity",
        &[Uuid::from_u128(803)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 80_100, 1, 120.0, 1_200.0, 1_500, false)
        .await?;

    let repo = PostgresWatchStatusRepository::new(pool);
    let items = repo
        .get_library_series_continue_watching(
            user_id,
            LibraryId(library_id),
            10,
        )
        .await?;

    assert_eq!(
        items.iter().map(|item| item.series_id).collect::<Vec<_>>(),
        vec![latest_id, alpha_low_id, alpha_high_id, beta_id]
    );

    let limited = repo
        .get_library_series_continue_watching(user_id, LibraryId(library_id), 2)
        .await?;
    assert_eq!(limited.len(), 2);
    assert_eq!(limited[0].series_id, latest_id);
    assert_eq!(limited[1].series_id, alpha_low_id);

    drop(repo);
    database.cleanup().await?;

    Ok(())
}

#[tokio::test]
async fn library_series_meaningful_watch_state_ids_scope_and_filter_noise()
-> Result<()> {
    let database = TestDatabase::create().await?;
    let pool = database.pool.clone();

    let user_id = Uuid::from_u128(3);
    let library_id = Uuid::from_u128(30);
    let other_library_id = Uuid::from_u128(31);
    seed_library(&pool, library_id, "series-library-watch-state").await?;
    seed_library(&pool, other_library_id, "other-series-library-watch-state")
        .await?;

    let resume_series_id = Uuid::from_u128(1_100);
    seed_series_with_episodes(
        &pool,
        library_id,
        resume_series_id,
        Uuid::from_u128(1_101),
        51_000,
        "Resume State Show",
        &[Uuid::from_u128(1_102)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 51_000, 1, 120.0, 1_200.0, 1_000, false)
        .await?;

    let completed_series_id = Uuid::from_u128(1_200);
    seed_series_with_episodes(
        &pool,
        library_id,
        completed_series_id,
        Uuid::from_u128(1_201),
        52_000,
        "Completed State Show",
        &[Uuid::from_u128(1_202)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 52_000, 1, 1.0, 1.0, 900, true).await?;

    let unwatched_series_id = Uuid::from_u128(1_300);
    seed_series_with_episodes(
        &pool,
        library_id,
        unwatched_series_id,
        Uuid::from_u128(1_301),
        53_000,
        "Unwatched Show",
        &[Uuid::from_u128(1_302)],
    )
    .await?;

    let noisy_series_id = Uuid::from_u128(1_400);
    seed_series_with_episodes(
        &pool,
        library_id,
        noisy_series_id,
        Uuid::from_u128(1_401),
        54_000,
        "Noise Show",
        &[Uuid::from_u128(1_402)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 54_000, 1, 5.0, 1_200.0, 800, false)
        .await?;

    let outside_series_id = Uuid::from_u128(1_500);
    seed_series_with_episodes(
        &pool,
        other_library_id,
        outside_series_id,
        Uuid::from_u128(1_501),
        55_000,
        "Outside State Show",
        &[Uuid::from_u128(1_502)],
    )
    .await?;
    seed_episode_state(&pool, user_id, 55_000, 1, 120.0, 1_200.0, 1_100, false)
        .await?;

    let repo = PostgresWatchStatusRepository::new(pool);
    let watched_ids = repo
        .list_library_series_ids_with_meaningful_watch_state(
            user_id,
            LibraryId(library_id),
        )
        .await?
        .into_iter()
        .collect::<HashSet<_>>();

    assert_eq!(
        watched_ids,
        HashSet::from([resume_series_id, completed_series_id])
    );
    assert!(!watched_ids.contains(&unwatched_series_id));
    assert!(!watched_ids.contains(&noisy_series_id));
    assert!(!watched_ids.contains(&outside_series_id));

    drop(repo);
    database.cleanup().await?;

    Ok(())
}
