use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use ferrex_core::{
    api::{ApiResponse, types::collections::*},
    error::MediaError,
    player_prelude::User,
};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::infra::app_state::AppState;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CollectionErrorCode {
    InvalidRequest,
    ValidationError,
    NotFound,
    Conflict,
    Internal,
}

#[derive(Debug, Serialize)]
pub(crate) struct CollectionErrorBody {
    pub code: CollectionErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

#[derive(Debug)]
pub(crate) struct CollectionHttpError {
    status: StatusCode,
    error: CollectionErrorBody,
}

impl CollectionHttpError {
    fn new(
        status: StatusCode,
        code: CollectionErrorCode,
        message: impl Into<String>,
        details: Value,
    ) -> Self {
        Self {
            status,
            error: CollectionErrorBody {
                code,
                message: message.into(),
                details,
            },
        }
    }

    fn from_media_error(error: MediaError) -> Self {
        match error {
            MediaError::InvalidMedia(message) => {
                let details = parse_error_details(&message);
                let code = if details.is_array() {
                    CollectionErrorCode::ValidationError
                } else {
                    CollectionErrorCode::InvalidRequest
                };
                Self::new(
                    StatusCode::BAD_REQUEST,
                    code,
                    message_from_details(&details).unwrap_or(message),
                    details,
                )
            }
            MediaError::NotFound(message) => Self::new(
                StatusCode::NOT_FOUND,
                CollectionErrorCode::NotFound,
                message,
                Value::Null,
            ),
            MediaError::Conflict(message) => {
                let details = parse_error_details(&message);
                Self::new(
                    StatusCode::CONFLICT,
                    CollectionErrorCode::Conflict,
                    message_from_details(&details).unwrap_or(message),
                    details,
                )
            }
            MediaError::Internal(message) => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                CollectionErrorCode::Internal,
                message,
                Value::Null,
            ),
            other => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                CollectionErrorCode::Internal,
                other.to_string(),
                Value::Null,
            ),
        }
    }
}

impl IntoResponse for CollectionHttpError {
    fn into_response(self) -> Response {
        let message = self.error.message.clone();
        let body = Json(json!({
            "status": "error",
            "error": self.error,
            "message": message,
        }));
        (self.status, body).into_response()
    }
}

fn parse_error_details(message: &str) -> Value {
    serde_json::from_str(message).unwrap_or(Value::Null)
}

fn message_from_details(details: &Value) -> Option<String> {
    details
        .get("message")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn collection_id(id: Uuid) -> CollectionId {
    CollectionId::from(id)
}

pub(crate) async fn create_collection_handler(
    State(state): State<AppState>,
    Json(request): Json<CreateCollectionRequest>,
) -> Result<Json<ApiResponse<CreateCollectionResponse>>, CollectionHttpError> {
    let collection = state
        .unit_of_work()
        .collections
        .create_collection(request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(CreateCollectionResponse {
        collection,
    })))
}

pub(crate) async fn update_collection_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateCollectionRequest>,
) -> Result<Json<ApiResponse<UpdateCollectionResponse>>, CollectionHttpError> {
    let collection = state
        .unit_of_work()
        .collections
        .update_collection(collection_id(id), request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(UpdateCollectionResponse {
        collection,
    })))
}

pub(crate) async fn archive_collection_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<Uuid>,
    Json(request): Json<ArchiveCollectionRequest>,
) -> Result<Json<ApiResponse<ArchiveCollectionResponse>>, CollectionHttpError> {
    let response = state
        .unit_of_work()
        .collections
        .archive_collection(collection_id(id), request, Some(user.id))
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn delete_collection_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<Uuid>,
    Json(request): Json<DeleteCollectionRequest>,
) -> Result<Json<ApiResponse<DeleteCollectionResponse>>, CollectionHttpError> {
    let response = state
        .unit_of_work()
        .collections
        .delete_collection(collection_id(id), request, Some(user.id))
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn manual_add_collection_items_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<Uuid>,
    Json(request): Json<ManualAddCollectionItemsRequest>,
) -> Result<
    Json<ApiResponse<ManualAddCollectionItemsResponse>>,
    CollectionHttpError,
> {
    let response = state
        .unit_of_work()
        .collections
        .manual_add_collection_items(collection_id(id), request, Some(user.id))
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn manual_remove_collection_items_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(request): Json<ManualRemoveCollectionItemsRequest>,
) -> Result<
    Json<ApiResponse<ManualRemoveCollectionItemsResponse>>,
    CollectionHttpError,
