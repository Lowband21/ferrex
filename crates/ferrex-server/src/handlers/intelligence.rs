use std::{
    collections::HashMap, convert::Infallible, pin::Pin, sync::Arc,
    time::Duration,
};

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Sse,
        sse::{Event, KeepAlive},
    },
};
use chrono::Utc;
use ferrex_core::{
    api::{
        ApiResponse,
        types::{
            collections::{
                CollectionId, CollectionMediaKind, CollectionMediaScope,
                CollectionMemberAvailabilityStatus, CollectionMemberKey,
                GetCollectionDetailRequest,
            },
            intelligence::*,
            smart_shelves::*,
        },
    },
    application::intelligence_runtime::IntelligenceRunManager,
    database::repository_ports::collections::{
        CollectionItemIdentity, CollectionReadMode,
    },
    domain::intelligence::IntelligenceProviderError,
    error::MediaError,
    player_prelude::User,
};
use ferrex_model::{
    EpisodeID, LibraryId, MediaID, MovieID, SeasonID, SeriesID,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
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
            MediaError::ConcurrencyLimit(message) => Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                IntelligenceErrorCode::ConcurrencyLimit,
                message,
            ),
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
    let mut response = state
        .unit_of_work()
        .intelligence
        .candidate_search(&request, Some(user.id))
        .await?;

    if request.include_transcript_grounding {
        attach_transcript_grounding(&state, user.id, &request, &mut response)
            .await?;
    }

    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn timed_text_search_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<TimedTextSnippetSearchRequest>,
) -> AppResult<Json<ApiResponse<TimedTextSnippetSearchResponse>>> {
    let request = apply_transcript_snippet_policy(&state, request);
    let response = state
        .unit_of_work()
        .transcripts
        .search_snippets(&request, Some(user.id))
        .await?;

    Ok(Json(ApiResponse::success(response)))
}

fn configured_transcript_snippet_chars(state: &AppState) -> u16 {
    clamp_timed_text_snippet_chars(
        state
            .scan_control()
            .orchestrator()
            .config()
            .transcript_indexing
            .max_chars_per_snippet,
    )
}

fn apply_transcript_snippet_policy(
    state: &AppState,
    mut request: TimedTextSnippetSearchRequest,
) -> TimedTextSnippetSearchRequest {
    let max_snippet_chars = configured_transcript_snippet_chars(state);
    request.caps.timed_text_snippet_max_chars = clamp_timed_text_snippet_chars(
        request.caps.timed_text_snippet_max_chars,
    )
    .min(max_snippet_chars);
    request.caps.summary_max_chars =
        clamp_intelligence_summary_chars(request.caps.summary_max_chars)
            .min(max_snippet_chars);
    request
}

async fn attach_transcript_grounding(
    state: &AppState,
    user_id: Uuid,
    request: &IntelligenceCandidateSearchRequest,
    response: &mut IntelligenceCandidateSearchResponse,
) -> AppResult<()> {
    let transcript_limit =
        clamp_timed_text_snippet_limit(request.caps.timed_text_snippet_limit);
    let grounding_limit = request.caps.grounding_limit;
    let snippet_chars = clamp_timed_text_snippet_chars(
        request.caps.timed_text_snippet_max_chars,
    )
    .min(clamp_intelligence_summary_chars(
        request.caps.summary_max_chars,
    ))
    .min(configured_transcript_snippet_chars(state));

    for candidate in &mut response.candidates {
        let remaining_grounding = grounding_limit.saturating_sub(
            candidate.grounding.len().try_into().unwrap_or(u16::MAX),
        );
        if remaining_grounding == 0 {
            continue;
        }

        let per_candidate_limit = transcript_limit.min(remaining_grounding);
        let Some(library_id) = candidate.media.library_id else {
            continue;
        };

        let mut caps = request.caps;
        caps.timed_text_snippet_limit = per_candidate_limit;
        caps.timed_text_snippet_max_chars = snippet_chars;
        caps.summary_max_chars = snippet_chars;

        let transcript_request = TimedTextSnippetSearchRequest {
            query: request.query.clone(),
            library_ids: vec![library_id],
            media_ids: vec![candidate.media.media_id],
            media_kinds: Vec::new(),
            language_codes: Vec::new(),
            source_kinds: Vec::new(),
            pagination: IntelligencePagination::new(None, per_candidate_limit),
            caps,
            include_artifacts: request.include_artifacts,
        };

        let transcript_response = state
            .unit_of_work()
            .transcripts
            .search_snippets(&transcript_request, Some(user_id))
            .await?;

        for snippet in transcript_response.snippets {
            if candidate.grounding.len() >= usize::from(grounding_limit) {
                break;
            }
            candidate.grounding.push(transcript_grounding_ref(&snippet));
            candidate.transcript_grounding.push(snippet);
        }
    }

    Ok(())
}

