use chrono::{DateTime, Utc};
use ferrex_model::{LibraryId, MediaID, VideoMediaType};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

pub const COLLECTION_CONTRACT_VERSION: u16 = 1;
pub const COLLECTION_RULE_SCHEMA_VERSION: u16 = 1;
pub const COLLECTION_SORT_SCHEMA_VERSION: u16 = 1;
pub const COLLECTION_LIMIT_SCHEMA_VERSION: u16 = 1;
pub const COLLECTION_MATERIALIZATION_SCHEMA_VERSION: u16 = 1;
pub const SHELF_PLACEMENT_SCHEMA_VERSION: u16 = 1;
pub const DEFAULT_COLLECTION_PAGE_LIMIT: u16 = 50;
pub const MAX_COLLECTION_PAGE_LIMIT: u16 = 250;

fn collection_contract_version() -> u16 {
    COLLECTION_CONTRACT_VERSION
}

fn collection_rule_schema_version() -> u16 {
    COLLECTION_RULE_SCHEMA_VERSION
}

fn collection_sort_schema_version() -> u16 {
    COLLECTION_SORT_SCHEMA_VERSION
}

fn collection_limit_schema_version() -> u16 {
    COLLECTION_LIMIT_SCHEMA_VERSION
}

fn collection_materialization_schema_version() -> u16 {
    COLLECTION_MATERIALIZATION_SCHEMA_VERSION
}

fn shelf_placement_schema_version() -> u16 {
    SHELF_PLACEMENT_SCHEMA_VERSION
}

fn default_collection_page_limit() -> u16 {
    DEFAULT_COLLECTION_PAGE_LIMIT
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[serde(transparent)]
pub struct CollectionId(pub Uuid);

impl CollectionId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn to_uuid(self) -> Uuid {
        self.0
    }

    pub fn stable_key(self) -> String {
        format!("collection:{}", self.0)
    }
}

impl Default for CollectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for CollectionId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl fmt::Display for CollectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[serde(transparent)]
pub struct ShelfPlacementId(pub Uuid);

impl ShelfPlacementId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn to_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for ShelfPlacementId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for ShelfPlacementId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl fmt::Display for ShelfPlacementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(transparent)]
pub struct CollectionMemberKey(String);

