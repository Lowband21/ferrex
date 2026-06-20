use axum::response::sse::{Event, KeepAlive};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Sse},
};
use base64::{
    Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD,
};
use ferrex_core::api::ScanQueueDepths;
use ferrex_core::api::types::{
    ActiveScansResponse, ApiResponse, LatestProgressResponse,
    ScanCommandAcceptedResponse, ScanCommandRequest, ScanFailureDebugDetails,
    ScanFailureDto, ScanPageMeta, ScanRecoveryRequest, ScanRecoveryResponse,
    ScanReplayGapResponse, ScanReplayInfo, ScanRunDetailResponse, ScanRunDto,
    ScanRunEventDto, ScanRunEventsPageResponse, ScanRunFailuresPageResponse,
    ScanRunListResponse, ScanSnapshotDto, ScanStartDisposition,
    ScannerHealthResponse, StartScanRequest, display_text_for_scan_failure,
    display_text_for_scan_status,
};
use ferrex_core::domain::scan::manifest::{
    DEFAULT_MANIFEST_WALK_BATCH_LIMIT, DEFAULT_MANIFEST_WALK_MAX_DEPTH,
    DEFAULT_MANIFEST_WALK_PARTITION_LIMIT, ManifestDiagnosticReason,
};
use ferrex_core::error::MediaError;
use ferrex_core::scan_observability::{
    ScanRunEventRecord, ScanRunFailurePageRequest, ScanRunFailureSummary,
    ScanRunPageRequest, ScanRunRecord, ScanRunStatus,
};
use ferrex_core::types::{LibraryId, MediaEvent, ScanProgressEvent};
use rkyv::{rancor::Error as RkyvError, to_bytes};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{convert::Infallible, pin::Pin, sync::Arc, time::Duration};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tracing::warn;
use uuid::Uuid;

use crate::infra::app_state::AppState;
use crate::infra::demo_mode;
use crate::infra::scan::scan_manager::{
    ScanBroadcastFrame, ScanCommandAccepted, ScanControlError,
    ScanControlPlane, ScanHistoryEntry, ScanRecoveryAccepted, ScanReplayGap,
    ScanRunEventReplayPage,
};
use ferrex_core::api::scan::{
    BudgetConfigView, BulkModeView, IncrementalScanPolicyView,
    IncrementalScanStatusView, LeaseConfigView, MaintenanceConfigView,
    ManifestScanConfigView, MetadataLimitsView, OrchestratorConfigView,
    QueueConfigView, RetryConfigView, ScanConfig, ScanMetrics, WatchConfigView,
};

const LAST_EVENT_ID_HEADER: &str = "last-event-id";
const MEDIA_EVENT_REPLAY_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Debug)]
pub struct ScanHttpError {
    status: StatusCode,
    message: String,
    replay_gap: Option<ScanReplayGapResponse>,
}

impl ScanHttpError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            replay_gap: None,
        }
    }
}

impl From<ScanControlError> for ScanHttpError {
    fn from(error: ScanControlError) -> Self {
        let status = error.status_code();
        let replay_gap = match &error {
            ScanControlError::ReplayGap(gap) => {
                Some(scan_replay_gap_response(gap))
            }
            _ => None,
        };
        let message = error.message();
        Self {
            status,
            message,
            replay_gap,
        }
    }
}

impl IntoResponse for ScanHttpError {
    fn into_response(self) -> axum::response::Response {
        if let Some(replay_gap) = self.replay_gap {
            let payload = Json(json!({
                "status": "error",
                "error": self.message,
                "message": replay_gap.recovery_hint,
                "recoverable": true,
                "recovery": replay_gap,
            }));
            return (self.status, payload).into_response();
        }

        let payload = Json(ApiResponse::<()>::error(self.message));
        (self.status, payload).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ProgressQuery {
    pub scan_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct MediaEventsQuery {
    pub last_sequence: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct RunsQuery {
    pub library_id: Option<Uuid>,
    pub status: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct RunEventsQuery {
    pub after_sequence: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct RunFailuresQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    #[serde(default)]
    pub debug: bool,
}

#[derive(Debug, Serialize)]
pub struct ScanHistoryResponse {
    pub history: Vec<ScanHistoryEntry>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct ScanEventsResponse {
    pub scan_id: Uuid,
    pub events: Vec<ScanBroadcastFrame>,
}

pub async fn start_scan_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
    Json(request): Json<StartScanRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(ScanHttpError::new(
            StatusCode::NOT_FOUND,
            "Library not found",
        ));
    }

    let accepted = state
        .scan_control()
        .start_library_scan(
            LibraryId(library_id),
            request.correlation_id,
            request.effective_mode(),
        )
        .await?;
    let status = scan_start_status(accepted.disposition);

    Ok((
        status,
        Json(ApiResponse::success(scan_command_response(accepted))),
    ))
}

pub async fn pause_scan_handler(
    State(state): State<AppState>,
    Path((library_id,)): Path<(Uuid,)>,
    Json(request): Json<ScanCommandRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    let accepted = state
        .scan_control()
        .pause_scan(LibraryId(library_id), &request.scan_id)
        .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success(scan_command_response(accepted))),
    ))
}

pub async fn resume_scan_handler(
    State(state): State<AppState>,
    Path((library_id,)): Path<(Uuid,)>,
    Json(request): Json<ScanCommandRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    let accepted = state
        .scan_control()
        .resume_scan(LibraryId(library_id), &request.scan_id)
        .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success(scan_command_response(accepted))),
    ))
}

