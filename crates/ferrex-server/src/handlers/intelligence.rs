use std::{convert::Infallible, pin::Pin, sync::Arc, time::Duration};

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Sse,
        sse::{Event, KeepAlive},
    },
};
use ferrex_core::{
    api::{ApiResponse, types::intelligence::*},
    application::intelligence_runtime::IntelligenceRunManager,
    domain::intelligence::IntelligenceProviderError,
    error::MediaError,
    player_prelude::User,
};
use ferrex_model::{
    EpisodeID, LibraryId, MediaID, MovieID, SeasonID, SeriesID,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};

/// Optional request body for item-context routes where the media id is carried
/// in the route path and the body only needs bounded response controls.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IntelligenceItemContextBody {
    #[serde(default)]
    pub library_id: Option<LibraryId>,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Optional request body for related-context routes where the seed media id is
/// carried in the route path.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IntelligenceRelatedContextBody {
    #[serde(default)]
    pub relationship_kinds: Vec<IntelligenceRelationshipKind>,
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Optional request body for run-audit routes where the run id is carried in
/// the route path.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IntelligenceRunAuditBody {
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IntelligenceRunEventsQuery {
    pub after_sequence: Option<i32>,
    pub limit: Option<u16>,
}

const LAST_EVENT_ID_HEADER: &str = "last-event-id";
const RUN_EVENTS_POLL_INTERVAL: Duration = Duration::from_millis(250);

type IntelligenceSseStream = Pin<
    Box<
        dyn tokio_stream::Stream<Item = Result<Event, Infallible>>
            + Send
            + 'static,
    >,
>;

#[derive(Debug)]
pub struct IntelligenceHttpError {
    status: StatusCode,
    error: IntelligenceError,
}

impl IntelligenceHttpError {
    fn new(
        status: StatusCode,
        code: IntelligenceErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            error: IntelligenceError {
                code,
                message: message.into(),
                retryable: false,
                details: Value::Null,
            },
        }
    }

    fn from_intelligence_error(error: IntelligenceError) -> Self {
        Self {
            status: status_for_intelligence_error(error.code),
            error,
        }
    }

    fn feature_disabled(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            IntelligenceErrorCode::FeatureDisabled,
            message,
        )
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            IntelligenceErrorCode::NotFound,
            message,
        )
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            IntelligenceErrorCode::Conflict,
            message,
        )
    }

    fn storage(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            IntelligenceErrorCode::StorageError,
            message,
        )
    }

    fn from_media_error(error: MediaError) -> Self {
        match error {
            MediaError::NotFound(message) => Self::not_found(message),
            MediaError::Conflict(message) => Self::conflict(message),
            MediaError::Cancelled(message) => Self::new(
                StatusCode::REQUEST_TIMEOUT,
                IntelligenceErrorCode::RunCancelled,
                message,
            ),
            MediaError::InvalidMedia(message) => {
                if message.to_ascii_lowercase().contains("disabled") {
                    Self::feature_disabled(message)
                } else {
                    Self::new(
                        StatusCode::BAD_REQUEST,
                        IntelligenceErrorCode::InvalidRequest,
                        message,
                    )
                }
            }
            MediaError::Database(error) => {
                Self::storage(format!("intelligence storage error: {error}"))
            }
            MediaError::Internal(message) => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                IntelligenceErrorCode::Internal,
                message,
            ),
            other => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                IntelligenceErrorCode::Internal,
                other.to_string(),
            ),
        }
    }

    fn from_provider_error(error: IntelligenceProviderError) -> Self {
        Self::from_intelligence_error(error.to_intelligence_error())
    }
}

impl IntoResponse for IntelligenceHttpError {
    fn into_response(self) -> axum::response::Response {
        let message = self.error.message.clone();
        let body = Json(json!({
            "status": "error",
            "error": self.error,
            "message": message,
        }));
        (self.status, body).into_response()
    }
}

