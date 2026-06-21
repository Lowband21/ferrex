use axum::{
    Extension, Json,
    extract::{Path, State},
};
use ferrex_core::{
    api::{ApiResponse, types::intelligence::*},
    player_prelude::User,
};
use ferrex_model::{
    EpisodeID, LibraryId, MediaID, MovieID, SeasonID, SeriesID,
};
use serde::Deserialize;
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
    if let Some((kind, id)) = raw.split_once('-') {
        return Some((normalize_media_kind(kind)?, id));
    }

    let open = raw.find('(')?;
    let close = raw.strip_suffix(')')?;
    let kind = normalize_media_kind(&raw[..open])?;
    Some((kind, &close[open + 1..]))
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
