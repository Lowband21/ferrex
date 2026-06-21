use std::{
    collections::HashSet, net::SocketAddr, path::PathBuf, time::Duration,
};

use anyhow::{Context, Result, anyhow};
use axum::Router;
use axum::http::StatusCode;
use axum_test::TestServer;
use ferrex_core::{
    api::{
        routes::{utils as route_utils, v1},
        types::{
            ActiveScansResponse, ApiResponse, ScanCommandAcceptedResponse,
            ScanLifecycleStatus, ScanRunMode, ScanStartDisposition,
        },
    },
    domain::scan::actors::LibraryActorConfig,
    types::{
        Library, LibraryId, LibraryReference, LibraryType,
        MovieReferenceBatchSize,
    },
};
use ferrex_server::{
    infra::app_state::AppState, infra::startup::NoopStartupHooks,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tempfile::TempDir;
use tokio::time::sleep;
use uuid::Uuid;

mod common;
use common::build_test_app_with_hooks;

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn extract_token_field<'a>(body: &'a Value, key: &str) -> &'a str {
    body["data"][key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} missing"))
}

async fn build_server(pool: PgPool) -> Result<(TestServer, AppState, TempDir)> {
    let app = build_test_app_with_hooks(pool, &NoopStartupHooks).await?;
    let (router, state, tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state.clone());
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow!(err.to_string()))?;
    Ok((server, state, tempdir))
}

async fn register_user(server: &TestServer, username: &str) -> Result<String> {
    let response = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": username.replace('_', " "),
            "password": "Password#123"
        }))
        .await;
    response.assert_status_ok();
    let body: Value = response.json();
    Ok(extract_token_field(&body, "access_token").to_string())
}

async fn create_registered_library(
    state: &AppState,
    root_path: PathBuf,
) -> Result<LibraryId> {
    let library_id = LibraryId(Uuid::now_v7());
    let name = format!("scan-lifecycle-{library_id}");
    let library_type = LibraryType::Movies;
    let paths = vec![root_path.clone()];
    let library = Library {
        id: library_id,
        name: name.clone(),
        library_type,
        paths: paths.clone(),
        scan_interval_minutes: 60,
        last_scan: None,
        enabled: true,
        auto_scan: true,
        watch_for_changes: true,
        analyze_on_scan: false,
        max_retry_attempts: 3,
        movie_ref_batch_size: MovieReferenceBatchSize::default(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        media: None,
    };

    state
        .unit_of_work()
        .libraries
        .create_library(library)
        .await?;
    state
        .scan_control()
        .orchestrator()
        .register_library(
            LibraryActorConfig {
                library: LibraryReference {
                    id: library_id,
                    name,
                    library_type,
                    paths,
                },
                root_paths: vec![root_path],
                max_outstanding_jobs: state
                    .config()
                    .scanner
                    .library_actor_max_outstanding_jobs,
            },
            true,
        )
        .await?;

    Ok(library_id)
}

fn scan_path(route: &str, library_id: LibraryId) -> String {
    route_utils::replace_param(route, "{id}", &library_id.to_string())
}

async fn start_scan(
    server: &TestServer,
    token: &str,
    library_id: LibraryId,
    mode: Option<&str>,
) -> Result<ScanCommandAcceptedResponse> {
    let mut body = serde_json::Map::new();
    if let Some(mode) = mode {
        body.insert("mode".to_string(), Value::String(mode.to_string()));
    }
    let response = server
        .post(&scan_path(v1::libraries::scans::START, library_id))
        .add_header("Authorization", bearer(token))
        .json(&Value::Object(body))
        .await;
    assert!(
        matches!(
            response.status_code(),
            StatusCode::ACCEPTED | StatusCode::OK
        ),
        "unexpected start status: {}",
        response.status_code()
    );
    let body: ApiResponse<ScanCommandAcceptedResponse> = response.json();
    body.data.context("start response missing data")
}

async fn post_scan_command(
    server: &TestServer,
    token: &str,
    route: &str,
    library_id: LibraryId,
    scan_id: Uuid,
) -> axum_test::TestResponse {
    server
        .post(&scan_path(route, library_id))
        .add_header("Authorization", bearer(token))
        .json(&json!({ "scan_id": scan_id }))
        .await
}

async fn active_run_ids(
    pool: &PgPool,
    library_id: LibraryId,
    mode: &str,
) -> Result<Vec<Uuid>> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT scan_id
        FROM library_scan_runs
        WHERE library_id = $1
          AND mode = $2
          AND status::text IN ('pending','running','paused')
        ORDER BY started_at ASC
        "#,
    )
    .bind(library_id.to_uuid())
    .bind(mode)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

