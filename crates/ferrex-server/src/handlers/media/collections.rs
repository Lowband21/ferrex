use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use ferrex_core::{
    api::{ApiResponse, types::collections::*},
    database::repository_ports::collections::CollectionReadMode,
    error::MediaError,
    player_prelude::{User, UserPermissions},
};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    handlers::collections as collection_read_handlers,
    infra::{app_state::AppState, errors::AppError},
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CollectionErrorCode {
    InvalidRequest,
    ValidationError,
    NotFound,
    Forbidden,
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

    fn from_app_error(error: AppError) -> Self {
        let code = match error.status {
            StatusCode::BAD_REQUEST => CollectionErrorCode::InvalidRequest,
            StatusCode::NOT_FOUND => CollectionErrorCode::NotFound,
            StatusCode::FORBIDDEN => CollectionErrorCode::Forbidden,
            StatusCode::CONFLICT => CollectionErrorCode::Conflict,
            StatusCode::INTERNAL_SERVER_ERROR => CollectionErrorCode::Internal,
            _ => CollectionErrorCode::Internal,
        };
        Self::new(error.status, code, error.message, Value::Null)
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            CollectionErrorCode::Forbidden,
            message,
            Value::Null,
        )
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            CollectionErrorCode::NotFound,
            message,
            Value::Null,
        )
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

fn authenticated_user_owner(user: &User) -> CollectionOwner {
    CollectionOwner {
        owner_type: CollectionOwnerType::User,
        user_id: Some(user.id),
        device_id: None,
        display_name: Some(user.display_name.clone()),
    }
}

fn is_default_system_owner(owner: &CollectionOwner) -> bool {
    owner.owner_type == CollectionOwnerType::System
        && owner.user_id.is_none()
        && owner
            .device_id
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
}

fn normalize_default_user_owner(
    request: &mut CreateCollectionRequest,
    user: &User,
) {
    if request.scope == CollectionScope::User
        && is_default_system_owner(&request.owner)
    {
        request.owner = authenticated_user_owner(user);
    }
}

fn authorize_collection_create_request(
    request: &mut CreateCollectionRequest,
    user: &User,
    permissions: &UserPermissions,
) -> Result<(), CollectionHttpError> {
    normalize_default_user_owner(request, user);
    if collection_read_handlers::is_admin_user(permissions) {
        return Ok(());
    }

    if request.owner.owner_type != CollectionOwnerType::User
        || request.owner.user_id != Some(user.id)
    {
        return Err(CollectionHttpError::forbidden(
            "collections can only be created for the authenticated user",
        ));
    }
    if request.scope != CollectionScope::User {
        return Err(CollectionHttpError::forbidden(
            "non-admin users can only create user-scoped collections",
        ));
    }
    if request.visibility != CollectionVisibility::Private {
        return Err(CollectionHttpError::forbidden(
            "non-admin users can only create private collections",
        ));
    }
    if !matches!(
        (request.kind, request.source),
        (CollectionKind::Manual, CollectionSource::Manual)
            | (CollectionKind::DynamicRule, CollectionSource::DynamicRule)
    ) {
        return Err(CollectionHttpError::forbidden(
            "admin access is required to create system, imported, or TMDB collections",
        ));
    }

    Ok(())
}

fn authorize_collection_update_request(
    request: &UpdateCollectionRequest,
    permissions: &UserPermissions,
) -> Result<(), CollectionHttpError> {
    if collection_read_handlers::is_admin_user(permissions) {
        return Ok(());
    }
    if request
        .visibility
        .is_some_and(|visibility| visibility != CollectionVisibility::Private)
    {
        return Err(CollectionHttpError::forbidden(
            "admin access is required to publish or share collections",
        ));
    }
    Ok(())
}

fn authorize_tmdb_import_request(
    permissions: &UserPermissions,
) -> Result<(), CollectionHttpError> {
    if collection_read_handlers::is_admin_user(permissions) {
        Ok(())
    } else {
        Err(CollectionHttpError::forbidden(
            "admin access is required to import TMDB collections",
        ))
    }
}

async fn authorize_collection_mutation(
    state: &AppState,
    user: &User,
    permissions: &UserPermissions,
    id: CollectionId,
) -> Result<(), CollectionHttpError> {
    let summary = collection_read_handlers::load_collection_summary(state, id)
        .await
        .map_err(CollectionHttpError::from_app_error)?;
    collection_read_handlers::authorize_collection_access(
        &summary,
        state,
        user,
        permissions,
        CollectionReadMode::Edit,
    )
    .map_err(CollectionHttpError::from_app_error)
}

async fn authorize_shelf_reorder(
    state: &AppState,
    user: &User,
    permissions: &UserPermissions,
    request: &ReorderShelfPlacementsRequest,
) -> Result<(), CollectionHttpError> {
    if request.ordering.is_empty() {
        return Ok(());
    }

    let placements = state
        .unit_of_work()
        .collections
        .list_shelf_placements(
            ListShelfPlacementsRequest {
                surface: None,
                shelf_key: None,
                include_unpinned: true,
            },
            if collection_read_handlers::is_admin_user(permissions) {
                CollectionReadMode::Admin
            } else {
                CollectionReadMode::Normal
            },
        )
        .await
        .map_err(CollectionHttpError::from_media_error)?;

    for order in &request.ordering {
        let placement = placements
            .placements
            .iter()
            .find(|placement| placement.id == order.placement_id)
            .ok_or_else(|| {
                CollectionHttpError::not_found("Shelf placement not found")
            })?;
        authorize_collection_mutation(
            state,
            user,
            permissions,
            placement.collection_id,
        )
        .await?;
    }

    Ok(())
}

