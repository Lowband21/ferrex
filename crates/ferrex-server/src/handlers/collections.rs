use std::collections::HashSet;

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use ferrex_core::{
    api::{ApiResponse, types::*},
    database::repository_ports::collections::CollectionReadMode,
    domain::users::{
        rbac::{
            UserPermissions, permissions as rbac_permissions,
            roles as rbac_roles,
        },
        user::User,
    },
};
use serde::Deserialize;
use uuid::Uuid;

use crate::infra::{app_state::AppState, demo_mode, errors::AppError};

fn collection_id(id: Uuid) -> CollectionId {
    CollectionId(id)
}

#[derive(Debug, Deserialize)]
pub struct ListCollectionsQueryParams {
    pub cursor: Option<String>,
    pub limit: Option<u16>,
    pub kind: Option<CollectionKind>,
    pub source: Option<CollectionSource>,
    pub scope: Option<CollectionScope>,
    pub visibility: Option<CollectionVisibility>,
    pub media_type: Option<CollectionMediaKind>,
    pub shelf_surface: Option<ShelfSurface>,
    pub shelf_key: Option<String>,
    pub pinned: Option<bool>,
    #[serde(default)]
    pub include_unpinned: bool,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub include_item_counts: bool,
    pub mode: Option<String>,
}