pub async fn cancel_scan_handler(
    State(state): State<AppState>,
    Path((library_id,)): Path<(Uuid,)>,
    Json(request): Json<ScanCommandRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    let accepted = state
        .scan_control()
        .cancel_scan(LibraryId(library_id), &request.scan_id)
        .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success(scan_command_response(accepted))),
    ))
}

fn scan_start_status(disposition: ScanStartDisposition) -> StatusCode {
    match disposition {
        ScanStartDisposition::Created => StatusCode::ACCEPTED,
        ScanStartDisposition::Reused => StatusCode::OK,
    }
}

fn scan_command_response(
    accepted: ScanCommandAccepted,
) -> ScanCommandAcceptedResponse {
    ScanCommandAcceptedResponse {
        scan_id: accepted.scan_id,
        correlation_id: accepted.correlation_id,
        status: accepted.status.into(),
        mode: accepted.mode,
        idempotency_key: accepted.idempotency_key,
        run_key: accepted.run_key,
        disposition: accepted.disposition,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_status_distinguishes_created_and_reused_runs() {
        assert_eq!(
            scan_start_status(ScanStartDisposition::Created),
            StatusCode::ACCEPTED
        );
        assert_eq!(
            scan_start_status(ScanStartDisposition::Reused),
            StatusCode::OK
        );
    }
}

pub async fn active_scans_handler(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ActiveScansResponse>>, ScanHttpError> {
    let scans = state.scan_control().active_scans().await;
    let count = scans.len();
    let dto_scans: Vec<ScanSnapshotDto> =
        scans.into_iter().map(Into::into).collect();
    let incremental = incremental_status(&state).await?;
    Ok(Json(ApiResponse::success(ActiveScansResponse {
        scans: dto_scans,
        count,
        incremental,
    })))
}

pub async fn scan_history_handler(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<ApiResponse<ScanHistoryResponse>>, ScanHttpError> {
    let history = state
        .scan_control()
        .history(query.limit.unwrap_or(25))
        .await;
    let count = history.len();
    Ok(Json(ApiResponse::success(ScanHistoryResponse {
        history,
        count,
    })))
}

pub async fn latest_progress_handler(
    State(state): State<AppState>,
    Query(query): Query<ProgressQuery>,
) -> Result<Json<ApiResponse<LatestProgressResponse>>, ScanHttpError> {
    let frames = state.scan_control().events(&query.scan_id).await?;
    let latest = frames.last().map(|frame| frame.payload.clone());
    Ok(Json(ApiResponse::success(LatestProgressResponse {
        scan_id: query.scan_id,
        latest,
    })))
}

pub async fn scan_events_handler(
    State(state): State<AppState>,
    Path(scan_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ScanEventsResponse>>, ScanHttpError> {
    let events = state.scan_control().events(&scan_id).await?;
    Ok(Json(ApiResponse::success(ScanEventsResponse {
        scan_id,
        events,
    })))
}

pub async fn scan_runs_handler(
    State(state): State<AppState>,
    Query(query): Query<RunsQuery>,
) -> Result<Json<ApiResponse<ScanRunListResponse>>, ScanHttpError> {
    let limit = query.limit.unwrap_or(25).clamp(1, 100);
    let offset = query.offset.unwrap_or(0);
    let status = parse_run_status_filter(query.status.as_deref())?;
    let page = state
        .scan_control()
        .runs_page(ScanRunPageRequest {
            library_id: query.library_id.map(LibraryId),
            status,
            limit: limit as i64,
            offset: offset.min(i64::MAX as usize) as i64,
        })
        .await?;
    let total = page.total.max(0) as usize;
    let runs = page
        .runs
        .into_iter()
        .map(|run| scan_run_to_dto(run, None))
        .collect::<Vec<_>>();
    let page_meta = ScanPageMeta::new(limit, offset, runs.len(), total);

    Ok(Json(ApiResponse::success(ScanRunListResponse {
        runs,
        page: page_meta,
    })))
}

pub async fn scan_run_detail_handler(
    State(state): State<AppState>,
    Path(scan_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ScanRunDetailResponse>>, ScanHttpError> {
    let run = state.scan_control().run_detail(scan_id).await?;
    let terminal_summary = run.terminal_summary.clone();
    Ok(Json(ApiResponse::success(ScanRunDetailResponse {
        run: scan_run_to_dto(run, None),
        terminal_summary,
    })))
}

pub async fn scan_run_events_handler(
    State(state): State<AppState>,
    Path(scan_id): Path<Uuid>,
    Query(query): Query<RunEventsQuery>,
) -> Result<Json<ApiResponse<ScanRunEventsPageResponse>>, ScanHttpError> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let replay = state
        .scan_control()
        .run_events_page(scan_id, query.after_sequence, limit as i64)
        .await?;
    let events = replay
        .events
        .iter()
        .cloned()
        .map(scan_event_to_dto)
        .collect::<Vec<_>>();
    let page = ScanPageMeta::new(
        limit,
        query.after_sequence.unwrap_or(0) as usize,
        events.len(),
        replay.bounds.max_sequence.unwrap_or(0).max(0) as usize,
    );

    Ok(Json(ApiResponse::success(ScanRunEventsPageResponse {
        scan_id,
        events,
        page,
        replay: Some(replay_info(&replay)),
    })))
}

pub async fn scan_run_failures_handler(
    State(state): State<AppState>,
    Path(scan_id): Path<Uuid>,
    Query(query): Query<RunFailuresQuery>,
) -> Result<Json<ApiResponse<ScanRunFailuresPageResponse>>, ScanHttpError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let offset = query.offset.unwrap_or(0);
    let page = state
        .scan_control()
        .run_failures_page(ScanRunFailurePageRequest {
            run_id: scan_id,
            limit: limit as i64,
            offset: offset.min(i64::MAX as usize) as i64,
        })
        .await?;
    let total = page.total.max(0) as usize;
    let failures = page
        .failures
        .into_iter()
        .map(|failure| scan_failure_to_dto(failure, query.debug))
        .collect::<Vec<_>>();

    Ok(Json(ApiResponse::success(ScanRunFailuresPageResponse {
        scan_id,
        page: ScanPageMeta::new(limit, offset, failures.len(), total),
        failures,
    })))
}

pub async fn scanner_health_handler(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ScannerHealthResponse>>, ScanHttpError> {
    let queue_depths = state
        .scan_control()
        .orchestrator()
        .queue_depths()
        .await
        .map_err(|e: MediaError| {
            ScanHttpError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
    let active_scans = state.scan_control().active_scans().await.len();
    let incremental = incremental_status(&state).await?;
    let retained = state
        .scan_control()
        .runs_page(ScanRunPageRequest {
            library_id: None,
            status: None,
            limit: 1,
            offset: 0,
        })
        .await?;
    let failed = state
        .scan_control()
        .runs_page(ScanRunPageRequest {
            library_id: None,
            status: Some(ScanRunStatus::Failed),
            limit: 1,
            offset: 0,
        })
        .await?;

    Ok(Json(ApiResponse::success(ScannerHealthResponse {
        queue_depths,
        active_scans,
        retained_runs: retained.total.max(0) as usize,
        failed_runs: failed.total.max(0) as usize,
        incremental,
    })))
}

pub async fn scan_recovery_handler(
    State(state): State<AppState>,
    Json(request): Json<ScanRecoveryRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    let accepted = state
        .scan_control()
        .recover_path(request.library_id, &request.path, request.correlation_id)
        .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success(recovery_response(accepted))),
    ))
}

pub async fn scan_progress_sse_handler(
    State(state): State<AppState>,
    Path(scan_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>,
    ScanHttpError,
> {
    let last_sequence = headers
        .get(LAST_EVENT_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<u64>().ok());
    let stream = build_scan_progress_stream(
        Arc::clone(&state.scan_control()),
        scan_id,
        last_sequence,
    )
    .await?;

    Ok(Sse::new(stream).keep_alive(default_keep_alive()))
}

pub async fn scan_metrics_handler(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ScanMetrics>>, ScanHttpError> {
    let depths: ScanQueueDepths = state
        .scan_control()
        .orchestrator()
        .queue_depths()
        .await
        .map_err(|e: MediaError| {
            ScanHttpError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
    let active = state.scan_control().active_scans().await.len();
    let incremental = incremental_status(&state).await?;
    Ok(Json(ApiResponse::success(ScanMetrics {
        queue_depths: depths,
        active_scans: active,
        manifest: incremental.manifest.clone(),
        incremental,
    })))
}

pub async fn scan_config_handler(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ScanConfig>>, ScanHttpError> {
    let cfg = state.scan_control().orchestrator().config();
    let scanner = &state.config().scanner;
    // Map internal config to view that is feature-agnostic
    let view = OrchestratorConfigView {
        queue: QueueConfigView {
            max_parallel_scans: cfg.queue.max_parallel_scans,
            max_parallel_series_resolve: cfg.queue.max_parallel_series_resolve,
            max_parallel_analyses: cfg.queue.max_parallel_analyses,
            max_parallel_metadata: cfg.queue.max_parallel_metadata,
            max_parallel_index: cfg.queue.max_parallel_index,
            max_parallel_image_fetch: cfg.queue.max_parallel_image_fetch,
            max_parallel_scans_per_device: cfg
                .queue
                .max_parallel_scans_per_device,
            default_library_cap: cfg.queue.default_library_cap,
        },
        retry: RetryConfigView {
            max_attempts: cfg.retry.max_attempts,
            backoff_base_ms: cfg.retry.backoff_base_ms,
            backoff_max_ms: cfg.retry.backoff_max_ms,
            fast_retry_attempts: cfg.retry.fast_retry_attempts,
            fast_retry_factor: cfg.retry.fast_retry_factor,
            heavy_library_attempt_threshold: cfg
                .retry
                .heavy_library_attempt_threshold,
            heavy_library_slowdown_factor: cfg
                .retry
                .heavy_library_slowdown_factor,
            jitter_ratio: cfg.retry.jitter_ratio,
            jitter_min_ms: cfg.retry.jitter_min_ms,
        },
        metadata_limits: MetadataLimitsView {
            max_concurrency: cfg.metadata_limits.max_concurrency,
            max_qps: cfg.metadata_limits.max_qps,
        },
        bulk_mode: BulkModeView {
            speedup_factor: cfg.bulk_mode.speedup_factor,
            maintenance_partition_count: cfg
                .bulk_mode
                .maintenance_partition_count,
        },
        maintenance: MaintenanceConfigView {
            enabled: cfg.maintenance.enabled,
            tick_interval_ms: cfg.maintenance.tick_interval_ms,
            max_jobs_per_library: cfg.maintenance.max_jobs_per_library,
            max_root_entries_per_library: cfg
                .maintenance
                .max_root_entries_per_library,
            error_backoff_ms: cfg.maintenance.error_backoff_ms,
            run_stall_timeout_ms: cfg.maintenance.run_stall_timeout_ms,
            scan_run_retention_days: cfg.maintenance.scan_run_retention_days,
        },
        lease: LeaseConfigView {
            lease_ttl_secs: cfg.lease.lease_ttl_secs,
        },
        watch: WatchConfigView {
            debounce_window_ms: cfg.watch.debounce_window_ms,
            max_batch_events: cfg.watch.max_batch_events,
            strategy: watch_strategy_label(cfg.watch.strategy),
            poll_interval_ms: cfg.watch.poll_interval_ms,
            poll_backoff_max_ms: cfg.watch.poll_backoff_max_ms,
        },
        budget: BudgetConfigView {
            library_scan_limit: cfg.budget.library_scan_limit,
        },
    };
    let incremental_policy = IncrementalScanPolicyView {
        default_auto_scan: true,
        default_watch_for_changes: true,
        default_scan_interval_minutes: 60,
        watch_strategy: watch_strategy_label(cfg.watch.strategy),
        poll_interval_ms: cfg.watch.poll_interval_ms,
        debounce_window_ms: cfg.watch.debounce_window_ms,
        max_batch_events: cfg.watch.max_batch_events,
        maintenance_enabled: cfg.maintenance.enabled,
        maintenance_tick_interval_ms: cfg.maintenance.tick_interval_ms,
        maintenance_max_jobs_per_library: cfg.maintenance.max_jobs_per_library,
        maintenance_max_root_entries_per_library: cfg
            .maintenance
            .max_root_entries_per_library,
        maintenance_scan_run_retention_days: cfg
            .maintenance
            .scan_run_retention_days,
        media_extensions: scanner.video_extensions.clone(),
        ignored_extensions: scanner.ignored_extensions.clone(),
        ignored_path_patterns: scanner.ignored_path_patterns.clone(),
    };

    Ok(Json(ApiResponse::success(ScanConfig {
        orchestrator: view,
        incremental_policy,
        manifest: manifest_scan_config_view(),
    })))
}

fn manifest_scan_config_view() -> ManifestScanConfigView {
    ManifestScanConfigView {
        max_entries_per_batch: DEFAULT_MANIFEST_WALK_BATCH_LIMIT,
        max_entries_per_partition: DEFAULT_MANIFEST_WALK_PARTITION_LIMIT,
        max_depth: DEFAULT_MANIFEST_WALK_MAX_DEPTH,
        supported_movie_layouts: vec![
            "/Movies/Alien.mkv".to_string(),
            "/Movies/Alien (1979)/Alien.mkv".to_string(),
        ],
        supported_series_layouts: vec![
            "/Series/Fringe/Season 01/S01E01.mkv".to_string(),
            "/Series/Fringe/Specials/S00E01.mkv".to_string(),
            "/Series/Fringe/S01E02.mkv".to_string(),
        ],
        diagnostic_codes: manifest_diagnostic_codes(),
    }
}

fn manifest_diagnostic_codes() -> Vec<String> {
    ManifestDiagnosticReason::all()
        .iter()
        .map(|reason| reason.code().to_string())
        .collect()
}

async fn incremental_status(
    state: &AppState,
) -> Result<IncrementalScanStatusView, ScanHttpError> {
    state
        .scan_control()
        .orchestrator()
        .incremental_status()
        .await
        .map_err(|err| {
            ScanHttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                err.to_string(),
            )
        })
}

fn watch_strategy_label(
    strategy: ferrex_core::domain::scan::orchestration::config::WatchStrategy,
) -> String {
    format!("{strategy:?}").to_ascii_lowercase()
}

fn parse_run_status_filter(
    raw: Option<&str>,
) -> Result<Option<ScanRunStatus>, ScanHttpError> {
    raw.map(|value| {
        ScanRunStatus::from_str(value).ok_or_else(|| {
            ScanHttpError::new(
                StatusCode::BAD_REQUEST,
                format!("invalid scan run status filter: {value}"),
            )
        })
    })
    .transpose()
}

fn scan_run_to_dto(
    run: ScanRunRecord,
    has_failures: Option<bool>,
) -> ScanRunDto {
    let status = run.status.as_str().to_string();
    let display = display_text_for_scan_status(&status);
    ScanRunDto {
        scan_id: run.id,
        library_id: run.library_id,
        source: run.source.as_str().to_string(),
        status,
        status_label: display.label,
        status_message: display.message,
        completed_items: run.completed_items.max(0) as u64,
        total_items: run.total_items.max(0) as u64,
        retrying_items: run.retrying_items.max(0) as u64,
        dead_lettered_items: run.dead_lettered_items.max(0) as u64,
        correlation_id: run.correlation_id,
        idempotency_key: run.idempotency_key,
        current_path: run.current_path,
        started_at: run.started_at,
        last_event_at: run.last_event_at,
        terminal_at: run.terminal_at,
        sequence: run.sequence.max(0) as u64,
        has_failures: has_failures.unwrap_or(
            run.dead_lettered_items > 0 || run.status == ScanRunStatus::Failed,
        ),
    }
}

fn scan_event_to_dto(record: ScanRunEventRecord) -> ScanRunEventDto {
    let display = display_text_for_scan_status(&record.status);
    ScanRunEventDto {
        event_id: record.id,
        scan_id: record.run_id,
        library_id: record.library_id,
        sequence: record.sequence.max(0) as u64,
        event_kind: record.event_kind,
        status: record.status,
        status_label: display.label,
        status_message: display.message,
        correlation_id: record.correlation_id,
        idempotency_key: record.idempotency_key,
        subject_key: record.subject_key,
        current_path: record.current_path,
        occurred_at: record.occurred_at,
        completed_items: record.completed_items.max(0) as u64,
        total_items: record.total_items.max(0) as u64,
        retrying_items: record.retrying_items.max(0) as u64,
        dead_lettered_items: record.dead_lettered_items.max(0) as u64,
        payload: record.payload,
    }
}

fn scan_failure_to_dto(
    failure: ScanRunFailureSummary,
    include_debug: bool,
) -> ScanFailureDto {
    let display =
        display_text_for_scan_failure(&failure.category, &failure.message_code);
    let debug = include_debug.then(|| ScanFailureDebugDetails {
        raw_debug_details: failure.raw_debug_details.clone(),
        last_error: failure.last_error.clone(),
        job_id: failure.job_id,
        idempotency_key: failure.idempotency_key.clone(),
    });

    ScanFailureDto {
        scan_id: failure.run_id,
        library_id: failure.library_id,
        subject_key: failure.subject_key,
        category: failure.category,
        category_label: display.label,
        message_code: failure.message_code,
        message: display.message,
        occurrences: failure.occurrences.max(1) as u32,
        first_seen_at: failure.first_seen_at,
        last_seen_at: failure.last_seen_at,
        retryable: failure.retryable,
        debug,
    }
}

fn replay_info(replay: &ScanRunEventReplayPage) -> ScanReplayInfo {
    ScanReplayInfo {
        requested_after_sequence: replay.requested_after_sequence,
        min_available_sequence: replay
            .bounds
            .min_sequence
            .map(|value| value.max(0) as u64),
        max_available_sequence: replay
            .bounds
            .max_sequence
            .map(|value| value.max(0) as u64),
        next_sequence: replay.next_sequence,
        recoverable: true,
        recovery_hint: "Use the latest returned sequence as Last-Event-ID; if the stream reports a gap, refetch this run's events without a cursor.".to_string(),
    }
}

fn scan_replay_gap_response(gap: &ScanReplayGap) -> ScanReplayGapResponse {
    ScanReplayGapResponse {
        scan_id: gap.scan_id,
        requested_after_sequence: gap.requested_after_sequence,
        min_available_sequence: gap.min_available_sequence,
        max_available_sequence: gap.max_available_sequence,
        recoverable: true,
        recovery_hint: "Requested scan events are no longer retained. Refetch the run detail/events without Last-Event-ID, then resume from the newest sequence.".to_string(),
    }
}

fn recovery_response(accepted: ScanRecoveryAccepted) -> ScanRecoveryResponse {
    ScanRecoveryResponse {
        library_id: accepted.library_id,
        path: accepted.original_path,
        normalized_path: accepted.normalized_path,
        job_id: accepted.handle.job_id.0,
        accepted: accepted.handle.accepted,
        merged_into: accepted.handle.merged_into.map(|job_id| job_id.0),
        idempotency_key: accepted.handle.dedupe_key,
        message: if accepted.handle.accepted {
            "Recovery scan enqueued without deleting user data.".to_string()
        } else {
            "Recovery scan already queued; existing retry work was reused."
                .to_string()
        },
    }
}

pub async fn build_scan_progress_stream(
    scan_control: Arc<ScanControlPlane>,
    scan_id: Uuid,
    last_sequence: Option<u64>,
) -> Result<
    Pin<
        Box<
            dyn tokio_stream::Stream<Item = Result<Event, Infallible>>
                + Send
                + 'static,
        >,
    >,
    ScanControlError,
> {
    let replay = scan_control
        .run_events_page(scan_id, last_sequence, 500)
        .await?;
    let receiver = match scan_control.subscribe_scan(scan_id).await {
        Ok(receiver) => Some(receiver),
        Err(ScanControlError::ScanNotFound) => None,
        Err(err) => return Err(err),
    };

    Ok(scan_progress_stream_from_parts(
        replay.events,
        receiver,
        last_sequence,
    ))
}

fn scan_progress_stream_from_parts(
    history: Vec<ScanRunEventRecord>,
    receiver: Option<tokio::sync::broadcast::Receiver<ScanBroadcastFrame>>,
    last_sequence: Option<u64>,
) -> Pin<
    Box<
        dyn tokio_stream::Stream<Item = Result<Event, Infallible>>
            + Send
            + 'static,
    >,
> {
    let mut history_last_sequence = last_sequence.unwrap_or(0);
    let history_events = history
        .into_iter()
        .filter_map(ScanBroadcastFrame::from_observability)
        .filter_map(|frame| {
            history_last_sequence =
                history_last_sequence.max(frame.payload.sequence);
            scan_frame_to_event(frame)
        })
        .map(Ok::<Event, Infallible>)
        .collect::<Vec<_>>();
    let history_stream = tokio_stream::iter(history_events);

    let Some(receiver) = receiver else {
        return Box::pin(history_stream);
    };

    let live_stream = async_stream::stream! {
        let mut live_receiver = BroadcastStream::new(receiver);
        let mut last_seen_sequence = history_last_sequence;
        use tokio_stream::StreamExt;

        while let Some(frame_result) = live_receiver.next().await {
            match frame_result {
                Ok(frame) => {
                    if frame.payload.sequence <= last_seen_sequence {
                        continue;
                    }
                    last_seen_sequence = frame.payload.sequence;
                    if let Some(event) = scan_frame_to_event(frame) {
                        yield Ok::<Event, Infallible>(event);
                    }
                }
                Err(err) => {
                    warn!("scan progress broadcast error: {err}");
                }
            }
        }
    };

    Box::pin(history_stream.chain(live_stream))
}

#[cfg(test)]
fn scan_progress_history_events(
    history: Vec<ScanBroadcastFrame>,
    last_sequence: Option<u64>,
) -> Vec<Result<Event, Infallible>> {
    history
        .into_iter()
        .filter(|frame| {
            last_sequence
                .map(|seq| frame.payload.sequence > seq)
                .unwrap_or(true)
        })
        .filter_map(scan_frame_to_event)
        .map(Ok::<Event, Infallible>)
        .collect::<Vec<_>>()
}

pub async fn media_events_sse_handler(
    State(state): State<AppState>,
    Query(query): Query<MediaEventsQuery>,
    headers: HeaderMap,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let resume_from = query.last_sequence.or_else(|| {
        headers
            .get(LAST_EVENT_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    });

    let scan_control = state.scan_control();
    let receiver = scan_control.subscribe_media_events();

    let history = match resume_from {
        Some(sequence) => {
            scan_control.media_event_history_since_sequence(sequence)
        }
        None => {
            let now = std::time::Instant::now();
            let cutoff =
                now.checked_sub(MEDIA_EVENT_REPLAY_WINDOW).unwrap_or(now);
            scan_control.media_event_history_since_instant(cutoff)
        }
    };

    let mut history_last_sequence = resume_from.unwrap_or(0);
    let history_events = history
        .into_iter()
        .filter_map(|frame| {
            history_last_sequence = history_last_sequence.max(frame.sequence);
            media_frame_to_sse(frame)
        })
        .map(Ok::<Event, Infallible>)
        .collect::<Vec<_>>();
    let history_stream = tokio_stream::iter(history_events);

    // Stream media events, but ensure primary poster availability for new movies/series
    let stream = async_stream::stream! {
        let mut live = BroadcastStream::new(receiver);
        use tokio_stream::StreamExt;

        let mut last_seen_sequence = history_last_sequence;
        while let Some(item) = live.next().await {
            match item {
                Ok(frame) => {
                    if frame.sequence <= last_seen_sequence {
                        continue;
                    }
                    last_seen_sequence = frame.sequence;
                    //let event = maybe_prepare_and_refresh(&state, event).await;
                    if let Some(sse) = media_frame_to_sse(frame) {
                        yield Ok::<Event, Infallible>(sse);
                    }
                }
                Err(err) => {
                    warn!("media event broadcast error: {err}");
                }
            }
        }
    };

    let stream = history_stream.chain(stream);
    Sse::new(stream).keep_alive(default_keep_alive())
}

fn scan_frame_to_event(frame: ScanBroadcastFrame) -> Option<Event> {
    let name = frame.event.as_sse_event_type().event_name();

    encode_scan_progress(&frame.payload).map(|data| {
        let mut event = Event::default().event(name).data(data);
        event = event.id(frame.payload.sequence.to_string());
        event
    })
}

fn media_frame_to_sse(
    frame: crate::infra::scan::media_event_bus::MediaEventFrame,
) -> Option<Event> {
    let name = frame.event.sse_event_type().event_name();

    encode_media_event(&frame.event).map(|data| {
        Event::default()
            .event(name)
            .id(frame.sequence.to_string())
            .data(data)
    })
}

// Legacy helper: prefetch primary posters using now-deprecated ImageLookupParams.
// This remains commented out intentionally; the new image provider uses
// VarInput/ImageSize/ImgDbLookup-based APIs instead.
//
// async fn maybe_prepare_primary_poster(state: &AppState, event: &MediaEvent) {
//     use tokio::time::{Duration, timeout};
//
//     // Only gate on new Movie/Series where a poster is expected
//     let (media_type, media_id) = match event {
//         MediaEvent::MovieAdded { movie } => ("movie", movie.id.0),
//         MediaEvent::SeriesAdded { series } => ("series", series.id.0),
//         _ => return,
//     };
//
//     let params = ImageLookupParams {
//         media_type: media_type.to_string(),
//         media_id: media_id.to_string(),
//         image_type: MediaImageKind::Poster,
//         index: 0,
//         // TMDB canonical near-300 width for fast grid display
//         variant: ImageSize::Poster::default(),
//     };
//
//     // Block briefly to ensure availability; fall through on timeout/errors
//     let image_service = state.image_service();
//     let fut = image_service.get_or_download_variant(&params);
//     let _ = timeout(Duration::from_secs(5), fut).await;
// }

// async fn maybe_prepare_and_refresh(
//     state: &AppState,
//     event: MediaEvent,
// ) -> MediaEvent {
//     // Ensure image readiness first
//     maybe_prepare_primary_poster(state, &event).await;

//     // Reload the reference to include any freshly computed theme_color
//     match event {
//         MediaEvent::MovieAdded { movie } => {
//             let uow = state.unit_of_work();
//             match uow
//                 .media_refs
//                 .get_movie_reference(&MovieID(movie.id.0))
//                 .await
//             {
//                 Ok(updated) => MediaEvent::MovieAdded { movie: updated },
//                 Err(_) => MediaEvent::MovieAdded { movie },
//             }
//         }
//         MediaEvent::SeriesAdded { series } => {
//             let uow = state.unit_of_work();
//             match uow
//                 .media_refs
//                 .get_series_reference(&SeriesID(series.id.0))
//                 .await
//             {
//                 Ok(updated) => MediaEvent::SeriesAdded { series: updated },
//                 Err(_) => MediaEvent::SeriesAdded { series },
//             }
//         }
//         other => other,
//     }
// }

fn encode_media_event(event: &MediaEvent) -> Option<String> {
    to_bytes::<RkyvError>(event)
        .map(|bytes| BASE64_STANDARD.encode(bytes.as_slice()))
        .map_err(|err| {
            warn!("failed to serialize media event with rkyv: {err}");
            err
        })
        .ok()
}

fn encode_scan_progress(payload: &ScanProgressEvent) -> Option<String> {
    serde_json::to_string(payload)
        .map_err(|err| {
            warn!("failed to serialize scan progress payload as JSON: {err}");
            err
        })
        .ok()
}

fn default_keep_alive() -> KeepAlive {
    KeepAlive::new()
        .interval(Duration::from_secs(15))
        .text("keep-alive")
}

#[cfg(test)]
mod sse_tests {
    use super::*;
    use chrono::Utc;
    use ferrex_core::types::ScanStageLatencySummary;

    use crate::infra::scan::scan_manager::ScanEventKind;

    fn sample_progress(sequence: u64) -> ScanProgressEvent {
        let scan_id = Uuid::now_v7();
        ScanProgressEvent {
            version: "2".to_string(),
            scan_id,
            library_id: LibraryId::new(),
            status: "completed".to_string(),
            completed_items: 3,
            total_items: 4,
            validated_items: 1,
            known_unchanged_items: 1,
            skipped_items: 1,
            failed_items: 1,
            needs_attention_items: 1,
            retrying_items: 0,
            sequence,
            current_path: Some("/library/movie".to_string()),
            path_key: None,
            p95_stage_latencies_ms: ScanStageLatencySummary {
                scan: 1,
                analyze: 2,
                index: 3,
            },
            correlation_id: scan_id,
            idempotency_key: format!("scan:{scan_id}:{sequence}"),
            emitted_at: Utc::now(),
            terminal_at: Some(Utc::now()),
            reason_details: Vec::new(),
        }
    }

    fn frame(sequence: u64) -> ScanBroadcastFrame {
        ScanBroadcastFrame {
            event: ScanEventKind::Progress,
            payload: sample_progress(sequence),
        }
    }

    #[test]
    fn scan_progress_sse_payloads_are_json_without_legacy_queue_field() {
        let payload = sample_progress(7);
        let encoded = encode_scan_progress(&payload).expect("json payload");

        assert!(encoded.starts_with('{'));
        assert!(encoded.contains("needs_attention_items"));
        assert!(encoded.contains("terminal_at"));
        assert!(!encoded.contains("dead_lettered_items"));

        let decoded: ScanProgressEvent =
            serde_json::from_str(&encoded).expect("decode JSON SSE payload");
        assert_eq!(decoded.version, "2");
        assert_eq!(decoded.failed_items, 1);
        assert_eq!(decoded.needs_attention_items, 1);
        assert!(decoded.terminal_at.is_some());
    }

    #[test]
    fn scan_progress_history_replay_respects_resume_sequence() {
        let history = vec![frame(1), frame(2), frame(3)];

        assert_eq!(
            scan_progress_history_events(history.clone(), None).len(),
            3
        );
        assert_eq!(
            scan_progress_history_events(history.clone(), Some(1)).len(),
            2
        );
        assert_eq!(scan_progress_history_events(history, Some(3)).len(), 0);
    }
}

#[cfg(test)]
mod observability_tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn run_status_filter_rejects_unknown_values() {
        assert_eq!(
            parse_run_status_filter(Some("failed")).unwrap(),
            Some(ScanRunStatus::Failed)
        );
        let err = parse_run_status_filter(Some("dead_letter")).unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn failure_dto_hides_debug_details_by_default() {
        let now = Utc::now();
        let dto = scan_failure_to_dto(
            ScanRunFailureSummary {
                run_id: Uuid::now_v7(),
                library_id: LibraryId(Uuid::now_v7()),
                subject_key: "/media/movies/Broken".to_string(),
                category: "filesystem_permission".to_string(),
                message_code: "scan.folder_permission_denied".to_string(),
                raw_debug_details: json!({"raw_error": "permission denied"}),
                last_error: Some("permission denied".to_string()),
                occurrences: 1,
                first_seen_at: now,
                last_seen_at: now,
                retryable: true,
                job_id: Some(Uuid::now_v7()),
                idempotency_key: "folder:/media/movies/Broken".to_string(),
            },
            false,
        );

        assert_eq!(dto.category_label, "Permission issue");
        assert!(dto.message.contains("permissions"));
        assert!(dto.debug.is_none());
    }

    #[test]
    fn replay_gap_response_is_recoverable() {
        let scan_id = Uuid::now_v7();
        let response = scan_replay_gap_response(&ScanReplayGap {
            scan_id,
            requested_after_sequence: 1,
            min_available_sequence: Some(5),
            max_available_sequence: Some(9),
        });

        assert_eq!(response.scan_id, scan_id);
        assert!(response.recoverable);
        assert!(response.recovery_hint.contains("Refetch"));
    }
}
