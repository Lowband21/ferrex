use async_trait::async_trait;
use ferrex_model::MediaID;
use uuid::Uuid;

use crate::api::types::collections::{
    ArchiveCollectionRequest, ArchiveCollectionResponse, CollectionDetail,
    CollectionId, CollectionMemberAvailability, CollectionMemberKey,
    CollectionPageInfo, CreateCollectionRequest, GetCollectionDetailRequest,
    ListCollectionItemsRequest, ListCollectionItemsResponse,
    ListCollectionsRequest, ListCollectionsResponse, MAX_COLLECTION_PAGE_LIMIT,
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

#[cfg(any(test, feature = "test-utils"))]
pub mod test_support {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::{Arc, Mutex};

    use chrono::Utc;

    use super::*;
    use crate::api::types::collections::{
        CollectionIdentity, CollectionMaterializationStatus, CollectionMember,
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
