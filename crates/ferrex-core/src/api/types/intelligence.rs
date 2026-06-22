//! Stable intelligence DTOs for planned context and runtime endpoints.
//!
//! These contracts intentionally expose compact summaries, stable media/artifact
//! identifiers, grounding references, runtime status/events, and audit envelopes.
//! They avoid raw provider metadata dumps so future handlers can evolve storage/model
//! internals without changing the external API boundary.

use ferrex_model::{LibraryId, MediaID};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// Default number of intelligence results returned by paginated endpoints.
pub const DEFAULT_INTELLIGENCE_PAGE_LIMIT: u16 = 20;
/// Maximum number of intelligence results returned by a single page.
pub const MAX_INTELLIGENCE_PAGE_LIMIT: u16 = 50;

/// Default number of candidate media results considered per request.
pub const DEFAULT_INTELLIGENCE_CANDIDATE_LIMIT: u16 = 12;
/// Maximum number of candidate media results considered per request.
pub const MAX_INTELLIGENCE_CANDIDATE_LIMIT: u16 = 50;

/// Default number of artifact summaries returned with context payloads.
pub const DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT: u16 = 8;
/// Maximum number of artifact summaries returned with context payloads.
pub const MAX_INTELLIGENCE_ARTIFACT_LIMIT: u16 = 24;

/// Default number of related items returned with item context payloads.
pub const DEFAULT_INTELLIGENCE_RELATED_LIMIT: u16 = 8;
/// Maximum number of related items returned with item context payloads.
pub const MAX_INTELLIGENCE_RELATED_LIMIT: u16 = 24;

/// Default number of facet values returned per facet group.
pub const DEFAULT_INTELLIGENCE_FACET_LIMIT: u16 = 12;
/// Maximum number of facet values returned per facet group.
pub const MAX_INTELLIGENCE_FACET_LIMIT: u16 = 32;

/// Default number of grounding/provenance references retained in responses.
pub const DEFAULT_INTELLIGENCE_GROUNDING_LIMIT: u16 = 12;
/// Maximum number of grounding/provenance references retained in responses.
pub const MAX_INTELLIGENCE_GROUNDING_LIMIT: u16 = 48;

/// Default number of tool-call audit entries returned for a run.
pub const DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT: u16 = 16;
/// Maximum number of tool-call audit entries returned for a run.
pub const MAX_INTELLIGENCE_TOOL_CALL_LIMIT: u16 = 64;

/// Default maximum size for bounded summaries returned by intelligence DTOs.
pub const DEFAULT_INTELLIGENCE_SUMMARY_CHARS: u16 = 400;
/// Absolute maximum size for bounded summaries returned by intelligence DTOs.
pub const MAX_INTELLIGENCE_SUMMARY_CHARS: u16 = 800;

/// Clamp a user-provided page limit to the stable intelligence API bounds.
pub fn clamp_intelligence_page_limit(limit: u16) -> u16 {
    clamp_bounded_limit(
        limit,
        DEFAULT_INTELLIGENCE_PAGE_LIMIT,
        MAX_INTELLIGENCE_PAGE_LIMIT,
    )
}

/// Clamp a user-provided summary character budget to stable API bounds.
pub fn clamp_intelligence_summary_chars(max_chars: u16) -> u16 {
    clamp_bounded_limit(
        max_chars,
        DEFAULT_INTELLIGENCE_SUMMARY_CHARS,
        MAX_INTELLIGENCE_SUMMARY_CHARS,
    )
}

pub const fn default_intelligence_page_limit() -> u16 {
    DEFAULT_INTELLIGENCE_PAGE_LIMIT
}

pub const fn default_intelligence_candidate_limit() -> u16 {
    DEFAULT_INTELLIGENCE_CANDIDATE_LIMIT
}

pub const fn default_intelligence_artifact_limit() -> u16 {
    DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT
}

pub const fn default_intelligence_related_limit() -> u16 {
    DEFAULT_INTELLIGENCE_RELATED_LIMIT
}

pub const fn default_intelligence_facet_limit() -> u16 {
    DEFAULT_INTELLIGENCE_FACET_LIMIT
}

pub const fn default_intelligence_grounding_limit() -> u16 {
    DEFAULT_INTELLIGENCE_GROUNDING_LIMIT
}

pub const fn default_intelligence_tool_call_limit() -> u16 {
    DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT
}

pub const fn default_intelligence_summary_chars() -> u16 {
    DEFAULT_INTELLIGENCE_SUMMARY_CHARS
}

const fn clamp_bounded_limit(limit: u16, default: u16, max: u16) -> u16 {
    if limit == 0 {
        default
    } else if limit > max {
        max
    } else {
        limit
    }
}

fn deserialize_page_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    Ok(clamp_intelligence_page_limit(value))
}

fn deserialize_candidate_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    Ok(clamp_bounded_limit(
        value,
        DEFAULT_INTELLIGENCE_CANDIDATE_LIMIT,
        MAX_INTELLIGENCE_CANDIDATE_LIMIT,
    ))
}

fn deserialize_artifact_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    Ok(clamp_bounded_limit(
        value,
        DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT,
        MAX_INTELLIGENCE_ARTIFACT_LIMIT,
    ))
}