impl ListCollectionsQueryParams {
    fn into_request(&self) -> ListCollectionsRequest {
        ListCollectionsRequest {
            page: CollectionPagination {
                cursor: normalize_optional_string(self.cursor.clone()),
                limit: self.limit.unwrap_or(DEFAULT_COLLECTION_PAGE_LIMIT),
            },
            kind: self.kind,
            scope: self.scope,
            visibility: self.visibility,
            media_type: self.media_type,
            include_archived: self.include_archived,
            include_item_counts: self.include_item_counts,
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
    fn into_request(&self) -> GetCollectionDetailRequest {
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
    fn into_request(&self) -> ListCollectionItemsRequest {
        ListCollectionItemsRequest {
            page: CollectionPagination {
                cursor: normalize_optional_string(self.cursor.clone()),
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
    fn into_request(&self) -> ListShelfPlacementsRequest {
        ListShelfPlacementsRequest {
            surface: self.surface,
            shelf_key: normalize_optional_string(self.shelf_key.clone()),
            include_unpinned: self.include_unpinned,
        }
    }
}

/// List collections visible to authenticated clients.
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

    let source_filter = params.source;
    let shelf_filter = collection_ids_for_shelf_filter(
        &state,
        &user,
        is_admin,
        params.shelf_surface,
        normalize_optional_string(params.shelf_key.clone()),
        params.pinned,
        params.include_unpinned,
    )
    .await?;

    let mut response = state
        .unit_of_work()
        .collections
        .list_collections(params.into_request(), mode)
        .await?;
    filter_collection_response(
        &state,
        &mut response,
        user.id,
        is_admin,
        source_filter,
        shelf_filter.as_ref(),
    );

    Ok(Json(ApiResponse::success(response)))
}

/// Fetch collection metadata and optional rule/item/shelf expansions.
pub async fn get_collection_detail_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(id): Path<Uuid>,
    Query(params): Query<GetCollectionDetailQueryParams>,
) -> Result<Json<ApiResponse<GetCollectionDetailResponse>>, AppError> {
    let id = collection_id(id);
    let mode = parse_collection_read_mode(params.mode.as_deref())?;
    let summary = load_collection_summary(&state, id).await?;
    authorize_collection_access(&summary, &state, &user, &permissions, mode)?;

    let mut collection = state
        .unit_of_work()
        .collections
        .get_collection_detail(id, params.into_request(), mode)
        .await?
        .ok_or_else(|| AppError::not_found("Collection not found"))?;
    if !is_admin_user(&permissions) {
        let owned = collection_owned_by(&collection.summary, user.id);
        collection.shelf_placements.retain(|placement| {
            placement.surface != ShelfSurface::Admin
                && (placement.visibility != CollectionVisibility::Private
                    || owned)
        });
    }

    Ok(Json(ApiResponse::success(GetCollectionDetailResponse {
        collection,
    })))
}

/// List collection members with availability-aware filtering.
pub async fn list_collection_items_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(permissions): Extension<UserPermissions>,
    Path(id): Path<Uuid>,
    Query(params): Query<ListCollectionItemsQueryParams>,
) -> Result<Json<ApiResponse<ListCollectionItemsResponse>>, AppError> {
    let id = collection_id(id);
    let mode = parse_collection_read_mode(params.mode.as_deref())?;
    let summary = load_collection_summary(&state, id).await?;
    authorize_collection_access(&summary, &state, &user, &permissions, mode)?;

    let response = state
        .unit_of_work()
        .collections
        .list_collection_items(id, params.into_request(), mode)
        .await?;
    Ok(Json(ApiResponse::success(response)))
}

/// List shelf placements visible to the authenticated user.
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

    let mut response = state
        .unit_of_work()
        .collections
        .list_shelf_placements(
            params.into_request(),
            if is_admin {
                CollectionReadMode::Admin
            } else {
                CollectionReadMode::Normal
            },
        )
        .await?;
    filter_shelf_placements(&state, &mut response, user.id, is_admin).await?;

    Ok(Json(ApiResponse::success(response)))
}

async fn collection_ids_for_shelf_filter(
    state: &AppState,
    user: &User,
    is_admin: bool,
    surface: Option<ShelfSurface>,
    shelf_key: Option<String>,
    pinned: Option<bool>,
    include_unpinned: bool,
) -> Result<Option<HashSet<CollectionId>>, AppError> {
    if surface.is_none() && shelf_key.is_none() && pinned.is_none() {
        return Ok(None);
    }

    let mut response = state
        .unit_of_work()
        .collections
        .list_shelf_placements(
            ListShelfPlacementsRequest {
                surface,
                shelf_key,
                include_unpinned: include_unpinned || pinned == Some(false),
            },
            if is_admin {
                CollectionReadMode::Admin
            } else {
                CollectionReadMode::Normal
            },
        )
        .await?;
    if let Some(expected_pinned) = pinned {
        response
            .placements
            .retain(|placement| placement.pinned == expected_pinned);
    }
    filter_shelf_placements(state, &mut response, user.id, is_admin).await?;

    Ok(Some(
        response
            .placements
            .into_iter()
            .map(|placement| placement.collection_id)
            .collect(),
    ))
}

pub(crate) async fn load_collection_summary(
    state: &AppState,
    id: CollectionId,
) -> Result<CollectionSummary, AppError> {
    state
        .unit_of_work()
        .collections
        .get_collection_detail(
            id,
            GetCollectionDetailRequest::default(),
            CollectionReadMode::Normal,
        )
        .await?
        .map(|detail| detail.summary)
        .ok_or_else(|| AppError::not_found("Collection not found"))
}

pub(crate) fn authorize_collection_access(
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

fn filter_collection_response(
    state: &AppState,
    response: &mut ListCollectionsResponse,
    user_id: Uuid,
    is_admin: bool,
    source_filter: Option<CollectionSource>,
    shelf_filter: Option<&HashSet<CollectionId>>,
) {
    response.collections.retain(|summary| {
        source_filter.is_none_or(|source| source == summary.source)
            && shelf_filter.is_none_or(|ids| ids.contains(&summary.identity.id))
            && collection_visible_to_user(summary, user_id, is_admin, is_admin)
            && collection_visible_in_demo(state, summary)
    });
    response.page.total = response.collections.len() as u64;
    if response.collections.is_empty() {
        response.page.next_cursor = None;
    }
}

async fn filter_shelf_placements(
    state: &AppState,
    response: &mut ListShelfPlacementsResponse,
    user_id: Uuid,
    is_admin: bool,
) -> Result<(), AppError> {
    let mut visible = Vec::with_capacity(response.placements.len());
    for placement in response.placements.drain(..) {
        if !is_admin && placement.surface == ShelfSurface::Admin {
            continue;
        }
        match load_collection_summary(state, placement.collection_id).await {
            Ok(summary) => {
                let owned = collection_owned_by(&summary, user_id);
                if collection_visible_to_user(
                    &summary, user_id, is_admin, is_admin,
                ) && collection_visible_in_demo(state, &summary)
                    && (placement.visibility != CollectionVisibility::Private
                        || owned
                        || is_admin)
                {
                    visible.push(placement);
                }
            }
            Err(err) if err.status == axum::http::StatusCode::NOT_FOUND => {}
            Err(err) => return Err(err),
        }
    }
    response.placements = visible;
    Ok(())
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

pub(crate) fn collection_owned_by(
    summary: &CollectionSummary,
    user_id: Uuid,
) -> bool {
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

pub(crate) fn is_admin_user(permissions: &UserPermissions) -> bool {
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