fn status_for_intelligence_error(code: IntelligenceErrorCode) -> StatusCode {
    match code {
        IntelligenceErrorCode::FeatureDisabled
        | IntelligenceErrorCode::ProviderNotConfigured
        | IntelligenceErrorCode::ProviderUnavailable
        | IntelligenceErrorCode::ModelUnavailable => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        IntelligenceErrorCode::ProviderUnauthorized
        | IntelligenceErrorCode::ProviderError => StatusCode::BAD_GATEWAY,
        IntelligenceErrorCode::ProviderRateLimited
        | IntelligenceErrorCode::ConcurrencyLimit => {
            StatusCode::TOO_MANY_REQUESTS
        }
        IntelligenceErrorCode::ProviderTimeout
        | IntelligenceErrorCode::RunTimedOut
        | IntelligenceErrorCode::ToolTimedOut => StatusCode::GATEWAY_TIMEOUT,
        IntelligenceErrorCode::InvalidRequest => StatusCode::BAD_REQUEST,
        IntelligenceErrorCode::NotFound => StatusCode::NOT_FOUND,
        IntelligenceErrorCode::Conflict => StatusCode::CONFLICT,
        IntelligenceErrorCode::RunCancelled => StatusCode::REQUEST_TIMEOUT,
        IntelligenceErrorCode::StorageError => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        IntelligenceErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn intelligence_runtime(
    state: &AppState,
) -> Result<Arc<IntelligenceRunManager>, IntelligenceHttpError> {
    state.intelligence_runtime().ok_or_else(|| {
        IntelligenceHttpError::feature_disabled(
            "intelligence runtime is disabled or unavailable",
        )
    })
}

async fn ensure_provider_available(
    runtime: &IntelligenceRunManager,
) -> Result<(), IntelligenceHttpError> {
    let status = runtime
        .provider_status()
        .await
        .map_err(IntelligenceHttpError::from_provider_error)?;

    if !status.enabled
        || matches!(status.state, IntelligenceProviderState::Disabled)
    {
        return Err(IntelligenceHttpError::feature_disabled(
            "intelligence runtime is disabled",
        ));
    }
    if let Some(error) = status.error {
        return Err(IntelligenceHttpError::from_intelligence_error(error));
    }
    match status.state {
        IntelligenceProviderState::Ready
        | IntelligenceProviderState::Degraded => Ok(()),
        IntelligenceProviderState::Disabled => {
            Err(IntelligenceHttpError::feature_disabled(
                "intelligence runtime is disabled",
            ))
        }
        IntelligenceProviderState::NotConfigured => {
            Err(IntelligenceHttpError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                IntelligenceErrorCode::ProviderNotConfigured,
                "intelligence provider is not configured",
            ))
        }
        IntelligenceProviderState::Unavailable => {
            Err(IntelligenceHttpError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                IntelligenceErrorCode::ProviderUnavailable,
                "intelligence provider is unavailable",
            ))
        }
    }
}

fn validate_run_start_request(
    request: &IntelligenceRunStartRequest,
) -> Result<(), IntelligenceHttpError> {
    if request.prompt.trim().is_empty() {
        return Err(IntelligenceHttpError::new(
            StatusCode::BAD_REQUEST,
            IntelligenceErrorCode::InvalidRequest,
            "run prompt must not be empty",
        ));
    }
    if request.prompt.chars().count() > 16_000 {
        return Err(IntelligenceHttpError::new(
            StatusCode::BAD_REQUEST,
            IntelligenceErrorCode::InvalidRequest,
            "run prompt exceeds the 16000 character limit",
        ));
    }
    if request
        .idempotency_key
        .as_deref()
        .is_some_and(|value| value.chars().count() > 128)
    {
        return Err(IntelligenceHttpError::new(
            StatusCode::BAD_REQUEST,
            IntelligenceErrorCode::InvalidRequest,
            "idempotency_key exceeds the 128 character limit",
        ));
    }
    Ok(())
}