fn deserialize_related_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    Ok(clamp_bounded_limit(
        value,
        DEFAULT_INTELLIGENCE_RELATED_LIMIT,
        MAX_INTELLIGENCE_RELATED_LIMIT,
    ))
}

fn deserialize_facet_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    Ok(clamp_bounded_limit(
        value,
        DEFAULT_INTELLIGENCE_FACET_LIMIT,
        MAX_INTELLIGENCE_FACET_LIMIT,
    ))
}

fn deserialize_grounding_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    Ok(clamp_bounded_limit(
        value,
        DEFAULT_INTELLIGENCE_GROUNDING_LIMIT,
        MAX_INTELLIGENCE_GROUNDING_LIMIT,
    ))
}

fn deserialize_tool_call_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    Ok(clamp_bounded_limit(
        value,
        DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT,
        MAX_INTELLIGENCE_TOOL_CALL_LIMIT,
    ))
}

fn deserialize_summary_chars<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    Ok(clamp_intelligence_summary_chars(value))
}

/// Cursor pagination request shared by Phase 1 intelligence endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligencePagination {
    /// Opaque cursor returned by a previous page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Bounded page size. Zero deserializes to the default, oversized values to the max.
    #[serde(
        default = "default_intelligence_page_limit",
        deserialize_with = "deserialize_page_limit"
    )]
    pub limit: u16,
}

impl Default for IntelligencePagination {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: DEFAULT_INTELLIGENCE_PAGE_LIMIT,
        }
    }
}

impl IntelligencePagination {
    pub fn new(cursor: Option<String>, limit: u16) -> Self {
        Self {
            cursor,
            limit: clamp_intelligence_page_limit(limit),
        }
    }
}

/// Cursor pagination metadata returned by intelligence endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligencePageInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(
        default = "default_intelligence_page_limit",
        deserialize_with = "deserialize_page_limit"
    )]
    pub limit: u16,
    #[serde(default)]
    pub has_more: bool,
}

impl Default for IntelligencePageInfo {
    fn default() -> Self {
        Self {
            next_cursor: None,
            limit: DEFAULT_INTELLIGENCE_PAGE_LIMIT,
            has_more: false,
        }
    }
}

/// Request-scoped caps for bounded intelligence responses.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceCaps {
    #[serde(
        default = "default_intelligence_candidate_limit",
        deserialize_with = "deserialize_candidate_limit"
    )]
    pub candidate_limit: u16,
    #[serde(
        default = "default_intelligence_artifact_limit",
        deserialize_with = "deserialize_artifact_limit"
    )]
    pub artifact_limit: u16,
    #[serde(
        default = "default_intelligence_related_limit",
        deserialize_with = "deserialize_related_limit"
    )]
    pub related_limit: u16,
    #[serde(
        default = "default_intelligence_facet_limit",
        deserialize_with = "deserialize_facet_limit"
    )]
    pub facet_limit: u16,
    #[serde(
        default = "default_intelligence_grounding_limit",
        deserialize_with = "deserialize_grounding_limit"
    )]
    pub grounding_limit: u16,
    #[serde(
        default = "default_intelligence_tool_call_limit",
        deserialize_with = "deserialize_tool_call_limit"
    )]
    pub tool_call_limit: u16,
    #[serde(
        default = "default_intelligence_summary_chars",
        deserialize_with = "deserialize_summary_chars"
    )]
    pub summary_max_chars: u16,
}

impl Default for IntelligenceCaps {
    fn default() -> Self {
        Self {
            candidate_limit: DEFAULT_INTELLIGENCE_CANDIDATE_LIMIT,
            artifact_limit: DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT,
            related_limit: DEFAULT_INTELLIGENCE_RELATED_LIMIT,
            facet_limit: DEFAULT_INTELLIGENCE_FACET_LIMIT,
            grounding_limit: DEFAULT_INTELLIGENCE_GROUNDING_LIMIT,
            tool_call_limit: DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT,
            summary_max_chars: DEFAULT_INTELLIGENCE_SUMMARY_CHARS,
        }
    }
}

/// A compact, explicitly bounded text summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceSummary {
    /// The bounded summary text. DTO constructors clamp this to `max_chars`.
    pub text: String,
    /// Character budget used to produce `text`.
    #[serde(
        default = "default_intelligence_summary_chars",
        deserialize_with = "deserialize_summary_chars"
    )]
    pub max_chars: u16,
    /// Indicates whether a longer source summary was truncated.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

impl IntelligenceSummary {
    pub fn new(text: impl Into<String>) -> Self {
        Self::with_max_chars(text, DEFAULT_INTELLIGENCE_SUMMARY_CHARS)
    }

    pub fn with_max_chars(text: impl Into<String>, max_chars: u16) -> Self {
        let max_chars = clamp_intelligence_summary_chars(max_chars);
        let (text, truncated) = truncate_to_chars(text.into(), max_chars);
        Self {
            text,
            max_chars,
            truncated,
        }
    }
}

