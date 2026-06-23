use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ferrex_model::{EpisodeID, MediaID, MovieID, SeasonID, SeriesID};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::types::collections::{
    ArchiveCollectionRequest, ArchiveCollectionResponse,
    COLLECTION_CONTRACT_VERSION, CollectionArtwork, CollectionDetail,
    CollectionDuplicatePolicy, CollectionId, CollectionIdentity,
    CollectionKind, CollectionManualAddItem, CollectionManualAddResult,
    CollectionManualAddStatus, CollectionManualMembershipConflictCode,
    CollectionManualOrder, CollectionMaterializationState,
    CollectionMaterializationStatus, CollectionMediaKind, CollectionMediaScope,
    CollectionMember, CollectionMemberAvailability,
    CollectionMemberAvailabilityStatus, CollectionMemberKey, CollectionOwner,
    CollectionOwnerType, CollectionPageInfo, CollectionPresentationMode,
    CollectionProvenance, CollectionScope, CollectionSource, CollectionSummary,
    CollectionTheme, CollectionTimestamps, CollectionVersion,
    CreateCollectionRequest, DynamicCollectionRule, GetCollectionDetailRequest,
    ListCollectionItemsRequest, ListCollectionItemsResponse,
    ListCollectionsRequest, ListCollectionsResponse,
    ManualAddCollectionItemsRequest, ManualAddCollectionItemsResponse,
    ManualRemoveCollectionItemsRequest, ManualRemoveCollectionItemsResponse,
    ManualReorderCollectionItemsRequest, ManualReorderCollectionItemsResponse,
    ShelfPlacement, ShelfPlacementId, ShelfSurface, UpdateCollectionRequest,
};
use crate::database::repository_ports::collections::{
    CollectionItemIdentity, CollectionReadMode, CollectionRepository,
    CollectionResolvedItem, clamp_collection_page_limit,
    collection_manual_membership_conflict, manual_position_key_for_index,
    page_info_for_slice, parse_collection_cursor,
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
    materialization_item_count: Option<i32>,
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
    materialization_item_count: Option<i32>,
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
struct ResolvedItemRow {
    id: Uuid,
    title: Option<String>,
    subtitle: Option<String>,
    status: String,
    reason: Option<String>,
}

#[derive(Debug, Clone)]
struct ManualAddCandidate {
    index: usize,
    item: CollectionManualAddItem,
    item_key: CollectionMemberKey,
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

    fn validate_manual_write_state(
        id: CollectionId,
        kind: CollectionKind,
        archived_at: Option<DateTime<Utc>>,
        current_revision: i64,
        expected_revision: Option<i64>,
    ) -> Result<()> {
        if kind != CollectionKind::Manual {
            return Err(MediaError::InvalidMedia(format!(
                "collection {id} is not a manual collection"
            )));
        }
        if archived_at.is_some() {
            return Err(MediaError::Conflict(format!(
                "collection {id} is archived"
            )));
        }
        if let Some(expected_revision) = expected_revision
            && expected_revision != current_revision
        {
            return Err(MediaError::Conflict(format!(
                "collection {id} revision conflict: expected {expected_revision}, current {current_revision}"
            )));
        }
        Ok(())
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
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
            row.materialization_item_count,
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
            row.materialization_item_count,
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
        materialization_item_count: Option<i32>,
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
            Some(state) => CollectionMaterializationStatus {
                state: Self::decode_materialization_state(&state)?,
                item_count: materialization_item_count
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(item_count),
                rule_hash: materialization_rule_hash,
                generated_at: materialization_generated_at,
                expires_at: materialization_expires_at,
                last_error: materialization_last_error,
                ..CollectionMaterializationStatus::default()
            },
            None => CollectionMaterializationStatus {
                item_count,
                ..CollectionMaterializationStatus::default()
            },
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
            item_count,
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
                cm.visible_count AS "materialization_item_count?",
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
                SELECT state, visible_count, rule_hash, evaluated_at, expires_at, error_message
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
        let input = rule.rule_hash_input_json().map_err(|e| {
            MediaError::Internal(format!(
                "failed to encode collection rule hash input: {e}"
            ))
        })?;
        Ok(Self::sha256_hex(input.as_bytes()))
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

    fn title_for_manual_add(
        item: &CollectionManualAddItem,
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
                    "manual reorder contains duplicate item keys".to_string(),
                ));
            }
            if !seen_positions.insert(order.position) {
                return Err(MediaError::InvalidMedia(
                    "manual reorder contains duplicate positions".to_string(),
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
                cm.visible_count AS "materialization_item_count?",
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
                SELECT state, visible_count, rule_hash, evaluated_at, expires_at, error_message
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
        let expected_revision =
            Self::expected_revision_i64(request.expected_revision)?;
        let requested_identities: Vec<_> = request
            .items
            .iter()
            .map(|item| CollectionItemIdentity::new(item.media_id))
            .collect();
        let resolved =
            self.resolve_collection_items(&requested_identities).await?;
        let resolved_by_key: HashMap<_, _> = resolved
            .into_iter()
            .map(|item| (item.item_key.clone(), item))
            .collect();
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to start manual collection add transaction: {e}"
            ))
        })?;

        let row = sqlx::query!(
            r#"
            SELECT
                kind::text AS "kind!",
                media_scope,
                duplicate_policy::text AS "duplicate_policy!",
                contract_version,
                revision,
                etag,
                archived_at
            FROM collection_definitions
            WHERE id = $1
              AND deleted_at IS NULL
            FOR UPDATE
            "#,
            id.to_uuid(),
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to lock collection {id} for manual add: {e}"
            ))
        })?;

        let Some(row) = row else {
            return Err(MediaError::NotFound(format!(
                "collection {id} not found"
            )));
        };
        let kind = Self::decode_kind(&row.kind)?;
        Self::validate_manual_write_state(
            id,
            kind,
            row.archived_at,
            row.revision,
            expected_revision,
        )?;
        let media_scope = Self::from_json::<CollectionMediaScope>(
            row.media_scope,
            "media scope",
        )?;
        let collection_duplicate_policy =
            Self::decode_duplicate_policy(&row.duplicate_policy)?;
        let duplicate_policy = request
            .duplicate_policy
            .unwrap_or(collection_duplicate_policy);
        let current_version = Self::version_from_parts(
            row.contract_version,
            row.revision,
            row.etag,
        )?;

        let existing_rows = sqlx::query!(
            r#"
            SELECT item_key
            FROM collection_manual_memberships
            WHERE collection_id = $1
            ORDER BY position_key, id
            FOR UPDATE
            "#,
            id.to_uuid(),
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to lock collection {id} manual members: {e}"
            ))
        })?;
        let existing_order: Vec<_> = existing_rows
            .iter()
            .map(|row| CollectionMemberKey::from(row.item_key.clone()))
            .collect();
        let existing_keys: HashSet<_> =
            existing_order.iter().cloned().collect();
        let mut seen_input = HashSet::new();
        let mut duplicate_keys = Vec::new();
        let mut result_slots = vec![None; request.items.len()];
        let mut candidates = Vec::new();

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

            candidates.push(ManualAddCandidate {
                index,
                item: item.clone(),
                item_key,
            });
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
            let code = if duplicate_policy == CollectionDuplicatePolicy::KeepAll
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
            .any(|candidate| candidate.item.position.is_some());
        let final_order = if has_positioned {
            let insertions: Vec<_> = candidates
                .iter()
                .map(|candidate| {
                    (
                        candidate.item_key.clone(),
                        candidate.item.position,
                        candidate.index,
                    )
                })
                .collect();
            Some(Self::final_order_with_insertions(
                &existing_order,
                &insertions,
            ))
        } else {
            None
        };
        let final_position_by_key: HashMap<_, _> = final_order
            .as_ref()
            .map(|order| {
                order
                    .iter()
                    .enumerate()
                    .map(|(index, key)| {
                        Ok((key.clone(), manual_position_key_for_index(index)?))
                    })
                    .collect::<Result<HashMap<_, _>>>()
            })
            .transpose()?
            .unwrap_or_default();

        if final_order.is_some() {
            sqlx::query!(
                r#"
                UPDATE collection_manual_memberships
                SET position_key = position_key + 1000000000000000000::numeric,
                    updated_at = NOW()
                WHERE collection_id = $1
                "#,
                id.to_uuid(),
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to reserve collection {id} manual order keys: {e}"
                ))
            })?;
        }

        let max_row = if final_order.is_none() {
            Some(
                sqlx::query!(
                    r#"
                    SELECT COALESCE(CEIL(MAX(position_key)), 0)::bigint AS "max_position_key!"
                    FROM collection_manual_memberships
                    WHERE collection_id = $1
                    "#,
                    id.to_uuid(),
                )
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to load collection {id} manual order tail: {e}"
                    ))
                })?,
            )
        } else {
            None
        };
        let mut next_position_key = max_row
            .map(|row| {
                u64::try_from(row.max_position_key)
                    .map_err(|_| {
                        MediaError::Internal(
                            "manual collection order key is negative"
                                .to_string(),
                        )
                    })?
                    .checked_add(1000)
                    .ok_or_else(|| {
                        MediaError::InvalidMedia(
                            "manual collection order key exceeds u64"
                                .to_string(),
                        )
                    })
            })
            .transpose()?
            .unwrap_or(1000);

        for candidate in candidates {
            let resolved = resolved_by_key
                .get(&candidate.item_key)
                .cloned()
                .unwrap_or(CollectionResolvedItem {
                    item_key: candidate.item_key.clone(),
                    media_id: candidate.item.media_id,
                    title: None,
                    subtitle: None,
                    availability: CollectionMemberAvailability {
                        status: CollectionMemberAvailabilityStatus::Missing,
                        reason: Some(
                            "media reference was not found".to_string(),
                        ),
                        checked_at: Some(Utc::now()),
                    },
                });
            let position_key = if let Some(position_key) =
                final_position_by_key.get(&candidate.item_key)
            {
                position_key.clone()
            } else {
                let position_key = next_position_key.to_string();
                next_position_key =
                    next_position_key.checked_add(1000).ok_or_else(|| {
                        MediaError::InvalidMedia(
                            "manual collection order key exceeds u64"
                                .to_string(),
                        )
                    })?;
                position_key
            };
            let media_kind = Self::media_kind_from_id(candidate.item.media_id);
            let media_type = Self::encode_media_kind(media_kind);
            let media_id = *candidate.item.media_id.as_uuid();
            let title_snapshot =
                Self::title_for_manual_add(&candidate.item, &resolved);
            let sort_key = Some(candidate.item_key.to_string());
            let availability_status =
                Self::encode_availability_status(resolved.availability.status);
            let availability_reason = resolved.availability.reason.clone();
            let availability_checked_at = resolved.availability.checked_at;

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
                    sort_key,
                    availability_status,
                    availability_reason,
                    availability_checked_at,
                    added_by
                ) VALUES (
                    $1, $2, ($3::text)::media_type, $4, $5, $6,
                    ($7::text)::numeric, $8, $9::varchar, $10, $11, $12
                )
                "#,
                id.to_uuid(),
                candidate.item_key.as_str(),
                media_type,
                media_id,
                title_snapshot,
                resolved.subtitle,
                position_key,
                sort_key,
                availability_status,
                availability_reason,
                availability_checked_at,
                added_by,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to add {item_key} to collection {id}: {e}",
                    item_key = candidate.item_key
                ))
            })?;

            result_slots[candidate.index] = Some(CollectionManualAddResult {
                item_key: candidate.item_key,
                status: CollectionManualAddStatus::Added,
                message: None,
            });
        }

        if let Some(final_order) = final_order.as_ref() {
            for (index, item_key) in final_order.iter().enumerate() {
                let position_key = manual_position_key_for_index(index)?;
                sqlx::query!(
                    r#"
                    UPDATE collection_manual_memberships
                    SET position_key = ($3::text)::numeric,
                        updated_at = NOW()
                    WHERE collection_id = $1
                      AND item_key = $2
                    "#,
                    id.to_uuid(),
                    item_key.as_str(),
                    position_key,
                )
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to persist collection {id} manual order: {e}"
                    ))
                })?;
            }
        }

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
            let version_row = sqlx::query!(
                r#"
                UPDATE collection_definitions
                SET revision = revision + 1,
                    etag = concat('collection:', id::text, ':v', revision + 1),
                    updated_at = NOW()
                WHERE id = $1
                RETURNING contract_version, revision, etag
                "#,
                id.to_uuid(),
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to bump collection {id} revision after manual add: {e}"
                ))
            })?;
            Self::version_from_parts(
                version_row.contract_version,
                version_row.revision,
                version_row.etag,
            )?
        } else {
            current_version
        };

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to commit manual collection add for {id}: {e}"
            ))
        })?;

        Ok(ManualAddCollectionItemsResponse {
            collection_id: id,
            results: result_slots
                .into_iter()
                .map(|result| {
                    result.ok_or_else(|| {
                        MediaError::Internal(
                            "manual add result was not recorded".to_string(),
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
        let expected_revision =
            Self::expected_revision_i64(request.expected_revision)?;
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to start manual collection remove transaction: {e}"
            ))
        })?;
        let row = sqlx::query!(
            r#"
            SELECT
                kind::text AS "kind!",
                contract_version,
                revision,
                etag,
                archived_at
            FROM collection_definitions
            WHERE id = $1
              AND deleted_at IS NULL
            FOR UPDATE
            "#,
            id.to_uuid(),
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to lock collection {id} for manual remove: {e}"
            ))
        })?;
        let Some(row) = row else {
            return Err(MediaError::NotFound(format!(
                "collection {id} not found"
            )));
        };
        let kind = Self::decode_kind(&row.kind)?;
        Self::validate_manual_write_state(
            id,
            kind,
            row.archived_at,
            row.revision,
            expected_revision,
        )?;
        let current_version = Self::version_from_parts(
            row.contract_version,
            row.revision,
            row.etag,
        )?;

        let mut seen = HashSet::new();
        let requested: Vec<String> = request
            .item_keys
            .iter()
            .filter(|key| seen.insert((*key).clone()))
            .map(|key| key.to_string())
            .collect();
        if requested.is_empty() {
            tx.commit().await.map_err(|e| {
                MediaError::Internal(format!(
                    "failed to commit empty manual collection remove for {id}: {e}"
                ))
            })?;
            return Ok(ManualRemoveCollectionItemsResponse {
                collection_id: id,
                removed_item_keys: Vec::new(),
                missing_item_keys: Vec::new(),
                version: current_version,
            });
        }

        let removed_rows = sqlx::query!(
            r#"
            DELETE FROM collection_manual_memberships
            WHERE collection_id = $1
              AND item_key = ANY($2::text[])
            RETURNING item_key
            "#,
            id.to_uuid(),
            &requested,
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to remove manual members from collection {id}: {e}"
            ))
        })?;
        let mut removed_item_keys: Vec<_> = removed_rows
            .into_iter()
            .map(|row| CollectionMemberKey::from(row.item_key))
            .collect();
        removed_item_keys.sort();
        let removed_set: HashSet<_> = removed_item_keys
            .iter()
            .map(|key| key.as_str().to_string())
            .collect();
        let mut missing_item_keys: Vec<_> = requested
            .into_iter()
            .filter(|key| !removed_set.contains(key))
            .map(CollectionMemberKey::from)
            .collect();
        missing_item_keys.sort();

        let version = if removed_item_keys.is_empty() {
            current_version
        } else {
            let version_row = sqlx::query!(
                r#"
                UPDATE collection_definitions
                SET revision = revision + 1,
                    etag = concat('collection:', id::text, ':v', revision + 1),
                    updated_at = NOW()
                WHERE id = $1
                RETURNING contract_version, revision, etag
                "#,
                id.to_uuid(),
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to bump collection {id} revision after manual remove: {e}"
                ))
            })?;
            Self::version_from_parts(
                version_row.contract_version,
                version_row.revision,
                version_row.etag,
            )?
        };
        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to commit manual collection remove for {id}: {e}"
            ))
        })?;

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
        let expected_revision =
            Self::expected_revision_i64(request.expected_revision)?;
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to start manual collection reorder transaction: {e}"
            ))
        })?;
        let row = sqlx::query!(
            r#"
            SELECT
                kind::text AS "kind!",
                contract_version,
                revision,
                etag,
                archived_at
            FROM collection_definitions
            WHERE id = $1
              AND deleted_at IS NULL
            FOR UPDATE
            "#,
            id.to_uuid(),
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to lock collection {id} for manual reorder: {e}"
            ))
        })?;
        let Some(row) = row else {
            return Err(MediaError::NotFound(format!(
                "collection {id} not found"
            )));
        };
        let kind = Self::decode_kind(&row.kind)?;
        Self::validate_manual_write_state(
            id,
            kind,
            row.archived_at,
            row.revision,
            expected_revision,
        )?;
        let current_version = Self::version_from_parts(
            row.contract_version,
            row.revision,
            row.etag,
        )?;
        if request.ordering.is_empty() {
            tx.commit().await.map_err(|e| {
                MediaError::Internal(format!(
                    "failed to commit empty manual collection reorder for {id}: {e}"
                ))
            })?;
            return Ok(ManualReorderCollectionItemsResponse {
                collection_id: id,
                version: current_version,
            });
        }

        let existing_rows = sqlx::query!(
            r#"
            SELECT item_key
            FROM collection_manual_memberships
            WHERE collection_id = $1
            ORDER BY position_key, id
            FOR UPDATE
            "#,
            id.to_uuid(),
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to lock collection {id} manual members for reorder: {e}"
            ))
        })?;
        let existing_order: Vec<_> = existing_rows
            .into_iter()
            .map(|row| CollectionMemberKey::from(row.item_key))
            .collect();
        let final_order = Self::final_order_with_reorder(
            id,
            &existing_order,
            &request.ordering,
        )?;
        let changed = final_order != existing_order;
        let version = if changed {
            sqlx::query!(
                r#"
                UPDATE collection_manual_memberships
                SET position_key = position_key + 1000000000000000000::numeric,
                    updated_at = NOW()
                WHERE collection_id = $1
                "#,
                id.to_uuid(),
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to reserve collection {id} manual reorder keys: {e}"
                ))
            })?;
            for (index, item_key) in final_order.iter().enumerate() {
                let position_key = manual_position_key_for_index(index)?;
                sqlx::query!(
                    r#"
                    UPDATE collection_manual_memberships
                    SET position_key = ($3::text)::numeric,
                        updated_at = NOW()
                    WHERE collection_id = $1
                      AND item_key = $2
                    "#,
                    id.to_uuid(),
                    item_key.as_str(),
                    position_key,
                )
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "failed to persist collection {id} manual reorder: {e}"
                    ))
                })?;
            }
            let version_row = sqlx::query!(
                r#"
                UPDATE collection_definitions
                SET revision = revision + 1,
                    etag = concat('collection:', id::text, ':v', revision + 1),
                    updated_at = NOW()
                WHERE id = $1
                RETURNING contract_version, revision, etag
                "#,
                id.to_uuid(),
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "failed to bump collection {id} revision after manual reorder: {e}"
                ))
            })?;
            Self::version_from_parts(
                version_row.contract_version,
                version_row.revision,
                version_row.etag,
            )?
        } else {
            current_version
        };
        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!(
                "failed to commit manual collection reorder for {id}: {e}"
            ))
        })?;
        Ok(ManualReorderCollectionItemsResponse {
            collection_id: id,
            version,
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