fn transcript_grounding_ref(
    snippet: &TimedTextSnippet,
) -> IntelligenceGroundingRef {
    IntelligenceGroundingRef {
        source: IntelligenceGroundingSource::IntelligenceArtifact,
        media_id: Some(snippet.media.media_id),
        artifact_id: snippet.artifact_id,
        field: Some("transcript".to_string()),
        label: format!(
            "Transcript {} {:?} at {}-{} ms",
            snippet.language_code,
            snippet.source_kind,
            snippet.start_ms,
            snippet.end_ms
        ),
        evidence: Some(snippet.snippet.clone()),
    }
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

pub(crate) async fn smart_shelf_start_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<SmartShelfStartRequest>,
) -> Result<Json<ApiResponse<SmartShelfStartResponse>>, IntelligenceHttpError> {
    let request = smart_shelf_run_start_request(request)?;
    validate_run_start_request(&request)?;
    let runtime = intelligence_runtime(&state)?;
    ensure_provider_available(&runtime).await?;
    let response = runtime
        .start_run(request, Some(user.id))
        .await
        .map_err(IntelligenceHttpError::from_media_error)?;

    Ok(Json(ApiResponse::success(response.into())))
}

pub(crate) async fn smart_shelf_draft_detail_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(artifact_id): Path<Uuid>,
) -> Result<Json<ApiResponse<SmartShelfDraftResponse>>, SmartShelfHttpError> {
    let access = load_smart_shelf_draft_access(&state, artifact_id).await?;
    ensure_smart_shelf_draft_readable(&access, user.id)?;
    let draft = state
        .unit_of_work()
        .intelligence
        .get_draft_artifact(artifact_id, Some(user.id))
        .await
        .map_err(SmartShelfHttpError::from_media_error)?
        .ok_or_else(|| SmartShelfHttpError::draft_hidden())?;

    Ok(Json(ApiResponse::success(
        SmartShelfDraftResponse::from_draft_artifact(draft),
    )))
}

pub(crate) async fn smart_shelf_save_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(artifact_id): Path<Uuid>,
    Json(request): Json<SmartShelfSaveRequest>,
) -> Result<Json<ApiResponse<SmartShelfSaveResponse>>, SmartShelfHttpError> {
    validate_smart_shelf_save_request(&request)?;
    let access = load_smart_shelf_draft_access(&state, artifact_id).await?;
    ensure_smart_shelf_draft_saveable(&access, user.id)?;
    let payload = state
        .unit_of_work()
        .intelligence
        .get_draft_artifact(artifact_id, Some(user.id))
        .await
        .map_err(SmartShelfHttpError::from_media_error)?
        .ok_or_else(|| SmartShelfHttpError::draft_hidden())?;
    let response =
        SmartShelfDraftResponse::from_draft_artifact(payload.clone());
    if !response.validation.valid {
        return Err(SmartShelfHttpError::from_validation(
            &response.validation,
            "smart-shelf draft is not valid for save",
        ));
    }
    let draft = response.draft.clone().ok_or_else(|| {
        SmartShelfHttpError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            SmartShelfErrorCode::DraftMalformed,
            "smart-shelf draft content is malformed",
        )
    })?;
    let accepted_items = accepted_smart_shelf_items(&draft, &request)?;
    let grounded = grounded_media_ids(payload.media_id, &payload.sources);
    let validation =
        validate_smart_shelf_draft_items(&accepted_items, &grounded);
    if !validation.valid {
        return Err(SmartShelfHttpError::from_validation(
            &validation,
            "accepted smart-shelf items are not valid for save",
        ));
    }

    let response = save_smart_shelf_collection(
        &state,
        &user,
        &payload,
        &draft,
        &accepted_items,
        &request,
    )
    .await?;

    Ok(Json(ApiResponse::success(response)))
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

#[derive(Debug, Clone)]
struct SmartShelfDraftAccess {
    user_id: Option<Uuid>,
    status: String,
    metadata: Value,
}

