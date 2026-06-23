use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use ferrex_core::{
    api::{ApiResponse, types::collections::*},
    database::repository_ports::collections::CollectionReadMode,
    domain::users::{
        rbac::{
            UserPermissions, permissions as rbac_permissions,
            roles as rbac_roles,
        },
        user::User,
    },
    error::MediaError,
    types::LibraryId,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::infra::{app_state::AppState, demo_mode, errors::AppError};

#[derive(Debug, Deserialize)]
pub struct ListCollectionsQueryParams {
    pub cursor: Option<String>,
    pub limit: Option<u16>,
    pub kind: Option<CollectionKind>,
    pub source: Option<CollectionSource>,
    pub scope: Option<CollectionScope>,
    pub owner_type: Option<CollectionOwnerType>,
    pub owner_user_id: Option<Uuid>,
    pub owner_device_id: Option<String>,
    pub visibility: Option<CollectionVisibility>,
    pub presentation: Option<CollectionPresentationMode>,
    pub media_type: Option<CollectionMediaKind>,
    pub library_id: Option<Uuid>,
    pub shelf_surface: Option<ShelfSurface>,
    pub shelf_key: Option<String>,
    pub pinned: Option<bool>,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub include_item_counts: bool,
    pub mode: Option<String>,
}

impl ListCollectionsQueryParams {
    fn into_request(
        self,
        viewer_user_id: Option<Uuid>,
    ) -> ListCollectionsRequest {
        ListCollectionsRequest {
            page: CollectionPagination {
                cursor: normalize_optional_string(self.cursor),
                limit: self.limit.unwrap_or(DEFAULT_COLLECTION_PAGE_LIMIT),
            },
            kind: self.kind,
            source: self.source,
            scope: self.scope,
            owner_type: self.owner_type,
            owner_user_id: self.owner_user_id,
            owner_device_id: normalize_optional_string(self.owner_device_id),
            visibility: self.visibility,
            presentation: self.presentation,
            media_type: self.media_type,
            library_id: self.library_id.map(LibraryId),
            shelf_surface: self.shelf_surface,
            shelf_key: normalize_optional_string(self.shelf_key),
            pinned: self.pinned,
            include_archived: self.include_archived,
            include_item_counts: self.include_item_counts,
            viewer_user_id,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct GetCollectionDetailQueryParams {
    #[serde(default)]
    pub include_rule: bool,
    #[serde(default)]
    pub include_items_preview: bool,
    #[serde(default)]
    pub include_shelf_placements: bool,
    pub mode: Option<String>,
}

impl GetCollectionDetailQueryParams {
    fn into_request(self) -> GetCollectionDetailRequest {
        GetCollectionDetailRequest {
            include_rule: self.include_rule,
            include_items_preview: self.include_items_preview,
            include_shelf_placements: self.include_shelf_placements,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ListCollectionItemsQueryParams {
    pub cursor: Option<String>,
    pub limit: Option<u16>,
    pub availability: Option<CollectionMemberAvailabilityStatus>,
    pub mode: Option<String>,
}

impl ListCollectionItemsQueryParams {
    fn into_request(self) -> ListCollectionItemsRequest {
        ListCollectionItemsRequest {
            page: CollectionPagination {
                cursor: normalize_optional_string(self.cursor),
                limit: self.limit.unwrap_or(DEFAULT_COLLECTION_PAGE_LIMIT),
            },
            availability: self.availability,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ListShelfPlacementsQueryParams {
    pub surface: Option<ShelfSurface>,
    pub shelf_key: Option<String>,
    #[serde(default)]
    pub include_unpinned: bool,
}

impl ListShelfPlacementsQueryParams {
    fn into_request(
        self,
        viewer_user_id: Option<Uuid>,
    ) -> ListShelfPlacementsRequest {
        ListShelfPlacementsRequest {
            surface: self.surface,
            shelf_key: normalize_optional_string(self.shelf_key),
            include_unpinned: self.include_unpinned,
            viewer_user_id,
        }
    }
}

pub async fn list_collections_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Query(params): Query<ListCollectionsQueryParams>,
) -> Result<Json<ApiResponse<ListCollectionsResponse>>, AppError> {
    let mode = parse_collection_read_mode(params.mode.as_deref())?;
    let is_admin = is_admin_user(&permissions);
    if mode != CollectionReadMode::Normal && !is_admin {
        return Err(AppError::forbidden(
            "admin access is required for non-normal collection list reads",
        ));
    }
    if params.include_archived && !is_admin {
        return Err(AppError::forbidden(
            "admin access is required to include archived collections",
        ));
    }
    if params.shelf_surface == Some(ShelfSurface::Admin) && !is_admin {
        return Err(AppError::forbidden(
            "admin access is required to filter admin shelf placements",
        ));
    }

    let request = params.into_request((!is_admin).then_some(user.id));
    let mut response = state
        .unit_of_work()
        .collections
        .list_collections(request, mode)
        .await
        .map_err(map_collection_error)?;
    filter_demo_collection_response(&state, &mut response);

    Ok(Json(ApiResponse::success(response)))
}

pub async fn get_collection_detail_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(collection_uuid): Path<Uuid>,
    Query(params): Query<GetCollectionDetailQueryParams>,
) -> Result<Json<ApiResponse<GetCollectionDetailResponse>>, AppError> {
    let id = CollectionId(collection_uuid);
    let mode = parse_collection_read_mode(params.mode.as_deref())?;
    let summary = load_collection_summary(&state, id).await?;
    authorize_collection_access(&summary, &state, &user, &permissions, mode)?;

    let mut detail = state
        .unit_of_work()
        .collections
        .get_collection_detail(id, params.into_request(), mode)
        .await
        .map_err(map_collection_error)?
        .ok_or_else(|| AppError::not_found("Collection not found"))?;
    if !is_admin_user(&permissions) {
        let owned = collection_owned_by(&detail.summary, user.id);
        detail.shelf_placements.retain(|placement| {
            placement.surface != ShelfSurface::Admin
                && (placement.visibility != CollectionVisibility::Private
                    || owned)
        });
    }

    Ok(Json(ApiResponse::success(GetCollectionDetailResponse {
        collection: detail,
    })))
}

pub async fn list_collection_items_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(collection_uuid): Path<Uuid>,
    Query(params): Query<ListCollectionItemsQueryParams>,
) -> Result<Json<ApiResponse<ListCollectionItemsResponse>>, AppError> {
    let id = CollectionId(collection_uuid);
    let mode = parse_collection_read_mode(params.mode.as_deref())?;
    let summary = load_collection_summary(&state, id).await?;
    authorize_collection_access(&summary, &state, &user, &permissions, mode)?;

    let response = state
        .unit_of_work()
        .collections
        .list_collection_items(id, params.into_request(), mode)
        .await
        .map_err(map_collection_error)?;

    Ok(Json(ApiResponse::success(response)))
}

pub async fn list_shelf_placements_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Query(params): Query<ListShelfPlacementsQueryParams>,
) -> Result<Json<ApiResponse<ListShelfPlacementsResponse>>, AppError> {
    let is_admin = is_admin_user(&permissions);
    if params.surface == Some(ShelfSurface::Admin) && !is_admin {
        return Err(AppError::forbidden(
            "admin access is required to read admin shelf placements",
        ));
    }

    let viewer_user_id = if is_admin { None } else { Some(user.id) };
    let mut response = state
        .unit_of_work()
        .collections
        .list_shelf_placements(
            params.into_request(viewer_user_id),
            if is_admin {
                CollectionReadMode::Admin
            } else {
                CollectionReadMode::Normal
            },
        )
        .await
        .map_err(map_collection_error)?;
    if !is_admin {
        response
            .placements
            .retain(|placement| placement.surface != ShelfSurface::Admin);
    }
    filter_demo_shelf_placements(&state, &mut response).await?;

    Ok(Json(ApiResponse::success(response)))
}

async fn load_collection_summary(
    state: &AppState,
    id: CollectionId,
) -> Result<CollectionSummary, AppError> {
    let request = GetCollectionDetailRequest::default();
    state
        .unit_of_work()
        .collections
        .get_collection_detail(id, request, CollectionReadMode::Normal)
        .await
        .map_err(map_collection_error)?
        .map(|detail| detail.summary)
        .ok_or_else(|| AppError::not_found("Collection not found"))
}

fn authorize_collection_access(
    summary: &CollectionSummary,
    state: &AppState,
    user: &User,
    permissions: &UserPermissions,
    mode: CollectionReadMode,
) -> Result<(), AppError> {
    if !collection_visible_in_demo(state, summary) {
        return Err(AppError::not_found("Collection not found"));
    }

    let is_admin = is_admin_user(permissions);
    let owned = collection_owned_by(summary, user.id);
    let archived_allowed =
        is_admin || (mode == CollectionReadMode::Edit && owned);
    if !collection_visible_to_user(summary, user.id, is_admin, archived_allowed)
    {
        return Err(AppError::not_found("Collection not found"));
    }

    match mode {
        CollectionReadMode::Normal => Ok(()),
        CollectionReadMode::Edit if owned || is_admin => Ok(()),
        CollectionReadMode::Admin | CollectionReadMode::Debug if is_admin => {
            Ok(())
        }
        CollectionReadMode::Edit => Err(AppError::forbidden(
            "collection edit reads require collection ownership or admin access",
        )),
        CollectionReadMode::Admin | CollectionReadMode::Debug => {
            Err(AppError::forbidden(
                "admin access is required for admin/debug collection reads",
            ))
        }
    }
}

fn collection_visible_to_user(
    summary: &CollectionSummary,
    user_id: Uuid,
    is_admin: bool,
    archived_allowed: bool,
) -> bool {
    if summary.timestamps.archived_at.is_some() && !archived_allowed {
        return false;
    }
    if is_admin {
        return true;
    }
    summary.visibility != CollectionVisibility::Private
        || collection_owned_by(summary, user_id)
}

fn collection_owned_by(summary: &CollectionSummary, user_id: Uuid) -> bool {
    summary.owner.owner_type == CollectionOwnerType::User
        && summary.owner.user_id == Some(user_id)
}

fn collection_visible_in_demo(
    state: &AppState,
    summary: &CollectionSummary,
) -> bool {
    if !demo_mode::is_demo_mode(state) {
        return true;
    }

    match &summary.media_scope {
        CollectionMediaScope::Library { library_id, .. } => {
            demo_mode::is_demo_library(library_id)
        }
        _ => true,
    }
}

fn filter_demo_collection_response(
    state: &AppState,
    response: &mut ListCollectionsResponse,
) {
    if !demo_mode::is_demo_mode(state) {
        return;
    }

    response
        .collections
        .retain(|summary| collection_visible_in_demo(state, summary));
    response.page.total = response.collections.len() as u64;
    if response.collections.is_empty() {
        response.page.next_cursor = None;
    }
}

async fn filter_demo_shelf_placements(
    state: &AppState,
    response: &mut ListShelfPlacementsResponse,
) -> Result<(), AppError> {
    if !demo_mode::is_demo_mode(state) {
        return Ok(());
    }

    let mut visible = Vec::with_capacity(response.placements.len());
    for placement in response.placements.drain(..) {
        match load_collection_summary(state, placement.collection_id).await {
            Ok(summary) if collection_visible_in_demo(state, &summary) => {
                visible.push(placement);
            }
            Ok(_) => {}
            Err(err) if err.status == axum::http::StatusCode::NOT_FOUND => {}
            Err(err) => return Err(err),
        }
    }
    response.placements = visible;
    Ok(())
}

fn parse_collection_read_mode(
    mode: Option<&str>,
) -> Result<CollectionReadMode, AppError> {
    match mode
        .unwrap_or("normal")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "normal" => Ok(CollectionReadMode::Normal),
        "edit" => Ok(CollectionReadMode::Edit),
        "admin" => Ok(CollectionReadMode::Admin),
        "debug" => Ok(CollectionReadMode::Debug),
        value => Err(AppError::bad_request(format!(
            "invalid collection read mode: {value}"
        ))),
    }
}

fn is_admin_user(permissions: &UserPermissions) -> bool {
    permissions.has_role(rbac_roles::ADMIN)
        || permissions.has_all_permissions(&[
            rbac_permissions::USERS_READ,
            rbac_permissions::USERS_CREATE,
            rbac_permissions::USERS_UPDATE,
            rbac_permissions::USERS_DELETE,
            rbac_permissions::USERS_MANAGE_ROLES,
        ])
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn map_collection_error(err: MediaError) -> AppError {
    match err {
        MediaError::InvalidMedia(message) => AppError::bad_request(message),
        MediaError::NotFound(message) => AppError::not_found(message),
        MediaError::Conflict(message) => AppError::conflict(message),
        MediaError::Internal(message) => AppError::internal(message),
        other => AppError::internal(other.to_string()),
    }
}
