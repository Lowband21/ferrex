use async_trait::async_trait;
use ferrex_model::MediaID;
use uuid::Uuid;

use crate::api::types::collections::{
    ArchiveCollectionRequest, ArchiveCollectionResponse, CollectionDetail,
    CollectionId, CollectionMemberAvailability, CollectionMemberKey,
    CollectionPageInfo, CreateCollectionRequest, DeleteCollectionRequest,
    DeleteCollectionResponse, GetCollectionDetailRequest,
    ListCollectionItemsRequest, ListCollectionItemsResponse,
    ListCollectionsRequest, ListCollectionsResponse,
    ListShelfPlacementsRequest, ListShelfPlacementsResponse,
    MAX_COLLECTION_PAGE_LIMIT, ManualAddCollectionItemsRequest,
    ManualAddCollectionItemsResponse, ManualRemoveCollectionItemsRequest,
    ManualRemoveCollectionItemsResponse, ManualReorderCollectionItemsRequest,
    ManualReorderCollectionItemsResponse, PinShelfPlacementRequest,
    PinShelfPlacementResponse, PreviewCollectionRuleRequest,
    PreviewCollectionRuleResponse, RefreshCollectionRuleRequest,
    RefreshCollectionRuleResponse, ReorderShelfPlacementsRequest,
    ReorderShelfPlacementsResponse, TmdbImportCollectionRequest,
    TmdbImportCollectionResponse, TmdbListCollectionsRequest,
    TmdbListCollectionsResponse, UpdateCollectionRequest,
    ValidateCollectionRuleRequest, ValidateCollectionRuleResponse,
};
use crate::api::types::system_collections::{
    MarkSystemCollectionsStaleRequest, SystemCollectionDefinition,
    SystemCollectionSeedReport, SystemCollectionsStaleResponse,
};
use crate::error::{MediaError, Result};

/// Read projection mode for collection membership availability.
///
/// Normal user-facing reads hide unavailable/tombstoned/missing members by
/// default. Edit, admin, and debug reads preserve the full membership list and
/// expose per-member status so callers can repair collections without losing
/// stored membership rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CollectionReadMode {
    #[default]
    Normal,
    Edit,
    Admin,
    Debug,
}

impl CollectionReadMode {
    pub const fn exposes_preserved_membership(self) -> bool {
        matches!(self, Self::Edit | Self::Admin | Self::Debug)
    }
}

/// Logical collection item input accepted by the shared availability resolver.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CollectionItemIdentity {
    pub item_key: CollectionMemberKey,
    pub media_id: MediaID,
}

impl CollectionItemIdentity {
    pub fn new(media_id: MediaID) -> Self {
        Self {
            item_key: CollectionMemberKey::for_media(&media_id),
            media_id,
        }
    }
}

/// Current catalog/availability state for one logical collection item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionResolvedItem {
    pub item_key: CollectionMemberKey,
    pub media_id: MediaID,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub availability: CollectionMemberAvailability,
}

/// Repository/service port for collection definition reads and foundational
/// availability-aware membership projections.
#[async_trait]
pub trait CollectionRepository: Send + Sync {
    async fn create_collection(
        &self,
        request: CreateCollectionRequest,
    ) -> Result<CollectionDetail>;

    async fn update_collection(
        &self,
        id: CollectionId,
        request: UpdateCollectionRequest,
    ) -> Result<CollectionDetail>;

    async fn archive_collection(
        &self,
        id: CollectionId,
        request: ArchiveCollectionRequest,
        archived_by: Option<Uuid>,
    ) -> Result<ArchiveCollectionResponse>;

    async fn delete_collection(
        &self,
        id: CollectionId,
        request: DeleteCollectionRequest,
        deleted_by: Option<Uuid>,
    ) -> Result<DeleteCollectionResponse> {
        let _ = (id, request, deleted_by);
        Err(MediaError::InvalidMedia(
            "collection deletion is not supported by this repository"
                .to_string(),
        ))
    }