#[derive(Debug)]
pub(crate) struct SmartShelfHttpError {
    status: StatusCode,
    error: SmartShelfError,
}

impl SmartShelfHttpError {
    fn new(
        status: StatusCode,
        code: SmartShelfErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            error: SmartShelfError {
                code,
                message: message.into(),
                retryable: false,
                details: Value::Null,
            },
        }
    }

    fn with_details(
        status: StatusCode,
        code: SmartShelfErrorCode,
        message: impl Into<String>,
        details: Value,
    ) -> Self {
        Self {
            status,
            error: SmartShelfError {
                code,
                message: message.into(),
                retryable: false,
                details,
            },
        }
    }

    fn draft_hidden() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            SmartShelfErrorCode::DraftHidden,
            "smart-shelf draft was not found",
        )
    }

    fn unauthorized() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            SmartShelfErrorCode::Unauthorized,
            "smart-shelf draft is not owned by the requesting user",
        )
    }

    fn stale() -> Self {
        Self::new(
            StatusCode::CONFLICT,
            SmartShelfErrorCode::DraftStale,
            "smart-shelf draft is no longer saveable",
        )
    }

    fn already_saved(collection_id: Option<CollectionId>) -> Self {
        let details = collection_id
            .map(|id| json!({"collection_id": id}))
            .unwrap_or(Value::Null);
        Self::with_details(
            StatusCode::CONFLICT,
            SmartShelfErrorCode::AlreadySaved,
            "smart-shelf draft has already been saved",
            details,
        )
    }

    fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            SmartShelfErrorCode::InvalidRequest,
            message,
        )
    }

    fn collection_conflict(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            SmartShelfErrorCode::CollectionConflict,
            message,
        )
    }

    fn storage(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            SmartShelfErrorCode::CollectionStorageError,
            message,
        )
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            SmartShelfErrorCode::Internal,
            message,
        )
    }

    fn from_validation(
        validation: &SmartShelfDraftValidation,
        message: impl Into<String>,
    ) -> Self {
        let code = validation
            .first_save_error_code()
            .unwrap_or(SmartShelfErrorCode::DraftMalformed);
        Self::with_details(
            StatusCode::UNPROCESSABLE_ENTITY,
            code,
            message,
            json!({"issues": validation.issues}),
        )
    }

    fn from_media_error(error: MediaError) -> Self {
        match error {
            MediaError::NotFound(message) => Self::new(
                StatusCode::NOT_FOUND,
                SmartShelfErrorCode::DraftHidden,
                message,
            ),
            MediaError::Conflict(message) => Self::collection_conflict(message),
            MediaError::InvalidMedia(message) => Self::invalid_request(message),
            MediaError::Database(error) => Self::storage(format!(
                "smart-shelf collection storage error: {error}"
            )),
            MediaError::Internal(message) => Self::internal(message),
            other => Self::internal(other.to_string()),
        }
    }
}

