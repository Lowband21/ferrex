use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ferrex_model::{EpisodeID, MediaID, MovieID, SeasonID, SeriesID};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sqlx::{PgPool, Row, postgres::PgRow};
use uuid::Uuid;

use super::collection_rule_evaluator::{
    DynamicCollectionEvaluation, DynamicCollectionEvaluator,
    page_info_for_materialized_slice,
};
use crate::api::types::collections::{
    ArchiveCollectionRequest, ArchiveCollectionResponse,
    COLLECTION_CONTRACT_VERSION, COLLECTION_MATERIALIZATION_SCHEMA_VERSION,
    CollectionArtwork, CollectionDetail, CollectionDuplicatePolicy,
    CollectionId, CollectionIdentity, CollectionKind,
    CollectionManualAddResult, CollectionManualAddStatus,
    CollectionMaterializationState, CollectionMaterializationStatus,
    CollectionMediaKind, CollectionMediaScope, CollectionMember,
    CollectionMemberAvailability, CollectionMemberAvailabilityStatus,
    CollectionMemberKey, CollectionOwner, CollectionOwnerType,
    CollectionPageInfo, CollectionPresentationMode, CollectionProvenance,
    CollectionScope, CollectionSource, CollectionSummary, CollectionTheme,
    CollectionTimestamps, CollectionVersion, CreateCollectionRequest,
    DeleteCollectionRequest, DeleteCollectionResponse, DynamicCollectionRule,
    GetCollectionDetailRequest, ListCollectionItemsRequest,
    ListCollectionItemsResponse, ListCollectionsRequest,
    ListCollectionsResponse, ListShelfPlacementsRequest,
    ListShelfPlacementsResponse, ManualAddCollectionItemsRequest,
    ManualAddCollectionItemsResponse, ManualRemoveCollectionItemsRequest,
    ManualRemoveCollectionItemsResponse, ManualReorderCollectionItemsRequest,
    ManualReorderCollectionItemsResponse, PinShelfPlacementRequest,
    PinShelfPlacementResponse, PreviewCollectionRuleRequest,
    PreviewCollectionRuleResponse, RefreshCollectionRuleRequest,
    RefreshCollectionRuleResponse, ReorderShelfPlacementsRequest,
    ReorderShelfPlacementsResponse, ShelfPlacement, ShelfPlacementId,
    ShelfSurface, TmdbCollectionImportKind, TmdbCollectionSummary,
    TmdbImportCollectionRequest, TmdbImportCollectionResponse,
    TmdbListCollectionsRequest, TmdbListCollectionsResponse,
    UpdateCollectionRequest, ValidateCollectionRuleRequest,
    ValidateCollectionRuleResponse,
};
use crate::api::types::system_collections::{
    MarkSystemCollectionsStaleRequest, SystemCollectionDefinition,
    SystemCollectionSeedReport, SystemCollectionsStaleResponse,
};
use crate::database::repository_ports::collections::{
    CollectionItemIdentity, CollectionReadMode, CollectionRepository,
    CollectionResolvedItem, clamp_collection_page_limit,
    manual_position_key_for_index, page_info_for_slice,
    parse_collection_cursor,
};
use crate::error::{MediaError, Result};

#[derive(Clone, Debug)]
pub struct PostgresCollectionRepository {
    pool: PgPool,
}

#[derive(Debug)]
struct CollectionDefinitionRow {
    id: Uuid,
    stable_key: String,
    external_key: Option<String>,
    title: String,
    description: Option<String>,
    kind: String,
    source: String,
    owner_type: String,
    owner_user_id: Option<Uuid>,
    owner_device_id: Option<String>,
    owner_display_name: Option<String>,
    scope: String,
    visibility: String,
    presentation: String,
    media_scope: Value,
    duplicate_policy: String,
    artwork: Value,
    theme: Value,
    provenance: Value,
    contract_version: i32,
    revision: i64,
    etag: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
    item_count: i64,
    materialization_state: Option<String>,
    materialization_total_count: Option<i32>,
    materialization_visible_count: Option<i32>,
    materialization_rule_hash: Option<String>,
    materialization_generated_at: Option<DateTime<Utc>>,
    materialization_expires_at: Option<DateTime<Utc>>,
    materialization_last_error: Option<String>,
}

#[derive(Debug)]
struct CollectionListRow {
    id: Uuid,
    stable_key: String,
    external_key: Option<String>,
    title: String,
    description: Option<String>,
    kind: String,
    source: String,
    owner_type: String,
    owner_user_id: Option<Uuid>,
    owner_device_id: Option<String>,
    owner_display_name: Option<String>,
    scope: String,
    visibility: String,
    presentation: String,
    media_scope: Value,
    duplicate_policy: String,
    artwork: Value,
    theme: Value,
    provenance: Value,
    contract_version: i32,
    revision: i64,
    etag: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
    item_count: i64,
    materialization_state: Option<String>,
    materialization_total_count: Option<i32>,
    materialization_visible_count: Option<i32>,
    materialization_rule_hash: Option<String>,
    materialization_generated_at: Option<DateTime<Utc>>,
    materialization_expires_at: Option<DateTime<Utc>>,
    materialization_last_error: Option<String>,
    total: i64,
}

#[derive(Debug)]
struct ManualMembershipRow {
    item_key: String,
    media_type: String,
    media_id: Uuid,
    title_snapshot: Option<String>,
    subtitle_snapshot: Option<String>,
    position: i32,
    sort_key: Option<String>,
    added_at: DateTime<Utc>,
    added_by: Option<Uuid>,
}

#[derive(Debug)]
struct MaterializedMembershipRow {
    item_key: String,
    media_type: String,
    media_id: Uuid,
    position: i32,
    order_key: String,
    visible: bool,
    hidden_reason: Option<String>,
    evaluated_at: DateTime<Utc>,
}

#[derive(Debug)]
struct MaterializationStatusRow {
    id: Uuid,
    state: String,
    rule_hash: String,
    evaluated_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    error_message: Option<String>,
    total_count: i32,
    visible_count: i32,
}

#[derive(Debug)]
struct ResolvedItemRow {
    id: Uuid,
    title: Option<String>,
    subtitle: Option<String>,
    status: String,
    reason: Option<String>,
}