    async fn get_collection_detail(
        &self,
        id: CollectionId,
        request: GetCollectionDetailRequest,
        mode: CollectionReadMode,
    ) -> Result<Option<CollectionDetail>>;

    async fn list_collections(
        &self,
        request: ListCollectionsRequest,
        mode: CollectionReadMode,
    ) -> Result<ListCollectionsResponse>;

    async fn list_collection_items(
        &self,
        id: CollectionId,
        request: ListCollectionItemsRequest,
        mode: CollectionReadMode,
    ) -> Result<ListCollectionItemsResponse>;

    async fn manual_add_collection_items(
        &self,
        id: CollectionId,
        request: ManualAddCollectionItemsRequest,
        added_by: Option<Uuid>,
    ) -> Result<ManualAddCollectionItemsResponse>;

    async fn manual_remove_collection_items(
        &self,
        id: CollectionId,
        request: ManualRemoveCollectionItemsRequest,
    ) -> Result<ManualRemoveCollectionItemsResponse>;

    async fn manual_reorder_collection_items(
        &self,
        id: CollectionId,
        request: ManualReorderCollectionItemsRequest,
    ) -> Result<ManualReorderCollectionItemsResponse>;

    async fn validate_collection_rule(
        &self,
        request: ValidateCollectionRuleRequest,
    ) -> Result<ValidateCollectionRuleResponse> {
        Ok(ValidateCollectionRuleResponse::from_rule(&request.rule))
    }

    async fn preview_collection_rule(
        &self,
        request: PreviewCollectionRuleRequest,
        mode: CollectionReadMode,
    ) -> Result<PreviewCollectionRuleResponse>;

    async fn refresh_collection_rule(
        &self,
        id: CollectionId,
        request: RefreshCollectionRuleRequest,
    ) -> Result<RefreshCollectionRuleResponse>;

    async fn list_shelf_placements(
        &self,
        request: ListShelfPlacementsRequest,
        mode: CollectionReadMode,
    ) -> Result<ListShelfPlacementsResponse> {
        let _ = (request, mode);
        Err(MediaError::InvalidMedia(
            "shelf placement reads are not supported by this repository"
                .to_string(),
        ))
    }

    async fn pin_shelf_placement(
        &self,
        request: PinShelfPlacementRequest,
        pinned_by: Option<Uuid>,
    ) -> Result<PinShelfPlacementResponse> {
        let _ = (request, pinned_by);
        Err(MediaError::InvalidMedia(
            "shelf placement pinning is not supported by this repository"
                .to_string(),
        ))
    }

    async fn reorder_shelf_placements(
        &self,
        request: ReorderShelfPlacementsRequest,
        reordered_by: Option<Uuid>,
    ) -> Result<ReorderShelfPlacementsResponse> {
        let _ = (request, reordered_by);
        Err(MediaError::InvalidMedia(
            "shelf placement reordering is not supported by this repository"
                .to_string(),
        ))
    }

    async fn tmdb_import_collection(
        &self,
        request: TmdbImportCollectionRequest,
        imported_by: Option<Uuid>,
    ) -> Result<TmdbImportCollectionResponse> {
        let _ = (request, imported_by);
        Err(MediaError::InvalidMedia(
            "TMDB collection import is not supported by this repository"
                .to_string(),
        ))
    }

    async fn tmdb_list_collections(
        &self,
        request: TmdbListCollectionsRequest,
    ) -> Result<TmdbListCollectionsResponse> {
        let _ = request;
        Err(MediaError::InvalidMedia(
            "TMDB collection listing is not supported by this repository"
                .to_string(),
        ))
    }

    async fn resolve_collection_items(
        &self,
        items: &[CollectionItemIdentity],
    ) -> Result<Vec<CollectionResolvedItem>>;

    async fn ensure_system_collections(
        &self,
        definitions: &[SystemCollectionDefinition],
    ) -> Result<SystemCollectionSeedReport>;