pub(crate) async fn library_overview_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<IntelligenceLibraryOverviewRequest>,
) -> AppResult<Json<ApiResponse<IntelligenceLibraryOverviewResponse>>> {
    let response = state
        .unit_of_work()
        .intelligence
        .library_overview(&request, Some(user.id))
        .await?;

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn facets_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<IntelligenceLibraryOverviewRequest>,
) -> AppResult<Json<ApiResponse<IntelligenceLibraryOverviewResponse>>> {
    let response = state
        .unit_of_work()
        .intelligence
        .library_overview(&request, Some(user.id))
        .await?;

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn candidate_search_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<IntelligenceCandidateSearchRequest>,
) -> AppResult<Json<ApiResponse<IntelligenceCandidateSearchResponse>>> {
    let response = state
        .unit_of_work()
        .intelligence
        .candidate_search(&request, Some(user.id))
        .await?;

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn artifact_search_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<IntelligenceArtifactSearchRequest>,
) -> AppResult<Json<ApiResponse<IntelligenceArtifactSearchResponse>>> {
    let response = state
        .unit_of_work()
        .intelligence
        .artifact_search(&request, Some(user.id))
        .await?;

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn artifact_detail_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(artifact_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<IntelligenceArtifactSummary>>> {
    let artifact = state
        .unit_of_work()
        .intelligence
        .get_artifact(artifact_id, Some(user.id))
        .await?
        .ok_or_else(|| {
            AppError::not_found("intelligence artifact not found")
        })?;

    Ok(Json(ApiResponse::success(artifact)))
}

pub(crate) async fn item_context_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<String>,
    Json(body): Json<IntelligenceItemContextBody>,
) -> AppResult<Json<ApiResponse<IntelligenceItemContextResponse>>> {
    let request = IntelligenceItemContextRequest {
        media_id: parse_media_id_path(&media_id)?,
        library_id: body.library_id,
        caps: body.caps,
    };

    let response = state
        .unit_of_work()
        .intelligence
        .item_context(&request, Some(user.id))
        .await?;

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn related_context_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(media_id): Path<String>,
    Json(body): Json<IntelligenceRelatedContextBody>,
) -> AppResult<Json<ApiResponse<IntelligenceRelatedContextResponse>>> {
    let request = IntelligenceRelatedContextRequest {
        media_id: parse_media_id_path(&media_id)?,
        relationship_kinds: body.relationship_kinds,
        pagination: body.pagination,
        caps: body.caps,
    };

    let response = state
        .unit_of_work()
        .intelligence
        .related_context(&request, Some(user.id))
        .await?;

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn run_audit_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(run_id): Path<Uuid>,
    Json(body): Json<IntelligenceRunAuditBody>,
) -> AppResult<Json<ApiResponse<IntelligenceRunAuditResponse>>> {
    let request = IntelligenceRunAuditRequest {
        run_id,
        pagination: body.pagination,
        caps: body.caps,
    };

    let response = state
        .unit_of_work()
        .intelligence
        .run_audit(&request, Some(user.id))
        .await?;

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn run_start_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<IntelligenceRunStartRequest>,
) -> Result<
    Json<ApiResponse<IntelligenceRunStartResponse>>,
    IntelligenceHttpError,
> {
    validate_run_start_request(&request)?;
    let runtime = intelligence_runtime(&state)?;
    ensure_provider_available(&runtime).await?;
    let response = runtime
        .start_run(request, Some(user.id))
        .await
        .map_err(IntelligenceHttpError::from_media_error)?;

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn run_status_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(run_id): Path<Uuid>,
) -> Result<
    Json<ApiResponse<IntelligenceRunStatusResponse>>,
    IntelligenceHttpError,
> {
    let runtime = intelligence_runtime(&state)?;
    let response = runtime
        .run_status(run_id, Some(user.id))
        .await
        .map_err(IntelligenceHttpError::from_media_error)?;

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn run_cancel_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(raw_run_id): Path<String>,
    Json(request): Json<IntelligenceRunCancelRequest>,
) -> Result<
    Json<ApiResponse<IntelligenceRunCancelResponse>>,
    IntelligenceHttpError,
> {
    let run_id = parse_cancel_run_id(&raw_run_id)?;
    let runtime = intelligence_runtime(&state)?;
    let response = runtime
        .cancel_run(run_id, Some(user.id), request)
        .await
        .map_err(IntelligenceHttpError::from_media_error)?;
    if !response.cancellation_requested {
        return Err(IntelligenceHttpError::conflict(
            response
                .message
                .clone()
                .unwrap_or_else(|| "run is already terminal".to_string()),
        ));
    }

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn run_events_sse_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(run_id): Path<Uuid>,
    Query(query): Query<IntelligenceRunEventsQuery>,
    headers: HeaderMap,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>,
    IntelligenceHttpError,
> {
    let runtime = intelligence_runtime(&state)?;
    runtime
        .run_status(run_id, Some(user.id))
        .await
        .map_err(IntelligenceHttpError::from_media_error)?;

    let last_event_id = headers
        .get(LAST_EVENT_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<i32>().ok());
    let request = IntelligenceRunEventsRequest {
        run_id,
        after_sequence: query.after_sequence.or(last_event_id),
        limit: query
            .limit
            .map(ferrex_core::api::types::intelligence::clamp_intelligence_page_limit)
            .unwrap_or_else(ferrex_core::api::types::intelligence::default_intelligence_page_limit),
    };
    let stream = build_run_events_stream(runtime, request, Some(user.id));

    Ok(Sse::new(stream).keep_alive(intelligence_keep_alive()))
}

pub(crate) async fn draft_artifact_detail_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(artifact_id): Path<Uuid>,
) -> Result<
    Json<ApiResponse<IntelligenceDraftArtifactPayload>>,
    IntelligenceHttpError,
> {
    let runtime = intelligence_runtime(&state)?;
    let request = IntelligenceDraftArtifactReadRequest { artifact_id };
    let draft = runtime
        .draft_artifact(request, Some(user.id))
        .await
        .map_err(IntelligenceHttpError::from_media_error)?
        .ok_or_else(|| {
            IntelligenceHttpError::not_found(
                "intelligence draft artifact not found",
            )
        })?;

    Ok(Json(ApiResponse::success(draft)))
}

pub(crate) async fn draft_artifact_list_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(request): Query<IntelligenceDraftArtifactListRequest>,
) -> Result<
    Json<ApiResponse<IntelligenceDraftArtifactListResponse>>,
    IntelligenceHttpError,
