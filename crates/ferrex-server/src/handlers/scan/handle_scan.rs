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
    ScanCommandAcceptedResponse, ScanCommandRequest, ScanSnapshotDto,
    ScanStartDisposition, StartScanRequest,
};
use ferrex_core::error::MediaError;
use ferrex_core::types::{
    EpisodeID, LibraryId, MediaEvent, MediaID, MovieID, ScanProgressEvent,
    VideoMediaType,
};
use rkyv::{rancor::Error as RkyvError, to_bytes};
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, pin::Pin, sync::Arc, time::Duration};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tracing::warn;
use uuid::Uuid;

use crate::infra::app_state::AppState;
use crate::infra::demo_mode;
use crate::infra::scan::scan_manager::{
    ScanBroadcastFrame, ScanCommandAccepted, ScanControlError,
    ScanControlPlane, ScanHistoryEntry,
};
use ferrex_core::api::scan::{
    BudgetConfigView, BulkModeView, IncrementalScanPolicyView,
    IncrementalScanStatusView, LeaseConfigView, MaintenanceConfigView,
    MetadataLimitsView, OrchestratorConfigView, QueueConfigView,
    RetryConfigView, ScanConfig, ScanMetrics, TranscriptIndexingConfigView,
    TranscriptRedactionConfigView, WatchConfigView,
};

const LAST_EVENT_ID_HEADER: &str = "last-event-id";
const MEDIA_EVENT_REPLAY_WINDOW: Duration = Duration::from_secs(10 * 60);

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

#[derive(Debug, Deserialize)]
pub struct MediaEventsQuery {
    pub last_sequence: Option<u64>,
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

#[derive(Debug, Serialize)]
pub struct TranscriptRefreshResponse {
    pub media_id: Uuid,
    pub media_type: String,
    pub media_file_id: Option<Uuid>,
    pub queued: bool,
    pub accepted: bool,
    pub job_id: Option<Uuid>,
    pub merged_into: Option<Uuid>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TranscriptPurgeRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TranscriptPurgeResponse {
    pub library_id: Uuid,
    pub media_id: Uuid,
    pub media_type: String,
    pub purged_sources: u64,
    pub media_file_id: Option<Uuid>,
    pub rebuild_queued: bool,
    pub accepted: bool,
    pub job_id: Option<Uuid>,
    pub merged_into: Option<Uuid>,
    pub reason: Option<String>,
}

pub async fn start_scan_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
    Json(request): Json<StartScanRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(ScanHttpError {
            status: StatusCode::NOT_FOUND,
            message: "Library not found".to_string(),
        });
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