    async fn mark_system_collections_stale(
        &self,
        request: MarkSystemCollectionsStaleRequest,
    ) -> Result<SystemCollectionsStaleResponse>;
}

pub(crate) fn clamp_collection_page_limit(limit: u16) -> u16 {
    limit.clamp(1, MAX_COLLECTION_PAGE_LIMIT)
}

pub(crate) fn parse_collection_cursor(cursor: Option<&str>) -> Result<usize> {
    match cursor {
        None | Some("") => Ok(0),
        Some(value) => value.parse::<usize>().map_err(|_| {
            MediaError::InvalidMedia(format!(
                "invalid collection pagination cursor: {value}"
            ))
        }),
    }
}

pub(crate) fn page_info_for_slice(
    offset: usize,
    limit: u16,
    total: usize,
) -> CollectionPageInfo {
    let next_offset = offset.saturating_add(limit as usize);
    CollectionPageInfo {
        next_cursor: (next_offset < total).then(|| next_offset.to_string()),
        limit,
        total: total as u64,
    }
}

pub(crate) fn manual_position_for_index(index: usize) -> Result<u64> {
    let one_based = u64::try_from(index)
        .map_err(|_| {
            MediaError::InvalidMedia(
                "manual collection order index exceeds u64".to_string(),
            )
        })?
        .checked_add(1)
        .ok_or_else(|| {
            MediaError::InvalidMedia(
                "manual collection order index exceeds u64".to_string(),
            )
        })?;
    one_based.checked_mul(1000).ok_or_else(|| {
        MediaError::InvalidMedia(
            "manual collection order key exceeds u64".to_string(),
        )
    })
}

pub(crate) fn manual_position_key_for_index(index: usize) -> Result<String> {
    Ok(manual_position_for_index(index)?.to_string())
}

#[cfg(any(test, feature = "test-utils"))]
pub mod test_support {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::{Arc, Mutex};

    use chrono::Utc;

    use super::*;
    use crate::api::types::collections::{
        CollectionDuplicatePolicy, CollectionIdentity,
        CollectionManualAddResult, CollectionManualAddStatus,
        CollectionMaterializationStatus, CollectionMember,
        CollectionMemberAvailabilityStatus, CollectionPagination,
        CollectionSummary, CollectionTimestamps, CollectionVersion,
        DynamicCollectionRule, SHELF_PLACEMENT_SCHEMA_VERSION, ShelfPlacement,
        ShelfPlacementId,
    };

    #[derive(Debug, Clone)]
    pub struct CollectionDefinitionRequestBuilder {
        request: CreateCollectionRequest,
    }

    impl CollectionDefinitionRequestBuilder {
        pub fn new(title: impl Into<String>) -> Self {
            Self {
                request: CreateCollectionRequest {
                    title: title.into(),
                    description: None,
                    kind: Default::default(),
                    source: Default::default(),
                    owner: Default::default(),
                    scope: Default::default(),
                    visibility: Default::default(),
                    presentation: Default::default(),
                    media_scope: Default::default(),
                    duplicate_policy: Default::default(),
                    artwork: Default::default(),
                    theme: Default::default(),
                    provenance: None,
                    rule: None,
                },
            }
        }

        pub fn description(mut self, description: impl Into<String>) -> Self {
            self.request.description = Some(description.into());
            self
        }

        pub fn rule(mut self, rule: DynamicCollectionRule) -> Self {
            self.request.rule = Some(rule);
            self
        }

        pub fn build(self) -> CreateCollectionRequest {
            self.request
        }
    }

    #[derive(Debug, Default)]
    struct InMemoryState {
        collections: BTreeMap<Uuid, CollectionDetail>,
        items: BTreeMap<Uuid, Vec<CollectionMember>>,
        resolved: HashMap<CollectionMemberKey, CollectionResolvedItem>,
    }

    /// Deterministic collection repository test double for application tests
    /// that need unit-of-work wiring without a Postgres database.
    #[derive(Debug, Default)]
    pub struct InMemoryCollectionRepository {
        state: Mutex<InMemoryState>,
    }