async fn active_scans(
    server: &TestServer,
    token: &str,
) -> Result<ActiveScansResponse> {
    let response = server
        .get(v1::scan::ACTIVE)
        .add_header("Authorization", bearer(token))
        .await;
    response.assert_status_ok();
    let body: ApiResponse<ActiveScansResponse> = response.json();
    body.data.context("active scans response missing data")
}

async fn wait_for_job_correlation(
    pool: &PgPool,
    library_id: LibraryId,
    correlation_id: Uuid,
) -> Result<i64> {
    for _ in 0..100 {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM orchestrator_jobs
            WHERE library_id = $1
              AND correlation_id = $2
            "#,
        )
        .bind(library_id.to_uuid())
        .bind(correlation_id)
        .fetch_one(pool)
        .await?;

        if count > 0 {
            return Ok(count);
        }

        sleep(Duration::from_millis(20)).await;
    }

    Err(anyhow!(
        "timed out waiting for job correlation {correlation_id}"
    ))
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn concurrent_start_posts_reuse_one_active_run_and_snapshot(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _app_tempdir) = build_server(pool.clone()).await?;
    let token = register_user(&server, "scan_concurrent_user").await?;
    let library_root = tempfile::tempdir()?;
    let library_id =
        create_registered_library(&state, library_root.path().to_path_buf())
            .await?;
    let start_path = scan_path(v1::libraries::scans::START, library_id);

    let requests = (0..8).map(|_| {
        let auth = bearer(&token);
        let start_path = start_path.clone();
        let server = &server;
        async move {
            server
                .post(&start_path)
                .add_header("Authorization", auth)
                .json(&json!({}))
                .await
        }
    });

    let responses = futures::future::join_all(requests).await;
    let mut accepted = Vec::new();
    for response in responses {
        assert!(
            matches!(
                response.status_code(),
                StatusCode::ACCEPTED | StatusCode::OK
            ),
            "unexpected start status: {}",
            response.status_code()
        );
        let body: ApiResponse<ScanCommandAcceptedResponse> = response.json();
        accepted.push(body.data.context("start response missing data")?);
    }

    let scan_ids: HashSet<Uuid> =
        accepted.iter().map(|response| response.scan_id).collect();
    assert_eq!(scan_ids.len(), 1, "all starts should reuse one scan_id");
    let scan_id = *scan_ids.iter().next().expect("scan id present");
    assert_eq!(
        accepted
            .iter()
            .filter(|response| {
                response.disposition == ScanStartDisposition::Created
            })
            .count(),
        1,
        "one request should create the durable run"
    );
    assert!(accepted.iter().all(|response| {
        response.mode == ScanRunMode::Manual
            && response.status == ScanLifecycleStatus::Running
            && response.run_key == ScanRunMode::Manual.run_key(library_id)
    }));

    let active_ids = active_run_ids(&pool, library_id, "manual").await?;
    assert_eq!(active_ids, vec![scan_id]);

    let active = active_scans(&server, &token).await?;
    assert_eq!(active.count, 1);
    assert_eq!(active.scans.len(), 1);
    assert_eq!(active.scans[0].scan_id, scan_id);
    assert_eq!(active.scans[0].library_id, library_id);
    assert_eq!(active.scans[0].mode, ScanRunMode::Manual);

    drop(library_root);
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn rehydrate_active_runs_restores_identity_and_job_correlation(
    pool: PgPool,
) -> Result<()> {
    let app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (_router, state, _app_tempdir) = app.into_parts();
    let library_root = tempfile::tempdir()?;
    std::fs::create_dir(library_root.path().join("Example Movie"))?;
    let library_id =
        create_registered_library(&state, library_root.path().to_path_buf())
            .await?;
    let correlation_id = Uuid::now_v7();

    let accepted = state
        .scan_control()
        .start_library_scan(
            library_id,
            Some(correlation_id),
            ScanRunMode::Manual,
        )
        .await?;
    assert_eq!(accepted.correlation_id, correlation_id);
    let persisted_jobs =
        wait_for_job_correlation(&pool, library_id, correlation_id).await?;
    assert_eq!(persisted_jobs, 1);

    let recovered_app =
        build_test_app_with_hooks(pool.clone(), &NoopStartupHooks).await?;
    let (_router, recovered_state, _recovered_tempdir) =
        recovered_app.into_parts();
    let restored = recovered_state
        .scan_control()
        .rehydrate_active_runs()
        .await?;
    assert_eq!(restored, 1);

    let snapshots = recovered_state.scan_control().active_scans().await;
    assert_eq!(snapshots.len(), 1);
    let snapshot = &snapshots[0];
    assert_eq!(snapshot.scan_id, accepted.scan_id);
    assert_eq!(snapshot.library_id, library_id);
    assert_eq!(snapshot.correlation_id, correlation_id);
    assert_eq!(snapshot.mode, ScanRunMode::Manual);
    assert_eq!(snapshot.status, ScanLifecycleStatus::Running.into());

    let restored_again = recovered_state
        .scan_control()
        .rehydrate_active_runs()
        .await?;
    assert_eq!(restored_again, 0);
    let snapshots_after_rehydrate =
        recovered_state.scan_control().active_scans().await;
    assert_eq!(snapshots_after_rehydrate.len(), 1);
    assert_eq!(snapshots_after_rehydrate[0].scan_id, accepted.scan_id);
    assert_eq!(snapshots_after_rehydrate[0].correlation_id, correlation_id);

    let events = recovered_state
        .scan_control()
        .events(&accepted.scan_id)
        .await?;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload.scan_id, accepted.scan_id);
    assert_eq!(events[0].payload.correlation_id, correlation_id);

    let correlated_jobs =
        wait_for_job_correlation(&pool, library_id, correlation_id).await?;
    assert_eq!(correlated_jobs, 1);

    drop(library_root);
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn scan_commands_reject_wrong_library_and_reuse_compatible_states(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _app_tempdir) = build_server(pool.clone()).await?;
    let token = register_user(&server, "scan_commands_user").await?;
    let library_root = tempfile::tempdir()?;
    let other_root = tempfile::tempdir()?;
    let library_id =
        create_registered_library(&state, library_root.path().to_path_buf())
            .await?;
    let other_library_id =
        create_registered_library(&state, other_root.path().to_path_buf())
            .await?;
    let started = start_scan(&server, &token, library_id, None).await?;

    for route in [
        v1::libraries::scans::PAUSE,
        v1::libraries::scans::RESUME,
        v1::libraries::scans::CANCEL,
    ] {
        let response = post_scan_command(
            &server,
            &token,
            route,
            other_library_id,
            started.scan_id,
        )
        .await;
        response.assert_status(StatusCode::BAD_REQUEST);
        let body: ApiResponse<()> = response.json();
        assert_eq!(body.error.as_deref(), Some("scan_library_mismatch"));
    }

    let paused = post_scan_command(
        &server,
        &token,
        v1::libraries::scans::PAUSE,
        library_id,
        started.scan_id,
    )
    .await;
    paused.assert_status(StatusCode::ACCEPTED);
    let paused: ApiResponse<ScanCommandAcceptedResponse> = paused.json();
    assert_eq!(
        paused.data.as_ref().expect("pause data").status,
        ScanLifecycleStatus::Paused
    );

    let paused_again = post_scan_command(
        &server,
        &token,
        v1::libraries::scans::PAUSE,
        library_id,
        started.scan_id,
    )
    .await;
    paused_again.assert_status(StatusCode::ACCEPTED);
    let paused_again: ApiResponse<ScanCommandAcceptedResponse> =
        paused_again.json();
    assert_eq!(
        paused_again.data.as_ref().expect("pause data").status,
        ScanLifecycleStatus::Paused
    );

    let resumed = post_scan_command(
        &server,
        &token,
        v1::libraries::scans::RESUME,
        library_id,
        started.scan_id,
    )
    .await;
    resumed.assert_status(StatusCode::ACCEPTED);
    let resumed: ApiResponse<ScanCommandAcceptedResponse> = resumed.json();
    assert_eq!(
        resumed.data.as_ref().expect("resume data").status,
        ScanLifecycleStatus::Running
    );

    let resumed_again = post_scan_command(
        &server,
        &token,
        v1::libraries::scans::RESUME,
        library_id,
        started.scan_id,
    )
    .await;
    resumed_again.assert_status(StatusCode::ACCEPTED);
    let resumed_again: ApiResponse<ScanCommandAcceptedResponse> =
        resumed_again.json();
    assert_eq!(
        resumed_again.data.as_ref().expect("resume data").status,
        ScanLifecycleStatus::Running
    );

    let canceled = post_scan_command(
        &server,
        &token,
        v1::libraries::scans::CANCEL,
        library_id,
        started.scan_id,
    )
    .await;
    canceled.assert_status(StatusCode::ACCEPTED);
    let canceled: ApiResponse<ScanCommandAcceptedResponse> = canceled.json();
    assert_eq!(
        canceled.data.as_ref().expect("cancel data").status,
        ScanLifecycleStatus::Canceled
    );

    let active_ids = active_run_ids(&pool, library_id, "manual").await?;
    assert!(active_ids.is_empty());

    drop(library_root);
    drop(other_root);
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn manual_and_maintenance_starts_keep_separate_watch_mode_runs(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _app_tempdir) = build_server(pool.clone()).await?;
    let token = register_user(&server, "scan_modes_user").await?;
    let library_root = tempfile::tempdir()?;
    let library_id =
        create_registered_library(&state, library_root.path().to_path_buf())
            .await?;

    let manual = start_scan(&server, &token, library_id, None).await?;
    let maintenance =
        start_scan(&server, &token, library_id, Some("maintenance")).await?;

    assert_ne!(manual.scan_id, maintenance.scan_id);
    assert_eq!(manual.mode, ScanRunMode::Manual);
    assert_eq!(maintenance.mode, ScanRunMode::Maintenance);
    assert_eq!(manual.run_key, ScanRunMode::Manual.run_key(library_id));
    assert_eq!(
        maintenance.run_key,
        ScanRunMode::Maintenance.run_key(library_id)
    );

    assert_eq!(active_run_ids(&pool, library_id, "manual").await?.len(), 1);
    assert_eq!(
        active_run_ids(&pool, library_id, "maintenance")
            .await?
            .len(),
        1
    );

    let active = active_scans(&server, &token).await?;
    let scan_ids: HashSet<_> = active
        .scans
        .iter()
        .map(|snapshot| snapshot.scan_id)
        .collect();
    assert_eq!(active.count, 2);
    assert!(scan_ids.contains(&manual.scan_id));
    assert!(scan_ids.contains(&maintenance.scan_id));

    drop(library_root);
    Ok(())
}