fn truncate_to_chars(text: String, max_chars: u16) -> (String, bool) {
    let max_chars = usize::from(max_chars);
    match text.char_indices().nth(max_chars) {
        Some((byte_index, _)) => (text[..byte_index].to_string(), true),
        None => (text, false),
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Stable media kind for intelligence JSON payloads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceMediaKind {
    Movie,
    Series,
    Season,
    Episode,
}

impl From<&MediaID> for IntelligenceMediaKind {
    fn from(value: &MediaID) -> Self {
        match value {
            MediaID::Movie(_) => Self::Movie,
            MediaID::Series(_) => Self::Series,
            MediaID::Season(_) => Self::Season,
            MediaID::Episode(_) => Self::Episode,
        }
    }
}

/// Lightweight media reference for intelligence contexts and candidates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceMediaRef {
    pub media_id: MediaID,
    pub media_kind: IntelligenceMediaKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_id: Option<LibraryId>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster_iid: Option<Uuid>,
    /// Artifact ids associated with this item; payloads remain out-of-band.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<Uuid>,
}

impl IntelligenceMediaRef {
    pub fn new(media_id: MediaID, title: impl Into<String>) -> Self {
        let media_kind = IntelligenceMediaKind::from(&media_id);
        Self {
            media_id,
            media_kind,
            library_id: None,
            title: title.into(),
            year: None,
            poster_iid: None,
            artifact_ids: Vec::new(),
        }
    }
}

/// Facets exposed by library overview and candidate endpoints.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceFacetKind {
    Genre,
    ReleaseDecade,
    RuntimeBucket,
    WatchState,
    Library,
    MediaKind,
    Language,
    ContentRating,
    Provider,
}

/// A single bounded facet value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceFacetValue {
    pub key: String,
    pub label: String,
    pub count: u64,
    /// Optional bounded sample of media ids behind this facet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sample_media_ids: Vec<MediaID>,
}

/// A bounded facet group.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceFacetGroup {
    pub kind: IntelligenceFacetKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<IntelligenceFacetValue>,
}

/// Aggregate counts suitable for overview prompts and client summaries.
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq,
)]
pub struct IntelligenceMediaCounts {
    #[serde(default)]
    pub movies: u64,
    #[serde(default)]
    pub series: u64,
    #[serde(default)]
    pub seasons: u64,
    #[serde(default)]
    pub episodes: u64,
    #[serde(default)]
    pub artifacts: u64,
}

/// Library overview summary for Phase 1 intelligence consumers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceLibraryOverview {
    pub library_id: LibraryId,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<IntelligenceSummary>,
    #[serde(default)]
    pub counts: IntelligenceMediaCounts,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facets: Vec<IntelligenceFacetGroup>,
    /// Artifact ids with richer offline summaries for this library.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<Uuid>,
}

/// Request for a bounded, high-level library intelligence overview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceLibraryOverviewRequest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_ids: Vec<LibraryId>,
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Response for library overview intelligence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceLibraryOverviewResponse {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<IntelligenceLibraryOverview>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facets: Vec<IntelligenceFacetGroup>,
    #[serde(default)]
    pub page: IntelligencePageInfo,
    #[serde(default)]
    pub caps: IntelligenceCaps,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at_epoch_seconds: Option<i64>,
}

/// Request for finding candidate media items to ground an intelligence task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceCandidateSearchRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_ids: Vec<LibraryId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_kinds: Vec<IntelligenceMediaKind>,
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
    #[serde(default)]
    pub include_artifacts: bool,
}

/// Candidate media item with bounded explanation and artifact links.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntelligenceCandidate {
    pub media: IntelligenceMediaRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<IntelligenceSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_reason: Option<IntelligenceSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grounding: Vec<IntelligenceGroundingRef>,
}

/// Response for candidate media search.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntelligenceCandidateSearchResponse {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<IntelligenceCandidate>,
    #[serde(default)]
    pub page: IntelligencePageInfo,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Artifact categories exposed at the intelligence contract boundary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceArtifactKind {
    Summary,
    EmbeddingChunk,
    TranscriptSegment,
    UserNote,
    GeneratedAnswer,
    Recommendation,
    AuditRecord,
}

/// Durable artifact lifecycle states exposed to runtime and draft readers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceArtifactStatus {
    Draft,
    Active,
    Stale,
    Superseded,
    Invalidated,
    Deleted,
    Failed,
}

impl IntelligenceArtifactStatus {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Stale => "stale",
            Self::Superseded => "superseded",
            Self::Invalidated => "invalidated",
            Self::Deleted => "deleted",
            Self::Failed => "failed",
        }
    }
}

/// Source edge kinds that can explain or ground a draft artifact.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceArtifactSourceKind {
    Media,
    MediaContext,
    SearchDocument,
    Artifact,
    Run,
    ToolCall,
    Manual,
}

impl IntelligenceArtifactSourceKind {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Media => "media",
            Self::MediaContext => "media_context",
            Self::SearchDocument => "search_document",
            Self::Artifact => "artifact",
            Self::Run => "run",
            Self::ToolCall => "tool_call",
            Self::Manual => "manual",
        }
    }
}