impl IntoResponse for SmartShelfHttpError {
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

fn smart_shelf_run_start_request(
    request: SmartShelfStartRequest,
) -> Result<IntelligenceRunStartRequest, IntelligenceHttpError> {
    let prompt = request.prompt.trim();
    if prompt.is_empty() {
        return Err(IntelligenceHttpError::new(
            StatusCode::BAD_REQUEST,
            IntelligenceErrorCode::InvalidRequest,
            "smart-shelf prompt must not be empty",
        ));
    }
    if !request.constraints.is_null() && !request.constraints.is_object() {
        return Err(IntelligenceHttpError::new(
            StatusCode::BAD_REQUEST,
            IntelligenceErrorCode::InvalidRequest,
            "smart-shelf constraints must be a JSON object",
        ));
    }
    if !request.metadata.is_null() && !request.metadata.is_object() {
        return Err(IntelligenceHttpError::new(
            StatusCode::BAD_REQUEST,
            IntelligenceErrorCode::InvalidRequest,
            "smart-shelf metadata must be a JSON object",
        ));
    }
    let media_kinds = if request.media_kinds.is_empty() {
        vec![IntelligenceMediaKind::Movie, IntelligenceMediaKind::Series]
    } else {
        request.media_kinds.clone()
    };
    if media_kinds.iter().any(|kind| {
        !matches!(
            kind,
            IntelligenceMediaKind::Movie | IntelligenceMediaKind::Series
        )
    }) {
        return Err(IntelligenceHttpError::new(
            StatusCode::BAD_REQUEST,
            IntelligenceErrorCode::InvalidRequest,
            "smart-shelf runs currently support movie and series media kinds only",
        ));
    }

    let prompt = build_smart_shelf_prompt(&request, &media_kinds);
    let metadata = json!({
        "smart_shelf": {
            "schema_version": SMART_SHELF_DRAFT_SCHEMA_VERSION,
            "template_id": request.template_id,
            "item_count": request.item_count,
            "media_kinds": media_kinds,
            "constraints": request.constraints,
            "locked_media_ids": request.locked_media_ids,
        },
        "client_metadata": request.metadata,
    });

    Ok(IntelligenceRunStartRequest {
        purpose: IntelligenceRunPurpose::Recommendation,
        library_id: request.library_id,
        media_id: None,
        prompt,
        idempotency_key: request.idempotency_key,
        model: request.model,
        caps: request.caps,
        metadata,
    })
}

fn build_smart_shelf_prompt(
    request: &SmartShelfStartRequest,
    media_kinds: &[IntelligenceMediaKind],
) -> String {
    let constraints = if request.constraints.is_null() {
        json!({})
    } else {
        request.constraints.clone()
    };
    let prompt_context = json!({
        "user_prompt": request.prompt.trim(),
        "template_id": request.template_id,
        "library_id": request.library_id,
        "item_count": request.item_count,
        "media_kinds": media_kinds,
        "constraints": constraints,
        "locked_media_ids": request.locked_media_ids,
    });
    format!(
        "Draft a Ferrex smart shelf from the bounded request below. Use Ferrex tools to ground every selected item. Create exactly one draft artifact with create_draft, then finish with final_response. The draft artifact content must be a JSON object with schema_version {schema_version}, title, optional description, optional interpreted_intent, requested_constraints, items, and optional alternates. Each item must include ordinal, media_id exactly as returned by Ferrex tools, title when available, a non-empty reason, and at least one source chip with label and media_id or artifact_id. Select only movie or series media, avoid duplicates, preserve any locked_media_ids, and include alternates only when they are grounded. Do not create collections or shelf placements; saving happens through the explicit smart-shelf save route.\n\nBounded smart-shelf request:\n{context}",
        schema_version = SMART_SHELF_DRAFT_SCHEMA_VERSION,
        context = prompt_context,
    )
}

async fn load_smart_shelf_draft_access(
    state: &AppState,
    artifact_id: Uuid,
) -> Result<SmartShelfDraftAccess, SmartShelfHttpError> {
    let row = sqlx::query(
        r#"
        SELECT user_id, status::text AS status, metadata
        FROM intelligence_artifacts
        WHERE id = $1
        "#,
    )
    .bind(artifact_id)
    .fetch_optional(state.postgres().pool())
    .await
    .map_err(|error| {
        SmartShelfHttpError::storage(format!(
            "load smart-shelf draft access failed: {error}"
        ))
    })?
    .ok_or_else(SmartShelfHttpError::draft_hidden)?;

    Ok(SmartShelfDraftAccess {
        user_id: row.try_get("user_id").map_err(|error| {
            SmartShelfHttpError::storage(format!(
                "decode smart-shelf draft owner failed: {error}"
            ))
        })?,
        status: row.try_get("status").map_err(|error| {
            SmartShelfHttpError::storage(format!(
                "decode smart-shelf draft status failed: {error}"
            ))
        })?,
        metadata: row.try_get("metadata").map_err(|error| {
            SmartShelfHttpError::storage(format!(
                "decode smart-shelf draft metadata failed: {error}"
            ))
        })?,
    })
}

fn ensure_smart_shelf_draft_readable(
    access: &SmartShelfDraftAccess,
    user_id: Uuid,
) -> Result<(), SmartShelfHttpError> {
    if access.user_id != Some(user_id) {
        return Err(SmartShelfHttpError::draft_hidden());
    }
    ensure_smart_shelf_status_saveable(access)
}

fn ensure_smart_shelf_draft_saveable(
    access: &SmartShelfDraftAccess,
    user_id: Uuid,
) -> Result<(), SmartShelfHttpError> {
    if access.user_id != Some(user_id) {
        return Err(SmartShelfHttpError::unauthorized());
    }
    ensure_smart_shelf_status_saveable(access)
}

fn ensure_smart_shelf_status_saveable(
    access: &SmartShelfDraftAccess,
) -> Result<(), SmartShelfHttpError> {
    if let Some(collection_id) =
        saved_collection_id_from_metadata(&access.metadata).map(CollectionId)
    {
        return Err(SmartShelfHttpError::already_saved(Some(collection_id)));
    }
    if access.status != "draft" {
        return Err(SmartShelfHttpError::stale());
    }
    Ok(())
}

fn validate_smart_shelf_save_request(
    request: &SmartShelfSaveRequest,
) -> Result<(), SmartShelfHttpError> {
    if request
        .title
        .as_deref()
        .is_some_and(|title| title.trim().is_empty())
    {
        return Err(SmartShelfHttpError::invalid_request(
            "smart-shelf save title must not be empty when provided",
        ));
    }
    if request
        .idempotency_key
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(SmartShelfHttpError::invalid_request(
            "smart-shelf save idempotency_key must not be empty when provided",
        ));
    }
    if request
        .idempotency_key
        .as_deref()
        .is_some_and(|value| value.chars().count() > 128)
    {
        return Err(SmartShelfHttpError::invalid_request(
            "smart-shelf save idempotency_key exceeds the 128 character limit",
        ));
    }
    if request.items.len() > usize::from(MAX_SMART_SHELF_ITEM_COUNT) {
        return Err(SmartShelfHttpError::invalid_request(format!(
            "smart-shelf save cannot include more than {MAX_SMART_SHELF_ITEM_COUNT} items"
        )));
    }
    Ok(())
}