pub(crate) async fn create_collection_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Json(mut request): Json<CreateCollectionRequest>,
) -> Result<Json<ApiResponse<CreateCollectionResponse>>, CollectionHttpError> {
    authorize_collection_create_request(&mut request, &user, &permissions)?;

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
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateCollectionRequest>,
) -> Result<Json<ApiResponse<UpdateCollectionResponse>>, CollectionHttpError> {
    let id = collection_id(id);
    authorize_collection_mutation(&state, &user, &permissions, id).await?;
    authorize_collection_update_request(&request, &permissions)?;

    let collection = state
        .unit_of_work()
        .collections
        .update_collection(id, request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(UpdateCollectionResponse {
        collection,
    })))
}

pub(crate) async fn archive_collection_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(id): Path<Uuid>,
    Json(request): Json<ArchiveCollectionRequest>,
) -> Result<Json<ApiResponse<ArchiveCollectionResponse>>, CollectionHttpError> {
    let id = collection_id(id);
    authorize_collection_mutation(&state, &user, &permissions, id).await?;

    let response = state
        .unit_of_work()
        .collections
        .archive_collection(id, request, Some(user.id))
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn delete_collection_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(id): Path<Uuid>,
    Json(request): Json<DeleteCollectionRequest>,
) -> Result<Json<ApiResponse<DeleteCollectionResponse>>, CollectionHttpError> {
    let id = collection_id(id);
    authorize_collection_mutation(&state, &user, &permissions, id).await?;

    let response = state
        .unit_of_work()
        .collections
        .delete_collection(id, request, Some(user.id))
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn manual_add_collection_items_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(id): Path<Uuid>,
    Json(request): Json<ManualAddCollectionItemsRequest>,
) -> Result<
    Json<ApiResponse<ManualAddCollectionItemsResponse>>,
    CollectionHttpError,
> {
    let id = collection_id(id);
    authorize_collection_mutation(&state, &user, &permissions, id).await?;

    let response = state
        .unit_of_work()
        .collections
        .manual_add_collection_items(id, request, Some(user.id))
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn manual_remove_collection_items_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(id): Path<Uuid>,
    Json(request): Json<ManualRemoveCollectionItemsRequest>,
) -> Result<
    Json<ApiResponse<ManualRemoveCollectionItemsResponse>>,
    CollectionHttpError,
> {
    let id = collection_id(id);
    authorize_collection_mutation(&state, &user, &permissions, id).await?;

    let response = state
        .unit_of_work()
        .collections
        .manual_remove_collection_items(id, request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn manual_reorder_collection_items_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(id): Path<Uuid>,
    Json(request): Json<ManualReorderCollectionItemsRequest>,
) -> Result<
    Json<ApiResponse<ManualReorderCollectionItemsResponse>>,
    CollectionHttpError,
> {
    let id = collection_id(id);
    authorize_collection_mutation(&state, &user, &permissions, id).await?;

    let response = state
        .unit_of_work()
        .collections
        .manual_reorder_collection_items(id, request)
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
        .preview_collection_rule(request, CollectionReadMode::Normal)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn refresh_collection_rule_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(id): Path<Uuid>,
    Json(request): Json<RefreshCollectionRuleRequest>,
) -> Result<Json<ApiResponse<RefreshCollectionRuleResponse>>, CollectionHttpError>
{
    let id = collection_id(id);
    authorize_collection_mutation(&state, &user, &permissions, id).await?;

    let response = state
        .unit_of_work()
        .collections
        .refresh_collection_rule(id, request)
        .await
        .map_err(CollectionHttpError::from_media_error)?;
    Ok(Json(ApiResponse::success(response)))
}

pub(crate) async fn pin_shelf_placement_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Json(request): Json<PinShelfPlacementRequest>,
) -> Result<Json<ApiResponse<PinShelfPlacementResponse>>, CollectionHttpError> {
    authorize_collection_mutation(
        &state,
        &user,
        &permissions,
        request.collection_id,
    )
    .await?;

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
    Extension(permissions): Extension<UserPermissions>,
    Json(request): Json<ReorderShelfPlacementsRequest>,
) -> Result<
    Json<ApiResponse<ReorderShelfPlacementsResponse>>,
    CollectionHttpError,
> {
    authorize_shelf_reorder(&state, &user, &permissions, &request).await?;

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
    Extension(permissions): Extension<UserPermissions>,
    Json(request): Json<TmdbImportCollectionRequest>,
) -> Result<Json<ApiResponse<TmdbImportCollectionResponse>>, CollectionHttpError>
{
    authorize_tmdb_import_request(&permissions)?;

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
    Query(request): Query<TmdbListCollectionsRequest>,
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