/// Structured provenance/source edge for draft artifact payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceArtifactSourceEdge {
    pub source_ordinal: i32,
    pub source_kind: IntelligenceArtifactSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_media_context_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_search_document_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_artifact_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_tool_call_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_library_id: Option<LibraryId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_user_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_media_id: Option<MediaID>,
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub source_revision: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_excerpt: Option<IntelligenceSummary>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub source_locator: serde_json::Value,
}

fn is_zero_i64(value: &i64) -> bool {
    *value == 0
}

/// Bounded artifact summary. The raw artifact body remains out-of-band.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceArtifactSummary {
    pub artifact_id: Uuid,
    pub kind: IntelligenceArtifactKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<IntelligenceMediaRef>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<IntelligenceSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance: Vec<IntelligenceProvenanceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grounding: Vec<IntelligenceGroundingRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_epoch_seconds: Option<i64>,
}

/// Request for artifact summaries by media, library, kind, or direct id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceArtifactSearchRequest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_ids: Vec<MediaID>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_ids: Vec<LibraryId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<IntelligenceArtifactKind>,
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Response for artifact summary search.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceArtifactSearchResponse {
    #[serde(default)]
    pub artifacts: Vec<IntelligenceArtifactSummary>,
    #[serde(default)]
    pub page: IntelligencePageInfo,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Request for reading a durable draft artifact payload by id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceDraftArtifactReadRequest {
    pub artifact_id: Uuid,
}

/// Raw draft artifact payload used by the runtime before a draft is promoted.
///
/// Unlike public artifact summaries, this payload intentionally includes the
/// bounded `content` JSON body because draft readers are internal/runtime
/// consumers that need to resume or inspect generated artifacts before they are
/// marked active.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceDraftArtifactPayload {
    pub artifact_id: Uuid,
    pub kind: IntelligenceArtifactKind,
    pub status: IntelligenceArtifactStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_id: Option<LibraryId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_user_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<MediaID>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<IntelligenceSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<IntelligenceSummary>,
    #[serde(default)]
    pub content: serde_json::Value,
    #[serde(default)]
    pub metadata: serde_json::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<IntelligenceArtifactSourceEdge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_epoch_seconds: Option<i64>,
}

/// Sources that can ground or explain an intelligence result.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceGroundingSource {
    FerrexLibrary,
    MediaMetadata,
    WatchState,
    SearchIndex,
    IntelligenceArtifact,
    ToolCall,
    UserRequest,
}

/// A bounded grounding reference to media, artifacts, or provider fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceGroundingRef {
    pub source: IntelligenceGroundingSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<MediaID>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<IntelligenceSummary>,
}

/// Provenance reference for generated summaries and artifacts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceProvenanceRef {
    pub source: IntelligenceGroundingSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grounding: Vec<IntelligenceGroundingRef>,
}

/// Request for an item's bounded intelligence context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceItemContextRequest {
    pub media_id: MediaID,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_id: Option<LibraryId>,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Request for related-item context around a seed media item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRelatedContextRequest {
    pub media_id: MediaID,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationship_kinds: Vec<IntelligenceRelationshipKind>,
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Item summary used for item-context and related-context responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceContextItem {
    pub media: IntelligenceMediaRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<IntelligenceSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facets: Vec<IntelligenceFacetValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance: Vec<IntelligenceProvenanceRef>,
}

/// Relationship classes used by related-context payloads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceRelationshipKind {
    SameSeries,
    SameLibrary,
    SimilarTitle,
    SimilarGenre,
    SharedCast,
    WatchNext,
    SearchMatch,
    ProviderRelated,
}

/// Related item with a bounded reason and optional relationship strength.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntelligenceRelatedContext {
    pub media: IntelligenceMediaRef,
    pub relationship: IntelligenceRelationshipKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strength: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<IntelligenceSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grounding: Vec<IntelligenceGroundingRef>,
}

/// Response for an item's complete bounded context packet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntelligenceItemContextResponse {
    pub item: IntelligenceContextItem,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<IntelligenceRelatedContext>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<IntelligenceArtifactSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grounding: Vec<IntelligenceGroundingRef>,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Response for related item context around a seed media item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntelligenceRelatedContextResponse {
    pub seed: IntelligenceMediaRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<IntelligenceRelatedContext>,
    #[serde(default)]
    pub page: IntelligencePageInfo,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Purpose categories for intelligence runs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceRunPurpose {
    LibraryOverview,
    CandidateSearch,
    ArtifactSearch,
    ItemContext,
    RelatedContext,
    ArtifactRefresh,
    Recommendation,
    Other,
}

/// Lifecycle status for an intelligence run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceRunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl IntelligenceRunStatus {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Structured runtime error codes shared by status, events, and cancel responses.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceErrorCode {
    FeatureDisabled,
    ProviderNotConfigured,
    ProviderUnavailable,
    ProviderUnauthorized,
    ProviderRateLimited,
    ProviderTimeout,
    ProviderError,
    ModelUnavailable,
    InvalidRequest,
    NotFound,
    Conflict,
    ConcurrencyLimit,
    RunCancelled,
    RunTimedOut,
    ToolTimedOut,
    StorageError,
    Internal,
}