impl CollectionMemberKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn for_media(media_id: &MediaID) -> Self {
        Self(collection_media_stable_key(media_id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for CollectionMemberKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for CollectionMemberKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for CollectionMemberKey {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

pub fn collection_media_stable_key(media_id: &MediaID) -> String {
    let media_kind = CollectionMediaKind::from(media_id);
    format!("{}:{}", media_kind.as_slug(), media_id.as_uuid())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionIdentity {
    pub id: CollectionId,
    pub stable_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_key: Option<String>,
}

impl CollectionIdentity {
    pub fn for_id(id: CollectionId) -> Self {
        Self {
            id,
            stable_key: id.stable_key(),
            external_key: None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionKind {
    #[default]
    Manual,
    DynamicRule,
    TmdbList,
    TmdbCollection,
    System,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionSource {
    #[default]
    Manual,
    DynamicRule,
    Tmdb,
    System,
    Imported,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionOwnerType {
    User,
    Device,
    External,
    #[default]
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionOwner {
    pub owner_type: CollectionOwnerType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

impl Default for CollectionOwner {
    fn default() -> Self {
        Self {
            owner_type: CollectionOwnerType::System,
            user_id: None,
            device_id: None,
            display_name: None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionScope {
    #[default]
    User,
    Global,
    Library,
    Shared,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionVisibility {
    #[default]
    Private,
    Shared,
    Public,
    System,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionPresentationMode {
    #[default]
    Shelf,
    Grid,
    List,
    Playlist,
    Hero,
    Hidden,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionMediaKind {
    #[default]
    Movie,
    Series,
    Season,
    Episode,
}

impl CollectionMediaKind {
    pub const fn as_slug(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Series => "series",
            Self::Season => "season",
            Self::Episode => "episode",
        }
    }
}

impl From<&MediaID> for CollectionMediaKind {
    fn from(value: &MediaID) -> Self {
        match value {
            MediaID::Movie(_) => Self::Movie,
            MediaID::Series(_) => Self::Series,
            MediaID::Season(_) => Self::Season,
            MediaID::Episode(_) => Self::Episode,
        }
    }
}

impl From<VideoMediaType> for CollectionMediaKind {
    fn from(value: VideoMediaType) -> Self {
        match value {
            VideoMediaType::Movie => Self::Movie,
            VideoMediaType::Series => Self::Series,
            VideoMediaType::Season => Self::Season,
            VideoMediaType::Episode => Self::Episode,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CollectionMediaScope {
    #[default]
    All,
    Types {
        media_types: Vec<CollectionMediaKind>,
    },
    Library {
        library_id: LibraryId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        media_types: Vec<CollectionMediaKind>,
    },
    ExplicitItems {
        item_keys: Vec<CollectionMemberKey>,
    },
}

impl CollectionMediaScope {
    pub fn allows_media(&self, media_id: &MediaID) -> bool {
        let item_key = CollectionMemberKey::for_media(media_id);
        self.allows_member(&item_key, CollectionMediaKind::from(media_id))
    }

    fn allows_member(
        &self,
        item_key: &CollectionMemberKey,
        media_kind: CollectionMediaKind,
    ) -> bool {
        match self {
            Self::All => true,
            Self::Types { media_types } => media_types.contains(&media_kind),
            Self::Library { media_types, .. } => {
                media_types.is_empty() || media_types.contains(&media_kind)
            }
            Self::ExplicitItems { item_keys } => item_keys.contains(item_key),
        }
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionDuplicatePolicy {
    KeepAll,
    DeduplicateMedia,
    DeduplicateLogical,
    #[default]
    RejectDuplicates,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionArtworkSource {
    #[default]
    Manual,
    FirstMember,
    Tmdb,
    Generated,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CollectionArtwork {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_iid: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backdrop_iid: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_iid: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_color_hex: Option<String>,
    #[serde(default)]
    pub source: CollectionArtworkSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_image_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionThemeToken {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CollectionTheme {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_color_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_color_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub prefer_backdrop: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<CollectionThemeToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionProvenance {
    pub source: CollectionSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<DateTime<Utc>>,
}

impl Default for CollectionProvenance {
    fn default() -> Self {
        Self {
            source: CollectionSource::Manual,
            imported_from: None,
            external_id: None,
            generated_by: None,
            rule_hash: None,
            last_refreshed_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionVersion {
    #[serde(default = "collection_contract_version")]
    pub contract_version: u16,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
}

impl Default for CollectionVersion {
    fn default() -> Self {
        Self {
            contract_version: COLLECTION_CONTRACT_VERSION,
            revision: 0,
            etag: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionTimestamps {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionMemberAvailabilityStatus {
    #[default]
    Available,
    Pending,
    Missing,
    Unavailable,
    Tombstoned,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionMemberAvailability {
    pub status: CollectionMemberAvailabilityStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<DateTime<Utc>>,
}

impl Default for CollectionMemberAvailability {
    fn default() -> Self {
        Self {
            status: CollectionMemberAvailabilityStatus::Available,
            reason: None,
            checked_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionMember {
    pub item_key: CollectionMemberKey,
    pub media_id: MediaID,
    pub media_type: CollectionMediaKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub position: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_key: Option<String>,
    #[serde(default)]
    pub availability: CollectionMemberAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_by: Option<Uuid>,
}

impl CollectionMember {
    pub fn new(
        media_id: MediaID,
        title: impl Into<String>,
        position: u32,
    ) -> Self {
        Self {
            item_key: CollectionMemberKey::for_media(&media_id),
            media_type: CollectionMediaKind::from(&media_id),
            media_id,
            title: title.into(),
            subtitle: None,
            position,
            sort_key: None,
            availability: CollectionMemberAvailability::default(),
            added_at: None,
            added_by: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionSummary {
    pub identity: CollectionIdentity,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub kind: CollectionKind,
    pub source: CollectionSource,
    pub owner: CollectionOwner,
    pub scope: CollectionScope,
    pub visibility: CollectionVisibility,
    pub presentation: CollectionPresentationMode,
    pub media_scope: CollectionMediaScope,
    pub duplicate_policy: CollectionDuplicatePolicy,
    #[serde(default)]
    pub artwork: CollectionArtwork,
    #[serde(default)]
    pub theme: CollectionTheme,
    #[serde(default)]
    pub provenance: CollectionProvenance,
    #[serde(default)]
    pub version: CollectionVersion,
    pub timestamps: CollectionTimestamps,
    #[serde(default)]
    pub item_count: u32,
    #[serde(default)]
    pub materialization: CollectionMaterializationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionDetail {
    pub summary: CollectionSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<DynamicCollectionRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items_preview: Vec<CollectionMember>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shelf_placements: Vec<ShelfPlacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionPagination {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default = "default_collection_page_limit")]
    pub limit: u16,
}

impl Default for CollectionPagination {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: DEFAULT_COLLECTION_PAGE_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionPageInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default = "default_collection_page_limit")]
    pub limit: u16,
    #[serde(default)]
    pub total: u64,
}

impl Default for CollectionPageInfo {
    fn default() -> Self {
        Self {
            next_cursor: None,
            limit: DEFAULT_COLLECTION_PAGE_LIMIT,
            total: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ListCollectionsRequest {
    #[serde(default)]
    pub page: CollectionPagination,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<CollectionKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<CollectionScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<CollectionVisibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<CollectionMediaKind>,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub include_item_counts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListCollectionsResponse {
    pub collections: Vec<CollectionSummary>,
    pub page: CollectionPageInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GetCollectionDetailRequest {
    #[serde(default)]
    pub include_rule: bool,
    #[serde(default)]
    pub include_items_preview: bool,
    #[serde(default)]
    pub include_shelf_placements: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GetCollectionDetailResponse {
    pub collection: CollectionDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ListCollectionItemsRequest {
    #[serde(default)]
    pub page: CollectionPagination,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<CollectionMemberAvailabilityStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListCollectionItemsResponse {
    pub collection_id: CollectionId,
    pub items: Vec<CollectionMember>,
    pub page: CollectionPageInfo,
    pub materialization: CollectionMaterializationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateCollectionRequest {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub kind: CollectionKind,
    #[serde(default)]
    pub source: CollectionSource,
    #[serde(default)]
    pub owner: CollectionOwner,
    #[serde(default)]
    pub scope: CollectionScope,
    #[serde(default)]
    pub visibility: CollectionVisibility,
    #[serde(default)]
    pub presentation: CollectionPresentationMode,
    #[serde(default)]
    pub media_scope: CollectionMediaScope,
    #[serde(default)]
    pub duplicate_policy: CollectionDuplicatePolicy,
    #[serde(default)]
    pub artwork: CollectionArtwork,
    #[serde(default)]
    pub theme: CollectionTheme,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<CollectionProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<DynamicCollectionRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateCollectionResponse {
    pub collection: CollectionDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UpdateCollectionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<CollectionVisibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<CollectionPresentationMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_scope: Option<CollectionMediaScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_policy: Option<CollectionDuplicatePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork: Option<CollectionArtwork>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<CollectionTheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<DynamicCollectionRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateCollectionResponse {
    pub collection: CollectionDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveCollectionRequest {
    #[serde(default = "default_archive_collection")]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

impl Default for ArchiveCollectionRequest {
    fn default() -> Self {
        Self {
            archived: true,
            reason: None,
            expected_revision: None,
        }
    }
}

fn default_archive_collection() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveCollectionResponse {
    pub collection_id: CollectionId,
    pub archived_at: Option<DateTime<Utc>>,
    pub version: CollectionVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionManualAddItem {
    pub media_id: MediaID,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ManualAddCollectionItemsRequest {
    pub items: Vec<CollectionManualAddItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_policy: Option<CollectionDuplicatePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CollectionManualMembershipConflictCode {
    DuplicateMember,
    MissingMember,
    UnsupportedDuplicatePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionManualMembershipConflict {
    pub code: CollectionManualMembershipConflictCode,
    pub collection_id: CollectionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_policy: Option<CollectionDuplicatePolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub item_keys: Vec<CollectionMemberKey>,
    pub message: String,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionManualAddStatus {
    #[default]
    Added,
    AlreadyPresent,
    DuplicateSkipped,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionManualAddResult {
    pub item_key: CollectionMemberKey,
    pub status: CollectionManualAddStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualAddCollectionItemsResponse {
    pub collection_id: CollectionId,
    pub results: Vec<CollectionManualAddResult>,
    pub version: CollectionVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ManualRemoveCollectionItemsRequest {
    pub item_keys: Vec<CollectionMemberKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualRemoveCollectionItemsResponse {
    pub collection_id: CollectionId,
    pub removed_item_keys: Vec<CollectionMemberKey>,
    pub missing_item_keys: Vec<CollectionMemberKey>,
    pub version: CollectionVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionManualOrder {
    pub item_key: CollectionMemberKey,
    pub position: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ManualReorderCollectionItemsRequest {
    pub ordering: Vec<CollectionManualOrder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualReorderCollectionItemsResponse {
    pub collection_id: CollectionId,
    pub version: CollectionVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DynamicCollectionRule {
    #[serde(default = "collection_rule_schema_version")]
    pub schema_version: u16,
    #[serde(default)]
    pub predicate: CollectionRulePredicate,
    #[serde(default)]
    pub sort: CollectionSortPolicy,
    #[serde(default)]
    pub limit: CollectionLimitPolicy,
}

impl Default for DynamicCollectionRule {
    fn default() -> Self {
        Self {
            schema_version: COLLECTION_RULE_SCHEMA_VERSION,
            predicate: CollectionRulePredicate::default(),
            sort: CollectionSortPolicy::default(),
            limit: CollectionLimitPolicy::default(),
        }
    }
}

impl DynamicCollectionRule {
    pub fn rule_hash_input_json(&self) -> Result<String, serde_json::Error> {
        let input = CollectionRuleHashInput {
            schema_version: self.schema_version,
            predicate: &self.predicate,
            sort: &self.sort,
            limit: &self.limit,
        };
        serde_json::to_string(&input)
    }
}

#[derive(Serialize)]
struct CollectionRuleHashInput<'a> {
    schema_version: u16,
    predicate: &'a CollectionRulePredicate,
    sort: &'a CollectionSortPolicy,
    limit: &'a CollectionLimitPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CollectionRulePredicate {
    All {
        clauses: Vec<CollectionRulePredicate>,
    },
    Any {
        clauses: Vec<CollectionRulePredicate>,
    },
    Not {
        clause: Box<CollectionRulePredicate>,
    },
    Field {
        field: CollectionRuleField,
        operator: CollectionRuleOperator,
        value: CollectionRuleValue,
    },
}

impl Default for CollectionRulePredicate {
    fn default() -> Self {
        Self::All {
            clauses: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CollectionRuleField {
    MediaType,
    LibraryId,
    Title,
    SortTitle,
    Genre,
    ReleaseYear,
    AddedAt,
    UpdatedAt,
    RuntimeMinutes,
    AudienceRating,
    CriticRating,
    WatchStatus,
    Availability,
    TmdbId,
    ActorName,
    DirectorName,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CollectionRuleOperator {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    In,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    Between,
    Exists,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum CollectionRuleValue {
    String(String),
    Strings(Vec<String>),
    Integer(i64),
    Integers(Vec<i64>),
    Decimal(String),
    Boolean(bool),
    Date(String),
    Uuid(Uuid),
    MediaType(CollectionMediaKind),
    Availability(CollectionMemberAvailabilityStatus),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionSortPolicy {
    #[serde(default = "collection_sort_schema_version")]
    pub schema_version: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<CollectionSortKey>,
    #[serde(default)]
    pub tie_breaker: CollectionSortTieBreaker,
}

impl Default for CollectionSortPolicy {
    fn default() -> Self {
        Self {
            schema_version: COLLECTION_SORT_SCHEMA_VERSION,
            keys: Vec::new(),
            tie_breaker: CollectionSortTieBreaker::StableMediaKey,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionSortKey {
    pub field: CollectionSortField,
    #[serde(default)]
    pub direction: CollectionSortDirection,
    #[serde(default)]
    pub nulls: CollectionSortNulls,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CollectionSortField {
    Title,
    SortTitle,
    ReleaseDate,
    AddedAt,
    UpdatedAt,
    RuntimeMinutes,
    AudienceRating,
    CriticRating,
    ManualPosition,
    RandomStable,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionSortDirection {
    #[default]
    Asc,
    Desc,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionSortNulls {
    First,
    #[default]
    Last,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionSortTieBreaker {
    #[default]
    StableMediaKey,
    TitleThenStableKey,
    ManualPositionThenStableKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionLimitPolicy {
    #[serde(default = "collection_limit_schema_version")]
    pub schema_version: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_media_type: Option<u32>,
    #[serde(default)]
    pub window: CollectionLimitWindow,
}

impl Default for CollectionLimitPolicy {
    fn default() -> Self {
        Self {
            schema_version: COLLECTION_LIMIT_SCHEMA_VERSION,
            max_items: None,
            per_media_type: None,
            window: CollectionLimitWindow::All,
        }
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionLimitWindow {
    #[default]
    All,
    Newest,
    Oldest,
    RecentlyAdded,
    RecentlyUpdated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionMaterializationStatus {
    #[serde(default = "collection_materialization_schema_version")]
    pub schema_version: u16,
    #[serde(default)]
    pub state: CollectionMaterializationState,
    #[serde(default)]
    pub item_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Default for CollectionMaterializationStatus {
    fn default() -> Self {
        Self {
            schema_version: COLLECTION_MATERIALIZATION_SCHEMA_VERSION,
            state: CollectionMaterializationState::NotMaterialized,
            item_count: 0,
            rule_hash: None,
            generated_at: None,
            expires_at: None,
            last_error: None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionMaterializationState {
    #[default]
    NotMaterialized,
    Pending,
    Refreshing,
    Ready,
    Stale,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidateCollectionRuleRequest {
    pub rule: DynamicCollectionRule,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionRuleValidationError {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidateCollectionRuleResponse {
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<CollectionRuleValidationError>,
    pub rule_hash_input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreviewCollectionRuleRequest {
    pub rule: DynamicCollectionRule,
    #[serde(default)]
    pub page: CollectionPagination,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreviewCollectionRuleResponse {
    pub items: Vec<CollectionMember>,
    pub page: CollectionPageInfo,
    pub materialization: CollectionMaterializationStatus,
    pub rule_hash_input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RefreshCollectionRuleRequest {
    #[serde(default)]
    pub force: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_rule_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RefreshCollectionRuleResponse {
    pub collection_id: CollectionId,
    pub materialization: CollectionMaterializationStatus,
    pub version: CollectionVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShelfPlacement {
    #[serde(default = "shelf_placement_schema_version")]
    pub schema_version: u16,
    pub id: ShelfPlacementId,
    pub collection_id: CollectionId,
    pub surface: ShelfSurface,
    pub shelf_key: String,
    pub position: u32,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub presentation: CollectionPresentationMode,
    #[serde(default)]
    pub visibility: CollectionVisibility,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum ShelfSurface {
    #[default]
    Home,
    Library,
    CollectionDetail,
    Search,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ListShelfPlacementsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<ShelfSurface>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shelf_key: Option<String>,
    #[serde(default)]
    pub include_unpinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListShelfPlacementsResponse {
    pub placements: Vec<ShelfPlacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinShelfPlacementRequest {
    pub collection_id: CollectionId,
    #[serde(default)]
    pub surface: ShelfSurface,
    pub shelf_key: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<CollectionPresentationMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinShelfPlacementResponse {
    pub placement: ShelfPlacement,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShelfPlacementOrder {
    pub placement_id: ShelfPlacementId,
    pub position: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReorderShelfPlacementsRequest {
    pub ordering: Vec<ShelfPlacementOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReorderShelfPlacementsResponse {
    pub placements: Vec<ShelfPlacement>,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum TmdbCollectionImportKind {
    #[default]
    List,
    Collection,
    Keyword,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TmdbImportCollectionRequest {
    pub tmdb_id: String,
    #[serde(default)]
    pub import_kind: TmdbCollectionImportKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_override: Option<String>,
    #[serde(default)]
    pub owner: CollectionOwner,
    #[serde(default)]
    pub visibility: CollectionVisibility,
    #[serde(default)]
    pub presentation: CollectionPresentationMode,
    #[serde(default)]
    pub duplicate_policy: CollectionDuplicatePolicy,
    #[serde(default)]
    pub media_scope: CollectionMediaScope,
    #[serde(default)]
    pub refresh_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TmdbImportCollectionResponse {
    pub collection: CollectionDetail,
    pub imported_items: u32,
    pub skipped_items: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TmdbListCollectionsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_kind: Option<TmdbCollectionImportKind>,
    #[serde(default)]
    pub page: CollectionPagination,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TmdbCollectionSummary {
    pub tmdb_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub import_kind: TmdbCollectionImportKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_path: Option<String>,
    #[serde(default)]
    pub item_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TmdbListCollectionsResponse {
    pub collections: Vec<TmdbCollectionSummary>,
    pub page: CollectionPageInfo,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_model::{MovieID, SeriesID};
    use serde_json::json;

    fn uuid(value: &str) -> Uuid {
        Uuid::parse_str(value).expect("valid uuid")
    }

    #[test]
    fn serde_names_and_defaults_are_stable() {
        assert_eq!(
            serde_json::to_string(&CollectionKind::DynamicRule).unwrap(),
            "\"dynamic_rule\""
        );
        assert_eq!(
            serde_json::to_string(&CollectionPresentationMode::Playlist)
                .unwrap(),
            "\"playlist\""
        );
        assert_eq!(
            serde_json::to_string(
                &CollectionMemberAvailabilityStatus::Tombstoned
            )
            .unwrap(),
            "\"tombstoned\""
        );

        let request: ListCollectionsRequest =
            serde_json::from_str("{}").expect("defaults decode");
        assert_eq!(request.page.limit, DEFAULT_COLLECTION_PAGE_LIMIT);
        assert!(!request.include_archived);
        assert!(!request.include_item_counts);

        let rule: DynamicCollectionRule =
            serde_json::from_str("{}").expect("rule defaults decode");
        assert_eq!(rule.schema_version, COLLECTION_RULE_SCHEMA_VERSION);
        assert_eq!(rule.predicate, CollectionRulePredicate::default());
        assert_eq!(rule.sort.schema_version, COLLECTION_SORT_SCHEMA_VERSION);
        assert_eq!(rule.limit.schema_version, COLLECTION_LIMIT_SCHEMA_VERSION);

        let materialization: CollectionMaterializationStatus =
            serde_json::from_str("{}")
                .expect("materialization defaults decode");
        assert_eq!(
            materialization.schema_version,
            COLLECTION_MATERIALIZATION_SCHEMA_VERSION
        );
        assert_eq!(
            materialization.state,
            CollectionMaterializationState::NotMaterialized
        );
    }

    #[test]
    fn media_item_keys_are_stable_across_media_types() {
        let movie_uuid = uuid("018f0c8a-2eab-7f03-a989-1fd8f8f03a11");
        let series_uuid = uuid("018f0c8a-2eab-7f03-a989-1fd8f8f03a12");
        let movie = MediaID::Movie(MovieID(movie_uuid));
        let series = MediaID::Series(SeriesID(series_uuid));

        assert_eq!(
            collection_media_stable_key(&movie),
            "movie:018f0c8a-2eab-7f03-a989-1fd8f8f03a11"
        );
        assert_eq!(
            CollectionMemberKey::for_media(&series).as_str(),
            "series:018f0c8a-2eab-7f03-a989-1fd8f8f03a12"
        );

        let member = CollectionMember::new(movie, "Arrival", 7);
        assert_eq!(
            member.item_key.as_str(),
            "movie:018f0c8a-2eab-7f03-a989-1fd8f8f03a11"
        );
        assert_eq!(member.media_type, CollectionMediaKind::Movie);
    }

    #[test]
    fn rule_hash_input_serialization_is_bounded_and_deterministic() {
        let rule = DynamicCollectionRule {
            schema_version: COLLECTION_RULE_SCHEMA_VERSION,
            predicate: CollectionRulePredicate::All {
                clauses: vec![
                    CollectionRulePredicate::Field {
                        field: CollectionRuleField::MediaType,
                        operator: CollectionRuleOperator::Equals,
                        value: CollectionRuleValue::MediaType(
                            CollectionMediaKind::Movie,
                        ),
                    },
                    CollectionRulePredicate::Field {
                        field: CollectionRuleField::ReleaseYear,
                        operator: CollectionRuleOperator::GreaterThanOrEqual,
                        value: CollectionRuleValue::Integer(1990),
                    },
                ],
            },
            sort: CollectionSortPolicy {
                schema_version: COLLECTION_SORT_SCHEMA_VERSION,
                keys: vec![CollectionSortKey {
                    field: CollectionSortField::ReleaseDate,
                    direction: CollectionSortDirection::Desc,
                    nulls: CollectionSortNulls::Last,
                }],
                tie_breaker: CollectionSortTieBreaker::StableMediaKey,
            },
            limit: CollectionLimitPolicy {
                schema_version: COLLECTION_LIMIT_SCHEMA_VERSION,
                max_items: Some(24),
                per_media_type: None,
                window: CollectionLimitWindow::Newest,
            },
        };

        let value: serde_json::Value = serde_json::from_str(
            &rule.rule_hash_input_json().expect("hash input json"),
        )
        .expect("hash input parses");

        assert_eq!(
            value,
            json!({
                "schema_version": 1,
                "predicate": {
                    "type": "all",
                    "clauses": [
                        {
                            "type": "field",
                            "field": "media_type",
                            "operator": "equals",
                            "value": {
                                "type": "media_type",
                                "value": "movie"
                            }
                        },
                        {
                            "type": "field",
                            "field": "release_year",
                            "operator": "greater_than_or_equal",
                            "value": {
                                "type": "integer",
                                "value": 1990
                            }
                        }
                    ]
                },
                "sort": {
                    "schema_version": 1,
                    "keys": [
                        {
                            "field": "release_date",
                            "direction": "desc",
                            "nulls": "last"
                        }
                    ],
                    "tie_breaker": "stable_media_key"
                },
                "limit": {
                    "schema_version": 1,
                    "max_items": 24,
                    "window": "newest"
                }
            })
        );
    }
}