    impl InMemoryCollectionRepository {
        pub fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        pub fn insert_items(
            &self,
            collection_id: CollectionId,
            items: Vec<CollectionMember>,
        ) {
            let mut state = self.state.lock().expect("collection state lock");
            for item in &items {
                state.resolved.insert(
                    item.item_key.clone(),
                    CollectionResolvedItem {
                        item_key: item.item_key.clone(),
                        media_id: item.media_id,
                        title: Some(item.title.clone()),
                        subtitle: item.subtitle.clone(),
                        availability: item.availability.clone(),
                    },
                );
            }
            state.items.insert(collection_id.to_uuid(), items);
        }

        pub fn set_resolved_item(&self, item: CollectionResolvedItem) {
            self.state
                .lock()
                .expect("collection state lock")
                .resolved
                .insert(item.item_key.clone(), item);
        }

        fn visible_items(
            items: &[CollectionMember],
            mode: CollectionReadMode,
            availability: Option<CollectionMemberAvailabilityStatus>,
        ) -> Vec<CollectionMember> {
            items
                .iter()
                .filter(|item| {
                    let status = item.availability.status;
                    if !mode.exposes_preserved_membership()
                        && status
                            != CollectionMemberAvailabilityStatus::Available
                    {
                        return false;
                    }
                    availability.is_none_or(|expected| expected == status)
                })
                .cloned()
                .collect()
        }
    }

    #[async_trait]
    impl CollectionRepository for InMemoryCollectionRepository {
        async fn create_collection(
            &self,
            request: CreateCollectionRequest,
        ) -> Result<CollectionDetail> {
            if request.title.trim().is_empty() {
                return Err(MediaError::InvalidMedia(
                    "collection title must not be empty".to_string(),
                ));
            }

            let id = CollectionId::new();
            let now = Utc::now();
            let summary = CollectionSummary {
                identity: CollectionIdentity::for_id(id),
                title: request.title.trim().to_string(),
                description: request.description,
                kind: request.kind,
                source: request.source,
                owner: request.owner,
                scope: request.scope,
                visibility: request.visibility,
                presentation: request.presentation,
                media_scope: request.media_scope,
                duplicate_policy: request.duplicate_policy,
                artwork: request.artwork,
                theme: request.theme,
                provenance: request.provenance.unwrap_or_default(),
                version: CollectionVersion::default(),
                timestamps: CollectionTimestamps {
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                },
                item_count: 0,
                materialization: CollectionMaterializationStatus::default(),
            };
            let detail = CollectionDetail {
                summary,
                rule: request.rule,
                items_preview: Vec::new(),
                shelf_placements: Vec::new(),
            };
            self.state
                .lock()
                .expect("collection state lock")
                .collections
                .insert(id.to_uuid(), detail.clone());
            Ok(detail)
        }

        async fn update_collection(
            &self,
            id: CollectionId,
            request: UpdateCollectionRequest,
        ) -> Result<CollectionDetail> {
            let mut state = self.state.lock().expect("collection state lock");
            let detail =
                state.collections.get_mut(id.as_uuid()).ok_or_else(|| {
                    MediaError::NotFound(format!("collection {id} not found"))
                })?;
            if let Some(expected) = request.expected_revision
                && expected != detail.summary.version.revision
            {
                return Err(MediaError::Conflict(format!(
                    "collection {id} revision conflict"
                )));
            }
            if let Some(title) = request.title {
                if title.trim().is_empty() {
                    return Err(MediaError::InvalidMedia(
                        "collection title must not be empty".to_string(),
                    ));
                }
                detail.summary.title = title.trim().to_string();
            }
            if let Some(description) = request.description {
                detail.summary.description = Some(description);
            }
            if let Some(visibility) = request.visibility {
                detail.summary.visibility = visibility;
            }
            if let Some(presentation) = request.presentation {
                detail.summary.presentation = presentation;
            }
            if let Some(media_scope) = request.media_scope {
                detail.summary.media_scope = media_scope;
            }
            if let Some(duplicate_policy) = request.duplicate_policy {
                detail.summary.duplicate_policy = duplicate_policy;
            }
            if let Some(artwork) = request.artwork {
                detail.summary.artwork = artwork;
            }
            if let Some(theme) = request.theme {
                detail.summary.theme = theme;
            }
            if let Some(rule) = request.rule {
                detail.rule = Some(rule);
            }
            detail.summary.version.revision += 1;
            detail.summary.version.etag = Some(format!(
                "collection:{}:v{}",
                id, detail.summary.version.revision
            ));
            detail.summary.timestamps.updated_at = Utc::now();
            Ok(detail.clone())
        }