fn accepted_smart_shelf_items(
    draft: &SmartShelfDraftContent,
    request: &SmartShelfSaveRequest,
) -> Result<Vec<SmartShelfDraftItem>, SmartShelfHttpError> {
    if request.items.is_empty() {
        return Ok(draft.items.clone());
    }

    let mut item_pool: HashMap<MediaID, SmartShelfDraftItem> = HashMap::new();
    for item in &draft.items {
        item_pool
            .entry(item.media_id)
            .or_insert_with(|| item.clone());
    }
    for alternate in &draft.alternates {
        let ordinal = alternate.target_ordinal.unwrap_or_else(|| {
            u32::try_from(draft.items.len().saturating_add(1))
                .unwrap_or(u32::MAX)
        });
        item_pool
            .entry(alternate.media_id)
            .or_insert_with(|| alternate.clone().into_item(ordinal));
    }

    let mut accepted = Vec::with_capacity(request.items.len());
    for (index, selected) in request.items.iter().enumerate() {
        let Some(candidate) = item_pool.get(&selected.media_id) else {
            let validation = SmartShelfDraftValidation::from_issues(vec![
                SmartShelfDraftValidationIssue::for_item(
                    SmartShelfDraftValidationIssueCode::UngroundedItem,
                    u32::try_from(index + 1).unwrap_or(u32::MAX),
                    selected.media_id,
                    "accepted smart-shelf item was not present in the draft or alternates",
                ),
            ]);
            return Err(SmartShelfHttpError::from_validation(
                &validation,
                "accepted smart-shelf item is not part of the draft",
            ));
        };
        let mut item = candidate.clone();
        item.ordinal = u32::try_from(index + 1).unwrap_or(u32::MAX);
        item.locked = selected.locked;
        item.replacement_of = selected.replacement_of.or(item.replacement_of);
        if selected
            .reason
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            item.reason = selected.reason.clone();
        }
        if !selected.sources.is_empty() {
            item.sources = selected.sources.clone();
        }
        accepted.push(item);
    }

    Ok(accepted)
}