impl PostgresCollectionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn etag_for(id: CollectionId, revision: u64) -> String {
        format!("collection:{id}:v{revision}")
    }

    fn version_from_parts(
        contract_version: i32,
        revision: i64,
        etag: Option<String>,
    ) -> Result<CollectionVersion> {
        let contract_version =
            u16::try_from(contract_version).map_err(|_| {
                MediaError::Internal(
                    "collection contract version exceeds u16".to_string(),
                )
            })?;
        let revision = u64::try_from(revision).map_err(|_| {
            MediaError::Internal("collection revision is negative".to_string())
        })?;
        Ok(CollectionVersion {
            contract_version,
            revision,
            etag,
        })
    }

    fn expected_revision_i64(
        expected_revision: Option<u64>,
    ) -> Result<Option<i64>> {
        expected_revision
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                MediaError::InvalidMedia(
                    "expected collection revision exceeds i64".to_string(),
                )
            })
    }

    fn to_json<T: serde::Serialize>(value: &T, label: &str) -> Result<Value> {
        serde_json::to_value(value).map_err(|e| {
            MediaError::Internal(format!("failed to encode {label}: {e}"))
        })
    }

    fn from_json<T: DeserializeOwned>(value: Value, label: &str) -> Result<T> {
        serde_json::from_value(value).map_err(|e| {
            MediaError::Internal(format!("invalid {label} payload: {e}"))
        })
    }

    fn encode_kind(value: CollectionKind) -> &'static str {
        match value {
            CollectionKind::Manual => "manual",
            CollectionKind::DynamicRule => "dynamic_rule",
            CollectionKind::TmdbList => "tmdb_list",
            CollectionKind::TmdbCollection => "tmdb_collection",
            CollectionKind::System => "system",
        }
    }

    fn decode_kind(value: &str) -> Result<CollectionKind> {
        match value {
            "manual" => Ok(CollectionKind::Manual),
            "dynamic_rule" => Ok(CollectionKind::DynamicRule),
            "tmdb_list" => Ok(CollectionKind::TmdbList),
            "tmdb_collection" => Ok(CollectionKind::TmdbCollection),
            "system" => Ok(CollectionKind::System),
            _ => Err(MediaError::Internal(format!(
                "unknown collection kind: {value}"
            ))),
        }
    }

    fn encode_source(value: CollectionSource) -> &'static str {
        match value {
            CollectionSource::Manual => "manual",
            CollectionSource::DynamicRule => "dynamic_rule",
            CollectionSource::Tmdb => "tmdb",
            CollectionSource::System => "system",
            CollectionSource::Imported => "imported",
        }
    }

    fn decode_source(value: &str) -> Result<CollectionSource> {
        match value {
            "manual" => Ok(CollectionSource::Manual),
            "dynamic_rule" => Ok(CollectionSource::DynamicRule),
            "tmdb" => Ok(CollectionSource::Tmdb),
            "system" => Ok(CollectionSource::System),
            "imported" => Ok(CollectionSource::Imported),
            _ => Err(MediaError::Internal(format!(
                "unknown collection source: {value}"
            ))),
        }
    }

    fn encode_owner_type(value: CollectionOwnerType) -> &'static str {
        match value {
            CollectionOwnerType::User => "user",
            CollectionOwnerType::Device => "device",
            CollectionOwnerType::External => "external",
            CollectionOwnerType::System => "system",
        }
    }

    fn decode_owner_type(value: &str) -> Result<CollectionOwnerType> {
        match value {
            "user" => Ok(CollectionOwnerType::User),
            "device" => Ok(CollectionOwnerType::Device),
            "external" => Ok(CollectionOwnerType::External),
            "system" => Ok(CollectionOwnerType::System),
            _ => Err(MediaError::Internal(format!(
                "unknown collection owner type: {value}"
            ))),
        }
    }

    fn encode_scope(value: CollectionScope) -> &'static str {
        match value {
            CollectionScope::User => "user",
            CollectionScope::Global => "global",
            CollectionScope::Library => "library",
            CollectionScope::Shared => "shared",
        }
    }

    fn decode_scope(value: &str) -> Result<CollectionScope> {
        match value {
            "user" => Ok(CollectionScope::User),
            "global" => Ok(CollectionScope::Global),
            "library" => Ok(CollectionScope::Library),
            "shared" => Ok(CollectionScope::Shared),
            _ => Err(MediaError::Internal(format!(
                "unknown collection scope: {value}"
            ))),
        }
    }

    fn encode_visibility(
        value: crate::api::types::collections::CollectionVisibility,
    ) -> &'static str {
        use crate::api::types::collections::CollectionVisibility as V;
        match value {
            V::Private => "private",
            V::Shared => "shared",
            V::Public => "public",
            V::System => "system",
        }
    }

    fn decode_visibility(
        value: &str,
    ) -> Result<crate::api::types::collections::CollectionVisibility> {
        use crate::api::types::collections::CollectionVisibility as V;
        match value {
            "private" => Ok(V::Private),
            "shared" => Ok(V::Shared),
            "public" => Ok(V::Public),
            "system" => Ok(V::System),
            _ => Err(MediaError::Internal(format!(
                "unknown collection visibility: {value}"
            ))),
        }
    }

    fn encode_presentation(value: CollectionPresentationMode) -> &'static str {
        match value {
            CollectionPresentationMode::Shelf => "shelf",
            CollectionPresentationMode::Grid => "grid",
            CollectionPresentationMode::List => "list",
            CollectionPresentationMode::Playlist => "playlist",
            CollectionPresentationMode::Hero => "hero",
            CollectionPresentationMode::Hidden => "hidden",
        }
    }

    fn decode_presentation(value: &str) -> Result<CollectionPresentationMode> {
        match value {
            "shelf" => Ok(CollectionPresentationMode::Shelf),
            "grid" => Ok(CollectionPresentationMode::Grid),
            "list" => Ok(CollectionPresentationMode::List),
            "playlist" => Ok(CollectionPresentationMode::Playlist),
            "hero" => Ok(CollectionPresentationMode::Hero),
            "hidden" => Ok(CollectionPresentationMode::Hidden),
            _ => Err(MediaError::Internal(format!(
                "unknown collection presentation: {value}"
            ))),
        }
    }

    fn encode_duplicate_policy(
        value: CollectionDuplicatePolicy,
    ) -> &'static str {
        match value {
            CollectionDuplicatePolicy::KeepAll => "keep_all",
            CollectionDuplicatePolicy::DeduplicateMedia => "deduplicate_media",
            CollectionDuplicatePolicy::DeduplicateLogical => {
                "deduplicate_logical"
            }
            CollectionDuplicatePolicy::RejectDuplicates => "reject_duplicates",
        }
    }

    fn decode_duplicate_policy(
        value: &str,
    ) -> Result<CollectionDuplicatePolicy> {
        match value {
            "keep_all" => Ok(CollectionDuplicatePolicy::KeepAll),
            "deduplicate_media" => {
                Ok(CollectionDuplicatePolicy::DeduplicateMedia)
            }
            "deduplicate_logical" => {
                Ok(CollectionDuplicatePolicy::DeduplicateLogical)
            }
            "reject_duplicates" => {
                Ok(CollectionDuplicatePolicy::RejectDuplicates)
            }
            _ => Err(MediaError::Internal(format!(
                "unknown collection duplicate policy: {value}"
            ))),
        }
    }

    fn encode_media_kind(value: CollectionMediaKind) -> &'static str {
        value.as_slug()
    }

    fn decode_media_kind(value: &str) -> Result<CollectionMediaKind> {
        match value {
            "movie" => Ok(CollectionMediaKind::Movie),
            "series" => Ok(CollectionMediaKind::Series),
            "season" => Ok(CollectionMediaKind::Season),
            "episode" => Ok(CollectionMediaKind::Episode),
            _ => Err(MediaError::Internal(format!(
                "unknown collection media kind: {value}"
            ))),
        }
    }

    fn media_id_from_kind(kind: CollectionMediaKind, id: Uuid) -> MediaID {
        match kind {
            CollectionMediaKind::Movie => MediaID::Movie(MovieID(id)),
            CollectionMediaKind::Series => MediaID::Series(SeriesID(id)),
            CollectionMediaKind::Season => MediaID::Season(SeasonID(id)),
            CollectionMediaKind::Episode => MediaID::Episode(EpisodeID(id)),
        }
    }

    fn media_kind_from_id(media_id: MediaID) -> CollectionMediaKind {
        CollectionMediaKind::from(&media_id)
    }

    fn encode_availability_status(
        value: CollectionMemberAvailabilityStatus,
    ) -> &'static str {
        match value {
            CollectionMemberAvailabilityStatus::Available => "available",
            CollectionMemberAvailabilityStatus::Pending => "pending",
            CollectionMemberAvailabilityStatus::Missing => "missing",
            CollectionMemberAvailabilityStatus::Unavailable => "unavailable",
            CollectionMemberAvailabilityStatus::Tombstoned => "tombstoned",
            CollectionMemberAvailabilityStatus::Archived => "archived",
        }
    }

    fn decode_availability_status(
        value: &str,
    ) -> Result<CollectionMemberAvailabilityStatus> {
        match value {
            "available" => Ok(CollectionMemberAvailabilityStatus::Available),
            "pending" => Ok(CollectionMemberAvailabilityStatus::Pending),
            "missing" => Ok(CollectionMemberAvailabilityStatus::Missing),
            "unavailable" => {
                Ok(CollectionMemberAvailabilityStatus::Unavailable)
            }
            "tombstoned" => Ok(CollectionMemberAvailabilityStatus::Tombstoned),
            "archived" => Ok(CollectionMemberAvailabilityStatus::Archived),
            _ => Err(MediaError::Internal(format!(
                "unknown collection availability status: {value}"
            ))),
        }
    }

    fn decode_materialization_state(
        value: &str,
    ) -> Result<CollectionMaterializationState> {
        match value {
            "not_materialized" => {
                Ok(CollectionMaterializationState::NotMaterialized)
            }
            "pending" => Ok(CollectionMaterializationState::Pending),
            "refreshing" => Ok(CollectionMaterializationState::Refreshing),
            "ready" => Ok(CollectionMaterializationState::Ready),
            "stale" => Ok(CollectionMaterializationState::Stale),
            "failed" => Ok(CollectionMaterializationState::Failed),
            _ => Err(MediaError::Internal(format!(
                "unknown collection materialization state: {value}"
            ))),
        }
    }

    fn encode_shelf_surface(value: ShelfSurface) -> &'static str {
        match value {
            ShelfSurface::Home => "home",
            ShelfSurface::Library => "library",
            ShelfSurface::CollectionDetail => "collection_detail",
            ShelfSurface::Search => "search",
            ShelfSurface::Admin => "admin",
        }
    }

    fn decode_shelf_surface(value: &str) -> Result<ShelfSurface> {
        match value {
            "home" => Ok(ShelfSurface::Home),
            "library" => Ok(ShelfSurface::Library),
            "collection_detail" => Ok(ShelfSurface::CollectionDetail),
            "search" => Ok(ShelfSurface::Search),
            "admin" => Ok(ShelfSurface::Admin),
            _ => Err(MediaError::Internal(format!(
                "unknown shelf surface: {value}"
            ))),
        }
    }

    fn shelf_placement_from_row(row: &PgRow) -> Result<ShelfPlacement> {
        let schema_version = u16::try_from(
            row.try_get::<i32, _>("schema_version").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement schema version: {e}"
                ))
            })?,
        )
        .map_err(|_| {
            MediaError::Internal(
                "shelf placement schema version exceeds u16".to_string(),
            )
        })?;
        let position =
            u32::try_from(row.try_get::<i32, _>("position").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement position: {e}"
                ))
            })?)
            .map_err(|_| {
                MediaError::Internal(
                    "shelf placement position is negative".to_string(),
                )
            })?;
        let surface: String = row.try_get("surface").map_err(|e| {
            MediaError::Internal(format!(
                "failed to decode shelf placement surface: {e}"
            ))
        })?;
        let presentation: String =
            row.try_get("presentation").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement presentation: {e}"
                ))
            })?;
        let visibility: String = row.try_get("visibility").map_err(|e| {
            MediaError::Internal(format!(
                "failed to decode shelf placement visibility: {e}"
            ))
        })?;
        Ok(ShelfPlacement {
            schema_version,
            id: ShelfPlacementId(row.try_get("id").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement id: {e}"
                ))
            })?),
            collection_id: CollectionId(row.try_get("collection_id").map_err(
                |e| {
                    MediaError::Internal(format!(
                        "failed to decode shelf placement collection id: {e}"
                    ))
                },
            )?),
            surface: Self::decode_shelf_surface(&surface)?,
            shelf_key: row.try_get("shelf_key").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement key: {e}"
                ))
            })?,
            position,
            pinned: row.try_get("pinned").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement pinned flag: {e}"
                ))
            })?,
            presentation: Self::decode_presentation(&presentation)?,
            visibility: Self::decode_visibility(&visibility)?,
            created_at: row.try_get("created_at").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement created_at: {e}"
                ))
            })?,
            updated_at: row.try_get("updated_at").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement updated_at: {e}"
                ))
            })?,
        })
    }

    fn validate_title(title: &str) -> Result<String> {
        let title = title.trim();
        if title.is_empty() {
            return Err(MediaError::InvalidMedia(
                "collection title must not be empty".to_string(),
            ));
        }
        Ok(title.to_string())
    }

    fn validate_owner(owner: &CollectionOwner) -> Result<()> {
        match owner.owner_type {
            CollectionOwnerType::User if owner.user_id.is_none() => {
                Err(MediaError::InvalidMedia(
                    "user-owned collections require owner.user_id".to_string(),
                ))
            }
            CollectionOwnerType::Device
                if owner
                    .device_id
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty()) =>
            {
                Err(MediaError::InvalidMedia(
                    "device-owned collections require owner.device_id"
                        .to_string(),
                ))
            }
            _ => Ok(()),
        }
    }

    fn library_id_for_scope(
        scope: CollectionScope,
        media_scope: &CollectionMediaScope,
    ) -> Result<Option<Uuid>> {
        let library_id = match media_scope {
            CollectionMediaScope::Library { library_id, .. } => {
                Some(library_id.to_uuid())
            }
            _ => None,
        };

        if scope == CollectionScope::Library && library_id.is_none() {
            return Err(MediaError::InvalidMedia(
                "library-scoped collections require media_scope.library.library_id"
                    .to_string(),
            ));
        }

        Ok(library_id)
    }

    fn provenance_for_request(
        source: CollectionSource,
        provenance: Option<CollectionProvenance>,
    ) -> CollectionProvenance {
        provenance.unwrap_or(CollectionProvenance {
            source,
            ..CollectionProvenance::default()
        })
    }

    fn map_definition_row(
        row: CollectionDefinitionRow,
    ) -> Result<CollectionSummary> {
        Self::map_definition_parts(
            row.id,
            row.stable_key,
            row.external_key,
            row.title,
            row.description,
            row.kind,
            row.source,
            row.owner_type,
            row.owner_user_id,
            row.owner_device_id,
            row.owner_display_name,
            row.scope,
            row.visibility,
            row.presentation,
            row.media_scope,
            row.duplicate_policy,
            row.artwork,
            row.theme,
            row.provenance,
            row.contract_version,
            row.revision,
            row.etag,
            row.created_at,
            row.updated_at,
            row.archived_at,
            row.item_count,
            row.materialization_state,
            row.materialization_total_count,
            row.materialization_visible_count,
            row.materialization_rule_hash,
            row.materialization_generated_at,
            row.materialization_expires_at,
            row.materialization_last_error,
        )
    }

    fn map_list_row(row: &CollectionListRow) -> Result<CollectionSummary> {
        Self::map_definition_parts(
            row.id,
            row.stable_key.clone(),
            row.external_key.clone(),
            row.title.clone(),
            row.description.clone(),
            row.kind.clone(),
            row.source.clone(),
            row.owner_type.clone(),
            row.owner_user_id,
            row.owner_device_id.clone(),
            row.owner_display_name.clone(),
            row.scope.clone(),
            row.visibility.clone(),
            row.presentation.clone(),
            row.media_scope.clone(),
            row.duplicate_policy.clone(),
            row.artwork.clone(),
            row.theme.clone(),
            row.provenance.clone(),
            row.contract_version,
            row.revision,
            row.etag.clone(),
            row.created_at,
            row.updated_at,
            row.archived_at,
            row.item_count,
            row.materialization_state.clone(),
            row.materialization_total_count,
            row.materialization_visible_count,
            row.materialization_rule_hash.clone(),
            row.materialization_generated_at,
            row.materialization_expires_at,
            row.materialization_last_error.clone(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn map_definition_parts(
        id: Uuid,
        stable_key: String,
        external_key: Option<String>,
        title: String,
        description: Option<String>,
        kind: String,
        source: String,
        owner_type: String,
        owner_user_id: Option<Uuid>,
        owner_device_id: Option<String>,
        owner_display_name: Option<String>,
        scope: String,
        visibility: String,
        presentation: String,
        media_scope: Value,
        duplicate_policy: String,
        artwork: Value,
        theme: Value,
        provenance: Value,
        contract_version: i32,
        revision: i64,
        etag: Option<String>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        archived_at: Option<DateTime<Utc>>,
        item_count: i64,
        materialization_state: Option<String>,
        materialization_total_count: Option<i32>,
        materialization_visible_count: Option<i32>,
        materialization_rule_hash: Option<String>,
        materialization_generated_at: Option<DateTime<Utc>>,
        materialization_expires_at: Option<DateTime<Utc>>,
        materialization_last_error: Option<String>,
    ) -> Result<CollectionSummary> {
        let item_count = u32::try_from(item_count).map_err(|_| {
            MediaError::Internal(
                "collection item count exceeds u32".to_string(),
            )
        })?;
        let revision = u64::try_from(revision).map_err(|_| {
            MediaError::Internal("collection revision is negative".to_string())
        })?;
        let contract_version =
            u16::try_from(contract_version).map_err(|_| {
                MediaError::Internal(
                    "collection contract version exceeds u16".to_string(),
                )
            })?;

        let materialization = match materialization_state {
            Some(state) => {
                let total_count = materialization_total_count
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(item_count);
                let visible_count = materialization_visible_count
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(total_count);
                CollectionMaterializationStatus {
                    state: Self::decode_materialization_state(&state)?,
                    item_count: visible_count,
                    total_count,
                    visible_count,
                    rule_hash: materialization_rule_hash,
                    generated_at: materialization_generated_at,
                    expires_at: materialization_expires_at,
                    last_error: materialization_last_error,
                    ..CollectionMaterializationStatus::default()
                }
            }
            None => CollectionMaterializationStatus {
                item_count,
                total_count: item_count,
                visible_count: item_count,
                ..CollectionMaterializationStatus::default()
            },
        };
        let summary_item_count = if matches!(
            materialization.state,
            CollectionMaterializationState::Ready
                | CollectionMaterializationState::Stale
                | CollectionMaterializationState::Failed
        ) {
            materialization.visible_count
        } else {
            item_count
        };

        Ok(CollectionSummary {
            identity: CollectionIdentity {
                id: CollectionId(id),
                stable_key,
                external_key,
            },
            title,
            description,
            kind: Self::decode_kind(&kind)?,
            source: Self::decode_source(&source)?,
            owner: CollectionOwner {
                owner_type: Self::decode_owner_type(&owner_type)?,
                user_id: owner_user_id,
                device_id: owner_device_id,
                display_name: owner_display_name,
            },
            scope: Self::decode_scope(&scope)?,
            visibility: Self::decode_visibility(&visibility)?,
            presentation: Self::decode_presentation(&presentation)?,
            media_scope: Self::from_json(
                media_scope,
                "collection media scope",
            )?,
            duplicate_policy: Self::decode_duplicate_policy(&duplicate_policy)?,
            artwork: Self::from_json::<CollectionArtwork>(
                artwork,
                "collection artwork",
            )?,
            theme: Self::from_json::<CollectionTheme>(
                theme,
                "collection theme",
            )?,
            provenance: Self::from_json::<CollectionProvenance>(
                provenance,
                "collection provenance",
            )?,
            version: CollectionVersion {
                contract_version,
                revision,
                etag,
            },
            timestamps: CollectionTimestamps {
                created_at,
                updated_at,
                archived_at,
            },
            item_count: summary_item_count,
            materialization,
        })
    }

    async fn load_definition_row(
        &self,
        id: CollectionId,
    ) -> Result<Option<CollectionDefinitionRow>> {
        let row = sqlx::query_as!(
            CollectionDefinitionRow,
            r#"
            SELECT
                cd.id,
                cd.stable_key,
                cd.external_key,
                cd.title,
                cd.description,
                cd.kind::text AS "kind!",
                cd.source::text AS "source!",
                cd.owner_type::text AS "owner_type!",
                cd.owner_user_id,
                cd.owner_device_id,
                cd.owner_display_name,
                cd.scope::text AS "scope!",
                cd.visibility::text AS "visibility!",
                cd.presentation::text AS "presentation!",
                cd.media_scope,
                cd.duplicate_policy::text AS "duplicate_policy!",
                cd.artwork,
                cd.theme,
                cd.provenance,
                cd.contract_version,
                cd.revision,
                cd.etag,
                cd.created_at,
                cd.updated_at,
                cd.archived_at,
                COALESCE(cmm.item_count, 0)::bigint AS "item_count!",
                cm.state::text AS "materialization_state?",
                cm.total_count AS "materialization_total_count?",
                cm.visible_count AS "materialization_visible_count?",
                cm.rule_hash AS "materialization_rule_hash?",
                cm.evaluated_at AS "materialization_generated_at?",
                cm.expires_at AS "materialization_expires_at?",
                cm.error_message AS "materialization_last_error?"
            FROM collection_definitions cd
            LEFT JOIN (
                SELECT collection_id, COUNT(*)::bigint AS item_count
                FROM collection_manual_memberships
                GROUP BY collection_id
            ) cmm ON cmm.collection_id = cd.id
            LEFT JOIN LATERAL (
                SELECT state, total_count, visible_count, rule_hash, evaluated_at, expires_at, error_message
                FROM collection_materializations
                WHERE collection_id = cd.id
                ORDER BY updated_at DESC, id DESC
                LIMIT 1
            ) cm ON TRUE
            WHERE cd.id = $1
              AND cd.deleted_at IS NULL
            "#,
            id.to_uuid(),
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load collection {id}: {e}"
            ))
        })?;

        Ok(row)
    }

    async fn get_current_revision(
        &self,
        id: CollectionId,
    ) -> Result<Option<i64>> {
        let row = sqlx::query!(
            r#"
            SELECT revision
            FROM collection_definitions
            WHERE id = $1
              AND deleted_at IS NULL
            "#,
            id.to_uuid()
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load collection {id} revision: {e}"
            ))
        })?;
        Ok(row.map(|row| row.revision))
    }

    async fn ensure_write_matched(
        &self,
        id: CollectionId,
        expected_revision: Option<u64>,
        affected: u64,
    ) -> Result<()> {
        if affected > 0 {
            return Ok(());
        }

        match self.get_current_revision(id).await? {
            None => {
                Err(MediaError::NotFound(format!("collection {id} not found")))
            }
            Some(current) if expected_revision.is_some() => {
                Err(MediaError::Conflict(format!(
                    "collection {id} revision conflict: expected {}, current {}",
                    expected_revision.unwrap(),
                    current
                )))
            }
            Some(_) => Err(MediaError::Conflict(format!(
                "collection {id} was not updated"
            ))),
        }
    }

    fn rule_hash(rule: &DynamicCollectionRule) -> Result<String> {
        rule.rule_hash().map_err(|e| {
            MediaError::Internal(format!(
                "failed to encode collection rule hash input: {e}"
            ))
        })
    }

    async fn upsert_rule(
        &self,
        collection_id: CollectionId,
        rule: &DynamicCollectionRule,
    ) -> Result<()> {
        let rule_json = Self::to_json(rule, "collection rule")?;
        let rule_hash = Self::rule_hash(rule)?;
        let schema_version = i32::from(rule.schema_version);

        sqlx::query!(
            r#"
            INSERT INTO collection_dynamic_rules (
                collection_id,
                rule_json,
                rule_schema_version,
                rule_hash,
                enabled,
                updated_at
            )
            VALUES ($1, $2, $3, $4, TRUE, NOW())
            ON CONFLICT (collection_id) DO UPDATE SET
                rule_json = EXCLUDED.rule_json,
                rule_schema_version = EXCLUDED.rule_schema_version,
                rule_hash = EXCLUDED.rule_hash,
                enabled = TRUE,
                last_validation_error = NULL,
                updated_at = NOW()
            "#,
            collection_id.to_uuid(),
            rule_json,
            schema_version,
            rule_hash,
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to upsert rule for collection {collection_id}: {e}"
            ))
        })?;

        sqlx::query!(
            r#"
            UPDATE collection_materializations
            SET
                state = 'stale',
                stale_at = COALESCE(stale_at, NOW()),
                stale_reason = COALESCE(stale_reason, 'collection rule changed'),
                updated_at = NOW()
            WHERE collection_id = $1
              AND rule_hash <> $2
              AND state <> 'stale'
            "#,
            collection_id.to_uuid(),
            rule_hash,
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to mark stale materializations for collection {collection_id}: {e}"
            ))
        })?;

        Ok(())
    }

    async fn load_rule(
        &self,
        collection_id: CollectionId,
    ) -> Result<Option<DynamicCollectionRule>> {
        let row = sqlx::query!(
            r#"
            SELECT rule_json
            FROM collection_dynamic_rules
            WHERE collection_id = $1
              AND enabled = TRUE
            "#,
            collection_id.to_uuid()
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load rule for collection {collection_id}: {e}"
            ))
        })?;

        row.map(|row| Self::from_json(row.rule_json, "collection rule"))
            .transpose()
    }

    async fn load_shelf_placements(
        &self,
        collection_id: CollectionId,
    ) -> Result<Vec<ShelfPlacement>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                schema_version,
                collection_id,
                surface::text AS "surface!",
                shelf_key,
                position,
                pinned,
                presentation::text AS "presentation!",
                visibility::text AS "visibility!",
                created_at,
                updated_at
            FROM collection_shelf_placements
            WHERE collection_id = $1
              AND hidden_at IS NULL
            ORDER BY pinned DESC, position_key, id
            "#,
            collection_id.to_uuid()
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load shelf placements for collection {collection_id}: {e}"
            ))
        })?;

        rows.into_iter()
            .map(|row| {
                let _schema_version = u16::try_from(row.schema_version)
                    .map_err(|_| {
                        MediaError::Internal(
                            "shelf placement schema version exceeds u16"
                                .to_string(),
                        )
                    })?;
                let position = u32::try_from(row.position).map_err(|_| {
                    MediaError::Internal(
                        "shelf placement position is negative".to_string(),
                    )
                })?;
                Ok(ShelfPlacement {
                    schema_version: _schema_version,
                    id: ShelfPlacementId(row.id),
                    collection_id: CollectionId(row.collection_id),
                    surface: Self::decode_shelf_surface(&row.surface)?,
                    shelf_key: row.shelf_key,
                    position,
                    pinned: row.pinned,
                    presentation: Self::decode_presentation(&row.presentation)?,
                    visibility: Self::decode_visibility(&row.visibility)?,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                })
            })
            .collect()
    }

    fn map_resolved_rows(
        kind: CollectionMediaKind,
        rows: Vec<ResolvedItemRow>,
        out: &mut HashMap<MediaID, CollectionResolvedItem>,
    ) -> Result<()> {
        let checked_at = Utc::now();
        for row in rows {
            let media_id = Self::media_id_from_kind(kind, row.id);
            let status = Self::decode_availability_status(&row.status)?;
            out.insert(
                media_id,
                CollectionResolvedItem {
                    item_key: CollectionMemberKey::for_media(&media_id),
                    media_id,
                    title: row.title,
                    subtitle: row.subtitle,
                    availability: CollectionMemberAvailability {
                        status,
                        reason: row.reason,
                        checked_at: Some(checked_at),
                    },
                },
            );
        }
        Ok(())
    }

    async fn resolve_movie_items(
        &self,
        ids: &[Uuid],
        out: &mut HashMap<MediaID, CollectionResolvedItem>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let rows = sqlx::query_as!(
            ResolvedItemRow,
            r#"
            WITH input AS (
                SELECT unnest($1::uuid[]) AS id
            )
            SELECT
                input.id AS "id!: Uuid",
                mr.title AS "title?",
                NULL::text AS "subtitle?",
                CASE
                    WHEN mr.id IS NULL THEN 'missing'
                    WHEN mf.id IS NULL THEN 'unavailable'
                    WHEN mf.is_available THEN 'available'
                    ELSE 'tombstoned'
                END AS "status!",
                CASE
                    WHEN mr.id IS NULL THEN 'movie reference was not found'
                    WHEN mf.id IS NULL THEN 'movie media file was not found'
                    WHEN mf.is_available THEN NULL
                    ELSE COALESCE(mf.tombstone_reason, 'movie media file is tombstoned')
                END AS reason
            FROM input
            LEFT JOIN movie_references mr ON mr.id = input.id
            LEFT JOIN media_files mf ON mf.id = mr.file_id
            ORDER BY input.id
            "#,
            ids,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to resolve collection movie items: {e}"
            ))
        })?;
        Self::map_resolved_rows(CollectionMediaKind::Movie, rows, out)
    }

    async fn resolve_episode_items(
        &self,
        ids: &[Uuid],
        out: &mut HashMap<MediaID, CollectionResolvedItem>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let rows = sqlx::query_as!(
            ResolvedItemRow,
            r#"
            WITH input AS (
                SELECT unnest($1::uuid[]) AS id
            )
            SELECT
                input.id AS "id!: Uuid",
                COALESCE(em.name, 'Episode ' || er.episode_number::text) AS "title?",
                CASE
                    WHEN er.id IS NULL THEN NULL
                    ELSE 'S' || er.season_number::text || ' E' || er.episode_number::text
                END AS "subtitle?",
                CASE
                    WHEN er.id IS NULL THEN 'missing'
                    WHEN mf.id IS NULL THEN 'unavailable'
                    WHEN mf.is_available THEN 'available'
                    ELSE 'tombstoned'
                END AS "status!",
                CASE
                    WHEN er.id IS NULL THEN 'episode reference was not found'
                    WHEN mf.id IS NULL THEN 'episode media file was not found'
                    WHEN mf.is_available THEN NULL
                    ELSE COALESCE(mf.tombstone_reason, 'episode media file is tombstoned')
                END AS reason
            FROM input
            LEFT JOIN episode_references er ON er.id = input.id
            LEFT JOIN episode_metadata em ON em.episode_id = er.id
            LEFT JOIN media_files mf ON mf.id = er.file_id
            ORDER BY input.id
            "#,
            ids,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to resolve collection episode items: {e}"
            ))
        })?;
        Self::map_resolved_rows(CollectionMediaKind::Episode, rows, out)
    }

    async fn resolve_season_items(
        &self,
        ids: &[Uuid],
        out: &mut HashMap<MediaID, CollectionResolvedItem>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let rows = sqlx::query_as!(
            ResolvedItemRow,
            r#"
            WITH input AS (
                SELECT unnest($1::uuid[]) AS id
            ), resolved AS (
                SELECT
                    input.id,
                    COALESCE(sm.name, 'Season ' || sr.season_number::text) AS title,
                    CASE WHEN sr.id IS NULL THEN NULL ELSE 'Season ' || sr.season_number::text END AS subtitle,
                    sr.id IS NOT NULL AS exists,
                    COUNT(er.id)::bigint AS episode_count,
                    COUNT(er.id) FILTER (WHERE mf.is_available)::bigint AS available_count,
                    COUNT(er.id) FILTER (WHERE mf.id IS NOT NULL AND NOT mf.is_available)::bigint AS tombstoned_count
                FROM input
                LEFT JOIN season_references sr ON sr.id = input.id
                LEFT JOIN season_metadata sm ON sm.season_id = sr.id
                LEFT JOIN episode_references er ON er.season_id = sr.id
                LEFT JOIN media_files mf ON mf.id = er.file_id
                GROUP BY input.id, sr.id, sr.season_number, sm.name
            )
            SELECT
                id AS "id!: Uuid",
                title AS "title?",
                subtitle AS "subtitle?",
                CASE
                    WHEN NOT exists THEN 'missing'
                    WHEN available_count > 0 THEN 'available'
                    WHEN tombstoned_count > 0 THEN 'tombstoned'
                    WHEN episode_count > 0 THEN 'unavailable'
                    ELSE 'unavailable'
                END AS "status!",
                CASE
                    WHEN NOT exists THEN 'season reference was not found'
                    WHEN available_count > 0 THEN NULL
                    WHEN tombstoned_count > 0 THEN 'all season media files are tombstoned'
                    WHEN episode_count > 0 THEN 'season has no available episodes'
                    ELSE 'season has no episodes'
                END AS reason
            FROM resolved
            ORDER BY id
            "#,
            ids,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to resolve collection season items: {e}"
            ))
        })?;
        Self::map_resolved_rows(CollectionMediaKind::Season, rows, out)
    }

    async fn resolve_series_items(
        &self,
        ids: &[Uuid],
        out: &mut HashMap<MediaID, CollectionResolvedItem>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let rows = sqlx::query_as!(
            ResolvedItemRow,
            r#"
            WITH input AS (
                SELECT unnest($1::uuid[]) AS id
            ), resolved AS (
                SELECT
                    input.id,
                    s.title,
                    NULL::text AS subtitle,
                    s.id IS NOT NULL AS exists,
                    COUNT(er.id)::bigint AS episode_count,
                    COUNT(er.id) FILTER (WHERE mf.is_available)::bigint AS available_count,
                    COUNT(er.id) FILTER (WHERE mf.id IS NOT NULL AND NOT mf.is_available)::bigint AS tombstoned_count
                FROM input
                LEFT JOIN series s ON s.id = input.id
                LEFT JOIN episode_references er ON er.series_id = s.id
                LEFT JOIN media_files mf ON mf.id = er.file_id
                GROUP BY input.id, s.id, s.title
            )
            SELECT
                id AS "id!: Uuid",
                title AS "title?",
                subtitle AS "subtitle?",
                CASE
                    WHEN NOT exists THEN 'missing'
                    WHEN available_count > 0 THEN 'available'
                    WHEN tombstoned_count > 0 THEN 'tombstoned'
                    WHEN episode_count > 0 THEN 'unavailable'
                    ELSE 'unavailable'
                END AS "status!",
                CASE
                    WHEN NOT exists THEN 'series reference was not found'
                    WHEN available_count > 0 THEN NULL
                    WHEN tombstoned_count > 0 THEN 'all series media files are tombstoned'
                    WHEN episode_count > 0 THEN 'series has no available episodes'
                    ELSE 'series has no episodes'
                END AS reason
            FROM resolved
            ORDER BY id
            "#,
            ids,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to resolve collection series items: {e}"
            ))
        })?;
        Self::map_resolved_rows(CollectionMediaKind::Series, rows, out)
    }

    fn kind_uses_dynamic_materialization(kind: CollectionKind) -> bool {
        matches!(kind, CollectionKind::DynamicRule | CollectionKind::System)
    }

    async fn load_collection_kind(
        &self,
        id: CollectionId,
    ) -> Result<Option<CollectionKind>> {
        let row = sqlx::query!(
            r#"
            SELECT kind::text AS "kind!"
            FROM collection_definitions
            WHERE id = $1
              AND deleted_at IS NULL
            "#,
            id.to_uuid(),
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load collection {id} kind: {e}"
            ))
        })?;

        row.map(|row| Self::decode_kind(&row.kind)).transpose()
    }

    fn materialization_identity_for_rule(
        rule: &DynamicCollectionRule,
    ) -> Result<(&'static str, String, Option<Uuid>)> {
        let watch_user_ids = rule.watch_user_ids();
        match watch_user_ids.as_slice() {
            [] => Ok(("global", "global".to_string(), None)),
            [user_id] => Ok(("user", format!("user:{user_id}"), Some(*user_id))),
            _ => Err(MediaError::InvalidMedia(
                "dynamic collection materialization supports one watch-state user per rule"
                    .to_string(),
            )),
        }
    }

    fn materialization_status_from_row(
        row: MaterializationStatusRow,
    ) -> Result<CollectionMaterializationStatus> {
        let total_count = u32::try_from(row.total_count).map_err(|_| {
            MediaError::Internal(
                "collection materialization total_count is negative"
                    .to_string(),
            )
        })?;
        let visible_count = u32::try_from(row.visible_count).map_err(|_| {
            MediaError::Internal(
                "collection materialization visible_count is negative"
                    .to_string(),
            )
        })?;
        Ok(CollectionMaterializationStatus {
            state: Self::decode_materialization_state(&row.state)?,
            item_count: visible_count,
            total_count,
            visible_count,
            rule_hash: Some(row.rule_hash),
            generated_at: row.evaluated_at,
            expires_at: row.expires_at,
            last_error: row.error_message,
            ..CollectionMaterializationStatus::default()
        })
    }

    async fn load_latest_materialization_status(
        &self,
        collection_id: CollectionId,
    ) -> Result<Option<CollectionMaterializationStatus>> {
        let row = sqlx::query_as!(
            MaterializationStatusRow,
            r#"
            SELECT
                id,
                state::text AS "state!",
                rule_hash,
                evaluated_at,
                expires_at,
                error_message,
                total_count,
                visible_count
            FROM collection_materializations
            WHERE collection_id = $1
            ORDER BY updated_at DESC, id DESC
            LIMIT 1
            "#,
            collection_id.to_uuid(),
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load materialization status for collection {collection_id}: {e}"
            ))
        })?;

        row.map(Self::materialization_status_from_row).transpose()
    }

    async fn list_materialized_collection_items(
        &self,
        id: CollectionId,
        request: ListCollectionItemsRequest,
        mode: CollectionReadMode,
    ) -> Result<ListCollectionItemsResponse> {
        let offset = parse_collection_cursor(request.page.cursor.as_deref())?;
        let limit = clamp_collection_page_limit(request.page.limit);
        let status_row = sqlx::query_as!(
            MaterializationStatusRow,
            r#"
            SELECT
                id,
                state::text AS "state!",
                rule_hash,
                evaluated_at,
                expires_at,
                error_message,
                total_count,
                visible_count
            FROM collection_materializations
            WHERE collection_id = $1
            ORDER BY updated_at DESC, id DESC
            LIMIT 1
            "#,
            id.to_uuid(),
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load materialization status for collection {id}: {e}"
            ))
        })?;

        let Some(status_row) = status_row else {
            return Ok(ListCollectionItemsResponse {
                collection_id: id,
                items: Vec::new(),
                page: page_info_for_materialized_slice(offset, limit, 0),
                materialization: CollectionMaterializationStatus::default(),
            });
        };
        let materialization_id = status_row.id;
        let materialization =
            Self::materialization_status_from_row(status_row)?;

        let rows = sqlx::query_as!(
            MaterializedMembershipRow,
            r#"
            SELECT
                item_key,
                media_type::text AS "media_type!",
                media_id,
                position,
                order_key,
                visible,
                hidden_reason,
                evaluated_at
            FROM collection_materialized_items
            WHERE materialization_id = $1
            ORDER BY position, id
            "#,
            materialization_id,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to list materialized collection {id} items: {e}"
            ))
        })?;

        let identities: Vec<_> = rows
            .iter()
            .map(|row| {
                let kind = Self::decode_media_kind(&row.media_type)?;
                Ok(CollectionItemIdentity {
                    item_key: CollectionMemberKey::from(row.item_key.clone()),
                    media_id: Self::media_id_from_kind(kind, row.media_id),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let resolved = self.resolve_collection_items(&identities).await?;
        let resolved_by_key: HashMap<_, _> = resolved
            .into_iter()
            .map(|item| (item.item_key.clone(), item))
            .collect();

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let kind = Self::decode_media_kind(&row.media_type)?;
            let media_id = Self::media_id_from_kind(kind, row.media_id);
            let item_key = CollectionMemberKey::from(row.item_key.clone());
            let resolved = resolved_by_key.get(&item_key);
            let availability = resolved
                .map(|item| item.availability.clone())
                .unwrap_or(CollectionMemberAvailability {
                    status: CollectionMemberAvailabilityStatus::Missing,
                    reason: row.hidden_reason.clone().or_else(|| {
                        Some("media reference was not found".to_string())
                    }),
                    checked_at: Some(Utc::now()),
                });
            let keep = if mode.exposes_preserved_membership() {
                request
                    .availability
                    .is_none_or(|expected| expected == availability.status)
            } else {
                row.visible
                    && availability.status
                        == CollectionMemberAvailabilityStatus::Available
                    && request.availability.is_none_or(|expected| {
                        expected
                            == CollectionMemberAvailabilityStatus::Available
                    })
            };
            if !keep {
                continue;
            }

            let position = u32::try_from(row.position).map_err(|_| {
                MediaError::Internal(
                    "collection materialized item position is negative"
                        .to_string(),
                )
            })?;
            items.push(CollectionMember {
                item_key,
                media_id,
                media_type: Self::media_kind_from_id(media_id),
                title: resolved
                    .and_then(|item| item.title.clone())
                    .unwrap_or_else(|| media_id.to_string()),
                subtitle: resolved.and_then(|item| item.subtitle.clone()),
                position,
                sort_key: Some(row.order_key),
                availability,
                added_at: Some(row.evaluated_at),
                added_by: None,
            });
        }

        let total = items.len();
        let page_items = items
            .into_iter()
            .skip(offset)
            .take(limit as usize)
            .collect();

        Ok(ListCollectionItemsResponse {
            collection_id: id,
            items: page_items,
            page: page_info_for_materialized_slice(offset, limit, total),
            materialization,
        })
    }

    async fn persist_materialization_success(
        &self,
        collection_id: CollectionId,
        rule: &DynamicCollectionRule,
        evaluation: &DynamicCollectionEvaluation,
    ) -> Result<CollectionMaterializationStatus> {
        let (scope, key, user_id) =
            Self::materialization_identity_for_rule(rule)?;
        let total_count =
            i32::try_from(evaluation.total_count).map_err(|_| {
                MediaError::InvalidMedia(
                    "dynamic collection materialization total exceeds i32"
                        .to_string(),
                )
            })?;
        let visible_count =
            i32::try_from(evaluation.visible_count).map_err(|_| {
                MediaError::InvalidMedia(
                "dynamic collection materialization visible count exceeds i32"
                    .to_string(),
            )
            })?;
        let rule_schema_version = i32::from(rule.schema_version);
        let materialization_schema_version =
            i32::from(COLLECTION_MATERIALIZATION_SCHEMA_VERSION);

        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to start collection materialization transaction: {e}"
            ))
        })?;

        let status_row = sqlx::query_as!(
            MaterializationStatusRow,
            r#"
            INSERT INTO collection_materializations (
                collection_id,
                materialization_scope,
                materialization_key,
                user_id,
                rule_hash,
                rule_schema_version,
                materialization_schema_version,
                state,
                evaluated_at,
                stale_at,
                stale_reason,
                error_at,
                error_code,
                error_message,
                total_count,
                visible_count,
                updated_at
            ) VALUES (
                $1, $2::varchar, $3, $4, $5, $6, $7,
                'ready', NOW(), NULL, NULL, NULL, NULL, NULL, $8, $9, NOW()
            )
            ON CONFLICT (collection_id, materialization_key) DO UPDATE SET
                rule_hash = EXCLUDED.rule_hash,
                rule_schema_version = EXCLUDED.rule_schema_version,
                materialization_schema_version = EXCLUDED.materialization_schema_version,
                state = 'ready',
                evaluated_at = EXCLUDED.evaluated_at,
                stale_at = NULL,
                stale_reason = NULL,
                error_at = NULL,
                error_code = NULL,
                error_message = NULL,
                total_count = EXCLUDED.total_count,
                visible_count = EXCLUDED.visible_count,
                expires_at = NULL,
                updated_at = NOW()
            RETURNING
                id,
                state::text AS "state!",
                rule_hash,
                evaluated_at,
                expires_at,
                error_message,
                total_count,
                visible_count
            "#,
            collection_id.to_uuid(),
            scope,
            key,
            user_id,
            evaluation.rule_hash,
            rule_schema_version,
            materialization_schema_version,
            total_count,
            visible_count,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to upsert materialization state for collection {collection_id}: {e}"
            ))
        })?;

        sqlx::query!(
            r#"
            DELETE FROM collection_materialized_items
            WHERE materialization_id = $1
            "#,
            status_row.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to clear materialized collection {collection_id} items: {e}"
            ))
        })?;

        for item in &evaluation.items {
            let position =
                i32::try_from(item.member.position).map_err(|_| {
                    MediaError::InvalidMedia(
                        "dynamic collection item position exceeds i32"
                            .to_string(),
                    )
                })?;
            let media_type = Self::encode_media_kind(item.member.media_type);
            sqlx::query!(
                r#"
                INSERT INTO collection_materialized_items (
                    materialization_id,
                    collection_id,
                    materialization_key,
                    item_key,
                    media_type,
                    media_id,
                    position,
                    order_key,
                    visible,
                    hidden_reason,
                    evaluated_at
                ) VALUES (
                    $1, $2, $3, $4, ($5::text)::media_type, $6, $7, $8, $9, $10, NOW()
                )
                "#,
                status_row.id,
                collection_id.to_uuid(),
                key,
                item.member.item_key.as_str(),
                media_type,
                *item.member.media_id.as_uuid(),
                position,
                item.order_key,
                item.visible,
                item.hidden_reason,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to insert materialized collection {collection_id} item {}: {e}",
                    item.member.item_key
                ))
            })?;
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to commit collection materialization transaction: {e}"
            ))
        })?;

        Self::materialization_status_from_row(status_row)
    }

    async fn persist_materialization_failure(
        &self,
        collection_id: CollectionId,
        rule: &DynamicCollectionRule,
        error_message: &str,
    ) -> Result<CollectionMaterializationStatus> {
        let (scope, key, user_id) = Self::materialization_identity_for_rule(
            rule,
        )
        .unwrap_or(("global", "global".to_string(), None));
        let rule_hash = Self::rule_hash(rule)
            .unwrap_or_else(|_| format!("invalid:{}", Uuid::now_v7()));
        let rule_schema_version = i32::from(rule.schema_version);
        let materialization_schema_version =
            i32::from(COLLECTION_MATERIALIZATION_SCHEMA_VERSION);
        let status_row = sqlx::query_as!(
            MaterializationStatusRow,
            r#"
            INSERT INTO collection_materializations (
                collection_id,
                materialization_scope,
                materialization_key,
                user_id,
                rule_hash,
                rule_schema_version,
                materialization_schema_version,
                state,
                evaluated_at,
                error_at,
                error_code,
                error_message,
                total_count,
                visible_count,
                updated_at
            ) VALUES (
                $1, $2::varchar, $3, $4, $5, $6, $7,
                'failed', NULL, NOW(), 'evaluation_failed', $8, 0, 0, NOW()
            )
            ON CONFLICT (collection_id, materialization_key) DO UPDATE SET
                rule_hash = EXCLUDED.rule_hash,
                rule_schema_version = EXCLUDED.rule_schema_version,
                materialization_schema_version = EXCLUDED.materialization_schema_version,
                state = 'failed',
                evaluated_at = NULL,
                error_at = NOW(),
                error_code = EXCLUDED.error_code,
                error_message = EXCLUDED.error_message,
                total_count = 0,
                visible_count = 0,
                expires_at = NULL,
                updated_at = NOW()
            RETURNING
                id,
                state::text AS "state!",
                rule_hash,
                evaluated_at,
                expires_at,
                error_message,
                total_count,
                visible_count
            "#,
            collection_id.to_uuid(),
            scope,
            key,
            user_id,
            rule_hash,
            rule_schema_version,
            materialization_schema_version,
            error_message,
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to persist materialization failure for collection {collection_id}: {e}"
            ))
        })?;

        sqlx::query!(
            r#"
            DELETE FROM collection_materialized_items
            WHERE materialization_id = $1
            "#,
            status_row.id,
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to clear failed materialized collection {collection_id} items: {e}"
            ))
        })?;

        Self::materialization_status_from_row(status_row)
    }

    async fn upsert_system_collection_definition(
        &self,
        definition: &SystemCollectionDefinition,
    ) -> Result<CollectionId> {
        let media_scope =
            Self::to_json(&definition.media_scope, "system media scope")?;
        let provenance =
            Self::to_json(&definition.provenance(), "system provenance")?;
        let artwork = Self::to_json(
            &CollectionArtwork::default(),
            "system collection artwork",
        )?;
        let theme = Self::to_json(
            &CollectionTheme::default(),
            "system collection theme",
        )?;
        let library_id = definition.library_id().map(|id| id.to_uuid());
        let contract_version = i32::from(COLLECTION_CONTRACT_VERSION);

        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            WITH resolved AS (
                SELECT COALESCE(
                    (
                        SELECT id
                        FROM collection_definitions
                        WHERE stable_key = $1
                        LIMIT 1
                    ),
                    uuidv7()
                ) AS id
            ), upsert AS (
                INSERT INTO collection_definitions (
                    id,
                    stable_key,
                    external_key,
                    title,
                    description,
                    kind,
                    source,
                    owner_type,
                    owner_user_id,
                    owner_device_id,
                    owner_display_name,
                    scope,
                    library_id,
                    visibility,
                    presentation,
                    media_scope,
                    duplicate_policy,
                    artwork,
                    theme,
                    provenance,
                    contract_version,
                    revision,
                    etag,
                    archived_at,
                    deleted_at,
                    updated_at
                )
                SELECT
                    id,
                    $1,
                    NULL,
                    $2,
                    $3,
                    'system',
                    'system',
                    'system',
                    NULL,
                    NULL,
                    'Ferrex',
                    $4::varchar,
                    $5,
                    'system',
                    'shelf',
                    $6::jsonb,
                    'deduplicate_media',
                    $7::jsonb,
                    $8::jsonb,
                    $9::jsonb,
                    $10,
                    0,
                    concat('collection:', id::text, ':v0'),
                    NULL,
                    NULL,
                    NOW()
                FROM resolved
                ON CONFLICT (stable_key) DO UPDATE SET
                    title = EXCLUDED.title,
                    description = EXCLUDED.description,
                    kind = 'system',
                    source = 'system',
                    owner_type = 'system',
                    owner_user_id = NULL,
                    owner_device_id = NULL,
                    owner_display_name = 'Ferrex',
                    scope = EXCLUDED.scope,
                    library_id = EXCLUDED.library_id,
                    visibility = 'system',
                    presentation = 'shelf',
                    media_scope = EXCLUDED.media_scope,
                    duplicate_policy = EXCLUDED.duplicate_policy,
                    artwork = EXCLUDED.artwork,
                    theme = EXCLUDED.theme,
                    provenance = EXCLUDED.provenance,
                    archived_at = NULL,
                    deleted_at = NULL,
                    updated_at = CASE
                        WHEN collection_definitions.title IS DISTINCT FROM EXCLUDED.title
                          OR collection_definitions.description IS DISTINCT FROM EXCLUDED.description
                          OR collection_definitions.scope IS DISTINCT FROM EXCLUDED.scope
                          OR collection_definitions.library_id IS DISTINCT FROM EXCLUDED.library_id
                          OR collection_definitions.media_scope IS DISTINCT FROM EXCLUDED.media_scope
                          OR collection_definitions.provenance IS DISTINCT FROM EXCLUDED.provenance
                        THEN NOW()
                        ELSE collection_definitions.updated_at
                    END,
                    etag = COALESCE(
                        collection_definitions.etag,
                        concat('collection:', collection_definitions.id::text, ':v', collection_definitions.revision)
                    )
                RETURNING id
            )
            SELECT id FROM upsert
            "#,
        )
        .bind(&definition.stable_key)
        .bind(&definition.title)
        .bind(definition.description())
        .bind(Self::encode_scope(definition.scope))
        .bind(library_id)
        .bind(media_scope)
        .bind(artwork)
        .bind(theme)
        .bind(provenance)
        .bind(contract_version)
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to upsert system collection {}: {e}",
                definition.stable_key
            ))
        })?;

        Ok(CollectionId(id))
    }

    async fn upsert_system_shelf_placement(
        &self,
        collection_id: CollectionId,
        definition: &SystemCollectionDefinition,
    ) -> Result<()> {
        let placement = &definition.placement;
        let scope_user_id = placement.scope.user_id();
        let scope_library_id =
            placement.scope.library_id().map(|id| id.to_uuid());
        let metadata = json!({
            "generated_by": crate::api::types::system_collections::SYSTEM_DISCOVERY_GENERATOR,
            "collection_stable_key": &definition.stable_key,
            "discovery_section_id": &definition.section_id,
            "layout_hint": definition.layout_hint,
        });
        let position = i32::try_from(placement.position).map_err(|_| {
            MediaError::InvalidMedia(
                "system shelf placement position exceeds i32".to_string(),
            )
        })?;
        let position_key = placement.position.to_string();

        sqlx::query(
            r#"
            INSERT INTO collection_shelf_placements (
                collection_id,
                collection_stable_key,
                surface,
                shelf_key,
                placement_scope,
                placement_scope_key,
                scope_user_id,
                scope_library_id,
                visibility,
                presentation,
                pinned,
                pinned_at,
                position,
                position_key,
                hidden_at,
                metadata,
                updated_at
            ) VALUES (
                $1, $2, $3::varchar, $4, $5::varchar, $6,
                $7, $8, 'system', 'shelf', $9, CASE WHEN $9 THEN NOW() ELSE NULL END,
                $10, ($11::text)::numeric, NULL, $12::jsonb, NOW()
            )
            ON CONFLICT (
                surface,
                shelf_key,
                placement_scope,
                placement_scope_key,
                collection_id
            ) DO UPDATE SET
                collection_stable_key = EXCLUDED.collection_stable_key,
                scope_user_id = EXCLUDED.scope_user_id,
                scope_library_id = EXCLUDED.scope_library_id,
                visibility = 'system',
                presentation = 'shelf',
                pinned = EXCLUDED.pinned,
                pinned_at = CASE
                    WHEN EXCLUDED.pinned THEN COALESCE(collection_shelf_placements.pinned_at, NOW())
                    ELSE NULL
                END,
                position = EXCLUDED.position,
                position_key = EXCLUDED.position_key,
                hidden_at = NULL,
                metadata = EXCLUDED.metadata,
                updated_at = NOW()
            "#,
        )
        .bind(collection_id.to_uuid())
        .bind(&definition.stable_key)
        .bind(Self::encode_shelf_surface(placement.surface))
        .bind(&placement.shelf_key)
        .bind(placement.scope.placement_scope())
        .bind(placement.scope.placement_scope_key())
        .bind(scope_user_id)
        .bind(scope_library_id)
        .bind(placement.pinned)
        .bind(position)
        .bind(position_key)
        .bind(metadata)
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to upsert system shelf placement {}: {e}",
                definition.stable_key
            ))
        })?;

        Ok(())
    }

    async fn ensure_manual_collection_for_write(
        &self,
        id: CollectionId,
        expected_revision: Option<u64>,
    ) -> Result<CollectionSummary> {
        let Some(row) = self.load_definition_row(id).await? else {
            return Err(MediaError::NotFound(format!(
                "collection {id} not found"
            )));
        };
        let summary = Self::map_definition_row(row)?;
        if summary.kind != CollectionKind::Manual {
            return Err(MediaError::InvalidMedia(format!(
                "collection {id} is not a manual collection"
            )));
        }
        if let Some(expected) = expected_revision
            && expected != summary.version.revision
        {
            return Err(MediaError::Conflict(format!(
                "collection {id} revision conflict: expected {expected}, current {}",
                summary.version.revision
            )));
        }
        Ok(summary)
    }

    async fn bump_collection_revision(
        &self,
        id: CollectionId,
        expected_revision: Option<u64>,
    ) -> Result<CollectionVersion> {
        let expected_revision = expected_revision
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                MediaError::InvalidMedia(
                    "expected collection revision exceeds i64".to_string(),
                )
            })?;
        let row = sqlx::query!(
            r#"
            UPDATE collection_definitions
            SET
                revision = revision + 1,
                etag = concat('collection:', id::text, ':v', revision + 1),
                updated_at = NOW()
            WHERE id = $1
              AND deleted_at IS NULL
              AND ($2::bigint IS NULL OR revision = $2)
            RETURNING contract_version, revision, etag
            "#,
            id.to_uuid(),
            expected_revision,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to bump collection {id} revision: {e}"
            ))
        })?;

        let Some(row) = row else {
            self.ensure_write_matched(
                id,
                expected_revision.and_then(|value| u64::try_from(value).ok()),
                0,
            )
            .await?;
            unreachable!("ensure_write_matched returns an error for zero rows")
        };
        let contract_version =
            u16::try_from(row.contract_version).map_err(|_| {
                MediaError::Internal(
                    "collection contract version exceeds u16".to_string(),
                )
            })?;
        let revision = u64::try_from(row.revision).map_err(|_| {
            MediaError::Internal("collection revision is negative".to_string())
        })?;
        Ok(CollectionVersion {
            contract_version,
            revision,
            etag: row.etag,
        })
    }
}

