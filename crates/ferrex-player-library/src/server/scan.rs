use crate::scan_dashboard::{
    ScanDashboardOverviewPayload, ScanDashboardRunPayload, is_active_status,
};
use ferrex_core::player_prelude::{
    ActiveScansResponse, LatestProgressResponse, LibraryId,
    ScanCommandAcceptedResponse, ScanCommandRequest, ScanRecoveryRequest,
    ScanRecoveryResponse, ScanSnapshotDto, StartScanRequest,
};

use ferrex_player_api::services::api::ApiService;

use anyhow::anyhow;
use std::sync::Arc;
use uuid::Uuid;

pub async fn start_library_scan(
    client: Arc<dyn ApiService>,
    library_id: LibraryId,
    correlation_id: Option<Uuid>,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .start_library_scan(
            library_id,
            StartScanRequest {
                correlation_id,
                mode: None,
            },
        )
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn pause_library_scan(
    client: Arc<dyn ApiService>,
    library_id: LibraryId,
    scan_id: Uuid,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .pause_library_scan(library_id, ScanCommandRequest { scan_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn resume_library_scan(
    client: Arc<dyn ApiService>,
    library_id: LibraryId,
    scan_id: Uuid,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .resume_library_scan(library_id, ScanCommandRequest { scan_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn cancel_library_scan(
    client: Arc<dyn ApiService>,
    library_id: LibraryId,
    scan_id: Uuid,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .cancel_library_scan(library_id, ScanCommandRequest { scan_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn fetch_active_scans(
    client: Arc<dyn ApiService>,
) -> Result<Vec<ScanSnapshotDto>, anyhow::Error> {
    let response: ActiveScansResponse = client
        .fetch_active_scans()
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    Ok(response.scans)
}

pub async fn fetch_latest_scan_progress(
    client: Arc<dyn ApiService>,
    scan_id: Uuid,
) -> Result<Option<LatestProgressResponse>, anyhow::Error> {
    let response = client
        .fetch_latest_scan_progress(scan_id)
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    Ok(Some(response))
}

pub async fn fetch_scan_dashboard_overview(
    client: Arc<dyn ApiService>,
) -> Result<ScanDashboardOverviewPayload, anyhow::Error> {
    let health = client
        .fetch_scanner_health()
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    let recent_runs = client
        .fetch_scan_runs(None, None, 50, 0)
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    let mut active_runs = Vec::new();
    for status in ["pending", "running", "paused"] {
        let mut page = client
            .fetch_scan_runs(None, Some(status.to_string()), 50, 0)
            .await
            .map_err(|e| anyhow!(e.to_string()))?
            .runs;
        active_runs.append(&mut page);
    }
    active_runs.retain(|run| is_active_status(&run.status));
    active_runs.sort_by(|a, b| b.last_event_at.cmp(&a.last_event_at));

    Ok(ScanDashboardOverviewPayload {
        health,
        active_runs,
        recent_runs,
    })
}

pub async fn fetch_scan_dashboard_run(
    client: Arc<dyn ApiService>,
    scan_id: Uuid,
) -> Result<ScanDashboardRunPayload, anyhow::Error> {
    let detail = client
        .fetch_scan_run_detail(scan_id)
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    let events = client
        .fetch_scan_run_events(scan_id, None, 200)
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    let failures = client
        .fetch_scan_run_failures(scan_id, 100, 0, false)
        .await
        .map_err(|e| anyhow!(e.to_string()))?;

    Ok(ScanDashboardRunPayload {
        detail,
        events,
        failures,
    })
}

pub async fn recover_scan_path(
    client: Arc<dyn ApiService>,
    request: ScanRecoveryRequest,
) -> Result<ScanRecoveryResponse, anyhow::Error> {
    client
        .recover_scan_path(request)
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan_dashboard::{
        ScanDashboardState, scan_failure_display_text, scan_status_display_text,
    };
    use chrono::{DateTime, TimeZone, Utc};
    use ferrex_core::{
        api::scan::{IncrementalScanStatusView, ScanQueueDepths},
        player_prelude::{
            ScanFailureDto, ScanPageMeta, ScanRunDetailResponse, ScanRunDto,
            ScanRunEventDto, ScannerHealthResponse,
        },
    };
    use ferrex_player_api::{
        services::api::ApiService, testing::stubs::api::TestApiService,
    };

    fn id(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }

    fn library_id() -> LibraryId {
        LibraryId(id(10))
    }

    fn ts(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }

    fn page(count: usize) -> ScanPageMeta {
        ScanPageMeta::new(200, 0, count, count)
    }

    fn health() -> ScannerHealthResponse {
        ScannerHealthResponse {
            queue_depths: ScanQueueDepths {
                folder_scan: 0,
                manifest_scan: 0,
                analyze: 0,
                metadata: 0,
                index: 0,
                image_fetch: 0,
            },
            active_scans: 1,
            retained_runs: 2,
            failed_runs: 1,
            incremental: IncrementalScanStatusView::default(),
        }
    }

    fn run(scan_id: Uuid, status: &str, sequence: u64) -> ScanRunDto {
        let display = scan_status_display_text(status);
        ScanRunDto {
            scan_id,
            library_id: library_id(),
            source: "manual".to_string(),
            status: status.to_string(),
            status_label: display.label,
            status_message: display.message,
            completed_items: sequence,
            total_items: 10,
            retrying_items: if status == "failed" { 2 } else { 0 },
            dead_lettered_items: if status == "failed" { 1 } else { 0 },
            correlation_id: id(100 + u128::from(sequence)),
            idempotency_key: format!("scan:{scan_id}:{sequence}"),
            current_path: Some("/media/movies/Broken.mkv".to_string()),
            started_at: ts(1),
            last_event_at: ts(sequence as i64),
            terminal_at: if status == "failed" {
                Some(ts(sequence as i64))
            } else {
                None
            },
            sequence,
            has_failures: status == "failed",
        }
    }

    fn event(
        scan_id: Uuid,
        sequence: u64,
        event_kind: &str,
        status: &str,
    ) -> ScanRunEventDto {
        let display = scan_status_display_text(status);
        ScanRunEventDto {
            event_id: id(200 + u128::from(sequence)),
            scan_id,
            library_id: library_id(),
            sequence,
            event_kind: event_kind.to_string(),
            status: status.to_string(),
            status_label: display.label,
            status_message: display.message,
            correlation_id: id(30),
            idempotency_key: format!("scan:{scan_id}:{sequence}"),
            subject_key: Some("/media/movies/Broken.mkv".to_string()),
            current_path: Some("/media/movies/Broken.mkv".to_string()),
            occurred_at: ts(sequence as i64),
            completed_items: sequence,
            total_items: 10,
            retrying_items: if status == "failed" { 2 } else { 0 },
            dead_lettered_items: if status == "failed" { 1 } else { 0 },
            payload: serde_json::json!({
                "correlation_id": id(30),
                "failed_after_retries": status == "failed",
            }),
        }
    }

    fn failure(scan_id: Uuid) -> ScanFailureDto {
        let display = scan_failure_display_text(
            "filesystem_permission",
            "scan.folder_permission_denied",
        );
        ScanFailureDto {
            scan_id,
            library_id: library_id(),
            subject_key: "/media/movies/Broken.mkv".to_string(),
            category: "filesystem_permission".to_string(),
            category_label: display.label,
            message_code: "scan.folder_permission_denied".to_string(),
            message: display.message,
            occurrences: 3,
            first_seen_at: ts(2),
            last_seen_at: ts(10),
            retryable: true,
            debug: None,
        }
    }

    #[tokio::test]
    async fn dashboard_contract_reconstructs_failed_after_retries_timeline()
    -> anyhow::Result<()> {
        let active_id = id(1);
        let failed_id = id(2);
        let active = run(active_id, "running", 3);
        let failed = run(failed_id, "failed", 10);
        let stub = TestApiService::new("https://ferrex.test");
        stub.set_scan_health(health());
        stub.set_scan_runs(vec![active.clone(), failed.clone()]);
        stub.insert_scan_run_detail(ScanRunDetailResponse {
            run: failed.clone(),
            terminal_summary: serde_json::json!({
                "status": "needs_attention",
                "failed_after_retries": 1,
                "message_code": "scan.folder_permission_denied",
            }),
        });
        stub.insert_scan_run_events(
            failed_id,
            vec![
                event(failed_id, 1, "started", "running"),
                event(failed_id, 10, "failed", "failed"),
            ],
        );
        stub.insert_scan_run_failures(failed_id, vec![failure(failed_id)]);
        let api: Arc<dyn ApiService> = Arc::new(stub);

        let overview = fetch_scan_dashboard_overview(Arc::clone(&api)).await?;
        assert_eq!(overview.active_runs.len(), 1);
        assert_eq!(overview.active_runs[0].scan_id, active_id);
        assert!(
            overview.recent_runs.runs.iter().any(|run| {
                run.scan_id == failed_id && run.status == "failed"
            })
        );

        let run_payload =
            fetch_scan_dashboard_run(Arc::clone(&api), failed_id).await?;
        assert_eq!(run_payload.events.events.len(), 2);
        assert_eq!(run_payload.events.page, page(2));
        assert_eq!(run_payload.failures.failures.len(), 1);
        assert_eq!(
            run_payload.failures.failures[0].category_label,
            "Permission issue"
        );
        assert!(
            run_payload.failures.failures[0]
                .message
                .contains("permissions")
        );
        assert!(
            !run_payload.failures.failures[0]
                .message
                .to_ascii_lowercase()
                .contains("dead-letter")
        );

        let mut dashboard = ScanDashboardState::default();
        dashboard.apply_overview(overview);
        dashboard.apply_run_payload(run_payload);

        let terminal = dashboard
            .terminal_summaries
            .get(&failed_id)
            .expect("terminal failed run is retained for player display");
        assert_eq!(terminal.status_label, "Failed");
        assert_eq!(terminal.dead_lettered_items, 1);
        assert_eq!(
            dashboard.selected_terminal_summary.as_ref().unwrap()["status"],
            "needs_attention"
        );
        Ok(())
    }

    #[tokio::test]
    async fn recovery_contract_uses_stable_correlation_without_mutating_libraries()
    -> anyhow::Result<()> {
        let stub = TestApiService::new("https://ferrex.test");
        let before_libraries = stub.fetch_libraries().await?.len();
        let api: Arc<dyn ApiService> = Arc::new(stub.clone());
        let correlation_id = id(99);
        let request = ScanRecoveryRequest {
            library_id: library_id(),
            path: "/media/movies/Broken.mkv".to_string(),
            correlation_id: Some(correlation_id),
        };

        let first =
            recover_scan_path(Arc::clone(&api), request.clone()).await?;
        let second =
            recover_scan_path(Arc::clone(&api), request.clone()).await?;

        assert!(first.accepted);
        assert!(second.accepted);
        assert_eq!(first.idempotency_key, correlation_id.to_string());
        assert_eq!(second.idempotency_key, correlation_id.to_string());
        assert_eq!(first.normalized_path, request.path.as_str());
        assert_eq!(second.normalized_path, request.path.as_str());

        let requests = stub.scan_recovery_requests();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|recorded| {
            recorded.library_id == request.library_id
                && recorded.path == request.path
                && recorded.correlation_id == request.correlation_id
        }));
        assert_eq!(stub.fetch_libraries().await?.len(), before_libraries);
        Ok(())
    }
}