async fn save_smart_shelf_collection(
    state: &AppState,
    user: &User,
    payload: &IntelligenceDraftArtifactPayload,
    draft: &SmartShelfDraftContent,
    accepted_items: &[SmartShelfDraftItem],
    request: &SmartShelfSaveRequest,
) -> Result<SmartShelfSaveResponse, SmartShelfHttpError> {
    let identities = accepted_items
        .iter()
        .map(|item| CollectionItemIdentity::new(item.media_id))
        .collect::<Vec<_>>();
    let resolved = state
        .unit_of_work()
        .collections
        .resolve_collection_items(&identities)
        .await
        .map_err(SmartShelfHttpError::from_media_error)?;
    let resolved_by_media = resolved
        .into_iter()
        .map(|item| (item.media_id, item))
        .collect::<HashMap<_, _>>();
    for item in accepted_items {
        let Some(resolved) = resolved_by_media.get(&item.media_id) else {
            return Err(SmartShelfHttpError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                SmartShelfErrorCode::UnsupportedMedia,
                format!(
                    "smart-shelf item {} could not be resolved",
                    item.media_id
                ),
            ));
        };
        if resolved.availability.status
            != CollectionMemberAvailabilityStatus::Available
        {
            return Err(SmartShelfHttpError::with_details(
                StatusCode::UNPROCESSABLE_ENTITY,
                SmartShelfErrorCode::UnsupportedMedia,
                format!(
                    "smart-shelf item {} is not available for collection save",
                    item.media_id
                ),
                json!({
                    "media_id": item.media_id,
                    "availability": resolved.availability,
                }),
            ));
        }
    }

    let title = request
        .title
        .as_deref()
        .unwrap_or(&draft.title)
        .trim()
        .to_string();
    if title.is_empty() {
        return Err(SmartShelfHttpError::invalid_request(
            "smart-shelf save title must not be empty",
        ));
    }
    let description = request
        .description
        .clone()
        .or_else(|| draft.description.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let collection_id = CollectionId::new();
    let stable_key = collection_id.stable_key();
    let external_key = request
        .idempotency_key
        .as_deref()
        .map(|value| format!("smart-shelf:{}:{}", user.id, value.trim()));
    let etag = format!("collection:{collection_id}:v1");
    let media_scope = smart_shelf_collection_media_scope(accepted_items);
    let media_scope = serde_json::to_value(media_scope).map_err(|error| {
        SmartShelfHttpError::internal(format!(
            "encode smart-shelf media scope failed: {error}"
        ))
    })?;
    let provenance = json!({
        "source": "manual",
        "imported_from": "intelligence_draft",
        "external_id": payload.artifact_id.to_string(),
        "generated_by": "ferrex-smart-shelf",
        "last_refreshed_at": Utc::now(),
    });
    let saved_at = Utc::now();
    let save_metadata = json!({
        "collection_id": collection_id,
        "saved_at": saved_at,
        "saved_by_user_id": user.id,
        "item_count": accepted_items.len(),
    });

    let pool = state.postgres().pool().clone();
    let mut tx = pool.begin().await.map_err(|error| {
        SmartShelfHttpError::storage(format!(
            "begin smart-shelf save transaction failed: {error}"
        ))
    })?;

    sqlx::query(
        r#"
        INSERT INTO collection_definitions (
            id, stable_key, external_key, title, description, kind, source,
            owner_type, owner_user_id, owner_display_name, scope, library_id,
            visibility, presentation, media_scope, duplicate_policy, artwork,
            theme, provenance, contract_version, revision, etag
        ) VALUES (
            $1, $2, $3, $4, $5, 'manual', 'manual',
            'user', $6, $7, 'user', $8,
            'private', 'shelf', $9::jsonb, 'reject_duplicates', '{}'::jsonb,
            '{}'::jsonb, $10::jsonb, 1, 1, $11
        )
        "#,
    )
    .bind(collection_id.to_uuid())
    .bind(&stable_key)
    .bind(external_key.as_deref())
    .bind(&title)
    .bind(description.as_deref())
    .bind(user.id)
    .bind(&user.display_name)
    .bind(payload.library_id.map(|id| id.to_uuid()))
    .bind(&media_scope)
    .bind(&provenance)
    .bind(&etag)
    .execute(&mut *tx)
    .await
    .map_err(map_smart_shelf_sqlx_error)?;

    for (index, item) in accepted_items.iter().enumerate() {
        let resolved =
            resolved_by_media.get(&item.media_id).ok_or_else(|| {
                SmartShelfHttpError::internal(format!(
                    "resolved smart-shelf item disappeared: {}",
                    item.media_id
                ))
            })?;
        let item_key = CollectionMemberKey::for_media(&item.media_id);
        let media_type = collection_media_type_slug(item.media_id);
        let position = i64::try_from(index + 1).map_err(|_| {
            SmartShelfHttpError::invalid_request(
                "smart-shelf item position exceeds i64",
            )
        })?;
        let title_snapshot = item
            .title
            .clone()
            .or_else(|| resolved.title.clone())
            .unwrap_or_else(|| item.media_id.to_string());
        let subtitle_snapshot =
            item.subtitle.clone().or_else(|| resolved.subtitle.clone());
        let membership_metadata = json!({
            "smart_shelf": {
                "draft_artifact_id": payload.artifact_id,
                "run_id": payload.run_id,
                "draft_ordinal": item.ordinal,
                "saved_position": position,
                "reason": item.reason,
                "sources": item.sources,
                "locked": item.locked,
                "replacement_of": item.replacement_of,
                "title": item.title,
            }
        });

        sqlx::query(
            r#"
            INSERT INTO collection_manual_memberships (
                collection_id, item_key, media_type, media_id,
                title_snapshot, subtitle_snapshot, position_key, sort_key,
                availability_status, availability_reason,
                availability_checked_at, added_by, metadata
            ) VALUES (
                $1, $2, ($3::text)::media_type, $4,
                $5, $6, ($7::text)::numeric, $8,
                'available', NULL,
                NOW(), $9, $10::jsonb
            )
            "#,
        )
        .bind(collection_id.to_uuid())
        .bind(item_key.as_str())
        .bind(media_type)
        .bind(*item.media_id.as_uuid())
        .bind(&title_snapshot)
        .bind(subtitle_snapshot.as_deref())
        .bind(position.to_string())
        .bind(item.reason.as_deref())
        .bind(user.id)
        .bind(&membership_metadata)
        .execute(&mut *tx)
        .await
        .map_err(map_smart_shelf_sqlx_error)?;
    }

    let updated = sqlx::query(
        r#"
        UPDATE intelligence_artifacts
        SET status = 'superseded',
            metadata = jsonb_set(metadata, '{smart_shelf_save}', $2::jsonb, true),
            updated_at = NOW()
        WHERE id = $1
          AND user_id = $3
          AND status = 'draft'
        "#,
    )
    .bind(payload.artifact_id)
    .bind(&save_metadata)
    .bind(user.id)
    .execute(&mut *tx)
    .await
    .map_err(map_smart_shelf_sqlx_error)?;
    if updated.rows_affected() != 1 {
        return Err(SmartShelfHttpError::stale());
    }

    tx.commit().await.map_err(|error| {
        SmartShelfHttpError::storage(format!(
            "commit smart-shelf save transaction failed: {error}"
        ))
    })?;

    let detail = state
        .unit_of_work()
        .collections
        .get_collection_detail(
            collection_id,
            GetCollectionDetailRequest {
                include_rule: false,
                include_items_preview: true,
                include_shelf_placements: false,
            },
            CollectionReadMode::Admin,
        )
        .await
        .map_err(SmartShelfHttpError::from_media_error)?
        .ok_or_else(|| {
            SmartShelfHttpError::storage(format!(
                "saved smart-shelf collection {collection_id} could not be loaded"
            ))
        })?;

    Ok(SmartShelfSaveResponse {
        draft_artifact_id: payload.artifact_id,
        collection_id,
        collection: detail.summary,
        item_count: u32::try_from(accepted_items.len()).unwrap_or(u32::MAX),
        saved_at_epoch_seconds: Some(saved_at.timestamp()),
    })
}