#[async_trait]
impl CollectionRepository for PostgresCollectionRepository {
    async fn create_collection(
        &self,
        request: CreateCollectionRequest,
    ) -> Result<CollectionDetail> {
        let title = Self::validate_title(&request.title)?;
        Self::validate_owner(&request.owner)?;
        let library_id =
            Self::library_id_for_scope(request.scope, &request.media_scope)?;
        let id = CollectionId::new();
        let stable_key = id.stable_key();
        let external_key: Option<String> = None;
        let kind = Self::encode_kind(request.kind);
        let source = Self::encode_source(request.source);
        let owner_type = Self::encode_owner_type(request.owner.owner_type);
        let owner_device_id = request
            .owner
            .device_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let owner_display_name = request
            .owner
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let scope = Self::encode_scope(request.scope);
        let visibility = Self::encode_visibility(request.visibility);
        let presentation = Self::encode_presentation(request.presentation);
        let duplicate_policy =
            Self::encode_duplicate_policy(request.duplicate_policy);
        let media_scope = Self::to_json(&request.media_scope, "media scope")?;
        let artwork = Self::to_json(&request.artwork, "collection artwork")?;
        let theme = Self::to_json(&request.theme, "collection theme")?;
        let provenance =
            Self::provenance_for_request(request.source, request.provenance);
        let provenance = Self::to_json(&provenance, "collection provenance")?;
        let contract_version = i32::from(COLLECTION_CONTRACT_VERSION);
        let revision = 0_i64;
        let etag = Self::etag_for(id, 0);

        sqlx::query!(
            r#"
            INSERT INTO collection_definitions (
                id,
                stable_key,
                external_key,
                title,
                description,
                kind,
                source,
                owner_type,
                owner_user_id,
                owner_device_id,
                owner_display_name,
                scope,
                library_id,
                visibility,
                presentation,
                media_scope,
                duplicate_policy,
                artwork,
                theme,
                provenance,
                contract_version,
                revision,
                etag
            ) VALUES (
                $1, $2, $3, $4, $5, $6::varchar, $7::varchar,
                $8::varchar, $9, $10, $11, $12::varchar, $13,
                $14::varchar, $15::varchar, $16, $17::varchar,
                $18, $19, $20, $21, $22, $23
            )
            "#,
            id.to_uuid(),
            stable_key,
            external_key,
            title,
            request.description,
            kind,
            source,
            owner_type,
            request.owner.user_id,
            owner_device_id,
            owner_display_name,
            scope,
            library_id,
            visibility,
            presentation,
            media_scope,
            duplicate_policy,
            artwork,
            theme,
            provenance,
            contract_version,
            revision,
            etag,
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to create collection {id}: {e}"
            ))
        })?;

        if let Some(rule) = &request.rule {
            self.upsert_rule(id, rule).await?;
        }

        self.get_collection_detail(
            id,
            GetCollectionDetailRequest {
                include_rule: request.rule.is_some(),
                include_items_preview: false,
                include_shelf_placements: false,
            },
            CollectionReadMode::Admin,
        )
        .await?
        .ok_or_else(|| {
            MediaError::Internal(format!(
                "created collection {id} could not be reloaded"
            ))
        })
    }

    async fn update_collection(
        &self,
        id: CollectionId,
        request: UpdateCollectionRequest,
    ) -> Result<CollectionDetail> {
        let title = request
            .title
            .as_deref()
            .map(Self::validate_title)
            .transpose()?;
        let visibility = request.visibility.map(Self::encode_visibility);
        let presentation = request.presentation.map(Self::encode_presentation);
        let media_scope = request
            .media_scope
            .as_ref()
            .map(|value| Self::to_json(value, "media scope"))
            .transpose()?;
        let duplicate_policy =
            request.duplicate_policy.map(Self::encode_duplicate_policy);
        let artwork = request
            .artwork
            .as_ref()
            .map(|value| Self::to_json(value, "collection artwork"))
            .transpose()?;
        let theme = request
            .theme
            .as_ref()
            .map(|value| Self::to_json(value, "collection theme"))
            .transpose()?;
        let expected_revision = request
            .expected_revision
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                MediaError::InvalidMedia(
                    "expected collection revision exceeds i64".to_string(),
                )
            })?;

        let result = sqlx::query!(
            r#"
            UPDATE collection_definitions
            SET
                title = COALESCE($2, title),
                description = COALESCE($3, description),
                visibility = COALESCE($4::varchar, visibility),
                presentation = COALESCE($5::varchar, presentation),
                media_scope = COALESCE($6::jsonb, media_scope),
                duplicate_policy = COALESCE($7::varchar, duplicate_policy),
                artwork = COALESCE($8::jsonb, artwork),
                theme = COALESCE($9::jsonb, theme),
                revision = revision + 1,
                etag = concat('collection:', id::text, ':v', revision + 1),
                updated_at = NOW()
            WHERE id = $1
              AND deleted_at IS NULL
              AND ($10::bigint IS NULL OR revision = $10)
            "#,
            id.to_uuid(),
            title,
            request.description,
            visibility,
            presentation,
            media_scope,
            duplicate_policy,
            artwork,
            theme,
            expected_revision,
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to update collection {id}: {e}"
            ))
        })?;

        self.ensure_write_matched(
            id,
            request.expected_revision,
            result.rows_affected(),
        )
        .await?;

        if let Some(rule) = &request.rule {
            self.upsert_rule(id, rule).await?;
        }

        self.get_collection_detail(
            id,
            GetCollectionDetailRequest {
                include_rule: request.rule.is_some(),
                include_items_preview: false,
                include_shelf_placements: false,
            },
            CollectionReadMode::Admin,
        )
        .await?
        .ok_or_else(|| {
            MediaError::Internal(format!(
                "updated collection {id} could not be reloaded"
            ))
        })
    }

    async fn archive_collection(
        &self,
        id: CollectionId,
        request: ArchiveCollectionRequest,
        archived_by: Option<Uuid>,
    ) -> Result<ArchiveCollectionResponse> {
        let expected_revision = request
            .expected_revision
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                MediaError::InvalidMedia(
                    "expected collection revision exceeds i64".to_string(),
                )
            })?;
        let reason = request
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);

        let row = sqlx::query!(
            r#"
            UPDATE collection_definitions
            SET
                archived_at = CASE
                    WHEN $2 THEN COALESCE(archived_at, NOW())
                    ELSE NULL
                END,
                archived_by = CASE WHEN $2 THEN $3::uuid ELSE NULL::uuid END,
                archive_reason = CASE WHEN $2 THEN $4::text ELSE NULL::text END,
                revision = revision + 1,
                etag = concat('collection:', id::text, ':v', revision + 1),
                updated_at = NOW()
            WHERE id = $1
              AND deleted_at IS NULL
              AND ($5::bigint IS NULL OR revision = $5)
            RETURNING archived_at, revision, etag
            "#,
            id.to_uuid(),
            request.archived,
            archived_by,
            reason,
            expected_revision,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to archive collection {id}: {e}"
            ))
        })?;

        let Some(row) = row else {
            self.ensure_write_matched(id, request.expected_revision, 0)
                .await?;
            unreachable!("ensure_write_matched returns an error for zero rows")
        };

        let revision = u64::try_from(row.revision).map_err(|_| {
            MediaError::Internal("collection revision is negative".to_string())
        })?;

        Ok(ArchiveCollectionResponse {
            collection_id: id,
            archived_at: row.archived_at,
            version: CollectionVersion {
                contract_version: COLLECTION_CONTRACT_VERSION,
                revision,
                etag: row.etag,
            },
        })
    }

    async fn delete_collection(
        &self,
        id: CollectionId,
        request: DeleteCollectionRequest,
        deleted_by: Option<Uuid>,
    ) -> Result<DeleteCollectionResponse> {
        let expected_revision =
            Self::expected_revision_i64(request.expected_revision)?;
        let reason = request
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);

        let row = sqlx::query(
            r#"
            UPDATE collection_definitions
            SET
                deleted_at = NOW(),
                deleted_by = $2,
                delete_reason = $3,
                revision = revision + 1,
                etag = concat('collection:', id::text, ':v', revision + 1),
                updated_at = NOW()
            WHERE id = $1
              AND deleted_at IS NULL
              AND ($4::bigint IS NULL OR revision = $4)
            RETURNING deleted_at, contract_version, revision, etag
            "#,
        )
        .bind(id.to_uuid())
        .bind(deleted_by)
        .bind(reason)
        .bind(expected_revision)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to delete collection {id}: {e}"
            ))
        })?;

        let Some(row) = row else {
            self.ensure_write_matched(id, request.expected_revision, 0)
                .await?;
            unreachable!("ensure_write_matched returns an error for zero rows")
        };

        Ok(DeleteCollectionResponse {
            collection_id: id,
            deleted_at: row.try_get::<DateTime<Utc>, _>("deleted_at").map_err(
                |e| {
                    MediaError::Internal(format!(
                        "failed to decode collection {id} deleted_at: {e}"
                    ))
                },
            )?,
            version: Self::version_from_parts(
                row.try_get::<i32, _>("contract_version").map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to decode collection {id} contract version: {e}"
                    ))
                })?,
                row.try_get::<i64, _>("revision").map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to decode collection {id} revision: {e}"
                    ))
                })?,
                row.try_get::<Option<String>, _>("etag").map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to decode collection {id} etag: {e}"
                    ))
                })?,
            )?,
        })
    }

    async fn get_collection_detail(
        &self,
        id: CollectionId,
        request: GetCollectionDetailRequest,
        mode: CollectionReadMode,
    ) -> Result<Option<CollectionDetail>> {
        let Some(row) = self.load_definition_row(id).await? else {
            return Ok(None);
        };
        let summary = Self::map_definition_row(row)?;
        let rule = if request.include_rule {
            self.load_rule(id).await?
        } else {
            None
        };
        let items_preview = if request.include_items_preview {
            self.list_collection_items(
                id,
                ListCollectionItemsRequest {
                    page: Default::default(),
                    availability: None,
                },
                mode,
            )
            .await?
            .items
        } else {
            Vec::new()
        };
        let shelf_placements = if request.include_shelf_placements {
            self.load_shelf_placements(id).await?
        } else {
            Vec::new()
        };

        Ok(Some(CollectionDetail {
            summary,
            rule,
            items_preview,
            shelf_placements,
        }))
    }

    async fn list_collections(
        &self,
        request: ListCollectionsRequest,
        _mode: CollectionReadMode,
    ) -> Result<ListCollectionsResponse> {
        let offset = parse_collection_cursor(request.page.cursor.as_deref())?;
        let limit = clamp_collection_page_limit(request.page.limit);
        let fetch_limit = i64::from(limit) + 1;
        let offset_i64 = i64::try_from(offset).map_err(|_| {
            MediaError::InvalidMedia(
                "collection pagination cursor is too large".to_string(),
            )
        })?;
        let kind = request.kind.map(Self::encode_kind);
        let scope = request.scope.map(Self::encode_scope);
        let visibility = request.visibility.map(Self::encode_visibility);
        let media_type = request.media_type.map(Self::encode_media_kind);

        let rows = sqlx::query_as!(
            CollectionListRow,
            r#"
            SELECT
                cd.id,
                cd.stable_key,
                cd.external_key,
                cd.title,
                cd.description,
                cd.kind::text AS "kind!",
                cd.source::text AS "source!",
                cd.owner_type::text AS "owner_type!",
                cd.owner_user_id,
                cd.owner_device_id,
                cd.owner_display_name,
                cd.scope::text AS "scope!",
                cd.visibility::text AS "visibility!",
                cd.presentation::text AS "presentation!",
                cd.media_scope,
                cd.duplicate_policy::text AS "duplicate_policy!",
                cd.artwork,
                cd.theme,
                cd.provenance,
                cd.contract_version,
                cd.revision,
                cd.etag,
                cd.created_at,
                cd.updated_at,
                cd.archived_at,
                COALESCE(cmm.item_count, 0)::bigint AS "item_count!",
                cm.state::text AS "materialization_state?",
                cm.total_count AS "materialization_total_count?",
                cm.visible_count AS "materialization_visible_count?",
                cm.rule_hash AS "materialization_rule_hash?",
                cm.evaluated_at AS "materialization_generated_at?",
                cm.expires_at AS "materialization_expires_at?",
                cm.error_message AS "materialization_last_error?",
                COUNT(*) OVER()::bigint AS "total!"
            FROM collection_definitions cd
            LEFT JOIN (
                SELECT collection_id, COUNT(*)::bigint AS item_count
                FROM collection_manual_memberships
                GROUP BY collection_id
            ) cmm ON cmm.collection_id = cd.id
            LEFT JOIN LATERAL (
                SELECT state, total_count, visible_count, rule_hash, evaluated_at, expires_at, error_message
                FROM collection_materializations
                WHERE collection_id = cd.id
                ORDER BY updated_at DESC, id DESC
                LIMIT 1
            ) cm ON TRUE
            WHERE cd.deleted_at IS NULL
              AND ($1::text IS NULL OR cd.kind = $1::varchar)
              AND ($2::text IS NULL OR cd.scope = $2::varchar)
              AND ($3::text IS NULL OR cd.visibility = $3::varchar)
              AND ($4::bool OR cd.archived_at IS NULL)
              AND (
                    $5::text IS NULL
                    OR cd.media_scope->>'type' = 'all'
                    OR (cd.media_scope->>'type' IN ('types', 'library')
                        AND cd.media_scope->'media_types' ? $5)
                  )
            ORDER BY cd.updated_at DESC, cd.id
            LIMIT $6 OFFSET $7
            "#,
            kind,
            scope,
            visibility,
            request.include_archived,
            media_type,
            fetch_limit,
            offset_i64,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to list collections: {e}"
            ))
        })?;

        let total = rows
            .first()
            .and_then(|row| usize::try_from(row.total).ok())
            .unwrap_or(0);
        let has_next = rows.len() > limit as usize;
        let mut collections =
            Vec::with_capacity(rows.len().min(limit as usize));
        for row in rows.into_iter().take(limit as usize) {
            collections.push(Self::map_list_row(&row)?);
        }
        let page = CollectionPageInfo {
            next_cursor: has_next
                .then(|| offset.saturating_add(limit as usize).to_string()),
            limit,
            total: total as u64,
        };

        Ok(ListCollectionsResponse { collections, page })
    }

    async fn list_collection_items(
        &self,
        id: CollectionId,
        request: ListCollectionItemsRequest,
        mode: CollectionReadMode,
    ) -> Result<ListCollectionItemsResponse> {
        if self
            .load_collection_kind(id)
            .await?
            .is_some_and(Self::kind_uses_dynamic_materialization)
        {
            return self
                .list_materialized_collection_items(id, request, mode)
                .await;
        }

        let offset = parse_collection_cursor(request.page.cursor.as_deref())?;
        let limit = clamp_collection_page_limit(request.page.limit);
        let rows = sqlx::query_as!(
            ManualMembershipRow,
            r#"
            SELECT
                item_key,
                media_type::text AS "media_type!",
                media_id,
                title_snapshot,
                subtitle_snapshot,
                (ROW_NUMBER() OVER (ORDER BY position_key, id) - 1)::int AS "position!",
                sort_key,
                added_at,
                added_by
            FROM collection_manual_memberships
            WHERE collection_id = $1
            ORDER BY position_key, id
            "#,
            id.to_uuid(),
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to list collection {id} items: {e}"
            ))
        })?;

        let identities: Vec<_> = rows
            .iter()
            .map(|row| {
                let kind = Self::decode_media_kind(&row.media_type)?;
                Ok(CollectionItemIdentity {
                    item_key: CollectionMemberKey::from(row.item_key.clone()),
                    media_id: Self::media_id_from_kind(kind, row.media_id),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let resolved = self.resolve_collection_items(&identities).await?;
        let resolved_by_key: HashMap<_, _> = resolved
            .into_iter()
            .map(|item| (item.item_key.clone(), item))
            .collect();

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let kind = Self::decode_media_kind(&row.media_type)?;
            let media_id = Self::media_id_from_kind(kind, row.media_id);
            let item_key = CollectionMemberKey::from(row.item_key.clone());
            let resolved = resolved_by_key.get(&item_key);
            let availability = resolved
                .map(|item| item.availability.clone())
                .unwrap_or(CollectionMemberAvailability {
                    status: CollectionMemberAvailabilityStatus::Missing,
                    reason: Some("media reference was not found".to_string()),
                    checked_at: Some(Utc::now()),
                });
            let keep = if mode.exposes_preserved_membership() {
                request
                    .availability
                    .is_none_or(|expected| expected == availability.status)
            } else {
                availability.status
                    == CollectionMemberAvailabilityStatus::Available
                    && request.availability.is_none_or(|expected| {
                        expected
                            == CollectionMemberAvailabilityStatus::Available
                    })
            };
            if !keep {
                continue;
            }

            let position = u32::try_from(row.position).map_err(|_| {
                MediaError::Internal(
                    "collection item position is negative".to_string(),
                )
            })?;
            items.push(CollectionMember {
                item_key,
                media_id,
                media_type: Self::media_kind_from_id(media_id),
                title: resolved
                    .and_then(|item| item.title.clone())
                    .or(row.title_snapshot)
                    .unwrap_or_else(|| media_id.to_string()),
                subtitle: resolved
                    .and_then(|item| item.subtitle.clone())
                    .or(row.subtitle_snapshot),
                position,
                sort_key: row.sort_key,
                availability,
                added_at: Some(row.added_at),
                added_by: row.added_by,
            });
        }

        let total = items.len();
        let page_items = items
            .into_iter()
            .skip(offset)
            .take(limit as usize)
            .collect();

        Ok(ListCollectionItemsResponse {
            collection_id: id,
            items: page_items,
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
        let summary = self
            .ensure_manual_collection_for_write(id, request.expected_revision)
            .await?;
        let existing_rows = sqlx::query!(
            r#"
            SELECT
                item_key,
                media_type::text AS "media_type!",
                media_id,
                position_key::bigint AS "position!"
            FROM collection_manual_memberships
            WHERE collection_id = $1
            "#,
            id.to_uuid(),
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load collection {id} manual members: {e}"
            ))
        })?;

        let mut existing_keys = HashSet::new();
        let mut existing_media = HashSet::new();
        let mut occupied_positions = HashSet::new();
        let mut next_position = 1_u32;
        for row in existing_rows {
            existing_keys.insert(row.item_key.clone());
            existing_media.insert((row.media_type, row.media_id));
            if let Ok(position) = u32::try_from(row.position) {
                occupied_positions.insert(position);
                next_position = next_position.max(position.saturating_add(1));
            }
        }

        let identities = request
            .items
            .iter()
            .map(|item| CollectionItemIdentity::new(item.media_id))
            .collect::<Vec<_>>();
        let resolved = self.resolve_collection_items(&identities).await?;
        let resolved_by_key: HashMap<_, _> = resolved
            .into_iter()
            .map(|item| (item.item_key.clone(), item))
            .collect();
        let policy =
            request.duplicate_policy.unwrap_or(summary.duplicate_policy);
        let mut results = Vec::with_capacity(request.items.len());
        let mut changed = false;

        for item in request.items {
            let item_key = CollectionMemberKey::for_media(&item.media_id);
            let media_type = Self::media_kind_from_id(item.media_id);
            let media_type_slug = Self::encode_media_kind(media_type);
            let media_uuid = *item.media_id.as_uuid();
            let already_present = existing_keys.contains(item_key.as_str())
                || existing_media
                    .contains(&(media_type_slug.to_string(), media_uuid));
            if already_present {
                if policy == CollectionDuplicatePolicy::RejectDuplicates {
                    return Err(MediaError::Conflict(format!(
                        "collection {id} already contains {item_key}"
                    )));
                }
                results.push(CollectionManualAddResult {
                    item_key,
                    status: if policy == CollectionDuplicatePolicy::KeepAll {
                        CollectionManualAddStatus::AlreadyPresent
                    } else {
                        CollectionManualAddStatus::DuplicateSkipped
                    },
                    message: Some(
                        "Item is already present in this collection"
                            .to_string(),
                    ),
                });
                continue;
            }

            let resolved = resolved_by_key.get(&item_key);
            let availability = resolved
                .map(|item| item.availability.clone())
                .unwrap_or(CollectionMemberAvailability {
                    status: CollectionMemberAvailabilityStatus::Missing,
                    reason: Some("media reference was not found".to_string()),
                    checked_at: Some(Utc::now()),
                });
            if availability.status
                != CollectionMemberAvailabilityStatus::Available
            {
                results.push(CollectionManualAddResult {
                    item_key,
                    status: CollectionManualAddStatus::Unavailable,
                    message: Some(
                        availability.reason.clone().unwrap_or_else(|| {
                            "Item is unavailable".to_string()
                        }),
                    ),
                });
                continue;
            }

            let position = item.position.unwrap_or_else(|| {
                while occupied_positions.contains(&next_position) {
                    next_position = next_position.saturating_add(1);
                }
                let position = next_position;
                next_position = next_position.saturating_add(1);
                position
            });
            if !occupied_positions.insert(position) {
                return Err(MediaError::Conflict(format!(
                    "collection {id} already has an item at position {position}"
                )));
            }

            let title_snapshot = item
                .title_override
                .or_else(|| resolved.and_then(|item| item.title.clone()))
                .unwrap_or_else(|| item.media_id.to_string());
            let subtitle_snapshot =
                resolved.and_then(|item| item.subtitle.clone());
            let availability_status =
                Self::encode_availability_status(availability.status);
            let availability_reason = availability.reason.clone();

            sqlx::query!(
                r#"
                INSERT INTO collection_manual_memberships (
                    collection_id,
                    item_key,
                    media_type,
                    media_id,
                    title_snapshot,
                    subtitle_snapshot,
                    position_key,
                    availability_status,
                    availability_reason,
                    availability_checked_at,
                    added_by
                ) VALUES (
                    $1, $2, ($3::text)::media_type, $4, $5, $6,
                    ($7::text)::numeric, $8::varchar, $9, NOW(), $10
                )
                "#,
                id.to_uuid(),
                item_key.as_str(),
                media_type_slug,
                media_uuid,
                title_snapshot,
                subtitle_snapshot,
                position.to_string(),
                availability_status,
                availability_reason,
                added_by,
            )
            .execute(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to add {item_key} to collection {id}: {e}"
                ))
            })?;

            existing_keys.insert(item_key.as_str().to_string());
            existing_media.insert((media_type_slug.to_string(), media_uuid));
            results.push(CollectionManualAddResult {
                item_key,
                status: CollectionManualAddStatus::Added,
                message: None,
            });
            changed = true;
        }

        let version = if changed {
            self.bump_collection_revision(id, request.expected_revision)
                .await?
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
        let summary = self
            .ensure_manual_collection_for_write(id, request.expected_revision)
            .await?;
        if request.item_keys.is_empty() {
            return Ok(ManualRemoveCollectionItemsResponse {
                collection_id: id,
                removed_item_keys: Vec::new(),
                missing_item_keys: Vec::new(),
                version: summary.version,
            });
        }

        let requested = request
            .item_keys
            .iter()
            .map(|key| key.as_str().to_string())
            .collect::<Vec<_>>();
        let rows = sqlx::query!(
            r#"
            DELETE FROM collection_manual_memberships
            WHERE collection_id = $1
              AND item_key = ANY($2::text[])
            RETURNING item_key
            "#,
            id.to_uuid(),
            &requested,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to remove collection {id} manual members: {e}"
            ))
        })?;

        let removed_set = rows
            .iter()
            .map(|row| row.item_key.clone())
            .collect::<HashSet<_>>();
        let removed_item_keys = rows
            .into_iter()
            .map(|row| CollectionMemberKey::from(row.item_key))
            .collect::<Vec<_>>();
        let missing_item_keys = request
            .item_keys
            .into_iter()
            .filter(|key| !removed_set.contains(key.as_str()))
            .collect::<Vec<_>>();
        let version = if removed_item_keys.is_empty() {
            summary.version
        } else {
            self.bump_collection_revision(id, request.expected_revision)
                .await?
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
        let summary = self
            .ensure_manual_collection_for_write(id, request.expected_revision)
            .await?;
        if request.ordering.is_empty() {
            return Ok(ManualReorderCollectionItemsResponse {
                collection_id: id,
                version: summary.version,
            });
        }

        let mut seen_keys = HashSet::new();
        let mut seen_positions = HashSet::new();
        for order in &request.ordering {
            if !seen_keys.insert(order.item_key.as_str()) {
                return Err(MediaError::InvalidMedia(format!(
                    "duplicate reorder item {} for collection {id}",
                    order.item_key
                )));
            }
            if !seen_positions.insert(order.position) {
                return Err(MediaError::InvalidMedia(format!(
                    "duplicate reorder position {} for collection {id}",
                    order.position
                )));
            }
        }
        let requested = request
            .ordering
            .iter()
            .map(|order| order.item_key.as_str().to_string())
            .collect::<Vec<_>>();
        let rows = sqlx::query!(
            r#"
            SELECT item_key
            FROM collection_manual_memberships
            WHERE collection_id = $1
              AND item_key = ANY($2::text[])
            "#,
            id.to_uuid(),
            &requested,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load collection {id} reorder members: {e}"
            ))
        })?;
        let found = rows
            .iter()
            .map(|row| row.item_key.as_str())
            .collect::<HashSet<_>>();
        if found.len() != requested.len() {
            let missing = requested
                .iter()
                .filter(|key| !found.contains(key.as_str()))
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            return Err(MediaError::InvalidMedia(format!(
                "collection {id} cannot reorder missing items: {missing}"
            )));
        }
        let max_position = sqlx::query!(
            r#"
            SELECT COALESCE(MAX(position_key), 0)::bigint AS "max_position!"
            FROM collection_manual_memberships
            WHERE collection_id = $1
            "#,
            id.to_uuid(),
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load collection {id} max position: {e}"
            ))
        })?
        .max_position;
        let temp_base = max_position.saturating_add(1_000_000);

        for (index, order) in request.ordering.iter().enumerate() {
            let temp_position = temp_base.saturating_add(index as i64);
            sqlx::query!(
                r#"
                UPDATE collection_manual_memberships
                SET position_key = ($3::text)::numeric, updated_at = NOW()
                WHERE collection_id = $1
                  AND item_key = $2
                "#,
                id.to_uuid(),
                order.item_key.as_str(),
                temp_position.to_string(),
            )
            .execute(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to prepare collection {id} reorder for {}: {e}",
                    order.item_key
                ))
            })?;
        }

        for order in &request.ordering {
            sqlx::query!(
                r#"
                UPDATE collection_manual_memberships
                SET position_key = ($3::text)::numeric, updated_at = NOW()
                WHERE collection_id = $1
                  AND item_key = $2
                "#,
                id.to_uuid(),
                order.item_key.as_str(),
                order.position.to_string(),
            )
            .execute(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to reorder collection {id} item {}: {e}",
                    order.item_key
                ))
            })?;
        }

        let version = self
            .bump_collection_revision(id, request.expected_revision)
            .await?;
        Ok(ManualReorderCollectionItemsResponse {
            collection_id: id,
            version,
        })
    }

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
    ) -> Result<PreviewCollectionRuleResponse> {
        DynamicCollectionEvaluator::new(self.pool())
            .preview(&request.rule, request.page, mode)
            .await
    }

    async fn refresh_collection_rule(
        &self,
        id: CollectionId,
        request: RefreshCollectionRuleRequest,
    ) -> Result<RefreshCollectionRuleResponse> {
        let Some(row) = self.load_definition_row(id).await? else {
            return Err(MediaError::NotFound(format!(
                "collection {id} not found"
            )));
        };
        let summary = Self::map_definition_row(row)?;
        if !Self::kind_uses_dynamic_materialization(summary.kind) {
            return Err(MediaError::InvalidMedia(format!(
                "collection {id} is not a dynamic rule collection"
            )));
        }
        let rule = self.load_rule(id).await?.ok_or_else(|| {
            MediaError::InvalidMedia(format!(
                "collection {id} does not have an enabled dynamic rule"
            ))
        })?;
        let current_rule_hash = Self::rule_hash(&rule)?;
        if let Some(expected) = request.expected_rule_hash.as_deref() {
            if expected != current_rule_hash {
                return Err(MediaError::Conflict(format!(
                    "collection {id} rule hash conflict: expected {expected}, current {current_rule_hash}"
                )));
            }
        }

        if !request.force {
            if let Some(status) =
                self.load_latest_materialization_status(id).await?
            {
                if status.rule_hash.as_deref()
                    == Some(current_rule_hash.as_str())
                    && status.state == CollectionMaterializationState::Ready
                {
                    return Ok(RefreshCollectionRuleResponse {
                        collection_id: id,
                        materialization: status,
                        version: summary.version,
                    });
                }
            }
        }

        let evaluator = DynamicCollectionEvaluator::new(self.pool());
        match evaluator.evaluate(&rule).await {
            Ok(evaluation) => {
                let materialization = self
                    .persist_materialization_success(id, &rule, &evaluation)
                    .await?;
                Ok(RefreshCollectionRuleResponse {
                    collection_id: id,
                    materialization,
                    version: summary.version,
                })
            }
            Err(error) => {
                let error_message = error.to_string();
                self.persist_materialization_failure(id, &rule, &error_message)
                    .await?;
                Err(error)
            }
        }
    }

    async fn list_shelf_placements(
        &self,
        request: ListShelfPlacementsRequest,
        _mode: CollectionReadMode,
    ) -> Result<ListShelfPlacementsResponse> {
        let surface = request.surface.map(Self::encode_shelf_surface);
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                schema_version,
                collection_id,
                surface::text AS surface,
                shelf_key,
                position,
                pinned,
                presentation::text AS presentation,
                visibility::text AS visibility,
                created_at,
                updated_at
            FROM collection_shelf_placements
            WHERE hidden_at IS NULL
              AND ($1::text IS NULL OR surface = $1::varchar)
              AND ($2::text IS NULL OR shelf_key = $2)
              AND ($3::bool OR pinned)
            ORDER BY pinned DESC, position_key, id
            "#,
        )
        .bind(surface)
        .bind(request.shelf_key)
        .bind(request.include_unpinned)
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to list shelf placements: {e}"
            ))
        })?;
        let placements = rows
            .iter()
            .map(Self::shelf_placement_from_row)
            .collect::<Result<Vec<_>>>()?;
        Ok(ListShelfPlacementsResponse { placements })
    }

    async fn pin_shelf_placement(
        &self,
        request: PinShelfPlacementRequest,
        pinned_by: Option<Uuid>,
    ) -> Result<PinShelfPlacementResponse> {
        let shelf_key = request.shelf_key.trim().to_string();
        if shelf_key.is_empty() {
            return Err(MediaError::InvalidMedia(
                "shelf_key must not be empty".to_string(),
            ));
        }
        let surface = Self::encode_shelf_surface(request.surface);
        let presentation = request.presentation.map(Self::encode_presentation);
        let collection = self
            .load_definition_row(request.collection_id)
            .await?
            .ok_or_else(|| {
                MediaError::NotFound(format!(
                    "collection {} not found",
                    request.collection_id
                ))
            })?;
        let collection_summary = Self::map_definition_row(collection)?;
        let collection_presentation = presentation.unwrap_or_else(|| {
            Self::encode_presentation(collection_summary.presentation)
        });
        let visibility = Self::encode_visibility(collection_summary.visibility);
        let position = if request.pinned {
            match request.position {
                Some(position) => position,
                None => {
                    let row = sqlx::query(
                        r#"
                        SELECT COALESCE(MAX(position), -1) + 1 AS next_position
                        FROM collection_shelf_placements
                        WHERE surface = $1::varchar
                          AND shelf_key = $2
                          AND placement_scope = 'global'
                          AND placement_scope_key = 'global'
                          AND hidden_at IS NULL
                        "#,
                    )
                    .bind(surface)
                    .bind(&shelf_key)
                    .fetch_one(self.pool())
                    .await
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "failed to load shelf placement tail: {e}"
                        ))
                    })?;
                    u32::try_from(
                        row.try_get::<i32, _>("next_position").map_err(
                            |e| {
                                MediaError::Internal(format!(
                                    "failed to decode shelf placement tail: {e}"
                                ))
                            },
                        )?,
                    )
                    .map_err(|_| {
                        MediaError::Internal(
                            "shelf placement tail is negative".to_string(),
                        )
                    })?
                }
            }
        } else {
            let row = sqlx::query(
                r#"
                SELECT COALESCE(MAX(position), -1) + 1 AS next_position
                FROM collection_shelf_placements
                WHERE surface = $1::varchar
                  AND shelf_key = $2
                  AND placement_scope = 'global'
                  AND placement_scope_key = 'global'
                "#,
            )
            .bind(surface)
            .bind(&shelf_key)
            .fetch_one(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to load hidden shelf placement tail: {e}"
                ))
            })?;
            u32::try_from(row.try_get::<i32, _>("next_position").map_err(
                |e| {
                    MediaError::Internal(format!(
                        "failed to decode hidden shelf placement tail: {e}"
                    ))
                },
            )?)
            .map_err(|_| {
                MediaError::Internal(
                    "hidden shelf placement tail is negative".to_string(),
                )
            })?
        };
        let position_key = manual_position_key_for_index(position as usize)?;
        let row = sqlx::query(
            r#"
            INSERT INTO collection_shelf_placements (
                collection_id,
                collection_stable_key,
                surface,
                shelf_key,
                placement_scope,
                placement_scope_key,
                visibility,
                presentation,
                pinned,
                pinned_at,
                pinned_by,
                position,
                position_key,
                hidden_at,
                updated_at
            ) VALUES (
                $1, $2, $3::varchar, $4, 'global', 'global', $5::varchar,
                $6::varchar, $7, CASE WHEN $7 THEN NOW() ELSE NULL END,
                CASE WHEN $7 THEN $8 ELSE NULL END,
                $9, ($10::text)::numeric,
                CASE WHEN $7 THEN NULL ELSE NOW() END,
                NOW()
            )
            ON CONFLICT (surface, shelf_key, placement_scope, placement_scope_key, collection_id)
            DO UPDATE SET
                pinned = EXCLUDED.pinned,
                pinned_at = CASE WHEN EXCLUDED.pinned THEN NOW() ELSE NULL END,
                pinned_by = CASE WHEN EXCLUDED.pinned THEN $8 ELSE NULL END,
                position = EXCLUDED.position,
                position_key = EXCLUDED.position_key,
                visibility = EXCLUDED.visibility,
                presentation = EXCLUDED.presentation,
                hidden_at = CASE WHEN EXCLUDED.pinned THEN NULL ELSE NOW() END,
                updated_at = NOW()
            RETURNING
                id,
                schema_version,
                collection_id,
                surface::text AS surface,
                shelf_key,
                position,
                pinned,
                presentation::text AS presentation,
                visibility::text AS visibility,
                created_at,
                updated_at
            "#,
        )
        .bind(request.collection_id.to_uuid())
        .bind(collection_summary.identity.stable_key)
        .bind(surface)
        .bind(shelf_key)
        .bind(visibility)
        .bind(collection_presentation)
        .bind(request.pinned)
        .bind(pinned_by)
        .bind(i32::try_from(position).map_err(|_| {
            MediaError::InvalidMedia(
                "shelf placement position exceeds i32".to_string(),
            )
        })?)
        .bind(position_key)
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            if let Some(db_error) = e.as_database_error()
                && db_error.is_unique_violation()
            {
                return MediaError::Conflict(format!(
                    "shelf placement position {position} is already occupied"
                ));
            }
            MediaError::Internal(format!(
                "failed to pin shelf placement for collection {}: {e}",
                request.collection_id
            ))
        })?;
        Ok(PinShelfPlacementResponse {
            placement: Self::shelf_placement_from_row(&row)?,
        })
    }

    async fn reorder_shelf_placements(
        &self,
        request: ReorderShelfPlacementsRequest,
        reordered_by: Option<Uuid>,
    ) -> Result<ReorderShelfPlacementsResponse> {
        let mut seen_ids = HashSet::new();
        let mut seen_positions = HashSet::new();
        for order in &request.ordering {
            if !seen_ids.insert(order.placement_id) {
                return Err(MediaError::InvalidMedia(
                    "shelf reorder contains duplicate placement ids"
                        .to_string(),
                ));
            }
            if !seen_positions.insert(order.position) {
                return Err(MediaError::InvalidMedia(
                    "shelf reorder contains duplicate positions".to_string(),
                ));
            }
        }
        if request.ordering.is_empty() {
            return Ok(ReorderShelfPlacementsResponse { placements: vec![] });
        }
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to start shelf reorder transaction: {e}"
            ))
        })?;
        let placement_ids: Vec<_> = request
            .ordering
            .iter()
            .map(|order| order.placement_id.to_uuid())
            .collect();
        let rows = sqlx::query(
            r#"
            SELECT id, surface::text AS surface, shelf_key, placement_scope, placement_scope_key
            FROM collection_shelf_placements
            WHERE id = ANY($1::uuid[])
              AND hidden_at IS NULL
            FOR UPDATE
            "#,
        )
        .bind(&placement_ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to lock shelf placements for reorder: {e}"
            ))
        })?;
        if rows.len() != placement_ids.len() {
            return Err(MediaError::Conflict(
                "shelf reorder references missing placements".to_string(),
            ));
        }
        let first = rows.first().expect("non-empty reorder rows");
        let surface: String = first.try_get("surface").map_err(|e| {
            MediaError::Internal(format!(
                "failed to decode shelf placement surface: {e}"
            ))
        })?;
        let shelf_key: String = first.try_get("shelf_key").map_err(|e| {
            MediaError::Internal(format!(
                "failed to decode shelf placement key: {e}"
            ))
        })?;
        let placement_scope: String =
            first.try_get("placement_scope").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement scope: {e}"
                ))
            })?;
        let placement_scope_key: String =
            first.try_get("placement_scope_key").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement scope key: {e}"
                ))
            })?;
        for row in &rows {
            let row_surface: String = row.try_get("surface").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode shelf placement surface: {e}"
                ))
            })?;
            let row_shelf_key: String =
                row.try_get("shelf_key").map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to decode shelf placement key: {e}"
                    ))
                })?;
            let row_scope: String =
                row.try_get("placement_scope").map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to decode shelf placement scope: {e}"
                    ))
                })?;
            let row_scope_key: String =
                row.try_get("placement_scope_key").map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to decode shelf placement scope key: {e}"
                    ))
                })?;
            if row_surface != surface
                || row_shelf_key != shelf_key
                || row_scope != placement_scope
                || row_scope_key != placement_scope_key
            {
                return Err(MediaError::Conflict(
                    "shelf reorder must target one shelf".to_string(),
                ));
            }
        }
        let shelf_rows = sqlx::query(
            r#"
            SELECT id
            FROM collection_shelf_placements
            WHERE surface = $1::varchar
              AND shelf_key = $2
              AND placement_scope = $3::varchar
              AND placement_scope_key = $4
              AND hidden_at IS NULL
            ORDER BY position_key, id
            FOR UPDATE
            "#,
        )
        .bind(&surface)
        .bind(&shelf_key)
        .bind(&placement_scope)
        .bind(&placement_scope_key)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load shelf order for reorder: {e}"
            ))
        })?;
        let requested: HashSet<_> = request
            .ordering
            .iter()
            .map(|order| order.placement_id.to_uuid())
            .collect();
        let mut final_order: Vec<Uuid> = shelf_rows
            .iter()
            .filter_map(|row| {
                let id: Uuid = row.try_get("id").ok()?;
                (!requested.contains(&id)).then_some(id)
            })
            .collect();
        let mut ordering = request.ordering.clone();
        ordering.sort_by_key(|order| order.position);
        for order in ordering {
            let index = (order.position as usize).min(final_order.len());
            final_order.insert(index, order.placement_id.to_uuid());
        }
        sqlx::query(
            r#"
            UPDATE collection_shelf_placements
            SET position_key = position_key + 1000000000000000000::numeric,
                updated_at = NOW()
            WHERE surface = $1::varchar
              AND shelf_key = $2
              AND placement_scope = $3::varchar
              AND placement_scope_key = $4
              AND hidden_at IS NULL
            "#,
        )
        .bind(&surface)
        .bind(&shelf_key)
        .bind(&placement_scope)
        .bind(&placement_scope_key)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to reserve shelf reorder positions: {e}"
            ))
        })?;
        for (index, placement_id) in final_order.iter().enumerate() {
            let position_key = manual_position_key_for_index(index)?;
            let position = i32::try_from(index).map_err(|_| {
                MediaError::InvalidMedia(
                    "shelf placement position exceeds i32".to_string(),
                )
            })?;
            sqlx::query(
                r#"
                UPDATE collection_shelf_placements
                SET position = $2,
                    position_key = ($3::text)::numeric,
                    reordered_at = NOW(),
                    reordered_by = $4,
                    reorder_revision = reorder_revision + 1,
                    updated_at = NOW()
                WHERE id = $1
                "#,
            )
            .bind(*placement_id)
            .bind(position)
            .bind(position_key)
            .bind(reordered_by)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to persist shelf reorder position: {e}"
                ))
            })?;
        }
        let response_rows = sqlx::query(
            r#"
            SELECT
                id,
                schema_version,
                collection_id,
                surface::text AS surface,
                shelf_key,
                position,
                pinned,
                presentation::text AS presentation,
                visibility::text AS visibility,
                created_at,
                updated_at
            FROM collection_shelf_placements
            WHERE surface = $1::varchar
              AND shelf_key = $2
              AND placement_scope = $3::varchar
              AND placement_scope_key = $4
              AND hidden_at IS NULL
            ORDER BY position_key, id
            "#,
        )
        .bind(&surface)
        .bind(&shelf_key)
        .bind(&placement_scope)
        .bind(&placement_scope_key)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load reordered shelf placements: {e}"
            ))
        })?;
        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("failed to commit shelf reorder: {e}"))
        })?;
        Ok(ReorderShelfPlacementsResponse {
            placements: response_rows
                .iter()
                .map(Self::shelf_placement_from_row)
                .collect::<Result<Vec<_>>>()?,
        })
    }

    async fn tmdb_import_collection(
        &self,
        request: TmdbImportCollectionRequest,
        imported_by: Option<Uuid>,
    ) -> Result<TmdbImportCollectionResponse> {
        let tmdb_collection_id =
            request.tmdb_id.trim().parse::<i64>().map_err(|_| {
                MediaError::InvalidMedia(
                    "tmdb_id must be a numeric TMDB collection id".to_string(),
                )
            })?;
        let legacy_rows = sqlx::query(
            r#"
            SELECT mcm.movie_id, mcm.library_id, mcm.batch_id, mcm.collection_id,
                   mcm.name, mcm.poster_path, mcm.backdrop_path,
                   mr.title AS movie_title
            FROM movie_collection_membership mcm
            JOIN movie_references mr
              ON mr.id = mcm.movie_id
             AND mr.library_id = mcm.library_id
             AND mr.batch_id = mcm.batch_id
            WHERE mcm.collection_id = $1
            ORDER BY mr.title, mcm.movie_id
            "#,
        )
        .bind(tmdb_collection_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load legacy TMDB collection {}: {e}",
                request.tmdb_id
            ))
        })?;
        let Some(first) = legacy_rows.first() else {
            return Err(MediaError::NotFound(format!(
                "TMDB collection {} is not available for import",
                request.tmdb_id
            )));
        };
        let title: String = request.title_override.clone().unwrap_or(
            first.try_get("name").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode legacy TMDB collection title: {e}"
                ))
            })?,
        );
        let collection = self
            .create_collection(CreateCollectionRequest {
                title,
                description: None,
                kind: CollectionKind::TmdbCollection,
                source: CollectionSource::Tmdb,
                owner: request.owner,
                scope: CollectionScope::Global,
                visibility: request.visibility,
                presentation: request.presentation,
                media_scope: request.media_scope,
                duplicate_policy: request.duplicate_policy,
                artwork: Default::default(),
                theme: Default::default(),
                provenance: Some(CollectionProvenance {
                    source: CollectionSource::Tmdb,
                    imported_from: Some(
                        "legacy_movie_collection_membership".to_string(),
                    ),
                    external_id: Some(request.tmdb_id.clone()),
                    generated_by: imported_by.map(|id| id.to_string()),
                    rule_hash: None,
                    last_refreshed_at: Some(Utc::now()),
                }),
                rule: None,
            })
            .await?;
        let collection_id = collection.summary.identity.id;
        let source_kind = match request.import_kind {
            TmdbCollectionImportKind::Collection => "tmdb_collection",
            TmdbCollectionImportKind::List => "tmdb_list",
            TmdbCollectionImportKind::Keyword => "tmdb_keyword",
        };
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to start TMDB import transaction: {e}"
            ))
        })?;
        let source_row = sqlx::query(
            r#"
            INSERT INTO collection_sources (
                collection_id,
                provider,
                source_kind,
                source_key,
                source_scope_key,
                title,
                refreshed_at,
                updated_at
            ) VALUES ($1, 'tmdb', $2::varchar, $3, 'global', $4, NOW(), NOW())
            ON CONFLICT (provider, source_kind, source_key, source_scope_key)
            DO UPDATE SET
                collection_id = EXCLUDED.collection_id,
                title = EXCLUDED.title,
                refreshed_at = NOW(),
                updated_at = NOW()
            RETURNING id
            "#,
        )
        .bind(collection_id.to_uuid())
        .bind(source_kind)
        .bind(&request.tmdb_id)
        .bind(collection.summary.title.clone())
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to upsert TMDB collection source: {e}"
            ))
        })?;
        let source_id: Uuid = source_row.try_get("id").map_err(|e| {
            MediaError::Internal(format!(
                "failed to decode TMDB collection source id: {e}"
            ))
        })?;
        let mut imported_items = 0_u32;
        for (index, row) in legacy_rows.iter().enumerate() {
            let movie_id: Uuid = row.try_get("movie_id").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode legacy TMDB movie id: {e}"
                ))
            })?;
            let media_id = MediaID::Movie(MovieID(movie_id));
            let item_key = CollectionMemberKey::for_media(&media_id);
            let source_order_key = manual_position_key_for_index(index)?;
            let title: String = row.try_get("movie_title").map_err(|e| {
                MediaError::Internal(format!(
                    "failed to decode legacy TMDB movie title: {e}"
                ))
            })?;
            sqlx::query(
                r#"
                INSERT INTO collection_source_memberships (
                    source_id,
                    collection_id,
                    item_key,
                    media_type,
                    media_id,
                    external_media_type,
                    external_id,
                    external_position,
                    source_order_key,
                    title,
                    poster_path,
                    backdrop_path,
                    match_status,
                    matched_at,
                    legacy_movie_collection_movie_id,
                    legacy_movie_collection_tmdb_id,
                    updated_at
                ) VALUES (
                    $1, $2, $3, 'movie'::media_type, $4, 'movie', $5, $6,
                    $7, $8, $9, $10, 'matched', NOW(), $4, $11, NOW()
                )
                ON CONFLICT (source_id, external_media_type, external_id)
                DO UPDATE SET
                    collection_id = EXCLUDED.collection_id,
                    item_key = EXCLUDED.item_key,
                    media_type = EXCLUDED.media_type,
                    media_id = EXCLUDED.media_id,
                    external_position = EXCLUDED.external_position,
                    source_order_key = EXCLUDED.source_order_key,
                    title = EXCLUDED.title,
                    poster_path = EXCLUDED.poster_path,
                    backdrop_path = EXCLUDED.backdrop_path,
                    match_status = 'matched',
                    matched_at = NOW(),
                    updated_at = NOW()
                "#,
            )
            .bind(source_id)
            .bind(collection_id.to_uuid())
            .bind(item_key.as_str())
            .bind(movie_id)
            .bind(movie_id.to_string())
            .bind(i32::try_from(index).map_err(|_| {
                MediaError::InvalidMedia(
                    "TMDB import position exceeds i32".to_string(),
                )
            })?)
            .bind(source_order_key)
            .bind(title)
            .bind(row.try_get::<Option<String>, _>("poster_path").map_err(
                |e| {
                    MediaError::Internal(format!(
                        "failed to decode legacy TMDB poster path: {e}"
                    ))
                },
            )?)
            .bind(row.try_get::<Option<String>, _>("backdrop_path").map_err(
                |e| {
                    MediaError::Internal(format!(
                        "failed to decode legacy TMDB backdrop path: {e}"
                    ))
                },
            )?)
            .bind(tmdb_collection_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to upsert TMDB source membership: {e}"
                ))
            })?;
            imported_items = imported_items.saturating_add(1);
        }
        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to commit TMDB collection import: {e}"
            ))
        })?;
        Ok(TmdbImportCollectionResponse {
            collection,
            imported_items,
            skipped_items: 0,
            warnings: Vec::new(),
        })
    }

    async fn tmdb_list_collections(
        &self,
        request: TmdbListCollectionsRequest,
    ) -> Result<TmdbListCollectionsResponse> {
        let offset = parse_collection_cursor(request.page.cursor.as_deref())?;
        let limit = clamp_collection_page_limit(request.page.limit);
        let rows = sqlx::query(
            r#"
            SELECT collection_id, name,
                   MIN(poster_path) AS poster_path,
                   COUNT(*)::bigint AS item_count
            FROM movie_collection_membership
            GROUP BY collection_id, name
            ORDER BY name, collection_id
            "#,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to list legacy TMDB collections: {e}"
            ))
        })?;
        let mut collections = Vec::with_capacity(rows.len());
        for row in rows {
            let item_count = u32::try_from(
                row.try_get::<i64, _>("item_count").map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to decode TMDB collection item count: {e}"
                    ))
                })?,
            )
            .map_err(|_| {
                MediaError::Internal(
                    "TMDB collection item count exceeds u32".to_string(),
                )
            })?;
            collections.push(TmdbCollectionSummary {
                tmdb_id: row
                    .try_get::<i64, _>("collection_id")
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "failed to decode TMDB collection id: {e}"
                        ))
                    })?
                    .to_string(),
                title: row.try_get("name").map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to decode TMDB collection title: {e}"
                    ))
                })?,
                description: None,
                import_kind: request
                    .import_kind
                    .unwrap_or(TmdbCollectionImportKind::Collection),
                poster_path: row.try_get("poster_path").map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to decode TMDB collection poster path: {e}"
                    ))
                })?,
                item_count,
            });
        }
        let total = collections.len();
        Ok(TmdbListCollectionsResponse {
            collections: collections
                .into_iter()
                .skip(offset)
                .take(limit as usize)
                .collect(),
            page: page_info_for_slice(offset, limit, total),
        })
    }

    async fn resolve_collection_items(
        &self,
        items: &[CollectionItemIdentity],
    ) -> Result<Vec<CollectionResolvedItem>> {
        let mut movie_ids = Vec::new();
        let mut series_ids = Vec::new();
        let mut season_ids = Vec::new();
        let mut episode_ids = Vec::new();
        for item in items {
            match item.media_id {
                MediaID::Movie(id) => movie_ids.push(*id.as_uuid()),
                MediaID::Series(id) => series_ids.push(*id.as_uuid()),
                MediaID::Season(id) => season_ids.push(*id.as_uuid()),
                MediaID::Episode(id) => episode_ids.push(*id.as_uuid()),
            }
        }

        let mut resolved = HashMap::new();
        self.resolve_movie_items(&movie_ids, &mut resolved).await?;
        self.resolve_series_items(&series_ids, &mut resolved)
            .await?;
        self.resolve_season_items(&season_ids, &mut resolved)
            .await?;
        self.resolve_episode_items(&episode_ids, &mut resolved)
            .await?;

        let checked_at = Utc::now();
        Ok(items
            .iter()
            .map(|item| {
                resolved.get(&item.media_id).cloned().unwrap_or_else(|| {
                    CollectionResolvedItem {
                        item_key: item.item_key.clone(),
                        media_id: item.media_id,
                        title: None,
                        subtitle: None,
                        availability: CollectionMemberAvailability {
                            status: CollectionMemberAvailabilityStatus::Missing,
                            reason: Some(
                                "media reference was not found".to_string(),
                            ),
                            checked_at: Some(checked_at),
                        },
                    }
                })
            })
            .collect())
    }

    async fn ensure_system_collections(
        &self,
        definitions: &[SystemCollectionDefinition],
    ) -> Result<SystemCollectionSeedReport> {
        for definition in definitions {
            let report = definition.rule.validation_report();
            if !report.valid {
                return Err(MediaError::InvalidMedia(format!(
                    "invalid system collection rule for {}: {}",
                    definition.stable_key,
                    report
                        .errors
                        .iter()
                        .map(|error| format!(
                            "{}: {}",
                            error.path, error.message
                        ))
                        .collect::<Vec<_>>()
                        .join("; ")
                )));
            }

            let id =
                self.upsert_system_collection_definition(definition).await?;
            self.upsert_rule(id, &definition.rule).await?;
            self.upsert_system_shelf_placement(id, definition).await?;
        }

        Ok(SystemCollectionSeedReport {
            requested: definitions.len(),
            upserted: definitions.len(),
        })
    }

    async fn mark_system_collections_stale(
        &self,
        request: MarkSystemCollectionsStaleRequest,
    ) -> Result<SystemCollectionsStaleResponse> {
        let library_id = request.library_id.map(|id| id.to_uuid());
        let result = sqlx::query(
            r#"
            UPDATE collection_materializations cm
            SET
                state = 'stale',
                stale_at = NOW(),
                stale_reason = $1,
                updated_at = NOW()
            FROM collection_definitions cd
            WHERE cm.collection_id = cd.id
              AND cd.kind = 'system'
              AND cd.source = 'system'
              AND cd.deleted_at IS NULL
              AND cm.state <> 'stale'
              AND ($2::uuid IS NULL OR cd.library_id IS NULL OR cd.library_id = $2)
              AND (
                    $3::text <> 'watch_state'
                    OR $4::uuid IS NULL
                    OR cm.user_id = $4
                  )
            "#,
        )
        .bind(request.cause.stale_reason())
        .bind(library_id)
        .bind(request.cause.as_str())
        .bind(request.user_id)
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to mark system collections stale: {e}"
            ))
        })?;

        Ok(SystemCollectionsStaleResponse {
            materializations_marked_stale: result.rows_affected(),
        })
    }
}