> {
    let runtime = intelligence_runtime(&state)?;
    let limit =
        ferrex_core::api::types::intelligence::clamp_intelligence_page_limit(
            request.limit,
        );
    let fetch_limit = i64::from(limit).saturating_add(1);
    let ids = sqlx::query_scalar!(
        r#"
        SELECT id AS "id!"
        FROM intelligence_artifacts
        WHERE status = 'draft'
          AND (user_id IS NULL OR user_id = $1)
          AND ($2::uuid IS NULL OR run_id = $2)
        ORDER BY updated_at DESC, id
        LIMIT $3
        "#,
        user.id,
        request.run_id,
        fetch_limit
    )
    .fetch_all(state.postgres().pool())
    .await
    .map_err(|err| {
        IntelligenceHttpError::storage(format!(
            "list intelligence draft artifacts failed: {err}"
        ))
    })?;
    let has_more = ids.len() > usize::from(limit);

    let mut drafts = Vec::with_capacity(ids.len().min(usize::from(limit)));
    for artifact_id in ids.into_iter().take(usize::from(limit)) {
        let request = IntelligenceDraftArtifactReadRequest { artifact_id };
        if let Some(draft) = runtime
            .draft_artifact(request, Some(user.id))
            .await
            .map_err(IntelligenceHttpError::from_media_error)?
        {
            drafts.push(draft);
        }
    }

    Ok(Json(ApiResponse::success(
        IntelligenceDraftArtifactListResponse {
            drafts,
            page: IntelligencePageInfo {
                next_cursor: None,
                limit,
                has_more,
            },
        },
    )))
}