        async fn archive_collection(
            &self,
            id: CollectionId,
            request: ArchiveCollectionRequest,
            _archived_by: Option<Uuid>,
        ) -> Result<ArchiveCollectionResponse> {
            let mut state = self.state.lock().expect("collection state lock");
            let detail =
                state.collections.get_mut(id.as_uuid()).ok_or_else(|| {
                    MediaError::NotFound(format!("collection {id} not found"))
                })?;
            if let Some(expected) = request.expected_revision
                && expected != detail.summary.version.revision
            {
                return Err(MediaError::Conflict(format!(
                    "collection {id} revision conflict"
                )));
            }
            let archived_at = request.archived.then(Utc::now);
            detail.summary.timestamps.archived_at = archived_at;
            detail.summary.version.revision += 1;
            detail.summary.version.etag = Some(format!(
                "collection:{}:v{}",
                id, detail.summary.version.revision
            ));
            Ok(ArchiveCollectionResponse {
                collection_id: id,
                archived_at,
                version: detail.summary.version.clone(),
            })
        }

        async fn get_collection_detail(
            &self,
            id: CollectionId,
            request: GetCollectionDetailRequest,
            mode: CollectionReadMode,
        ) -> Result<Option<CollectionDetail>> {
            let mut detail = match self
                .state
                .lock()
                .expect("collection state lock")
                .collections
                .get(id.as_uuid())
                .cloned()
            {
                Some(detail) => detail,
                None => return Ok(None),
            };
            if !request.include_rule {
                detail.rule = None;
            }
            if request.include_items_preview {
                let response = self
                    .list_collection_items(
                        id,
                        ListCollectionItemsRequest {
                            page: CollectionPagination::default(),
                            availability: None,
                        },
                        mode,
                    )
                    .await?;
                detail.items_preview = response.items;
            } else {
                detail.items_preview.clear();
            }
            if !request.include_shelf_placements {
                detail.shelf_placements.clear();
            }
            Ok(Some(detail))
        }

        async fn list_collections(
            &self,
            request: ListCollectionsRequest,
            mode: CollectionReadMode,
        ) -> Result<ListCollectionsResponse> {
            let offset =
                parse_collection_cursor(request.page.cursor.as_deref())?;
            let limit = clamp_collection_page_limit(request.page.limit);
            let state = self.state.lock().expect("collection state lock");
            let mut collections: Vec<_> = state
                .collections
                .values()
                .filter(|detail| {
                    let _ = mode;
                    (request.include_archived
                        || detail.summary.timestamps.archived_at.is_none())
                        && request
                            .kind
                            .is_none_or(|kind| kind == detail.summary.kind)
                        && request
                            .scope
                            .is_none_or(|scope| scope == detail.summary.scope)
                        && request.visibility.is_none_or(|visibility| {
                            visibility == detail.summary.visibility
                        })
                })
                .map(|detail| detail.summary.clone())
                .collect();
            collections.sort_by(|a, b| {
                b.timestamps
                    .updated_at
                    .cmp(&a.timestamps.updated_at)
                    .then_with(|| a.identity.id.cmp(&b.identity.id))
            });
            let total = collections.len();
            let page = collections
                .into_iter()
                .skip(offset)
                .take(limit as usize)
                .collect();
            Ok(ListCollectionsResponse {
                collections: page,
                page: page_info_for_slice(offset, limit, total),
            })
        }

