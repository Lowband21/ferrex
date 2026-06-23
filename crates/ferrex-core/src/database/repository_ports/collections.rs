use async_trait::async_trait;
use ferrex_model::MediaID;
use uuid::Uuid;

use crate::api::types::collections::{
    ArchiveCollectionRequest, ArchiveCollectionResponse, CollectionDetail,
    CollectionDuplicatePolicy, CollectionId,
    CollectionManualMembershipConflict, CollectionManualMembershipConflictCode,
    CollectionMemberAvailability, CollectionMemberKey, CollectionPageInfo,
    CreateCollectionRequest, GetCollectionDetailRequest,
    ListCollectionItemsRequest, ListCollectionItemsResponse,
    ListCollectionsRequest, ListCollectionsResponse, MAX_COLLECTION_PAGE_LIMIT,
    ManualAddCollectionItemsRequest, ManualAddCollectionItemsResponse,
    ManualRemoveCollectionItemsRequest, ManualRemoveCollectionItemsResponse,
    ManualReorderCollectionItemsRequest, ManualReorderCollectionItemsResponse,
    UpdateCollectionRequest,
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

    async fn resolve_collection_items(
        &self,
        items: &[CollectionItemIdentity],
    ) -> Result<Vec<CollectionResolvedItem>>;
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

pub(crate) fn collection_manual_membership_conflict(
    code: CollectionManualMembershipConflictCode,
    collection_id: CollectionId,
    duplicate_policy: Option<CollectionDuplicatePolicy>,
    item_keys: Vec<CollectionMemberKey>,
    message: impl Into<String>,
) -> MediaError {
    let conflict = CollectionManualMembershipConflict {
        code,
        collection_id,
        duplicate_policy,
        item_keys,
        message: message.into(),
    };
    let message = serde_json::to_string(&conflict)
        .unwrap_or_else(|_| conflict.message.clone());
    MediaError::Conflict(message)
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
    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    use chrono::Utc;

    use super::*;
    use crate::api::types::collections::{
        CollectionIdentity, CollectionKind, CollectionManualAddItem,
        CollectionManualAddResult, CollectionManualAddStatus,
        CollectionManualOrder, CollectionMaterializationStatus,
        CollectionMediaKind, CollectionMediaScope, CollectionMember,
        CollectionMemberAvailabilityStatus, CollectionPagination,
        CollectionSummary, CollectionTimestamps, CollectionVersion,
        DynamicCollectionRule,
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

        pub fn media_scope(
            mut self,
            media_scope: CollectionMediaScope,
        ) -> Self {
            self.request.media_scope = media_scope;
            self
        }

        pub fn duplicate_policy(
            mut self,
            duplicate_policy: CollectionDuplicatePolicy,
        ) -> Self {
            self.request.duplicate_policy = duplicate_policy;
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

        fn current_version(detail: &CollectionDetail) -> CollectionVersion {
            detail.summary.version.clone()
        }

        fn bump_collection_version(
            detail: &mut CollectionDetail,
        ) -> CollectionVersion {
            detail.summary.version.revision += 1;
            detail.summary.version.etag = Some(format!(
                "collection:{}:v{}",
                detail.summary.identity.id, detail.summary.version.revision
            ));
            detail.summary.timestamps.updated_at = Utc::now();
            detail.summary.version.clone()
        }

        fn ensure_manual_editable(
            detail: &CollectionDetail,
            expected_revision: Option<u64>,
        ) -> Result<()> {
            if detail.summary.kind != CollectionKind::Manual {
                return Err(MediaError::InvalidMedia(format!(
                    "collection {} is not a manual collection",
                    detail.summary.identity.id
                )));
            }
            if detail.summary.timestamps.archived_at.is_some() {
                return Err(MediaError::Conflict(format!(
                    "collection {} is archived",
                    detail.summary.identity.id
                )));
            }
            if let Some(expected) = expected_revision
                && expected != detail.summary.version.revision
            {
                return Err(MediaError::Conflict(format!(
                    "collection {} revision conflict",
                    detail.summary.identity.id
                )));
            }
            Ok(())
        }

        fn missing_resolved_item(media_id: MediaID) -> CollectionResolvedItem {
            CollectionResolvedItem {
                item_key: CollectionMemberKey::for_media(&media_id),
                media_id,
                title: None,
                subtitle: None,
                availability: CollectionMemberAvailability {
                    status: CollectionMemberAvailabilityStatus::Missing,
                    reason: Some("media reference was not found".to_string()),
                    checked_at: Some(Utc::now()),
                },
            }
        }

        fn title_for_add(
            item: &crate::api::types::collections::CollectionManualAddItem,
            resolved: &CollectionResolvedItem,
        ) -> String {
            item.title_override
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| resolved.title.clone())
                .unwrap_or_else(|| item.media_id.to_string())
        }

        fn final_order_with_insertions(
            existing: &[CollectionMemberKey],
            insertions: &[(CollectionMemberKey, Option<u32>, usize)],
        ) -> Vec<CollectionMemberKey> {
            let mut order = existing.to_vec();
            let mut positioned: Vec<_> = insertions
                .iter()
                .filter_map(|(key, position, index)| {
                    position.map(|position| (key.clone(), position, *index))
                })
                .collect();
            positioned.sort_by_key(|(_, position, index)| (*position, *index));
            for (key, position, _) in positioned {
                let index = (position as usize).min(order.len());
                order.insert(index, key);
            }
            for (key, position, _) in insertions {
                if position.is_none() {
                    order.push(key.clone());
                }
            }
            order
        }

        fn final_order_with_reorder(
            collection_id: CollectionId,
            existing: &[CollectionMemberKey],
            ordering: &[CollectionManualOrder],
        ) -> Result<Vec<CollectionMemberKey>> {
            let mut seen_keys = HashSet::new();
            let mut seen_positions = HashSet::new();
            for order in ordering {
                if !seen_keys.insert(order.item_key.clone()) {
                    return Err(MediaError::InvalidMedia(
                        "manual reorder contains duplicate item keys"
                            .to_string(),
                    ));
                }
                if !seen_positions.insert(order.position) {
                    return Err(MediaError::InvalidMedia(
                        "manual reorder contains duplicate positions"
                            .to_string(),
                    ));
                }
            }

            let existing_set: HashSet<_> = existing.iter().cloned().collect();
            let missing: Vec<_> = ordering
                .iter()
                .filter(|order| !existing_set.contains(&order.item_key))
                .map(|order| order.item_key.clone())
                .collect();
            if !missing.is_empty() {
                return Err(collection_manual_membership_conflict(
                    CollectionManualMembershipConflictCode::MissingMember,
                    collection_id,
                    None,
                    missing,
                    "manual reorder references members that are not in the collection",
                ));
            }

            let requested: HashSet<_> = ordering
                .iter()
                .map(|order| order.item_key.clone())
                .collect();
            let mut order: Vec<_> = existing
                .iter()
                .filter(|key| !requested.contains(*key))
                .cloned()
                .collect();
            let mut placements = ordering.to_vec();
            placements.sort_by_key(|order| order.position);
            for placement in placements {
                let index = (placement.position as usize).min(order.len());
                order.insert(index, placement.item_key);
            }
            Ok(order)
        }

        fn apply_order(
            items: &mut [CollectionMember],
            order: &[CollectionMemberKey],
        ) -> Result<()> {
            let positions: HashMap<_, _> = order
                .iter()
                .enumerate()
                .map(|(index, key)| {
                    Ok((key.clone(), manual_position_for_index(index)?))
                })
                .collect::<Result<HashMap<_, _>>>()?;
            for item in items {
                if let Some(position) = positions.get(&item.item_key) {
                    item.position = u32::try_from(*position).map_err(|_| {
                        MediaError::InvalidMedia(
                            "manual collection order key exceeds u32"
                                .to_string(),
                        )
                    })?;
                }
            }
            Ok(())
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
            let mut items =
                state.items.get(id.as_uuid()).cloned().unwrap_or_default();
            for item in &mut items {
                let resolved =
                    state.resolved.get(&item.item_key).cloned().unwrap_or_else(
                        || Self::missing_resolved_item(item.media_id),
                    );
                if let Some(title) = resolved.title {
                    item.title = title;
                }
                item.subtitle = resolved.subtitle;
                item.availability = resolved.availability;
            }
            items.sort_by(|a, b| {
                a.position
                    .cmp(&b.position)
                    .then_with(|| a.item_key.cmp(&b.item_key))
            });
            let mut filtered =
                Self::visible_items(&items, mode, request.availability);
            for (index, item) in filtered.iter_mut().enumerate() {
                item.position = u32::try_from(index).map_err(|_| {
                    MediaError::InvalidMedia(
                        "collection item position exceeds u32".to_string(),
                    )
                })?;
            }
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
            let collection_uuid = id.to_uuid();
            let detail =
                state.collections.get(&collection_uuid).ok_or_else(|| {
                    MediaError::NotFound(format!("collection {id} not found"))
                })?;
            Self::ensure_manual_editable(detail, request.expected_revision)?;
            let media_scope = detail.summary.media_scope.clone();
            let duplicate_policy = request
                .duplicate_policy
                .unwrap_or(detail.summary.duplicate_policy);
            let current_version = Self::current_version(detail);
            let resolved_items = state.resolved.clone();
            let existing_items = state
                .items
                .get(&collection_uuid)
                .cloned()
                .unwrap_or_default();
            let existing_keys: HashSet<_> = existing_items
                .iter()
                .map(|item| item.item_key.clone())
                .collect();
            let existing_order: Vec<_> = existing_items
                .iter()
                .map(|item| item.item_key.clone())
                .collect();
            let mut seen_input = HashSet::new();
            let mut duplicate_keys = Vec::new();
            let mut result_slots = vec![None; request.items.len()];
            let mut candidates: Vec<(
                usize,
                CollectionManualAddItem,
                CollectionMemberKey,
            )> = Vec::new();

            for (index, item) in request.items.iter().enumerate() {
                let item_key = CollectionMemberKey::for_media(&item.media_id);
                if !media_scope.allows_media(&item.media_id) {
                    return Err(MediaError::InvalidMedia(format!(
                        "collection {id} media scope does not allow {item_key}"
                    )));
                }

                let duplicate = existing_keys.contains(&item_key)
                    || !seen_input.insert(item_key.clone());
                if duplicate {
                    duplicate_keys.push(item_key.clone());
                    if matches!(
                        duplicate_policy,
                        CollectionDuplicatePolicy::DeduplicateMedia
                            | CollectionDuplicatePolicy::DeduplicateLogical
                    ) {
                        let status = if existing_keys.contains(&item_key) {
                            CollectionManualAddStatus::AlreadyPresent
                        } else {
                            CollectionManualAddStatus::DuplicateSkipped
                        };
                        result_slots[index] = Some(CollectionManualAddResult {
                            item_key,
                            status,
                            message: Some(
                                "manual collection already contains this item"
                                    .to_string(),
                            ),
                        });
                    }
                    continue;
                }

                candidates.push((index, item.clone(), item_key));
            }

            if !duplicate_keys.is_empty()
                && !matches!(
                    duplicate_policy,
                    CollectionDuplicatePolicy::DeduplicateMedia
                        | CollectionDuplicatePolicy::DeduplicateLogical
                )
            {
                duplicate_keys.sort();
                duplicate_keys.dedup();
                let code = if duplicate_policy
                    == CollectionDuplicatePolicy::KeepAll
                {
                    CollectionManualMembershipConflictCode::UnsupportedDuplicatePolicy
                } else {
                    CollectionManualMembershipConflictCode::DuplicateMember
                };
                return Err(collection_manual_membership_conflict(
                    code,
                    id,
                    Some(duplicate_policy),
                    duplicate_keys,
                    "manual collection already contains one or more requested items",
                ));
            }

            let has_positioned = candidates
                .iter()
                .any(|(_, item, _)| item.position.is_some());
            let final_order = if has_positioned {
                let insertions: Vec<_> = candidates
                    .iter()
                    .map(|(index, item, key)| {
                        (key.clone(), item.position, *index)
                    })
                    .collect();
                Some(Self::final_order_with_insertions(
                    &existing_order,
                    &insertions,
                ))
            } else {
                None
            };

            let items = state.items.entry(collection_uuid).or_default();
            let mut next_position = items
                .iter()
                .map(|item| u64::from(item.position))
                .max()
                .unwrap_or(0)
                .saturating_add(1000);
            for (candidate_index, item, item_key) in candidates {
                let resolved =
                    resolved_items.get(&item_key).cloned().unwrap_or_else(
                        || Self::missing_resolved_item(item.media_id),
                    );
                let position = if let Some(order) = final_order.as_ref() {
                    let order_index = order
                        .iter()
                        .position(|key| key == &item_key)
                        .ok_or_else(|| {
                            MediaError::Internal(
                                "new manual member was missing from final order"
                                    .to_string(),
                            )
                        })?;
                    manual_position_for_index(order_index)?
                } else {
                    let position = next_position;
                    next_position = next_position.saturating_add(1000);
                    position
                };
                let position = u32::try_from(position).map_err(|_| {
                    MediaError::InvalidMedia(
                        "manual collection order key exceeds u32".to_string(),
                    )
                })?;
                items.push(CollectionMember {
                    item_key: item_key.clone(),
                    media_id: item.media_id,
                    media_type: CollectionMediaKind::from(&item.media_id),
                    title: Self::title_for_add(&item, &resolved),
                    subtitle: resolved.subtitle,
                    position,
                    sort_key: Some(item_key.to_string()),
                    availability: resolved.availability,
                    added_at: Some(Utc::now()),
                    added_by,
                });
                result_slots[candidate_index] =
                    Some(CollectionManualAddResult {
                        item_key,
                        status: CollectionManualAddStatus::Added,
                        message: None,
                    });
            }

            if let Some(order) = final_order.as_ref() {
                Self::apply_order(items, order)?;
            }
            items.sort_by(|a, b| {
                a.position
                    .cmp(&b.position)
                    .then_with(|| a.item_key.cmp(&b.item_key))
            });
            let item_count = u32::try_from(items.len()).map_err(|_| {
                MediaError::InvalidMedia(
                    "collection item count exceeds u32".to_string(),
                )
            })?;
            let changed = result_slots.iter().any(|result| {
                matches!(
                    result,
                    Some(CollectionManualAddResult {
                        status: CollectionManualAddStatus::Added,
                        ..
                    })
                )
            });
            let version = if changed {
                let detail = state
                    .collections
                    .get_mut(&collection_uuid)
                    .ok_or_else(|| {
                        MediaError::NotFound(format!(
                            "collection {id} not found"
                        ))
                    })?;
                detail.summary.item_count = item_count;
                Self::bump_collection_version(detail)
            } else {
                current_version
            };

            Ok(ManualAddCollectionItemsResponse {
                collection_id: id,
                results: result_slots
                    .into_iter()
                    .map(|result| {
                        result.ok_or_else(|| {
                            MediaError::Internal(
                                "manual add result was not recorded"
                                    .to_string(),
                            )
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
                version,
            })
        }

        async fn manual_remove_collection_items(
            &self,
            id: CollectionId,
            request: ManualRemoveCollectionItemsRequest,
        ) -> Result<ManualRemoveCollectionItemsResponse> {
            let mut state = self.state.lock().expect("collection state lock");
            let collection_uuid = id.to_uuid();
            let detail =
                state.collections.get(&collection_uuid).ok_or_else(|| {
                    MediaError::NotFound(format!("collection {id} not found"))
                })?;
            Self::ensure_manual_editable(detail, request.expected_revision)?;
            let current_version = Self::current_version(detail);
            let requested: HashSet<_> =
                request.item_keys.iter().cloned().collect();
            let items = state.items.entry(collection_uuid).or_default();
            let before = items.len();
            let mut removed_item_keys = Vec::new();
            items.retain(|item| {
                if requested.contains(&item.item_key) {
                    removed_item_keys.push(item.item_key.clone());
                    false
                } else {
                    true
                }
            });
            removed_item_keys.sort();
            let removed_set: HashSet<_> =
                removed_item_keys.iter().cloned().collect();
            let mut missing_item_keys: Vec<_> = request
                .item_keys
                .into_iter()
                .filter(|key| !removed_set.contains(key))
                .collect();
            missing_item_keys.sort();
            missing_item_keys.dedup();
            let item_count = u32::try_from(items.len()).map_err(|_| {
                MediaError::InvalidMedia(
                    "collection item count exceeds u32".to_string(),
                )
            })?;
            let changed = before != items.len();
            let version = if changed {
                let detail = state
                    .collections
                    .get_mut(&collection_uuid)
                    .ok_or_else(|| {
                        MediaError::NotFound(format!(
                            "collection {id} not found"
                        ))
                    })?;
                detail.summary.item_count = item_count;
                Self::bump_collection_version(detail)
            } else {
                current_version
            };
            Ok(ManualRemoveCollectionItemsResponse {
                collection_id: id,
                removed_item_keys,
                missing_item_keys,
                version,
            })
        }

        async fn manual_reorder_collection_items(
            &self,
            id: CollectionId,
            request: ManualReorderCollectionItemsRequest,
        ) -> Result<ManualReorderCollectionItemsResponse> {
            let mut state = self.state.lock().expect("collection state lock");
            let collection_uuid = id.to_uuid();
            let detail =
                state.collections.get(&collection_uuid).ok_or_else(|| {
                    MediaError::NotFound(format!("collection {id} not found"))
                })?;
            Self::ensure_manual_editable(detail, request.expected_revision)?;
            let current_version = Self::current_version(detail);
            if request.ordering.is_empty() {
                return Ok(ManualReorderCollectionItemsResponse {
                    collection_id: id,
                    version: current_version,
                });
            }
            let items = state.items.entry(collection_uuid).or_default();
            items.sort_by(|a, b| {
                a.position
                    .cmp(&b.position)
                    .then_with(|| a.item_key.cmp(&b.item_key))
            });
            let existing_order: Vec<_> =
                items.iter().map(|item| item.item_key.clone()).collect();
            let final_order = Self::final_order_with_reorder(
                id,
                &existing_order,
                &request.ordering,
            )?;
            let changed = final_order != existing_order;
            if changed {
                Self::apply_order(items, &final_order)?;
                items.sort_by(|a, b| {
                    a.position
                        .cmp(&b.position)
                        .then_with(|| a.item_key.cmp(&b.item_key))
                });
            }
            let version = if changed {
                let detail = state
                    .collections
                    .get_mut(&collection_uuid)
                    .ok_or_else(|| {
                        MediaError::NotFound(format!(
                            "collection {id} not found"
                        ))
                    })?;
                Self::bump_collection_version(detail)
            } else {
                current_version
            };
            Ok(ManualReorderCollectionItemsResponse {
                collection_id: id,
                version,
            })
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
    }
}

#[cfg(test)]
mod tests {
    use ferrex_model::{EpisodeID, MediaID, MovieID, SeriesID};
    use uuid::Uuid;

    use super::test_support::{
        CollectionDefinitionRequestBuilder, InMemoryCollectionRepository,
    };
    use super::*;
    use crate::api::types::collections::{
        CollectionManualMembershipConflict, CollectionManualOrder,
        CollectionMediaKind, CollectionMediaScope,
        CollectionMemberAvailability, CollectionMemberAvailabilityStatus,
        ManualAddCollectionItemsRequest, ManualRemoveCollectionItemsRequest,
        ManualReorderCollectionItemsRequest,
    };

    fn movie_id(suffix: u128) -> MediaID {
        MediaID::Movie(MovieID(Uuid::from_u128(
            0x10000000000070008000000000000000 + suffix,
        )))
    }

    fn episode_id(suffix: u128) -> MediaID {
        MediaID::Episode(EpisodeID(Uuid::from_u128(
            0x20000000000070008000000000000000 + suffix,
        )))
    }

    fn series_id(suffix: u128) -> MediaID {
        MediaID::Series(SeriesID(Uuid::from_u128(
            0x30000000000070008000000000000000 + suffix,
        )))
    }

    fn resolved(media_id: MediaID, title: &str) -> CollectionResolvedItem {
        CollectionResolvedItem {
            item_key: CollectionMemberKey::for_media(&media_id),
            media_id,
            title: Some(title.to_string()),
            subtitle: None,
            availability: CollectionMemberAvailability::default(),
        }
    }

    fn tombstoned(media_id: MediaID, title: &str) -> CollectionResolvedItem {
        CollectionResolvedItem {
            item_key: CollectionMemberKey::for_media(&media_id),
            media_id,
            title: Some(title.to_string()),
            subtitle: None,
            availability: CollectionMemberAvailability {
                status: CollectionMemberAvailabilityStatus::Tombstoned,
                reason: Some("fixture tombstone".to_string()),
                checked_at: None,
            },
        }
    }

    fn duplicate_conflict_code(
        err: MediaError,
    ) -> CollectionManualMembershipConflictCode {
        let MediaError::Conflict(message) = err else {
            panic!("expected conflict error");
        };
        let conflict: CollectionManualMembershipConflict =
            serde_json::from_str(&message)
                .expect("structured conflict payload");
        conflict.code
    }

    #[tokio::test]
    async fn manual_membership_add_remove_and_reorder_items() -> Result<()> {
        let repo = InMemoryCollectionRepository::new();
        let collection = repo
            .create_collection(
                CollectionDefinitionRequestBuilder::new("Manual picks").build(),
            )
            .await?;
        let first = movie_id(1);
        let second = episode_id(2);
        repo.set_resolved_item(resolved(first, "First movie"));
        repo.set_resolved_item(resolved(second, "Second episode"));

        let added = repo
            .manual_add_collection_items(
                collection.summary.identity.id,
                ManualAddCollectionItemsRequest {
                    items: vec![
                        crate::api::types::collections::CollectionManualAddItem {
                            media_id: first,
                            title_override: None,
                            position: None,
                        },
                        crate::api::types::collections::CollectionManualAddItem {
                            media_id: second,
                            title_override: None,
                            position: None,
                        },
                    ],
                    duplicate_policy: None,
                    expected_revision: Some(0),
                },
                None,
            )
            .await?;
        assert_eq!(added.version.revision, 1);
        assert_eq!(added.results.len(), 2);

        repo.manual_reorder_collection_items(
            collection.summary.identity.id,
            ManualReorderCollectionItemsRequest {
                ordering: vec![CollectionManualOrder {
                    item_key: CollectionMemberKey::for_media(&second),
                    position: 0,
                }],
                expected_revision: Some(1),
            },
        )
        .await?;

        let reordered = repo
            .list_collection_items(
                collection.summary.identity.id,
                ListCollectionItemsRequest::default(),
                CollectionReadMode::Edit,
            )
            .await?;
        assert_eq!(reordered.items.len(), 2);
        assert_eq!(reordered.items[0].media_id, second);
        assert_eq!(reordered.items[1].media_id, first);

        let removed = repo
            .manual_remove_collection_items(
                collection.summary.identity.id,
                ManualRemoveCollectionItemsRequest {
                    item_keys: vec![CollectionMemberKey::for_media(&second)],
                    expected_revision: Some(2),
                },
            )
            .await?;
        assert_eq!(removed.version.revision, 3);
        assert_eq!(removed.removed_item_keys.len(), 1);
        let remaining = repo
            .list_collection_items(
                collection.summary.identity.id,
                ListCollectionItemsRequest::default(),
                CollectionReadMode::Edit,
            )
            .await?;
        assert_eq!(remaining.items.len(), 1);
        assert_eq!(remaining.items[0].media_id, first);

        Ok(())
    }

    #[tokio::test]
    async fn manual_membership_validates_scope_duplicates_and_stale_versions()
    -> Result<()> {
        let repo = InMemoryCollectionRepository::new();
        let collection = repo
            .create_collection(
                CollectionDefinitionRequestBuilder::new("Mixed scope")
                    .media_scope(CollectionMediaScope::Types {
                        media_types: vec![
                            CollectionMediaKind::Movie,
                            CollectionMediaKind::Episode,
                        ],
                    })
                    .build(),
            )
            .await?;
        let movie = movie_id(10);
        let episode = episode_id(11);
        let series = series_id(12);
        repo.set_resolved_item(resolved(movie, "Allowed movie"));
        repo.set_resolved_item(resolved(episode, "Allowed episode"));

        repo.manual_add_collection_items(
            collection.summary.identity.id,
            ManualAddCollectionItemsRequest {
                items: vec![
                    crate::api::types::collections::CollectionManualAddItem {
                        media_id: movie,
                        title_override: None,
                        position: None,
                    },
                ],
                duplicate_policy: None,
                expected_revision: Some(0),
            },
            None,
        )
        .await?;

        let scope_error = repo
            .manual_add_collection_items(
                collection.summary.identity.id,
                ManualAddCollectionItemsRequest {
                    items: vec![crate::api::types::collections::CollectionManualAddItem {
                        media_id: series,
                        title_override: None,
                        position: None,
                    }],
                    duplicate_policy: None,
                    expected_revision: Some(1),
                },
                None,
            )
            .await
            .expect_err("series is outside the collection media scope");
        assert!(matches!(scope_error, MediaError::InvalidMedia(_)));

        let duplicate = repo
            .manual_add_collection_items(
                collection.summary.identity.id,
                ManualAddCollectionItemsRequest {
                    items: vec![crate::api::types::collections::CollectionManualAddItem {
                        media_id: movie,
                        title_override: None,
                        position: None,
                    }],
                    duplicate_policy: None,
                    expected_revision: Some(1),
                },
                None,
            )
            .await
            .expect_err("duplicates reject by default");
        assert_eq!(
            duplicate_conflict_code(duplicate),
            CollectionManualMembershipConflictCode::DuplicateMember
        );

        let stale = repo
            .manual_add_collection_items(
                collection.summary.identity.id,
                ManualAddCollectionItemsRequest {
                    items: vec![crate::api::types::collections::CollectionManualAddItem {
                        media_id: episode,
                        title_override: None,
                        position: None,
                    }],
                    duplicate_policy: None,
                    expected_revision: Some(0),
                },
                None,
            )
            .await
            .expect_err("stale collection revision should conflict");
        assert!(matches!(stale, MediaError::Conflict(_)));

        Ok(())
    }

    #[tokio::test]
    async fn manual_membership_preserves_unavailable_items_for_edit_reads()
    -> Result<()> {
        let repo = InMemoryCollectionRepository::new();
        let collection = repo
            .create_collection(
                CollectionDefinitionRequestBuilder::new("Unavailable picks")
                    .build(),
            )
            .await?;
        let unavailable = movie_id(20);
        repo.set_resolved_item(tombstoned(unavailable, "Tombstoned movie"));

        repo.manual_add_collection_items(
            collection.summary.identity.id,
            ManualAddCollectionItemsRequest {
                items: vec![
                    crate::api::types::collections::CollectionManualAddItem {
                        media_id: unavailable,
                        title_override: None,
                        position: None,
                    },
                ],
                duplicate_policy: None,
                expected_revision: Some(0),
            },
            None,
        )
        .await?;

        let normal = repo
            .list_collection_items(
                collection.summary.identity.id,
                ListCollectionItemsRequest::default(),
                CollectionReadMode::Normal,
            )
            .await?;
        assert!(normal.items.is_empty());

        let edit = repo
            .list_collection_items(
                collection.summary.identity.id,
                ListCollectionItemsRequest::default(),
                CollectionReadMode::Edit,
            )
            .await?;
        assert_eq!(edit.items.len(), 1);
        assert_eq!(edit.items[0].media_id, unavailable);
        assert_eq!(
            edit.items[0].availability.status,
            CollectionMemberAvailabilityStatus::Tombstoned
        );

        Ok(())
    }
}