pub(crate) async fn provider_status_handler(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<IntelligenceProviderStatus>>, IntelligenceHttpError>
{
    let runtime = intelligence_runtime(&state)?;
    let status = runtime
        .provider_status()
        .await
        .map_err(IntelligenceHttpError::from_provider_error)?;
    Ok(Json(ApiResponse::success(status)))
}

fn build_run_events_stream(
    runtime: Arc<IntelligenceRunManager>,
    request: IntelligenceRunEventsRequest,
    user_id: Option<Uuid>,
) -> IntelligenceSseStream {
    Box::pin(async_stream::stream! {
        let run_id = request.run_id;
        let limit = request.limit;
        let mut last_seen = request.after_sequence;
        loop {
            let events = runtime
                .run_events(
                    IntelligenceRunEventsRequest {
                        run_id,
                        after_sequence: last_seen,
                        limit,
                    },
                    user_id,
                )
                .await;
            let has_more = match events {
                Ok(response) => {
                    let has_more = response.page.has_more;
                    for event in response.events {
                        last_seen = Some(
                            last_seen
                                .map(|sequence| sequence.max(event.sequence))
                                .unwrap_or(event.sequence),
                        );
                        if let Some(sse) = run_event_to_sse(&event) {
                            yield Ok::<Event, Infallible>(sse);
                        }
                    }
                    has_more
                }
                Err(error) => {
                    yield Ok::<Event, Infallible>(error_sse_event(
                        IntelligenceHttpError::from_media_error(error).error,
                    ));
                    break;
                }
            };
            if has_more {
                continue;
            }

            match runtime.run_status(run_id, user_id).await {
                Ok(status) if status.terminal => break,
                Ok(_) => tokio::time::sleep(RUN_EVENTS_POLL_INTERVAL).await,
                Err(error) => {
                    yield Ok::<Event, Infallible>(error_sse_event(
                        IntelligenceHttpError::from_media_error(error).error,
                    ));
                    break;
                }
            }
        }
    })
}

fn run_event_to_sse(event: &IntelligenceRunEvent) -> Option<Event> {
    serde_json::to_string(event).ok().map(|payload| {
        Event::default()
            .event(event.event_kind.as_db_str())
            .id(event.sequence.to_string())
            .data(payload)
    })
}

fn error_sse_event(error: IntelligenceError) -> Event {
    let payload = serde_json::to_string(&error).unwrap_or_else(|_| {
        json!({
            "code": IntelligenceErrorCode::Internal,
            "message": "failed to serialize intelligence error"
        })
        .to_string()
    });
    Event::default().event("error").data(payload)
}

fn intelligence_keep_alive() -> KeepAlive {
    KeepAlive::new()
        .interval(Duration::from_secs(15))
        .text("keep-alive")
}

fn parse_cancel_run_id(raw: &str) -> Result<Uuid, IntelligenceHttpError> {
    let Some(run_id) = raw.strip_suffix(":cancel") else {
        return Err(IntelligenceHttpError::new(
            StatusCode::BAD_REQUEST,
            IntelligenceErrorCode::InvalidRequest,
            "cancel route must end with :cancel",
        ));
    };
    Uuid::parse_str(run_id).map_err(|_| {
        IntelligenceHttpError::new(
            StatusCode::BAD_REQUEST,
            IntelligenceErrorCode::InvalidRequest,
            "cancel route must contain a valid run UUID",
        )
    })
}

fn parse_media_id_path(raw: &str) -> AppResult<MediaID> {
    let raw = raw.trim();
    let (kind, id) = split_media_path(raw).ok_or_else(|| {
        AppError::bad_request(
            "media_id must be encoded as movie:<uuid>, series:<uuid>, season:<uuid>, or episode:<uuid>",
        )
    })?;
    let uuid = Uuid::parse_str(id).map_err(|_| {
        AppError::bad_request(
            "media_id must contain a valid UUID after its media kind prefix",
        )
    })?;

    match kind {
        "movie" => Ok(MediaID::Movie(MovieID(uuid))),
        "series" => Ok(MediaID::Series(SeriesID(uuid))),
        "season" => Ok(MediaID::Season(SeasonID(uuid))),
        "episode" => Ok(MediaID::Episode(EpisodeID(uuid))),
        _ => Err(AppError::bad_request(
            "media_id kind must be movie, series, season, or episode",
        )),
    }
}

fn split_media_path(raw: &str) -> Option<(&str, &str)> {
    if let Some((kind, id)) = raw.split_once(':') {
        return Some((normalize_media_kind(kind)?, id));
    }
    if let Some((kind, id)) = raw.split_once('_') {
        return Some((normalize_media_kind(kind)?, id));
    }
    if let Some(open) = raw.find('(') {
        let close = raw.strip_suffix(')')?;
        let kind = normalize_media_kind(&raw[..open])?;
        return Some((kind, &close[open + 1..]));
    }

    if let Some((kind, id)) = raw.split_once('-') {
        return Some((normalize_media_kind(kind)?, id));
    }

    None
}

fn normalize_media_kind(kind: &str) -> Option<&'static str> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "movie" | "movies" => Some("movie"),
        "series" | "show" | "shows" => Some("series"),
        "season" | "seasons" => Some("season"),
        "episode" | "episodes" => Some("episode"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_media_id_path_variants() {
        let uuid = Uuid::from_u128(42);
        assert_eq!(
            parse_media_id_path(&format!("movie:{uuid}")).unwrap(),
            MediaID::Movie(MovieID(uuid))
        );
        assert_eq!(
            parse_media_id_path(&format!("Series({uuid})")).unwrap(),
            MediaID::Series(SeriesID(uuid))
        );
        assert!(parse_media_id_path(&uuid.to_string()).is_err());
        assert!(parse_media_id_path("movie:not-a-uuid").is_err());
    }
}