        async fn list_collection_items(
            &self,
            id: CollectionId,
            request: ListCollectionItemsRequest,
            mode: CollectionReadMode,
        ) -> Result<ListCollectionItemsResponse> {
            let offset =
                parse_collection_cursor(request.page.cursor.as_deref())?;
            let limit = clamp_collection_page_limit(request.page.limit);
            let state = self.state.lock().expect("collection state lock");
            let items =
                state.items.get(id.as_uuid()).cloned().unwrap_or_default();
            let filtered =
                Self::visible_items(&items, mode, request.availability);
            let total = filtered.len();
            Ok(ListCollectionItemsResponse {
                collection_id: id,
                items: filtered
                    .into_iter()
                    .skip(offset)
                    .take(limit as usize)
                    .collect(),
                page: page_info_for_slice(offset, limit, total),
                materialization: CollectionMaterializationStatus::default(),
            })
        }

        async fn manual_add_collection_items(
            &self,
            id: CollectionId,
            request: ManualAddCollectionItemsRequest,
            added_by: Option<Uuid>,
        ) -> Result<ManualAddCollectionItemsResponse> {
            let mut state = self.state.lock().expect("collection state lock");
            let summary = state
                .collections
                .get(id.as_uuid())
                .ok_or_else(|| {
                    MediaError::NotFound(format!("collection {id} not found"))
                })?
                .summary
                .clone();
            if let Some(expected) = request.expected_revision
                && expected != summary.version.revision
            {
                return Err(MediaError::Conflict(format!(
                    "collection {id} revision conflict"
                )));
            }

            let policy =
                request.duplicate_policy.unwrap_or(summary.duplicate_policy);
            let mut items =
                state.items.get(id.as_uuid()).cloned().unwrap_or_default();
            let mut results = Vec::with_capacity(request.items.len());
            let mut changed = false;
            for item in request.items {
                let item_key = CollectionMemberKey::for_media(&item.media_id);
                let exists = items.iter().any(|member| {
                    member.item_key == item_key
                        || member.media_id == item.media_id
                });
                if exists {
                    if policy == CollectionDuplicatePolicy::RejectDuplicates {
                        return Err(MediaError::Conflict(format!(
                            "collection {id} already contains {item_key}"
                        )));
                    }
                    results.push(CollectionManualAddResult {
                        item_key,
                        status: CollectionManualAddStatus::DuplicateSkipped,
                        message: Some(
                            "Item is already present in this collection"
                                .to_string(),
                        ),
                    });
                    continue;
                }
                let position = item.position.unwrap_or_else(|| {
                    items
                        .iter()
                        .map(|member| member.position)
                        .max()
                        .unwrap_or(0)
                        .saturating_add(1)
                });
                let resolved = state.resolved.get(&item_key).cloned();
                let availability = resolved
                    .as_ref()
                    .map(|item| item.availability.clone())
                    .unwrap_or_default();
                let mut member = CollectionMember::new(
                    item.media_id,
                    item.title_override
                        .or_else(|| {
                            resolved
                                .as_ref()
                                .and_then(|item| item.title.clone())
                        })
                        .unwrap_or_else(|| item.media_id.to_string()),
                    position,
                );
                member.subtitle = resolved.and_then(|item| item.subtitle);
                member.availability = availability;
                member.added_at = Some(Utc::now());
                member.added_by = added_by;
                items.push(member);
                results.push(CollectionManualAddResult {
                    item_key,
                    status: CollectionManualAddStatus::Added,
                    message: None,
                });
                changed = true;
            }

            let version = if changed {
                let item_count = items.len() as u32;
                state.items.insert(id.to_uuid(), items);
                let detail = state
                    .collections
                    .get_mut(id.as_uuid())
                    .ok_or_else(|| {
                        MediaError::NotFound(format!(
                            "collection {id} not found"
                        ))
                    })?;
                detail.summary.version.revision += 1;
                detail.summary.version.etag = Some(format!(
                    "collection:{}:v{}",
                    id, detail.summary.version.revision
                ));
                detail.summary.timestamps.updated_at = Utc::now();
                detail.summary.item_count = item_count;
                detail.summary.version.clone()
            } else {
                summary.version
            };
            Ok(ManualAddCollectionItemsResponse {
                collection_id: id,
                results,
                version,
            })
        }