fn smart_shelf_collection_media_scope(
    items: &[SmartShelfDraftItem],
) -> CollectionMediaScope {
    let mut media_types = Vec::new();
    for item in items {
        let media_type = CollectionMediaKind::from(&item.media_id);
        if !media_types.contains(&media_type) {
            media_types.push(media_type);
        }
    }
    if media_types.is_empty() {
        CollectionMediaScope::All
    } else {
        CollectionMediaScope::Types { media_types }
    }
}

fn collection_media_type_slug(media_id: MediaID) -> &'static str {
    match CollectionMediaKind::from(&media_id) {
        CollectionMediaKind::Movie => "movie",
        CollectionMediaKind::Series => "series",
        CollectionMediaKind::Season => "season",
        CollectionMediaKind::Episode => "episode",
    }
}

fn map_smart_shelf_sqlx_error(error: sqlx::Error) -> SmartShelfHttpError {
    if let sqlx::Error::Database(database) = &error
        && let Some(constraint) = database.constraint()
        && matches!(
            constraint,
            "uq_collection_definitions_external_key"
                | "uq_collection_manual_memberships_item_key"
                | "uq_collection_manual_memberships_media"
                | "uq_collection_manual_memberships_position"
        )
    {
        return SmartShelfHttpError::collection_conflict(
            "smart-shelf save conflicted with an existing collection write",
        );
    }
    SmartShelfHttpError::storage(format!(
        "smart-shelf collection save failed: {error}"
    ))
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