    #[test]
    fn transcript_refresh_media_id_accepts_playable_media_only() {
        let id = Uuid::from_u128(42);

        assert_eq!(
            parse_transcript_refresh_media_id("movies", id)
                .expect("movie refresh target"),
            (MediaID::Movie(MovieID(id)), VideoMediaType::Movie)
        );
        assert_eq!(
            parse_transcript_refresh_media_id("Episode", id)
                .expect("episode refresh target"),
            (MediaID::Episode(EpisodeID(id)), VideoMediaType::Episode)
        );
        assert!(parse_transcript_refresh_media_id("series", id).is_err());
        assert!(parse_transcript_refresh_media_id("season", id).is_err());
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

pub async fn refresh_transcript_handler(
    State(state): State<AppState>,
    Path((media_type, media_id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, ScanHttpError> {
    let (typed_media_id, variant) =
        parse_transcript_refresh_media_id(&media_type, media_id)?;
    let normalized_media_type = transcript_media_type_label(variant);
    let orchestrator = state.scan_control().orchestrator();

    if !orchestrator.config().transcript_indexing.enabled {
        return Ok((
            StatusCode::OK,
            Json(ApiResponse::success(TranscriptRefreshResponse {
                media_id,
                media_type: normalized_media_type.to_string(),
                media_file_id: None,
                queued: false,
                accepted: false,
                job_id: None,
                merged_into: None,
                reason: Some("transcript_indexing_disabled".to_string()),
            })),
        ));
    }

    let media_file = state
        .unit_of_work()
        .media_files_read
        .get_by_media_id(&typed_media_id)
        .await
        .map_err(internal_scan_error)?
        .ok_or_else(|| ScanHttpError {
            status: StatusCode::NOT_FOUND,
            message: "Playable media file not found for transcript refresh"
                .to_string(),
        })?;

    let handle = orchestrator
        .enqueue_transcript_refresh(
            media_file.library_id,
            typed_media_id,
            variant,
            media_file.id,
            media_file.path.to_string_lossy().to_string(),
            None,
        )
        .await
        .map_err(internal_scan_error)?;

    let status = if handle.is_some() {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    let response = match handle {
        Some(handle) => TranscriptRefreshResponse {
            media_id,
            media_type: normalized_media_type.to_string(),
            media_file_id: Some(media_file.id),
            queued: true,
            accepted: handle.accepted,
            job_id: Some(handle.job_id.0),
            merged_into: handle.merged_into.map(|id| id.0),
            reason: None,
        },
        None => TranscriptRefreshResponse {
            media_id,
            media_type: normalized_media_type.to_string(),
            media_file_id: Some(media_file.id),
            queued: false,
            accepted: false,
            job_id: None,
            merged_into: None,
            reason: Some("media_file_unavailable".to_string()),
        },
    };

    Ok((status, Json(ApiResponse::success(response))))
}

pub async fn purge_transcript_handler(
    State(state): State<AppState>,
    Path((library_id, media_type, media_id)): Path<(Uuid, String, Uuid)>,
    Json(request): Json<TranscriptPurgeRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    let (typed_media_id, variant) =
        parse_transcript_refresh_media_id(&media_type, media_id)?;
    let normalized_media_type = transcript_media_type_label(variant);
    let reason = transcript_purge_reason(
        request.reason,
        "operator requested transcript purge",
    );
    let purged_sources = state
        .unit_of_work()
        .transcripts
        .purge_media(LibraryId(library_id), typed_media_id, &reason)
        .await
        .map_err(transcript_repo_error)?;

    Ok((
        StatusCode::OK,
        Json(ApiResponse::success(TranscriptPurgeResponse {
            library_id,
            media_id,
            media_type: normalized_media_type.to_string(),
            purged_sources,
            media_file_id: None,
            rebuild_queued: false,
            accepted: false,
            job_id: None,
            merged_into: None,
            reason: Some(reason),
        })),
    ))
}

pub async fn rebuild_transcript_handler(
    State(state): State<AppState>,
    Path((library_id, media_type, media_id)): Path<(Uuid, String, Uuid)>,
    Json(request): Json<TranscriptPurgeRequest>,
) -> Result<impl IntoResponse, ScanHttpError> {
    let (typed_media_id, variant) =
        parse_transcript_refresh_media_id(&media_type, media_id)?;
    let normalized_media_type = transcript_media_type_label(variant);
    let reason = transcript_purge_reason(
        request.reason,
        "operator requested transcript rebuild",
    );
    let library = LibraryId(library_id);
    let purged_sources = state
        .unit_of_work()
        .transcripts
        .purge_media(library, typed_media_id, &reason)
        .await
        .map_err(transcript_repo_error)?;

    let orchestrator = state.scan_control().orchestrator();
    let media_file = state
        .unit_of_work()
        .media_files_read
        .get_by_media_id(&typed_media_id)
        .await
        .map_err(internal_scan_error)?
        .filter(|file| file.library_id == library);

    let handle = if let Some(media_file) = &media_file {
        orchestrator
            .enqueue_transcript_refresh(
                media_file.library_id,
                typed_media_id,
                variant,
                media_file.id,
                media_file.path.to_string_lossy().to_string(),
                None,
            )
            .await
            .map_err(internal_scan_error)?
    } else {
        None
    };

    let status = if handle.is_some() {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    let disabled = !orchestrator.config().transcript_indexing.enabled;
    let media_file_id = media_file.as_ref().map(|file| file.id);
    let response = match handle {
        Some(handle) => TranscriptPurgeResponse {
            library_id,
            media_id,
            media_type: normalized_media_type.to_string(),
            purged_sources,
            media_file_id,
            rebuild_queued: true,
            accepted: handle.accepted,
            job_id: Some(handle.job_id.0),
            merged_into: handle.merged_into.map(|id| id.0),
            reason: None,
        },
        None => TranscriptPurgeResponse {
            library_id,
            media_id,
            media_type: normalized_media_type.to_string(),
            purged_sources,
            media_file_id,
            rebuild_queued: false,
            accepted: false,
            job_id: None,
            merged_into: None,
            reason: Some(if disabled {
                "transcript_indexing_disabled".to_string()
            } else {
                "media_file_unavailable".to_string()
            }),
        },
    };

    Ok((status, Json(ApiResponse::success(response))))
}

pub async fn scan_metrics_handler(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ScanMetrics>>, ScanHttpError> {
    let depths: ScanQueueDepths = state
        .scan_control()
        .orchestrator()
        .queue_depths()
        .await
        .map_err(|e: MediaError| ScanHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        })?;
    let active = state.scan_control().active_scans().await.len();
    let incremental = incremental_status(&state).await?;
    let transcripts = state
        .scan_control()
        .orchestrator()
        .transcript_scan_status()
        .await
        .map_err(|e: MediaError| ScanHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        })?;
    Ok(Json(ApiResponse::success(ScanMetrics {
        queue_depths: depths,
        active_scans: active,
        incremental,
        transcripts,
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
            max_parallel_transcript_extract: cfg
                .queue
                .max_parallel_transcript_extract,
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
        transcript_indexing: TranscriptIndexingConfigView {
            enabled: cfg.transcript_indexing.enabled,
            embedded_enabled: cfg.transcript_indexing.embedded_enabled,
            sidecar_enabled: cfg.transcript_indexing.sidecar_enabled,
            allowed_languages: cfg
                .transcript_indexing
                .allowed_languages
                .clone(),
            max_subtitle_bytes: cfg.transcript_indexing.max_subtitle_bytes,
            max_segments_per_media: cfg
                .transcript_indexing
                .max_segments_per_media,
            max_chars_per_segment: cfg
                .transcript_indexing
                .max_chars_per_segment,
            max_chars_per_snippet: cfg
                .transcript_indexing
                .max_chars_per_snippet,
            extraction_timeout_ms: cfg
                .transcript_indexing
                .extraction_timeout_ms,
            concurrency_budget: cfg.transcript_indexing.concurrency_budget,
            redaction: TranscriptRedactionConfigView {
                enabled: cfg.transcript_indexing.redaction.enabled,
                redact_emails: cfg.transcript_indexing.redaction.redact_emails,
                redact_phone_numbers: cfg
                    .transcript_indexing
                    .redaction
                    .redact_phone_numbers,
                redact_url_secrets: cfg
                    .transcript_indexing
                    .redaction
                    .redact_url_secrets,
                redact_bearer_tokens: cfg
                    .transcript_indexing
                    .redaction
                    .redact_bearer_tokens,
                custom_regexes: cfg
                    .transcript_indexing
                    .redaction
                    .custom_regexes
                    .clone(),
            },
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
            media_analysis_limit: cfg.budget.media_analysis_limit,
            metadata_limit: cfg.budget.metadata_limit,
            indexing_limit: cfg.budget.indexing_limit,
            image_fetch_limit: cfg.budget.image_fetch_limit,
            transcript_extraction_limit: cfg.budget.transcript_extraction_limit,
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
        media_extensions: scanner.video_extensions.clone(),
        ignored_extensions: scanner.ignored_extensions.clone(),
        ignored_path_patterns: scanner.ignored_path_patterns.clone(),
    };

    Ok(Json(ApiResponse::success(ScanConfig {
        orchestrator: view,
        incremental_policy,
    })))
}

async fn incremental_status(
    state: &AppState,
) -> Result<IncrementalScanStatusView, ScanHttpError> {
    state
        .scan_control()
        .orchestrator()
        .incremental_status()
        .await
        .map_err(|err| ScanHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        })
}

fn watch_strategy_label(
    strategy: ferrex_core::domain::scan::orchestration::config::WatchStrategy,
) -> String {
    format!("{strategy:?}").to_ascii_lowercase()
}

fn parse_transcript_refresh_media_id(
    media_type: &str,
    media_id: Uuid,
) -> Result<(MediaID, VideoMediaType), ScanHttpError> {
    match media_type.trim().to_ascii_lowercase().as_str() {
        "movie" | "movies" => Ok((
            MediaID::Movie(MovieID(media_id)),
            VideoMediaType::Movie,
        )),
        "episode" | "episodes" => Ok((
            MediaID::Episode(EpisodeID(media_id)),
            VideoMediaType::Episode,
        )),
        _ => Err(ScanHttpError {
            status: StatusCode::BAD_REQUEST,
            message: "Transcript refresh is supported only for movie or episode media"
                .to_string(),
        }),
    }
}

fn transcript_media_type_label(media_type: VideoMediaType) -> &'static str {
    match media_type {
        VideoMediaType::Movie => "movie",
        VideoMediaType::Episode => "episode",
        VideoMediaType::Series => "series",
        VideoMediaType::Season => "season",
    }
}

fn transcript_purge_reason(
    reason: Option<String>,
    fallback: &'static str,
) -> String {
    reason
        .map(|value| value.trim().chars().take(512).collect::<String>())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn transcript_repo_error(error: MediaError) -> ScanHttpError {
    match error {
        MediaError::InvalidMedia(message) => ScanHttpError {
            status: StatusCode::BAD_REQUEST,
            message,
        },
        other => internal_scan_error(other),
    }
}

fn internal_scan_error(error: MediaError) -> ScanHttpError {
    ScanHttpError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: error.to_string(),
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
    let history = scan_control.events(&scan_id).await?;
    let receiver = match scan_control.subscribe_scan(scan_id).await {
        Ok(receiver) => Some(receiver),
        Err(ScanControlError::ScanNotFound) if !history.is_empty() => None,
        Err(err) => return Err(err),
    };

    Ok(scan_progress_stream_from_parts(
        history,
        receiver,
        last_sequence,
    ))
}

fn scan_progress_stream_from_parts(
    history: Vec<ScanBroadcastFrame>,
    receiver: Option<tokio::sync::broadcast::Receiver<ScanBroadcastFrame>>,
    last_sequence: Option<u64>,
) -> Pin<
    Box<
        dyn tokio_stream::Stream<Item = Result<Event, Infallible>>
            + Send
            + 'static,
    >,
> {
    let history_events = scan_progress_history_events(history, last_sequence);
    let history_stream = tokio_stream::iter(history_events);

    let Some(receiver) = receiver else {
        return Box::pin(history_stream);
    };

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

    Box::pin(history_stream.chain(live_stream))
}

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
    frame: crate::infra::scan::scan_manager::MediaEventFrame,
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