        async fn manual_remove_collection_items(
            &self,
            id: CollectionId,
            request: ManualRemoveCollectionItemsRequest,
        ) -> Result<ManualRemoveCollectionItemsResponse> {
            let mut state = self.state.lock().expect("collection state lock");
            let current_revision = state
                .collections
                .get(id.as_uuid())
                .ok_or_else(|| {
                    MediaError::NotFound(format!("collection {id} not found"))
                })?
                .summary
                .version
                .revision;
            if let Some(expected) = request.expected_revision
                && expected != current_revision
            {
                return Err(MediaError::Conflict(format!(
                    "collection {id} revision conflict"
                )));
            }

            let mut removed_item_keys = Vec::new();
            let mut missing_item_keys = Vec::new();
            let item_count = {
                let items = state.items.entry(id.to_uuid()).or_default();
                for item_key in request.item_keys {
                    let before = items.len();
                    items.retain(|member| member.item_key != item_key);
                    if items.len() < before {
                        removed_item_keys.push(item_key);
                    } else {
                        missing_item_keys.push(item_key);
                    }
                }
                items.len() as u32
            };
            let detail =
                state.collections.get_mut(id.as_uuid()).ok_or_else(|| {
                    MediaError::NotFound(format!("collection {id} not found"))
                })?;
            if !removed_item_keys.is_empty() {
                detail.summary.version.revision += 1;
                detail.summary.version.etag = Some(format!(
                    "collection:{}:v{}",
                    id, detail.summary.version.revision
                ));
                detail.summary.timestamps.updated_at = Utc::now();
                detail.summary.item_count = item_count;
            }
            Ok(ManualRemoveCollectionItemsResponse {
                collection_id: id,
                removed_item_keys,
                missing_item_keys,
                version: detail.summary.version.clone(),
            })
        }

        async fn manual_reorder_collection_items(
            &self,
            id: CollectionId,
            request: ManualReorderCollectionItemsRequest,
        ) -> Result<ManualReorderCollectionItemsResponse> {
            let mut state = self.state.lock().expect("collection state lock");
            let current_revision = state
                .collections
                .get(id.as_uuid())
                .ok_or_else(|| {
                    MediaError::NotFound(format!("collection {id} not found"))
                })?
                .summary
                .version
                .revision;
            if let Some(expected) = request.expected_revision
                && expected != current_revision
            {
                return Err(MediaError::Conflict(format!(
                    "collection {id} revision conflict"
                )));
            }
            let mut changed = false;
            if let Some(items) = state.items.get_mut(id.as_uuid()) {
                for order in request.ordering {
                    if let Some(member) = items
                        .iter_mut()
                        .find(|member| member.item_key == order.item_key)
                        && member.position != order.position
                    {
                        member.position = order.position;
                        changed = true;
                    }
                }
            }
            let detail =
                state.collections.get_mut(id.as_uuid()).ok_or_else(|| {
                    MediaError::NotFound(format!("collection {id} not found"))
                })?;
            if changed {
                detail.summary.version.revision += 1;
                detail.summary.version.etag = Some(format!(
                    "collection:{}:v{}",
                    id, detail.summary.version.revision
                ));
                detail.summary.timestamps.updated_at = Utc::now();
            }
            Ok(ManualReorderCollectionItemsResponse {
                collection_id: id,
                version: detail.summary.version.clone(),
            })
        }

