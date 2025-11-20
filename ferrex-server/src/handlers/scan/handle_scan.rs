use axum::response::sse::{Event, KeepAlive};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Sse},
};
use base64::{
    Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD,
};
use ferrex_core::api_types::{
    ActiveScansResponse, ApiResponse, LatestProgressResponse,
    ScanCommandAcceptedResponse, ScanCommandRequest, ScanSnapshotDto,
    StartScanRequest,
};
use ferrex_core::types::{LibraryID, MediaEvent, ScanProgressEvent};
use rkyv::{rancor::Error as RkyvError, to_bytes};
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, pin::Pin, sync::Arc, time::Duration};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tracing::warn;
use uuid::Uuid;

use crate::infra::app_state::AppState;
use crate::infra::scan::scan_manager::{
    ScanBroadcastFrame, ScanControlError, ScanControlPlane, ScanHistoryEntry,
};
use ferrex_core::api_scan::{
    BudgetConfigView, BulkModeView, LeaseConfigView, MetadataLimitsView,
    OrchestratorConfigView, QueueConfigView, RetryConfigView, ScanConfig,
    ScanMetrics, WatchConfigView,
};

const LAST_EVENT_ID_HEADER: &str = "last-event-id";

#[derive(Debug)]
pub struct ScanHttpError {
    status: StatusCode,
    message: String,
}

impl From<ScanControlError> for ScanHttpError {
    fn from(error: ScanControlError) -> Self {
        let status = error.status_code();
        let message = error.message();
        Self { status, message }
    }
}

impl IntoResponse for ScanHttpError {
    fn into_response(self) -> axum::response::Response {
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
    let accepted = state
        .scan_control()
        .start_library_scan(LibraryID(library_id), request.correlation_id)
        .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success(ScanCommandAcceptedResponse {
            scan_id: accepted.scan_id,
            correlation_id: accepted.correlation_id,
        })),
    ))
}

pub async fn pause_scan_handler(
    State(state): State<AppState>,
    Path((_library_id,)): Path<(Uuid,)>,
    Json(request): Json<ScanCommandRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    let accepted = state.scan_control().pause_scan(&request.scan_id).await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success(ScanCommandAcceptedResponse {
            scan_id: accepted.scan_id,
            correlation_id: accepted.correlation_id,
        })),
    ))
}

pub async fn resume_scan_handler(
    State(state): State<AppState>,
    Path((_library_id,)): Path<(Uuid,)>,
    Json(request): Json<ScanCommandRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    let accepted = state.scan_control().resume_scan(&request.scan_id).await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success(ScanCommandAcceptedResponse {
            scan_id: accepted.scan_id,
            correlation_id: accepted.correlation_id,
        })),
    ))
}

pub async fn cancel_scan_handler(
    State(state): State<AppState>,
    Path((_library_id,)): Path<(Uuid,)>,
    Json(request): Json<ScanCommandRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    let accepted = state.scan_control().cancel_scan(&request.scan_id).await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success(ScanCommandAcceptedResponse {
            scan_id: accepted.scan_id,
            correlation_id: accepted.correlation_id,
        })),
    ))
}

pub async fn active_scans_handler(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ActiveScansResponse>>, ScanHttpError> {
    let scans = state.scan_control().active_scans().await;
    let count = scans.len();
    let dto_scans: Vec<ScanSnapshotDto> =
        scans.into_iter().map(Into::into).collect();
    Ok(Json(ApiResponse::success(ActiveScansResponse {
        scans: dto_scans,
        count,
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
    let depths = state
        .scan_control()
        .orchestrator()
        .queue_depths()
        .await
        .map_err(|e| ScanHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        })?;
    let active = state.scan_control().active_scans().await.len();
    Ok(Json(ApiResponse::success(ScanMetrics {
        queue_depths: depths,
        active_scans: active,
    })))
}

pub async fn scan_config_handler(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ScanConfig>>, ScanHttpError> {
    let cfg = state.scan_control().orchestrator().config();
    // Map internal config to view that is feature-agnostic
    let view = OrchestratorConfigView {
        queue: QueueConfigView {
            max_parallel_scans: cfg.queue.max_parallel_scans,
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
        lease: LeaseConfigView {
            lease_ttl_secs: cfg.lease.lease_ttl_secs,
        },
        watch: WatchConfigView {
            debounce_window_ms: cfg.watch.debounce_window_ms,
            max_batch_events: cfg.watch.max_batch_events,
        },
        budget: BudgetConfigView {
            library_scan_limit: cfg.budget.library_scan_limit,
        },
    };
    Ok(Json(ApiResponse::success(ScanConfig {
        orchestrator: view,
    })))
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
    let history = scan_control.events(&scan_id).await?;
    let receiver = scan_control.subscribe_scan(scan_id).await?;

    let history_last_sequence = last_sequence;
    let history_events = history
        .into_iter()
        .filter(|frame| {
            history_last_sequence
                .map(|seq| frame.payload.sequence > seq)
                .unwrap_or(true)
        })
        .filter_map(scan_frame_to_event)
        .map(Ok::<Event, Infallible>)
        .collect::<Vec<_>>();
    let history_stream = tokio_stream::iter(history_events);

    let initial_sequence = last_sequence.unwrap_or(0);
    let live_stream = async_stream::stream! {
        let mut live_receiver = BroadcastStream::new(receiver);
        let mut last_seen_sequence = initial_sequence;
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

    let stream = history_stream.chain(live_stream);
    Ok(Box::pin(stream))
}

pub async fn media_events_sse_handler(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.scan_control().subscribe_media_events();
    let stream = BroadcastStream::new(receiver).filter_map(|item| match item {
        Ok(event) => media_event_to_sse(event).map(Ok),
        Err(err) => {
            warn!("media event broadcast error: {err}");
            None
        }
    });

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

fn media_event_to_sse(event: MediaEvent) -> Option<Event> {
    let name = event.sse_event_type().event_name();

    encode_media_event(&event)
        .map(|data| Event::default().event(name).data(data))
}

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
    to_bytes::<RkyvError>(payload)
        .map(|bytes| BASE64_STANDARD.encode(bytes.as_slice()))
        .map_err(|err| {
            warn!("failed to serialize scan progress payload with rkyv: {err}");
            err
        })
        .ok()
}

fn default_keep_alive() -> KeepAlive {
    KeepAlive::new()
        .interval(Duration::from_secs(15))
        .text("keep-alive")
}