impl IntelligenceErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FeatureDisabled => "feature_disabled",
            Self::ProviderNotConfigured => "provider_not_configured",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ProviderUnauthorized => "provider_unauthorized",
            Self::ProviderRateLimited => "provider_rate_limited",
            Self::ProviderTimeout => "provider_timeout",
            Self::ProviderError => "provider_error",
            Self::ModelUnavailable => "model_unavailable",
            Self::InvalidRequest => "invalid_request",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::ConcurrencyLimit => "concurrency_limit",
            Self::RunCancelled => "run_cancelled",
            Self::RunTimedOut => "run_timed_out",
            Self::ToolTimedOut => "tool_timed_out",
            Self::StorageError => "storage_error",
            Self::Internal => "internal",
        }
    }
}

/// Structured runtime error envelope with no provider-secret material.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceError {
    pub code: IntelligenceErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub details: serde_json::Value,
}

/// Request for starting a runtime-backed intelligence run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunStartRequest {
    pub purpose: IntelligenceRunPurpose,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_id: Option<LibraryId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<MediaID>,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub caps: IntelligenceCaps,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

/// Response returned once a run has been accepted for execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunStartResponse {
    pub run_id: Uuid,
    pub status: IntelligenceRunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_at_epoch_seconds: Option<i64>,
}

/// Response for polling current runtime status of a run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunStatusResponse {
    pub run_id: Uuid,
    pub purpose: IntelligenceRunPurpose,
    pub status: IntelligenceRunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_step: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub draft_artifact_ids: Vec<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IntelligenceError>,
}

/// Request for cancelling a runtime-backed intelligence run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunCancelRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Response returned after a cancel request is recorded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunCancelResponse {
    pub run_id: Uuid,
    pub status: IntelligenceRunStatus,
    pub cancellation_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancelled_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IntelligenceError>,
}

/// Runtime event classes used for durable replay and streaming surfaces.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceRunEventKind {
    Queued,
    Started,
    StatusChanged,
    ModelToken,
    ToolCallStarted,
    ToolCallFinished,
    DraftArtifactCreated,
    DraftArtifactUpdated,
    CancelRequested,
    Cancelled,
    Completed,
    Failed,
    Heartbeat,
}

impl IntelligenceRunEventKind {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Started => "started",
            Self::StatusChanged => "status_changed",
            Self::ModelToken => "model_token",
            Self::ToolCallStarted => "tool_call_started",
            Self::ToolCallFinished => "tool_call_finished",
            Self::DraftArtifactCreated => "draft_artifact_created",
            Self::DraftArtifactUpdated => "draft_artifact_updated",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Heartbeat => "heartbeat",
        }
    }
}

/// A single ordered runtime event for replay or streaming.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunEvent {
    pub event_id: Uuid,
    pub run_id: Uuid,
    pub sequence: i32,
    pub event_kind: IntelligenceRunEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IntelligenceRunStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub payload: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IntelligenceError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_epoch_seconds: Option<i64>,
}

/// Request for replaying persisted run events after an optional sequence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunEventsRequest {
    pub run_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_sequence: Option<i32>,
    #[serde(
        default = "default_intelligence_page_limit",
        deserialize_with = "deserialize_page_limit"
    )]
    pub limit: u16,
}

/// Ordered event replay response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunEventsResponse {
    pub run_id: Uuid,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<IntelligenceRunEvent>,
    #[serde(default)]
    pub page: IntelligencePageInfo,
}

/// Provider/model readiness exposed to clients and operators.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceProviderState {
    Disabled,
    NotConfigured,
    Ready,
    Degraded,
    Unavailable,
}

/// Per-model status in the configured provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceModelStatus {
    pub name: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub selected: bool,
    #[serde(default)]
    pub available: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub supports_tools: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window_tokens: Option<u32>,
}

/// Provider readiness without exposing configured secrets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceProviderStatus {
    pub enabled: bool,
    pub provider_name: String,
    pub base_url: String,
    pub api_key_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    pub state: IntelligenceProviderState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<IntelligenceModelStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IntelligenceError>,
}

/// Lifecycle status for an individual tool call within a run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceToolCallStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

/// Request for bounded audit details about an intelligence run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunAuditRequest {
    pub run_id: Uuid,
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Bounded audit record for a single tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceToolCallAudit {
    pub tool_call_id: Uuid,
    pub name: String,
    pub status: IntelligenceToolCallStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_summary: Option<IntelligenceSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<IntelligenceSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_summary: Option<IntelligenceSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grounding: Vec<IntelligenceGroundingRef>,
}

/// Bounded audit record for an intelligence run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunAudit {
    pub run_id: Uuid,
    pub purpose: IntelligenceRunPurpose,
    pub status: IntelligenceRunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_by_user_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_epoch_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_summary: Option<IntelligenceSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<IntelligenceSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grounding: Vec<IntelligenceGroundingRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<IntelligenceToolCallAudit>,
}