        async fn preview_collection_rule(
            &self,
            _request: PreviewCollectionRuleRequest,
            _mode: CollectionReadMode,
        ) -> Result<PreviewCollectionRuleResponse> {
            Err(MediaError::InvalidMedia(
                "dynamic collection preview requires the Postgres collection repository"
                    .to_string(),
            ))
        }

        async fn refresh_collection_rule(
            &self,
            id: CollectionId,
            _request: RefreshCollectionRuleRequest,
        ) -> Result<RefreshCollectionRuleResponse> {
            Err(MediaError::InvalidMedia(format!(
                "dynamic collection materialization for {id} requires the Postgres collection repository"
            )))
        }

        async fn resolve_collection_items(
            &self,
            items: &[CollectionItemIdentity],
        ) -> Result<Vec<CollectionResolvedItem>> {
            let state = self.state.lock().expect("collection state lock");
            Ok(items
                .iter()
                .map(|item| {
                    state.resolved.get(&item.item_key).cloned().unwrap_or_else(
                        || CollectionResolvedItem {
                            item_key: item.item_key.clone(),
                            media_id: item.media_id,
                            title: None,
                            subtitle: None,
                            availability: CollectionMemberAvailability {
                                status:
                                    CollectionMemberAvailabilityStatus::Missing,
                                reason: Some(
                                    "media reference was not found".to_string(),
                                ),
                                checked_at: Some(Utc::now()),
                            },
                        },
                    )
                })
                .collect())
        }

        async fn ensure_system_collections(
            &self,
            definitions: &[SystemCollectionDefinition],
        ) -> Result<SystemCollectionSeedReport> {
            let mut state = self.state.lock().expect("collection state lock");
            let now = Utc::now();
            for definition in definitions {
                let existing_id =
                    state.collections.values().find_map(|detail| {
                        (detail.summary.identity.stable_key
                            == definition.stable_key)
                            .then_some(detail.summary.identity.id)
                    });
                let id = existing_id.unwrap_or_else(CollectionId::new);
                let placement = ShelfPlacement {
                    schema_version: SHELF_PLACEMENT_SCHEMA_VERSION,
                    id: ShelfPlacementId::new(),
                    collection_id: id,
                    surface: definition.placement.surface,
                    shelf_key: definition.placement.shelf_key.clone(),
                    position: definition.placement.position,
                    pinned: definition.placement.pinned,
                    presentation: definition.presentation(),
                    visibility: definition.visibility(),
                    created_at: now,
                    updated_at: now,
                };
                let summary = CollectionSummary {
                    identity: CollectionIdentity {
                        id,
                        stable_key: definition.stable_key.clone(),
                        external_key: None,
                    },
                    title: definition.title.clone(),
                    description: Some(definition.description()),
                    kind: definition.kind(),
                    source: definition.source(),
                    owner: definition.owner(),
                    scope: definition.scope,
                    visibility: definition.visibility(),
                    presentation: definition.presentation(),
                    media_scope: definition.media_scope.clone(),
                    duplicate_policy: definition.duplicate_policy(),
                    artwork: Default::default(),
                    theme: Default::default(),
                    provenance: definition.provenance(),
                    version: CollectionVersion::default(),
                    timestamps: CollectionTimestamps {
                        created_at: now,
                        updated_at: now,
                        archived_at: None,
                    },
                    item_count: 0,
                    materialization: CollectionMaterializationStatus::default(),
                };
                state.collections.insert(
                    id.to_uuid(),
                    CollectionDetail {
                        summary,
                        rule: Some(definition.rule.clone()),
                        items_preview: Vec::new(),
                        shelf_placements: vec![placement],
                    },
                );
            }

            Ok(SystemCollectionSeedReport {
                requested: definitions.len(),
                upserted: definitions.len(),
            })
        }

        async fn mark_system_collections_stale(
            &self,
            _request: MarkSystemCollectionsStaleRequest,
        ) -> Result<SystemCollectionsStaleResponse> {
            Ok(SystemCollectionsStaleResponse::default())
        }
    }
}
