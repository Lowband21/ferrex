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
pub const COLLECTION_RULE_HASH_ALGORITHM: &str = "sha256";
pub const MAX_COLLECTION_RULE_DEPTH: usize = 12;
pub const MAX_COLLECTION_RULE_CLAUSES: usize = 64;
pub const MAX_COLLECTION_LIMIT_ITEMS: u32 = 10_000;
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

fn deserialize_u16_from_string_or_number<'de, D>(
    deserializer: D,
) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(u16),
    }

    match StringOrNumber::deserialize(deserializer)? {
        StringOrNumber::String(value) => {
            value.parse::<u16>().map_err(serde::de::Error::custom)
        }
        StringOrNumber::Number(value) => Ok(value),
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
    #[default]
    DeduplicateMedia,
    DeduplicateLogical,
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
    #[serde(
        default = "default_collection_page_limit",
        deserialize_with = "deserialize_u16_from_string_or_number"
    )]
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
    #[serde(default, flatten)]
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
    #[serde(default, flatten)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DeleteCollectionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeleteCollectionResponse {
    pub collection_id: CollectionId,
    pub deleted_at: DateTime<Utc>,
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
    pub fn normalized(&self) -> Self {
        super::collection_rule_validation::normalized_rule(self)
    }

    pub fn validation_report(&self) -> CollectionRuleValidationReport {
        super::collection_rule_validation::validate_rule(self)
    }

    pub fn validate_errors(&self) -> Vec<CollectionRuleValidationError> {
        self.validation_report().errors
    }

    pub fn rule_hash_input_json(&self) -> Result<String, serde_json::Error> {
        super::collection_rule_validation::rule_hash_input_json(self)
    }

    pub fn rule_hash(&self) -> Result<String, serde_json::Error> {
        super::collection_rule_validation::rule_hash(self)
    }

    pub fn summary(&self) -> String {
        super::collection_rule_validation::rule_summary(self)
    }

    pub fn uses_user_scoped_watch_data(&self) -> bool {
        !self.watch_user_ids().is_empty()
    }

    pub fn watch_user_ids(&self) -> Vec<Uuid> {
        super::collection_rule_validation::watch_user_ids(self)
    }
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
    Overview,
    SearchText,
    Genre,
    Keyword,
    Person,
    ReleaseYear,
    ReleaseDate,
    AddedAt,
    DiscoveredAt,
    CreatedAt,
    UpdatedAt,
    RuntimeMinutes,
    AudienceRating,
    CriticRating,
    UserRating,
    Rating,
    Popularity,
    ContentRating,
    WatchStatus,
    WatchProgress,
    Availability,
    TmdbId,
    ActorName,
    DirectorName,
    FileSizeBytes,
    BitrateKbps,
    ResolutionWidth,
    ResolutionHeight,
    VideoCodec,
    AudioCodec,
    AudioChannelCount,
    SubtitleLanguage,
    HasSubtitles,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CollectionRuleOperator {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    In,
    NotIn,
    ContainsAny,
    ContainsAll,
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
    Decimals(Vec<String>),
    Boolean(bool),
    Date(String),
    Dates(Vec<String>),
    Uuid(Uuid),
    Uuids(Vec<Uuid>),
    MediaType(CollectionMediaKind),
    MediaTypes(Vec<CollectionMediaKind>),
    Availability(CollectionMemberAvailabilityStatus),
    Person(CollectionPersonRuleValue),
    WatchStatus(CollectionWatchStatusRuleValue),
    WatchProgress(CollectionWatchProgressRuleValue),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionPersonRuleValue {
    pub role: CollectionPersonRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmdb_id: Option<i64>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionPersonRole {
    Actor,
    Director,
    Writer,
    Producer,
    Creator,
    Crew,
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionWatchStatusRuleValue {
    pub user_id: Uuid,
    pub statuses: Vec<CollectionWatchStatus>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionWatchStatus {
    Unwatched,
    InProgress,
    Watched,
    Completed,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionWatchProgressRuleValue {
    pub user_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_percent: Option<u8>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CollectionSortField {
    RecentlyAdded,
    RecentlyReleased,
    Title,
    SortTitle,
    ReleaseDate,
    AddedAt,
    DiscoveredAt,
    CreatedAt,
    UpdatedAt,
    RuntimeMinutes,
    AudienceRating,
    CriticRating,
    UserRating,
    Rating,
    Popularity,
    FileSizeBytes,
    BitrateKbps,
    ResolutionWidth,
    ResolutionHeight,
    LastWatchedAt,
    WatchProgress,
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
    RecentlyReleased,
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
    #[serde(default)]
    pub total_count: u32,
    #[serde(default)]
    pub visible_count: u32,
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
            total_count: 0,
            visible_count: 0,
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
pub struct CollectionRuleValidationReport {
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<CollectionRuleValidationError>,
    pub rule_hash_input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_hash: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub uses_user_scoped_watch_data: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch_user_ids: Vec<Uuid>,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionRuleValidationCode {
    #[default]
    InvalidRule,
    UnsupportedSchemaVersion,
    TooComplex,
    EmptyPredicate,
    UnsupportedField,
    UnsupportedOperator,
    InvalidValue,
    MissingUserScope,
    ConflictingUserScopes,
    NonDeterministicSort,
    InvalidLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionRuleValidationError {
    pub path: String,
    #[serde(default)]
    pub code: CollectionRuleValidationCode,
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
    pub summary: String,
    #[serde(default)]
    pub uses_user_scoped_watch_data: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch_user_ids: Vec<Uuid>,
}

impl ValidateCollectionRuleResponse {
    pub fn from_rule(rule: &DynamicCollectionRule) -> Self {
        rule.validation_report().into()
    }
}

impl From<CollectionRuleValidationReport> for ValidateCollectionRuleResponse {
    fn from(value: CollectionRuleValidationReport) -> Self {
        Self {
            valid: value.valid,
            errors: value.errors,
            rule_hash_input: value.rule_hash_input,
            rule_hash: value.rule_hash,
            summary: value.summary,
            uses_user_scoped_watch_data: value.uses_user_scoped_watch_data,
            watch_user_ids: value.watch_user_ids,
        }
    }
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
    #[serde(default, flatten)]
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

    fn sort_key(
        field: CollectionSortField,
        direction: CollectionSortDirection,
    ) -> CollectionSortKey {
        CollectionSortKey {
            field,
            direction,
            nulls: CollectionSortNulls::Last,
            user_id: None,
        }
    }

    fn media_type_predicate(
        kind: CollectionMediaKind,
    ) -> CollectionRulePredicate {
        CollectionRulePredicate::Field {
            field: CollectionRuleField::MediaType,
            operator: CollectionRuleOperator::Equals,
            value: CollectionRuleValue::MediaType(kind),
        }
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
    fn query_requests_round_trip_through_urlencoded_forms() {
        let collections = ListCollectionsRequest {
            page: CollectionPagination {
                cursor: Some("25".to_string()),
                limit: 10,
            },
            kind: Some(CollectionKind::Manual),
            media_type: Some(CollectionMediaKind::Movie),
            include_item_counts: true,
            ..ListCollectionsRequest::default()
        };
        let encoded = serde_urlencoded::to_string(&collections)
            .expect("collection list query serializes");
        assert!(encoded.contains("cursor=25"));
        assert!(encoded.contains("limit=10"));
        assert!(encoded.contains("kind=manual"));
        assert!(encoded.contains("media_type=movie"));
        let decoded: ListCollectionsRequest =
            serde_urlencoded::from_str(&encoded)
                .expect("collection list query parses");
        assert_eq!(decoded, collections);

        let items = ListCollectionItemsRequest {
            page: CollectionPagination {
                cursor: Some("1".to_string()),
                limit: 2,
            },
            availability: Some(CollectionMemberAvailabilityStatus::Available),
        };
        let encoded = serde_urlencoded::to_string(&items)
            .expect("collection items query serializes");
        assert!(encoded.contains("availability=available"));
        let decoded: ListCollectionItemsRequest =
            serde_urlencoded::from_str(&encoded)
                .expect("collection items query parses");
        assert_eq!(decoded, items);

        let tmdb = TmdbListCollectionsRequest {
            account_id: Some("account".to_string()),
            import_kind: Some(TmdbCollectionImportKind::Collection),
            page: CollectionPagination {
                cursor: Some("2".to_string()),
                limit: 3,
            },
        };
        let encoded = serde_urlencoded::to_string(&tmdb)
            .expect("tmdb collection query serializes");
        assert!(encoded.contains("import_kind=collection"));
        let decoded: TmdbListCollectionsRequest =
            serde_urlencoded::from_str(&encoded)
                .expect("tmdb collection query parses");
        assert_eq!(decoded, tmdb);
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
                    user_id: None,
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

    #[test]
    fn rule_dsl_serde_supports_mvp_shapes() {
        let user_id = uuid("018f0c8a-2eab-7f03-a989-1fd8f8f03a22");
        let rule: DynamicCollectionRule = serde_json::from_value(json!({
            "schema_version": 1,
            "predicate": {
                "type": "all",
                "clauses": [
                    {
                        "type": "field",
                        "field": "person",
                        "operator": "contains",
                        "value": {
                            "type": "person",
                            "value": {
                                "role": "actor",
                                "name": "Sigourney Weaver"
                            }
                        }
                    },
                    {
                        "type": "field",
                        "field": "watch_status",
                        "operator": "in",
                        "value": {
                            "type": "watch_status",
                            "value": {
                                "user_id": user_id,
                                "statuses": ["in_progress", "completed"]
                            }
                        }
                    }
                ]
            },
            "sort": {
                "schema_version": 1,
                "keys": [
                    {
                        "field": "watch_progress",
                        "direction": "desc",
                        "nulls": "last",
                        "user_id": user_id
                    }
                ],
                "tie_breaker": "stable_media_key"
            },
            "limit": {
                "schema_version": 1,
                "max_items": 25,
                "window": "all"
            }
        }))
        .expect("rule JSON decodes");

        let report = rule.validation_report();
        assert!(report.valid, "unexpected errors: {:?}", report.errors);
        assert!(report.uses_user_scoped_watch_data);

        let serialized = serde_json::to_value(rule).expect("rule encodes");
        assert_eq!(
            serialized["predicate"]["clauses"][0]["value"]["value"]["role"],
            "actor"
        );
        assert_eq!(serialized["sort"]["keys"][0]["field"], "watch_progress");
    }

    #[test]
    fn required_example_rules_validate() {
        let library_id = uuid("018f0c8a-2eab-7f03-a989-1fd8f8f03a20");

        let action_adventure_movies = DynamicCollectionRule {
            predicate: CollectionRulePredicate::All {
                clauses: vec![
                    media_type_predicate(CollectionMediaKind::Movie),
                    CollectionRulePredicate::Field {
                        field: CollectionRuleField::Genre,
                        operator: CollectionRuleOperator::ContainsAll,
                        value: CollectionRuleValue::Strings(vec![
                            "Action".to_string(),
                            "Adventure".to_string(),
                        ]),
                    },
                ],
            },
            sort: CollectionSortPolicy {
                keys: vec![sort_key(
                    CollectionSortField::Title,
                    CollectionSortDirection::Asc,
                )],
                ..CollectionSortPolicy::default()
            },
            ..DynamicCollectionRule::default()
        };

        let episodes_with_actor = DynamicCollectionRule {
            predicate: CollectionRulePredicate::All {
                clauses: vec![
                    media_type_predicate(CollectionMediaKind::Episode),
                    CollectionRulePredicate::Field {
                        field: CollectionRuleField::Person,
                        operator: CollectionRuleOperator::Contains,
                        value: CollectionRuleValue::Person(
                            CollectionPersonRuleValue {
                                role: CollectionPersonRole::Actor,
                                name: Some("Sigourney Weaver".to_string()),
                                tmdb_id: None,
                            },
                        ),
                    },
                ],
            },
            ..DynamicCollectionRule::default()
        };

        let last_100_recently_added_movies = DynamicCollectionRule {
            predicate: CollectionRulePredicate::All {
                clauses: vec![
                    CollectionRulePredicate::Field {
                        field: CollectionRuleField::LibraryId,
                        operator: CollectionRuleOperator::Equals,
                        value: CollectionRuleValue::Uuid(library_id),
                    },
                    media_type_predicate(CollectionMediaKind::Movie),
                ],
            },
            sort: CollectionSortPolicy {
                keys: vec![sort_key(
                    CollectionSortField::RecentlyAdded,
                    CollectionSortDirection::Desc,
                )],
                ..CollectionSortPolicy::default()
            },
            limit: CollectionLimitPolicy {
                max_items: Some(100),
                window: CollectionLimitWindow::RecentlyAdded,
                ..CollectionLimitPolicy::default()
            },
            ..DynamicCollectionRule::default()
        };

        let recently_released_limited = DynamicCollectionRule {
            sort: CollectionSortPolicy {
                keys: vec![sort_key(
                    CollectionSortField::RecentlyReleased,
                    CollectionSortDirection::Desc,
                )],
                ..CollectionSortPolicy::default()
            },
            limit: CollectionLimitPolicy {
                max_items: Some(12),
                window: CollectionLimitWindow::RecentlyReleased,
                ..CollectionLimitPolicy::default()
            },
            ..DynamicCollectionRule::default()
        };

        for rule in [
            action_adventure_movies,
            episodes_with_actor,
            last_100_recently_added_movies,
            recently_released_limited,
        ] {
            let report = rule.validation_report();
            assert!(report.valid, "unexpected errors: {:?}", report.errors);
            assert!(report.rule_hash.is_some());
            assert!(!report.summary.is_empty());
        }
    }

    #[test]
    fn validation_rejects_unsupported_and_ill_scoped_combinations() {
        let unscoped_watch_status = DynamicCollectionRule {
            predicate: CollectionRulePredicate::Field {
                field: CollectionRuleField::WatchStatus,
                operator: CollectionRuleOperator::Equals,
                value: CollectionRuleValue::String("watched".to_string()),
            },
            ..DynamicCollectionRule::default()
        };
        let report = unscoped_watch_status.validation_report();
        assert!(!report.valid);
        assert!(report.errors.iter().any(|error| {
            error.code == CollectionRuleValidationCode::MissingUserScope
                && error.path == "predicate.value"
        }));

        let unscoped_last_watched_sort = DynamicCollectionRule {
            sort: CollectionSortPolicy {
                keys: vec![sort_key(
                    CollectionSortField::LastWatchedAt,
                    CollectionSortDirection::Desc,
                )],
                ..CollectionSortPolicy::default()
            },
            ..DynamicCollectionRule::default()
        };
        let report = unscoped_last_watched_sort.validation_report();
        assert!(!report.valid);
        assert!(report.errors.iter().any(|error| {
            error.code == CollectionRuleValidationCode::MissingUserScope
                && error.path == "sort.keys[0].user_id"
        }));

        let manual_sort = DynamicCollectionRule {
            sort: CollectionSortPolicy {
                keys: vec![sort_key(
                    CollectionSortField::ManualPosition,
                    CollectionSortDirection::Asc,
                )],
                ..CollectionSortPolicy::default()
            },
            ..DynamicCollectionRule::default()
        };
        let report = manual_sort.validation_report();
        assert!(!report.valid);
        assert!(report.errors.iter().any(|error| {
            error.code == CollectionRuleValidationCode::UnsupportedField
        }));
    }

    #[test]
    fn user_scoped_watch_predicates_are_detected() {
        let user_id = uuid("018f0c8a-2eab-7f03-a989-1fd8f8f03a21");
        let rule = DynamicCollectionRule {
            predicate: CollectionRulePredicate::Field {
                field: CollectionRuleField::WatchStatus,
                operator: CollectionRuleOperator::In,
                value: CollectionRuleValue::WatchStatus(
                    CollectionWatchStatusRuleValue {
                        user_id,
                        statuses: vec![
                            CollectionWatchStatus::InProgress,
                            CollectionWatchStatus::Completed,
                        ],
                    },
                ),
            },
            sort: CollectionSortPolicy {
                keys: vec![CollectionSortKey {
                    field: CollectionSortField::WatchProgress,
                    direction: CollectionSortDirection::Desc,
                    nulls: CollectionSortNulls::Last,
                    user_id: Some(user_id),
                }],
                ..CollectionSortPolicy::default()
            },
            ..DynamicCollectionRule::default()
        };

        let report = rule.validation_report();
        assert!(report.valid, "unexpected errors: {:?}", report.errors);
        assert!(rule.uses_user_scoped_watch_data());
        assert_eq!(rule.watch_user_ids(), vec![user_id]);
        assert_eq!(report.watch_user_ids, vec![user_id]);
    }

    #[test]
    fn deterministic_hashes_use_normalized_predicates() {
        let action_first = DynamicCollectionRule {
            predicate: CollectionRulePredicate::All {
                clauses: vec![
                    CollectionRulePredicate::Field {
                        field: CollectionRuleField::Genre,
                        operator: CollectionRuleOperator::ContainsAll,
                        value: CollectionRuleValue::Strings(vec![
                            "Adventure".to_string(),
                            " Action ".to_string(),
                        ]),
                    },
                    media_type_predicate(CollectionMediaKind::Movie),
                ],
            },
            ..DynamicCollectionRule::default()
        };
        let media_first = DynamicCollectionRule {
            predicate: CollectionRulePredicate::All {
                clauses: vec![
                    media_type_predicate(CollectionMediaKind::Movie),
                    CollectionRulePredicate::Field {
                        field: CollectionRuleField::Genre,
                        operator: CollectionRuleOperator::ContainsAll,
                        value: CollectionRuleValue::Strings(vec![
                            "action".to_string(),
                            "ADVENTURE".to_string(),
                        ]),
                    },
                ],
            },
            ..DynamicCollectionRule::default()
        };

        assert_eq!(
            action_first.rule_hash_input_json().unwrap(),
            media_first.rule_hash_input_json().unwrap()
        );
        assert_eq!(
            action_first.rule_hash().unwrap(),
            media_first.rule_hash().unwrap()
        );
    }

    #[test]
    fn validation_response_carries_summary_and_hash_input() {
        let rule = DynamicCollectionRule {
            sort: CollectionSortPolicy {
                keys: vec![sort_key(
                    CollectionSortField::Rating,
                    CollectionSortDirection::Desc,
                )],
                ..CollectionSortPolicy::default()
            },
            limit: CollectionLimitPolicy {
                max_items: Some(25),
                ..CollectionLimitPolicy::default()
            },
            ..DynamicCollectionRule::default()
        };

        let response = ValidateCollectionRuleResponse::from_rule(&rule);
        assert!(response.valid);
        assert!(response.rule_hash_input.contains("\"rating\""));
        assert!(
            response
                .rule_hash
                .as_deref()
                .unwrap()
                .starts_with("sha256:")
        );
        assert!(response.summary.contains("limit to 25 items"));
    }
}