/// Response for run/tool-call audit inspection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceRunAuditResponse {
    pub run: IntelligenceRunAudit,
    #[serde(default)]
    pub page: IntelligencePageInfo,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_model::MovieID;

    fn movie_ref() -> IntelligenceMediaRef {
        let media_id = MediaID::Movie(MovieID(Uuid::from_u128(1)));
        let mut media = IntelligenceMediaRef::new(media_id, "Arrival");
        media.library_id = Some(LibraryId(Uuid::from_u128(2)));
        media.year = Some(2016);
        media.poster_iid = Some(Uuid::from_u128(3));
        media.artifact_ids = vec![Uuid::from_u128(4)];
        media
    }

    fn grounding_ref() -> IntelligenceGroundingRef {
        IntelligenceGroundingRef {
            source: IntelligenceGroundingSource::MediaMetadata,
            media_id: Some(MediaID::Movie(MovieID(Uuid::from_u128(1)))),
            artifact_id: Some(Uuid::from_u128(4)),
            field: Some("overview".to_string()),
            label: "TMDB overview".to_string(),
            evidence: Some(IntelligenceSummary::with_max_chars(
                "A linguist works to communicate with visitors.",
                80,
            )),
        }
    }

    #[test]
    fn candidate_search_contract_round_trips_through_json() {
        let response = IntelligenceCandidateSearchResponse {
            candidates: vec![IntelligenceCandidate {
                media: movie_ref(),
                summary: Some(IntelligenceSummary::new(
                    "Reflective first-contact science fiction.",
                )),
                match_reason: Some(IntelligenceSummary::new(
                    "Matches the query themes and release decade.",
                )),
                score: Some(0.92),
                artifact_ids: vec![Uuid::from_u128(4)],
                grounding: vec![grounding_ref()],
            }],
            page: IntelligencePageInfo {
                next_cursor: Some("cursor-2".to_string()),
                limit: 12,
                has_more: true,
            },
            caps: IntelligenceCaps::default(),
        };

        let json = serde_json::to_string(&response)
            .expect("candidate response should serialize");
        let decoded: IntelligenceCandidateSearchResponse =
            serde_json::from_str(&json)
                .expect("candidate response should deserialize");

        assert_eq!(decoded, response);
        assert!(!json.contains("raw_metadata"));
    }

    #[test]
    fn item_context_and_audit_contracts_round_trip_through_json() {
        let artifact_id = Uuid::from_u128(4);
        let run_id = Uuid::from_u128(5);
        let tool_call_id = Uuid::from_u128(6);
        let grounding = grounding_ref();
        let provenance = IntelligenceProvenanceRef {
            source: IntelligenceGroundingSource::ToolCall,
            run_id: Some(run_id),
            tool_call_id: Some(tool_call_id),
            grounding: vec![grounding.clone()],
        };
        let artifact = IntelligenceArtifactSummary {
            artifact_id,
            kind: IntelligenceArtifactKind::Summary,
            media: Some(movie_ref()),
            title: "Arrival compact summary".to_string(),
            summary: Some(IntelligenceSummary::new(
                "A bounded artifact summary for retrieval.",
            )),
            provenance: vec![provenance.clone()],
            grounding: vec![grounding.clone()],
            created_at_epoch_seconds: Some(1_700_000_000),
            updated_at_epoch_seconds: None,
        };
        let context = IntelligenceItemContextResponse {
            item: IntelligenceContextItem {
                media: movie_ref(),
                summary: Some(IntelligenceSummary::new(
                    "Arrival item context summary.",
                )),
                facets: vec![IntelligenceFacetValue {
                    key: "science-fiction".to_string(),
                    label: "Science Fiction".to_string(),
                    count: 1,
                    sample_media_ids: vec![MediaID::Movie(MovieID(
                        Uuid::from_u128(1),
                    ))],
                }],
                artifact_ids: vec![artifact_id],
                provenance: vec![provenance.clone()],
            },
            related: vec![IntelligenceRelatedContext {
                media: movie_ref(),
                relationship: IntelligenceRelationshipKind::SimilarGenre,
                strength: Some(0.75),
                reason: Some(IntelligenceSummary::new(
                    "Shares cerebral science-fiction traits.",
                )),
                artifact_ids: vec![artifact_id],
                grounding: vec![grounding.clone()],
            }],
            artifacts: vec![artifact],
            grounding: vec![grounding.clone()],
            caps: IntelligenceCaps::default(),
        };
        let audit = IntelligenceRunAuditResponse {
            run: IntelligenceRunAudit {
                run_id,
                purpose: IntelligenceRunPurpose::ItemContext,
                status: IntelligenceRunStatus::Succeeded,
                requested_by_user_id: Some(Uuid::from_u128(7)),
                model: Some("local-phase-one".to_string()),
                queued_at_epoch_seconds: Some(1_700_000_000),
                started_at_epoch_seconds: Some(1_700_000_001),
                completed_at_epoch_seconds: Some(1_700_000_003),
                input_summary: Some(IntelligenceSummary::new(
                    "Build item context for one movie.",
                )),
                output_summary: Some(IntelligenceSummary::new(
                    "Context produced with one artifact.",
                )),
                artifact_ids: vec![artifact_id],
                grounding: vec![grounding.clone()],
                tool_calls: vec![IntelligenceToolCallAudit {
                    tool_call_id,
                    name: "library_lookup".to_string(),
                    status: IntelligenceToolCallStatus::Succeeded,
                    started_at_epoch_seconds: Some(1_700_000_001),
                    completed_at_epoch_seconds: Some(1_700_000_002),
                    input_summary: Some(IntelligenceSummary::new(
                        "Lookup movie reference by id.",
                    )),
                    output_summary: Some(IntelligenceSummary::new(
                        "Returned bounded media reference.",
                    )),
                    error_summary: None,
                    artifact_ids: vec![artifact_id],
                    grounding: vec![grounding],
                }],
            },
            page: IntelligencePageInfo::default(),
            caps: IntelligenceCaps::default(),
        };

        let context_json = serde_json::to_string(&context)
            .expect("context response should serialize");
        let audit_json = serde_json::to_string(&audit)
            .expect("audit response should serialize");

        let decoded_context: IntelligenceItemContextResponse =
            serde_json::from_str(&context_json)
                .expect("context response should deserialize");
        let decoded_audit: IntelligenceRunAuditResponse =
            serde_json::from_str(&audit_json)
                .expect("audit response should deserialize");

        assert_eq!(decoded_context, context);
        assert_eq!(decoded_audit, audit);
    }

    #[test]
    fn runtime_contracts_round_trip_through_json() {
        let run_id = Uuid::from_u128(10);
        let artifact_id = Uuid::from_u128(11);
        let tool_call_id = Uuid::from_u128(12);
        let error = IntelligenceError {
            code: IntelligenceErrorCode::ProviderTimeout,
            message: "provider timed out".to_string(),
            retryable: true,
            details: serde_json::json!({"timeout_ms": 70000}),
        };

        let start = IntelligenceRunStartRequest {
            purpose: IntelligenceRunPurpose::Recommendation,
            library_id: Some(LibraryId(Uuid::from_u128(2))),
            media_id: Some(MediaID::Movie(MovieID(Uuid::from_u128(1)))),
            prompt: "Recommend something grounded in my library".to_string(),
            idempotency_key: Some("recommend-1".to_string()),
            model: Some("gemma-4-12b".to_string()),
            caps: IntelligenceCaps::default(),
            metadata: serde_json::json!({"source": "test"}),
        };
        let start_json =
            serde_json::to_string(&start).expect("serialize start");
        let decoded_start: IntelligenceRunStartRequest =
            serde_json::from_str(&start_json).expect("deserialize start");
        assert_eq!(decoded_start, start);

        let status = IntelligenceRunStatusResponse {
            run_id,
            purpose: IntelligenceRunPurpose::Recommendation,
            status: IntelligenceRunStatus::Running,
            provider: Some("openai-compatible".to_string()),
            model: Some("gemma-4-12b".to_string()),
            queued_at_epoch_seconds: Some(1_700_000_000),
            started_at_epoch_seconds: Some(1_700_000_001),
            completed_at_epoch_seconds: None,
            current_step: Some(2),
            max_steps: Some(12),
            draft_artifact_ids: vec![artifact_id],
            error: None,
        };
        let status_json =
            serde_json::to_string(&status).expect("serialize status");
        let decoded_status: IntelligenceRunStatusResponse =
            serde_json::from_str(&status_json).expect("deserialize status");
        assert_eq!(decoded_status, status);

        let cancel = IntelligenceRunCancelResponse {
            run_id,
            status: IntelligenceRunStatus::Cancelled,
            cancellation_requested: true,
            cancelled_at_epoch_seconds: Some(1_700_000_002),
            message: Some("cancel recorded".to_string()),
            error: Some(error.clone()),
        };
        let cancel_json =
            serde_json::to_string(&cancel).expect("serialize cancel");
        let decoded_cancel: IntelligenceRunCancelResponse =
            serde_json::from_str(&cancel_json).expect("deserialize cancel");
        assert_eq!(decoded_cancel, cancel);

        let event = IntelligenceRunEvent {
            event_id: Uuid::from_u128(13),
            run_id,
            sequence: 3,
            event_kind: IntelligenceRunEventKind::ToolCallFinished,
            status: Some(IntelligenceRunStatus::Running),
            tool_call_id: Some(tool_call_id),
            artifact_id: Some(artifact_id),
            message: Some("tool call completed".to_string()),
            payload: serde_json::json!({"tool": "library_search"}),
            error: Some(error.clone()),
            created_at_epoch_seconds: Some(1_700_000_003),
        };
        let events = IntelligenceRunEventsResponse {
            run_id,
            events: vec![event.clone()],
            page: IntelligencePageInfo {
                next_cursor: None,
                limit: 20,
                has_more: false,
            },
        };
        let events_json =
            serde_json::to_string(&events).expect("serialize events");
        let decoded_events: IntelligenceRunEventsResponse =
            serde_json::from_str(&events_json).expect("deserialize events");
        assert_eq!(decoded_events, events);

        let draft = IntelligenceDraftArtifactPayload {
            artifact_id,
            kind: IntelligenceArtifactKind::GeneratedAnswer,
            status: IntelligenceArtifactStatus::Draft,
            library_id: Some(LibraryId(Uuid::from_u128(2))),
            owner_user_id: Some(Uuid::from_u128(14)),
            media_id: Some(MediaID::Movie(MovieID(Uuid::from_u128(1)))),
            run_id: Some(run_id),
            title: "Draft answer".to_string(),
            summary: Some(IntelligenceSummary::new("draft summary")),
            excerpt: Some(IntelligenceSummary::new("draft excerpt")),
            content: serde_json::json!({"body": "draft"}),
            metadata: serde_json::json!({"version": 1}),
            sources: vec![IntelligenceArtifactSourceEdge {
                source_ordinal: 0,
                source_kind: IntelligenceArtifactSourceKind::Media,
                source_media_context_id: None,
                source_search_document_id: None,
                source_artifact_id: None,
                source_run_id: Some(run_id),
                source_tool_call_id: Some(tool_call_id),
                source_library_id: Some(LibraryId(Uuid::from_u128(2))),
                source_user_id: Some(Uuid::from_u128(14)),
                source_media_id: Some(MediaID::Movie(MovieID(
                    Uuid::from_u128(1),
                ))),
                source_revision: 4,
                source_content_hash: Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                ),
                source_excerpt: Some(IntelligenceSummary::new(
                    "grounded source excerpt",
                )),
                source_locator: serde_json::json!({"field": "overview"}),
            }],
            created_at_epoch_seconds: Some(1_700_000_004),
            updated_at_epoch_seconds: Some(1_700_000_005),
        };
        let draft_json =
            serde_json::to_string(&draft).expect("serialize draft");
        let decoded_draft: IntelligenceDraftArtifactPayload =
            serde_json::from_str(&draft_json).expect("deserialize draft");
        assert_eq!(decoded_draft, draft);

        let provider = IntelligenceProviderStatus {
            enabled: true,
            provider_name: "openai-compatible".to_string(),
            base_url: "http://localhost:8081/v1".to_string(),
            api_key_configured: false,
            default_model: Some("gemma-4-12b".to_string()),
            state: IntelligenceProviderState::Ready,
            models: vec![IntelligenceModelStatus {
                name: "gemma-4-12b".to_string(),
                selected: true,
                available: true,
                supports_tools: true,
                context_window_tokens: Some(8192),
            }],
            checked_at_epoch_seconds: Some(1_700_000_006),
            error: None,
        };
        let provider_json =
            serde_json::to_string(&provider).expect("serialize provider");
        let decoded_provider: IntelligenceProviderStatus =
            serde_json::from_str(&provider_json).expect("deserialize provider");
        assert_eq!(decoded_provider, provider);
    }

    #[test]
    fn pagination_and_caps_default_and_clamp_on_deserialize() {
        let default_page: IntelligencePagination =
            serde_json::from_str("{}").expect("pagination defaults");
        assert_eq!(default_page, IntelligencePagination::default());

        let zero_page: IntelligencePagination =
            serde_json::from_str(r#"{"limit":0,"cursor":"first"}"#)
                .expect("zero limit maps to default");
        assert_eq!(zero_page.limit, DEFAULT_INTELLIGENCE_PAGE_LIMIT);
        assert_eq!(zero_page.cursor.as_deref(), Some("first"));

        let oversized_page: IntelligencePagination =
            serde_json::from_str(r#"{"limit":999}"#)
                .expect("oversized limit clamps to max");
        assert_eq!(oversized_page.limit, MAX_INTELLIGENCE_PAGE_LIMIT);

        let caps: IntelligenceCaps = serde_json::from_str(
            r#"{
                "candidate_limit": 999,
                "artifact_limit": 0,
                "related_limit": 999,
                "facet_limit": 0,
                "grounding_limit": 999,
                "tool_call_limit": 0,
                "summary_max_chars": 9999
            }"#,
        )
        .expect("caps should deserialize with clamps");

        assert_eq!(caps.candidate_limit, MAX_INTELLIGENCE_CANDIDATE_LIMIT);
        assert_eq!(caps.artifact_limit, DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT);
        assert_eq!(caps.related_limit, MAX_INTELLIGENCE_RELATED_LIMIT);
        assert_eq!(caps.facet_limit, DEFAULT_INTELLIGENCE_FACET_LIMIT);
        assert_eq!(caps.grounding_limit, MAX_INTELLIGENCE_GROUNDING_LIMIT);
        assert_eq!(caps.tool_call_limit, DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT);
        assert_eq!(caps.summary_max_chars, MAX_INTELLIGENCE_SUMMARY_CHARS);
    }

    #[test]
    fn bounded_summary_constructor_truncates_on_char_boundaries() {
        let summary = IntelligenceSummary::with_max_chars("éclair movie", 3);

        assert_eq!(summary.text, "écl");
        assert_eq!(summary.max_chars, 3);
        assert!(summary.truncated);

        let defaulted = IntelligenceSummary::with_max_chars("short", 0);
        assert_eq!(defaulted.max_chars, DEFAULT_INTELLIGENCE_SUMMARY_CHARS);
        assert!(!defaulted.truncated);
    }
}