> {
    let response = state
        .unit_of_work()
        .collections
        .manual_remove_collection_items(collection_id(id), request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn manual_reorder_collection_items_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(request): Json<ManualReorderCollectionItemsRequest>,
) -> Result<
    Json<ApiResponse<ManualReorderCollectionItemsResponse>>,
    CollectionHttpError,
> {
    let response = state
        .unit_of_work()
        .collections
        .manual_reorder_collection_items(collection_id(id), request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn validate_collection_rule_handler(
    State(state): State<AppState>,
    Json(request): Json<ValidateCollectionRuleRequest>,
) -> Result<
    Json<ApiResponse<ValidateCollectionRuleResponse>>,
    CollectionHttpError,
> {
    let response = state
        .unit_of_work()
        .collections
        .validate_collection_rule(request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn preview_collection_rule_handler(
    State(state): State<AppState>,
    Json(request): Json<PreviewCollectionRuleRequest>,
) -> Result<Json<ApiResponse<PreviewCollectionRuleResponse>>, CollectionHttpError>
{
    let response = state
        .unit_of_work()
        .collections
        .preview_collection_rule(request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn refresh_collection_rule_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(request): Json<RefreshCollectionRuleRequest>,
) -> Result<Json<ApiResponse<RefreshCollectionRuleResponse>>, CollectionHttpError>
{
    let response = state
        .unit_of_work()
        .collections
        .refresh_collection_rule(collection_id(id), request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn list_shelf_placements_handler(
    State(state): State<AppState>,
    Json(request): Json<ListShelfPlacementsRequest>,
) -> Result<Json<ApiResponse<ListShelfPlacementsResponse>>, CollectionHttpError>
{
    let response = state
        .unit_of_work()
        .collections
        .list_shelf_placements(request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn pin_shelf_placement_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<PinShelfPlacementRequest>,
) -> Result<Json<ApiResponse<PinShelfPlacementResponse>>, CollectionHttpError> {
    let response = state
        .unit_of_work()
        .collections
        .pin_shelf_placement(request, Some(user.id))
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn reorder_shelf_placements_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<ReorderShelfPlacementsRequest>,
) -> Result<
    Json<ApiResponse<ReorderShelfPlacementsResponse>>,
    CollectionHttpError,
> {
    let response = state
        .unit_of_work()
        .collections
        .reorder_shelf_placements(request, Some(user.id))
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn tmdb_import_collection_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<TmdbImportCollectionRequest>,
) -> Result<Json<ApiResponse<TmdbImportCollectionResponse>>, CollectionHttpError>
{
    let response = state
        .unit_of_work()
        .collections
        .tmdb_import_collection(request, Some(user.id))
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn tmdb_list_collections_handler(
    State(state): State<AppState>,
    Json(request): Json<TmdbListCollectionsRequest>,
) -> Result<Json<ApiResponse<TmdbListCollectionsResponse>>, CollectionHttpError>
{
    let response = state
        .unit_of_work()
        .collections
        .tmdb_list_collections(request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use ferrex_core::api::types::collections::{
        CollectionId, CollectionManualMembershipConflict,
        CollectionManualMembershipConflictCode, CollectionMemberKey,
    };

    #[tokio::test]
    async fn collection_error_contract_preserves_conflict_details() {
        let conflict = CollectionManualMembershipConflict {
            code: CollectionManualMembershipConflictCode::DuplicateMember,
            collection_id: CollectionId::from(Uuid::from_u128(42)),
            duplicate_policy: None,
            item_keys: vec![CollectionMemberKey::from("movie:duplicate")],
            message: "manual collection already contains this item".to_string(),
        };
        let error = CollectionHttpError::from_media_error(
            MediaError::Conflict(serde_json::to_string(&conflict).unwrap()),
        );
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["status"], "error");
        assert_eq!(value["error"]["code"], "conflict");
        assert_eq!(value["error"]["details"]["code"], "duplicate_member");
        assert_eq!(
            value["message"],
            "manual collection already contains this item"
        );
    }

    #[tokio::test]
    async fn collection_error_contract_marks_rule_errors_as_validation() {
        let details = json!([
            {
                "path": "limit.max_items",
                "message": "max_items must be greater than zero"
            }
        ]);
        let error = CollectionHttpError::from_media_error(
            MediaError::InvalidMedia(details.to_string()),
        );
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "validation_error");
        assert_eq!(value["error"]["details"][0]["path"], "limit.max_items");
    }
}