#[cfg(test)]
mod tests {
    use ferrex_model::LibraryId;

    use super::*;

    #[test]
    fn availability_status_strings_cover_tombstoned_members() {
        assert_eq!(
            PostgresCollectionRepository::encode_availability_status(
                CollectionMemberAvailabilityStatus::Tombstoned,
            ),
            "tombstoned"
        );
        assert_eq!(
            PostgresCollectionRepository::decode_availability_status(
                "tombstoned"
            )
            .unwrap(),
            CollectionMemberAvailabilityStatus::Tombstoned
        );
    }

    #[test]
    fn library_scope_requires_library_media_scope() {
        let result = PostgresCollectionRepository::library_id_for_scope(
            CollectionScope::Library,
            &CollectionMediaScope::All,
        );
        assert!(matches!(result, Err(MediaError::InvalidMedia(_))));

        let library_id = LibraryId(Uuid::now_v7());
        let result = PostgresCollectionRepository::library_id_for_scope(
            CollectionScope::Library,
            &CollectionMediaScope::Library {
                library_id,
                media_types: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(result, Some(library_id.to_uuid()));
    }

    #[test]
    fn rule_hash_is_stable() {
        let rule = DynamicCollectionRule::default();
        assert_eq!(
            PostgresCollectionRepository::rule_hash(&rule).unwrap(),
            PostgresCollectionRepository::rule_hash(&rule).unwrap()
        );
    }
}
