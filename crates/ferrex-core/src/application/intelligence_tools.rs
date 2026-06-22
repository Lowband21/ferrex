//! Bounded grounded intelligence tool registry and executor.
//!
//! This module is the only Ferrex surface intended for model tool execution.
//! It exposes a fixed allowlist of repository-backed tools, validates typed
//! JSON arguments, enforces per-tool row/byte/time budgets, scopes library/user
//! reads, persists bounded tool-call audit records, and returns a redacted model
//! context plus ledger grounding refs. The implementation only delegates to
//! existing read repositories and the draft-artifact creation port; it does not
//! execute shell commands, run scripts, touch the filesystem, issue arbitrary
//! SQL, or perform playlist/collection/admin writes.

use std::{fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use ferrex_model::{LibraryId, MediaID};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    api::types::intelligence::{
        DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT,
        DEFAULT_INTELLIGENCE_CANDIDATE_LIMIT, DEFAULT_INTELLIGENCE_FACET_LIMIT,
        DEFAULT_INTELLIGENCE_GROUNDING_LIMIT, DEFAULT_INTELLIGENCE_PAGE_LIMIT,
        DEFAULT_INTELLIGENCE_RELATED_LIMIT, DEFAULT_INTELLIGENCE_SUMMARY_CHARS,
        DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT, DEFAULT_TIMED_TEXT_SEGMENT_LIMIT,
        DEFAULT_TIMED_TEXT_SNIPPET_CHARS, DEFAULT_TIMED_TEXT_SNIPPET_LIMIT,
        IntelligenceArtifactKind, IntelligenceArtifactSearchRequest,
        IntelligenceArtifactSearchResponse, IntelligenceArtifactSourceEdge,
        IntelligenceCandidateSearchRequest,
        IntelligenceCandidateSearchResponse, IntelligenceCaps,
        IntelligenceDraftArtifactPayload, IntelligenceFacetGroup,
        IntelligenceGroundingRef, IntelligenceGroundingSource,
        IntelligenceItemContextRequest, IntelligenceItemContextResponse,
        IntelligenceLibraryOverviewRequest,
        IntelligenceLibraryOverviewResponse, IntelligenceMediaKind,
        IntelligencePagination, IntelligenceRelatedContextRequest,
        IntelligenceRelatedContextResponse, IntelligenceRelationshipKind,
        IntelligenceSummary, MAX_INTELLIGENCE_ARTIFACT_LIMIT,
        MAX_INTELLIGENCE_CANDIDATE_LIMIT, MAX_INTELLIGENCE_FACET_LIMIT,
        MAX_INTELLIGENCE_GROUNDING_LIMIT, MAX_INTELLIGENCE_PAGE_LIMIT,
        MAX_INTELLIGENCE_RELATED_LIMIT, MAX_INTELLIGENCE_SUMMARY_CHARS,
        MAX_INTELLIGENCE_TOOL_CALL_LIMIT, MAX_TIMED_TEXT_SEGMENT_LIMIT,
        MAX_TIMED_TEXT_SNIPPET_CHARS, MAX_TIMED_TEXT_SNIPPET_LIMIT,
    },
    database::repository_ports::{
        intelligence::{
            IntelligenceArtifactScope, IntelligenceDraftArtifactCreate,
            IntelligenceRepository, IntelligenceToolCallCreate,
            IntelligenceToolCallStatus as ToolCallStatusInternal,
            IntelligenceToolCallUpdate, IntelligenceToolKind,
        },
        query::QueryRepository,
    },
    error::{MediaError, Result},
    query::types::{
        MediaFilters, MediaQuery, MediaWithStatus, Pagination, SortCriteria,
        SortOrder,
    },
};

const TOOL_DEFAULT_MAX_BYTES: u32 = 32 * 1024;
const TOOL_DRAFT_MAX_BYTES: u32 = 16 * 1024;
const TOOL_DEFAULT_TIMEOUT_MS: u64 = 2_000;
const TOOL_DRAFT_TIMEOUT_MS: u64 = 3_000;
const REDACTED: &str = "[redacted]";
const REDACTION_MAX_STRING_CHARS: usize = 512;
const REDACTION_MAX_ARRAY_ITEMS: usize = 32;

/// Execution scope supplied by the runtime for one model-request tool call.
#[derive(Debug, Clone)]
pub struct IntelligenceToolCallContext {
    /// Intelligence run that owns the tool call audit record.
    pub run_id: Uuid,
    /// Deterministic sequence number within the run.
    pub sequence: i32,
    /// Requesting user. User-scoped rows are only visible when this is set.
    pub user_id: Option<Uuid>,
    /// Optional library allowlist. `None` means the caller did not provide a
    /// library restriction; `Some([])` means no library is accessible.
    pub allowed_library_ids: Option<Vec<LibraryId>>,
    /// Optional idempotency key for durable tool-call audit rows.
    pub idempotency_key: Option<String>,
}

impl IntelligenceToolCallContext {
    /// Construct a scoped tool-call context for a user-backed run.
    pub fn for_user(run_id: Uuid, sequence: i32, user_id: Uuid) -> Self {
        Self {
            run_id,
            sequence,
            user_id: Some(user_id),
            allowed_library_ids: None,
            idempotency_key: None,
        }
    }
}

/// Fixed names for every approved intelligence tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceToolName {
    LibraryOverview,
    Facets,
    CandidateSearch,
    MediaQuery,
    ItemContext,
    RelatedContext,
    WatchContext,
    ArtifactSearch,
    ArtifactDetailSample,
    ArtifactFacets,
    CreateDraft,
}

impl IntelligenceToolName {
    /// Ordered allowlist exposed to model providers.
    pub const ALL: [Self; 11] = [
        Self::LibraryOverview,
        Self::Facets,
        Self::CandidateSearch,
        Self::MediaQuery,
        Self::ItemContext,
        Self::RelatedContext,
        Self::WatchContext,
        Self::ArtifactSearch,
        Self::ArtifactDetailSample,
        Self::ArtifactFacets,
        Self::CreateDraft,
    ];

    /// Stable external tool name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LibraryOverview => "library_overview",
            Self::Facets => "facets",
            Self::CandidateSearch => "candidate_search",
            Self::MediaQuery => "media_query",
            Self::ItemContext => "item_context",
            Self::RelatedContext => "related_context",
            Self::WatchContext => "watch_context",
            Self::ArtifactSearch => "artifact_search",
            Self::ArtifactDetailSample => "artifact_detail_sample",
            Self::ArtifactFacets => "artifact_facets",
            Self::CreateDraft => "create_draft",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|tool| tool.as_str() == raw.trim())
    }

    const fn tool_kind(self) -> IntelligenceToolKind {
        match self {
            Self::CandidateSearch | Self::MediaQuery | Self::WatchContext => {
                IntelligenceToolKind::Search
            }
            Self::ArtifactSearch
            | Self::ArtifactDetailSample
            | Self::ArtifactFacets
            | Self::CreateDraft => IntelligenceToolKind::Artifact,
            Self::LibraryOverview
            | Self::Facets
            | Self::ItemContext
            | Self::RelatedContext => IntelligenceToolKind::ReadModel,
        }
    }

    const fn side_effect(self) -> IntelligenceToolSideEffect {
        match self {
            Self::CreateDraft => IntelligenceToolSideEffect::CreateDraft,
            _ => IntelligenceToolSideEffect::ReadOnly,
        }
    }

    const fn budget(self) -> IntelligenceToolBudget {
        match self {
            Self::LibraryOverview => IntelligenceToolBudget {
                max_rows: 50,
                max_bytes: TOOL_DEFAULT_MAX_BYTES,
                max_time_ms: TOOL_DEFAULT_TIMEOUT_MS,
            },
            Self::Facets => IntelligenceToolBudget {
                max_rows: 32,
                max_bytes: TOOL_DEFAULT_MAX_BYTES,
                max_time_ms: TOOL_DEFAULT_TIMEOUT_MS,
            },
            Self::CandidateSearch => IntelligenceToolBudget {
                max_rows: 50,
                max_bytes: TOOL_DEFAULT_MAX_BYTES,
                max_time_ms: TOOL_DEFAULT_TIMEOUT_MS,
            },
            Self::MediaQuery => IntelligenceToolBudget {
                max_rows: 50,
                max_bytes: TOOL_DEFAULT_MAX_BYTES,
                max_time_ms: TOOL_DEFAULT_TIMEOUT_MS,
            },
            Self::ItemContext => IntelligenceToolBudget {
                max_rows: 49,
                max_bytes: TOOL_DEFAULT_MAX_BYTES,
                max_time_ms: TOOL_DEFAULT_TIMEOUT_MS,
            },
            Self::RelatedContext => IntelligenceToolBudget {
                max_rows: 24,
                max_bytes: TOOL_DEFAULT_MAX_BYTES,
                max_time_ms: TOOL_DEFAULT_TIMEOUT_MS,
            },
            Self::WatchContext => IntelligenceToolBudget {
                max_rows: 50,
                max_bytes: TOOL_DEFAULT_MAX_BYTES,
                max_time_ms: TOOL_DEFAULT_TIMEOUT_MS,
            },
            Self::ArtifactSearch => IntelligenceToolBudget {
                max_rows: 24,
                max_bytes: TOOL_DEFAULT_MAX_BYTES,
                max_time_ms: TOOL_DEFAULT_TIMEOUT_MS,
            },
            Self::ArtifactDetailSample => IntelligenceToolBudget {
                max_rows: 12,
                max_bytes: TOOL_DEFAULT_MAX_BYTES,
                max_time_ms: TOOL_DEFAULT_TIMEOUT_MS,
            },
            Self::ArtifactFacets => IntelligenceToolBudget {
                max_rows: 24,
                max_bytes: TOOL_DEFAULT_MAX_BYTES,
                max_time_ms: TOOL_DEFAULT_TIMEOUT_MS,
            },
            Self::CreateDraft => IntelligenceToolBudget {
                max_rows: 1,
                max_bytes: TOOL_DRAFT_MAX_BYTES,
                max_time_ms: TOOL_DRAFT_TIMEOUT_MS,
            },
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::LibraryOverview => {
                "Return bounded library counts, summaries, facets, and artifact ids."
            }
            Self::Facets => {
                "Return bounded library facet groups for discovery planning."
            }
            Self::CandidateSearch => {
                "Search bounded intelligence read models for candidate media."
            }
            Self::MediaQuery => {
                "Run a bounded Ferrex media query and return media ids with watch status."
            }
            Self::ItemContext => {
                "Return bounded context, related items, artifacts, and grounding for one media item."
            }
            Self::RelatedContext => {
                "Return bounded related media context around one seed item."
            }
            Self::WatchContext => {
                "Return bounded user watch-state context such as in-progress or recently watched items."
            }
            Self::ArtifactSearch => {
                "Search bounded active intelligence artifact summaries."
            }
            Self::ArtifactDetailSample => {
                "Sample bounded artifact detail summaries by id without exposing raw payload bodies."
            }
            Self::ArtifactFacets => {
                "Build bounded facet counts from artifact summary samples."
            }
            Self::CreateDraft => {
                "Create one scoped draft intelligence artifact with provenance sources."
            }
        }
    }

    fn allowed_keys(self) -> &'static [&'static str] {
        match self {
            Self::LibraryOverview | Self::Facets => {
                &["library_ids", "pagination", "caps"]
            }
            Self::CandidateSearch => &[
                "query",
                "library_ids",
                "media_kinds",
                "pagination",
                "caps",
                "include_artifacts",
            ],
            Self::MediaQuery => &["query", "caps"],
            Self::ItemContext => &["media_id", "library_id", "caps"],
            Self::RelatedContext => &[
                "media_id",
                "library_id",
                "relationship_kinds",
                "pagination",
                "caps",
            ],
            Self::WatchContext => {
                &["kind", "library_ids", "recent_days", "pagination", "caps"]
            }
            Self::ArtifactSearch => &[
                "artifact_ids",
                "media_ids",
                "library_ids",
                "kinds",
                "pagination",
                "caps",
            ],
            Self::ArtifactDetailSample => {
                &["artifact_ids", "pagination", "caps"]
            }
            Self::ArtifactFacets => {
                &["media_ids", "library_ids", "kinds", "pagination", "caps"]
            }
            Self::CreateDraft => &[
                "artifact_id",
                "kind",
                "library_id",
                "media_id",
                "title",
                "summary",
                "excerpt",
                "content",
                "metadata",
                "sources",
                "source_revision",
            ],
        }
    }

    fn input_schema(self) -> Value {
        match self {
            Self::LibraryOverview => schema_library_overview(),
            Self::Facets => schema_facets(),
            Self::CandidateSearch => schema_candidate_search(),
            Self::MediaQuery => schema_media_query(),
            Self::ItemContext => schema_item_context(),
            Self::RelatedContext => schema_related_context(),
            Self::WatchContext => schema_watch_context(),
            Self::ArtifactSearch => schema_artifact_search(),
            Self::ArtifactDetailSample => schema_artifact_detail_sample(),
            Self::ArtifactFacets => schema_artifact_facets(),
            Self::CreateDraft => schema_create_draft(),
        }
    }
}

/// Side-effect class for an approved intelligence tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceToolSideEffect {
    ReadOnly,
    CreateDraft,
}

/// Hard execution budget advertised and enforced per tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntelligenceToolBudget {
    pub max_rows: u16,
    pub max_bytes: u32,
    pub max_time_ms: u64,
}

/// Tool definition supplied to model providers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntelligenceToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub budget: IntelligenceToolBudget,
    pub side_effect: IntelligenceToolSideEffect,
}

/// Stable error codes returned by the tool executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceToolErrorCode {
    UnknownTool,
    MalformedArguments,
    ScopeViolation,
    BudgetExceeded,
    ToolTimedOut,
    Cancelled,
    NotFound,
    InvalidRequest,
    StorageError,
    AuditError,
    Internal,
}

impl IntelligenceToolErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownTool => "unknown_tool",
            Self::MalformedArguments => "malformed_arguments",
            Self::ScopeViolation => "scope_violation",
            Self::BudgetExceeded => "budget_exceeded",
            Self::ToolTimedOut => "tool_timed_out",
            Self::Cancelled => "cancelled",
            Self::NotFound => "not_found",
            Self::InvalidRequest => "invalid_request",
            Self::StorageError => "storage_error",
            Self::AuditError => "audit_error",
            Self::Internal => "internal",
        }
    }
}

/// Deterministic, redacted error payload returned to the runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntelligenceToolError {
    pub code: IntelligenceToolErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

impl IntelligenceToolError {
    fn new(
        code: IntelligenceToolErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            details: Value::Null,
        }
    }

    fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    fn from_media(error: MediaError) -> Self {
        match error {
            MediaError::NotFound(_) => Self::new(
                IntelligenceToolErrorCode::NotFound,
                "requested Ferrex intelligence resource was not found",
            ),
            MediaError::InvalidMedia(_) => Self::new(
                IntelligenceToolErrorCode::InvalidRequest,
                "tool request was rejected by the Ferrex repository",
            ),
            MediaError::Cancelled(_) => Self::new(
                IntelligenceToolErrorCode::Cancelled,
                "tool execution was cancelled",
            ),
            MediaError::Conflict(_) => Self::new(
                IntelligenceToolErrorCode::InvalidRequest,
                "tool request conflicted with current Ferrex state",
            ),
            MediaError::ConcurrencyLimit(_) => Self::new(
                IntelligenceToolErrorCode::InvalidRequest,
                "tool request exceeded a Ferrex concurrency limit",
            ),
            MediaError::Serialization(_) => Self::new(
                IntelligenceToolErrorCode::Internal,
                "tool response could not be serialized safely",
            ),
            MediaError::Io(_)
            | MediaError::Http(_)
            | MediaError::HttpStatus { .. }
            | MediaError::Internal(_) => Self::new(
                IntelligenceToolErrorCode::StorageError,
                "Ferrex tool backend operation failed",
            )
            .retryable(true),
            #[cfg(feature = "ffmpeg")]
            MediaError::Ffmpeg(_) => Self::new(
                IntelligenceToolErrorCode::StorageError,
                "Ferrex media backend operation failed",
            )
            .retryable(true),
            #[cfg(feature = "database")]
            MediaError::Database(_) => Self::new(
                IntelligenceToolErrorCode::StorageError,
                "Ferrex storage operation failed",
            )
            .retryable(true),
        }
    }
}

/// Optional runtime controls for one tool execution.
#[derive(Debug, Clone, Default)]
pub struct IntelligenceToolExecutionControls {
    /// Runtime-wide tool deadline. The stricter per-tool registry budget still
    /// applies when this is larger than the approved tool budget.
    pub timeout: Option<Duration>,
    /// Cancellation token checked before and during tool dispatch.
    pub cancellation_token: Option<CancellationToken>,
    /// Preallocated audit id used by run orchestration to emit ordered events
    /// before the tool backend starts.
    pub tool_call_id: Option<Uuid>,
}

/// Successful tool execution returned to the runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntelligenceToolExecution {
    pub tool_call_id: Uuid,
    pub tool_name: String,
    pub summary: IntelligenceSummary,
    /// Redacted JSON object that may be inserted into model context.
    pub visible: Value,
    /// Runtime-ledger grounding refs kept out of free-form model context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grounding: Vec<IntelligenceGroundingRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_ids: Vec<MediaID>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<Uuid>,
    pub row_count: u16,
    pub visible_bytes: u32,
}

#[derive(Debug, Clone)]
struct RawToolOutput {
    summary: IntelligenceSummary,
    data: Value,
    grounding: Vec<IntelligenceGroundingRef>,
    media_ids: Vec<MediaID>,
    artifact_ids: Vec<Uuid>,
    row_count: u16,
}

/// Narrow backend used by the executor. The production implementation delegates
/// to [`IntelligenceRepository`] and [`QueryRepository`]; tests can substitute a
/// deterministic fake without implementing every repository port.
#[async_trait]
pub trait IntelligenceToolBackend: Send + Sync {
    async fn library_overview(
        &self,
        request: &IntelligenceLibraryOverviewRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceLibraryOverviewResponse>;

    async fn candidate_search(
        &self,
        request: &IntelligenceCandidateSearchRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceCandidateSearchResponse>;

    async fn item_context(
        &self,
        request: &IntelligenceItemContextRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceItemContextResponse>;

    async fn related_context(
        &self,
        request: &IntelligenceRelatedContextRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRelatedContextResponse>;

    async fn artifact_search(
        &self,
        request: &IntelligenceArtifactSearchRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceArtifactSearchResponse>;

    async fn get_draft_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<Option<IntelligenceDraftArtifactPayload>>;

    async fn create_draft_artifact(
        &self,
        create: IntelligenceDraftArtifactCreate,
    ) -> Result<Uuid>;

    async fn replace_artifact_sources(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
        sources: Vec<IntelligenceArtifactSourceEdge>,
    ) -> Result<()>;

    async fn query_media(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_in_progress_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_completed_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_unwatched_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_recently_watched_media(
        &self,
        user_id: Uuid,
        recent_days: u32,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn create_tool_call(
        &self,
        create: IntelligenceToolCallCreate,
    ) -> Result<Uuid>;

    async fn update_tool_call(
        &self,
        tool_call_id: Uuid,
        update: IntelligenceToolCallUpdate,
    ) -> Result<()>;
}

/// Production backend over the existing Phase 1 repository/query surfaces.
#[derive(Clone)]
pub struct RepositoryIntelligenceToolBackend {
    intelligence: Arc<dyn IntelligenceRepository>,
    query: Arc<dyn QueryRepository>,
}

impl fmt::Debug for RepositoryIntelligenceToolBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepositoryIntelligenceToolBackend")
            .field("intelligence", &"dyn IntelligenceRepository")
            .field("query", &"dyn QueryRepository")
            .finish()
    }
}

impl RepositoryIntelligenceToolBackend {
    pub fn new(
        intelligence: Arc<dyn IntelligenceRepository>,
        query: Arc<dyn QueryRepository>,
    ) -> Self {
        Self {
            intelligence,
            query,
        }
    }
}

#[async_trait]
impl IntelligenceToolBackend for RepositoryIntelligenceToolBackend {
    async fn library_overview(
        &self,
        request: &IntelligenceLibraryOverviewRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceLibraryOverviewResponse> {
        self.intelligence.library_overview(request, user_id).await
    }

    async fn candidate_search(
        &self,
        request: &IntelligenceCandidateSearchRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceCandidateSearchResponse> {
        self.intelligence.candidate_search(request, user_id).await
    }

    async fn item_context(
        &self,
        request: &IntelligenceItemContextRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceItemContextResponse> {
        self.intelligence.item_context(request, user_id).await
    }

    async fn related_context(
        &self,
        request: &IntelligenceRelatedContextRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRelatedContextResponse> {
        self.intelligence.related_context(request, user_id).await
    }

    async fn artifact_search(
        &self,
        request: &IntelligenceArtifactSearchRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceArtifactSearchResponse> {
        self.intelligence.artifact_search(request, user_id).await
    }

    async fn get_draft_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<Option<IntelligenceDraftArtifactPayload>> {
        self.intelligence
            .get_draft_artifact(artifact_id, user_id)
            .await
    }

    async fn create_draft_artifact(
        &self,
        create: IntelligenceDraftArtifactCreate,
    ) -> Result<Uuid> {
        self.intelligence.create_draft_artifact(create).await
    }

    async fn replace_artifact_sources(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
        sources: Vec<IntelligenceArtifactSourceEdge>,
    ) -> Result<()> {
        self.intelligence
            .replace_artifact_sources(artifact_id, user_id, sources)
            .await
    }

    async fn query_media(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        self.query.query_media(query).await
    }

    async fn query_in_progress_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        self.query.query_in_progress_media(user_id, query).await
    }

    async fn query_completed_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        self.query.query_completed_media(user_id, query).await
    }

    async fn query_unwatched_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        self.query.query_unwatched_media(user_id, query).await
    }

    async fn query_recently_watched_media(
        &self,
        user_id: Uuid,
        recent_days: u32,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        self.query
            .query_recently_watched_media(user_id, recent_days, query)
            .await
    }

    async fn create_tool_call(
        &self,
        create: IntelligenceToolCallCreate,
    ) -> Result<Uuid> {
        self.intelligence.create_tool_call(create).await
    }

    async fn update_tool_call(
        &self,
        tool_call_id: Uuid,
        update: IntelligenceToolCallUpdate,
    ) -> Result<()> {
        self.intelligence
            .update_tool_call(tool_call_id, update)
            .await
    }
}

/// Fixed Ferrex tool registry and executor.
#[derive(Clone)]
pub struct IntelligenceToolRegistry {
    backend: Arc<dyn IntelligenceToolBackend>,
}

impl fmt::Debug for IntelligenceToolRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntelligenceToolRegistry")
            .field("backend", &"dyn IntelligenceToolBackend")
            .finish()
    }
}

impl IntelligenceToolRegistry {
    pub fn new(backend: Arc<dyn IntelligenceToolBackend>) -> Self {
        Self { backend }
    }

    pub fn from_repositories(
        intelligence: Arc<dyn IntelligenceRepository>,
        query: Arc<dyn QueryRepository>,
    ) -> Self {
        Self::new(Arc::new(RepositoryIntelligenceToolBackend::new(
            intelligence,
            query,
        )))
    }

    /// Return every approved tool definition. There is no extension point for
    /// arbitrary tools in this registry.
    pub fn definitions(&self) -> Vec<IntelligenceToolDefinition> {
        IntelligenceToolName::ALL
            .into_iter()
            .map(definition_for)
            .collect()
    }

    pub fn definition(&self, name: &str) -> Option<IntelligenceToolDefinition> {
        IntelligenceToolName::parse(name).map(definition_for)
    }

    /// Execute one tool call with typed argument validation, scope checks,
    /// audit updates, runtime timeout, row/byte caps, and model-context
    /// redaction.
    pub async fn execute(
        &self,
        context: &IntelligenceToolCallContext,
        tool_name: &str,
        arguments: Value,
    ) -> std::result::Result<IntelligenceToolExecution, IntelligenceToolError>
    {
        self.execute_with_controls(
            context,
            tool_name,
            arguments,
            IntelligenceToolExecutionControls::default(),
        )
        .await
    }

    /// Execute one tool call with caller-supplied runtime controls.
    pub async fn execute_with_controls(
        &self,
        context: &IntelligenceToolCallContext,
        tool_name: &str,
        arguments: Value,
        controls: IntelligenceToolExecutionControls,
    ) -> std::result::Result<IntelligenceToolExecution, IntelligenceToolError>
    {
        let Some(name) = IntelligenceToolName::parse(tool_name) else {
            return Err(IntelligenceToolError::new(
                IntelligenceToolErrorCode::UnknownTool,
                "requested Ferrex intelligence tool is not approved",
            )
            .with_details(json!({
                "requested_tool": tool_name,
                "approved_tools": approved_tool_names(),
            })));
        };

        if controls
            .cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(cancelled_tool_error(name));
        }

        let budget = name.budget();
        let redacted_arguments = redact_json_for_model(arguments.clone());
        let tool_call_id = self
            .create_audit_record(
                context,
                name,
                redacted_arguments,
                controls.tool_call_id,
            )
            .await?;

        if controls
            .cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            let error = cancelled_tool_error(name);
            self.mark_tool_cancelled(tool_call_id, &error).await?;
            return Err(error);
        }

        self.mark_tool_running(tool_call_id).await?;

        let parsed = match parse_tool_input(name, arguments) {
            Ok(input) => input,
            Err(error) => {
                self.mark_tool_failed(tool_call_id, &error).await?;
                return Err(error);
            }
        };

        let scoped = match apply_scope_and_caps(name, parsed, context) {
            Ok(input) => input,
            Err(error) => {
                self.mark_tool_failed(tool_call_id, &error).await?;
                return Err(error);
            }
        };

        let operation = self.dispatch_tool(name, scoped, context, tool_call_id);
        let per_tool_timeout = Duration::from_millis(budget.max_time_ms);
        let effective_timeout = controls
            .timeout
            .map(|runtime_timeout| runtime_timeout.min(per_tool_timeout))
            .unwrap_or(per_tool_timeout);

        let raw_output = if let Some(token) =
            controls.cancellation_token.clone()
        {
            tokio::select! {
                _ = token.cancelled() => {
                    let error = cancelled_tool_error(name);
                    self.mark_tool_cancelled(tool_call_id, &error).await?;
                    return Err(error);
                }
                result = timeout(effective_timeout, operation) => {
                    self.tool_operation_result(name, tool_call_id, budget, result).await?
                }
            }
        } else {
            let result = timeout(effective_timeout, operation).await;
            self.tool_operation_result(name, tool_call_id, budget, result)
                .await?
        };

        let execution = match finalize_tool_output(
            name,
            tool_call_id,
            raw_output,
            budget,
        ) {
            Ok(output) => output,
            Err(error) => {
                self.mark_tool_failed(tool_call_id, &error).await?;
                return Err(error);
            }
        };

        self.mark_tool_succeeded(tool_call_id, &execution.visible)
            .await?;
        Ok(execution)
    }

    async fn create_audit_record(
        &self,
        context: &IntelligenceToolCallContext,
        name: IntelligenceToolName,
        redacted_arguments: Value,
        tool_call_id: Option<Uuid>,
    ) -> std::result::Result<Uuid, IntelligenceToolError> {
        let input_hash = Some(stable_json_hash(&redacted_arguments));
        self.backend
            .create_tool_call(IntelligenceToolCallCreate {
                tool_call_id,
                run_id: context.run_id,
                sequence: context.sequence,
                tool_kind: name.tool_kind(),
                tool_name: name.as_str().to_string(),
                idempotency_key: context.idempotency_key.clone(),
                input_hash,
                arguments: redacted_arguments,
            })
            .await
            .map_err(|_| {
                IntelligenceToolError::new(
                    IntelligenceToolErrorCode::AuditError,
                    "failed to create Ferrex tool-call audit record",
                )
                .retryable(true)
            })
    }

    async fn tool_operation_result(
        &self,
        name: IntelligenceToolName,
        tool_call_id: Uuid,
        budget: IntelligenceToolBudget,
        result: std::result::Result<
            std::result::Result<RawToolOutput, IntelligenceToolError>,
            tokio::time::error::Elapsed,
        >,
    ) -> std::result::Result<RawToolOutput, IntelligenceToolError> {
        match result {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(error)) => {
                self.mark_tool_failed(tool_call_id, &error).await?;
                Err(error)
            }
            Err(_) => {
                let error = IntelligenceToolError::new(
                    IntelligenceToolErrorCode::ToolTimedOut,
                    "Ferrex tool execution exceeded its time budget",
                )
                .retryable(true)
                .with_details(json!({
                    "tool": name.as_str(),
                    "max_time_ms": budget.max_time_ms,
                }));
                self.mark_tool_failed(tool_call_id, &error).await?;
                Err(error)
            }
        }
    }

    async fn mark_tool_running(
        &self,
        tool_call_id: Uuid,
    ) -> std::result::Result<(), IntelligenceToolError> {
        self.backend
            .update_tool_call(
                tool_call_id,
                IntelligenceToolCallUpdate {
                    status: Some(ToolCallStatusInternal::Running),
                    started_at: Some(Utc::now()),
                    ..IntelligenceToolCallUpdate::default()
                },
            )
            .await
            .map_err(|_| {
                IntelligenceToolError::new(
                    IntelligenceToolErrorCode::AuditError,
                    "failed to mark Ferrex tool-call audit record running",
                )
                .retryable(true)
            })
    }

    async fn mark_tool_succeeded(
        &self,
        tool_call_id: Uuid,
        visible: &Value,
    ) -> std::result::Result<(), IntelligenceToolError> {
        let result = redact_json_for_model(visible.clone());
        self.backend
            .update_tool_call(
                tool_call_id,
                IntelligenceToolCallUpdate {
                    status: Some(ToolCallStatusInternal::Succeeded),
                    output_hash: Some(stable_json_hash(&result)),
                    result: Some(result),
                    finished_at: Some(Utc::now()),
                    ..IntelligenceToolCallUpdate::default()
                },
            )
            .await
            .map_err(|_| {
                IntelligenceToolError::new(
                    IntelligenceToolErrorCode::AuditError,
                    "failed to mark Ferrex tool-call audit record succeeded",
                )
                .retryable(true)
            })
    }

    async fn mark_tool_failed(
        &self,
        tool_call_id: Uuid,
        error: &IntelligenceToolError,
    ) -> std::result::Result<(), IntelligenceToolError> {
        let excerpt = IntelligenceSummary::with_max_chars(
            format!("{}: {}", error.code.as_str(), error.message),
            DEFAULT_INTELLIGENCE_SUMMARY_CHARS,
        )
        .text;
        self.backend
            .update_tool_call(
                tool_call_id,
                IntelligenceToolCallUpdate {
                    status: Some(ToolCallStatusInternal::Failed),
                    error_excerpt: Some(excerpt),
                    finished_at: Some(Utc::now()),
                    ..IntelligenceToolCallUpdate::default()
                },
            )
            .await
            .map_err(|_| {
                IntelligenceToolError::new(
                    IntelligenceToolErrorCode::AuditError,
                    "failed to mark Ferrex tool-call audit record failed",
                )
                .retryable(true)
            })
    }

    async fn mark_tool_cancelled(
        &self,
        tool_call_id: Uuid,
        error: &IntelligenceToolError,
    ) -> std::result::Result<(), IntelligenceToolError> {
        let excerpt = IntelligenceSummary::with_max_chars(
            format!("{}: {}", error.code.as_str(), error.message),
            DEFAULT_INTELLIGENCE_SUMMARY_CHARS,
        )
        .text;
        self.backend
            .update_tool_call(
                tool_call_id,
                IntelligenceToolCallUpdate {
                    status: Some(ToolCallStatusInternal::Cancelled),
                    error_excerpt: Some(excerpt),
                    finished_at: Some(Utc::now()),
                    ..IntelligenceToolCallUpdate::default()
                },
            )
            .await
            .map_err(|_| {
                IntelligenceToolError::new(
                    IntelligenceToolErrorCode::AuditError,
                    "failed to mark Ferrex tool-call audit record cancelled",
                )
                .retryable(true)
            })
    }

    async fn dispatch_tool(
        &self,
        name: IntelligenceToolName,
        input: ParsedToolInput,
        context: &IntelligenceToolCallContext,
        tool_call_id: Uuid,
    ) -> std::result::Result<RawToolOutput, IntelligenceToolError> {
        match input {
            ParsedToolInput::LibraryOverview(request) => {
                let response = self
                    .backend
                    .library_overview(&request, context.user_id)
                    .await
                    .map_err(IntelligenceToolError::from_media)?;
                Ok(output_library_overview(response))
            }
            ParsedToolInput::Facets(request) => {
                let response = self
                    .backend
                    .library_overview(&request, context.user_id)
                    .await
                    .map_err(IntelligenceToolError::from_media)?;
                Ok(output_facets(response))
            }
            ParsedToolInput::CandidateSearch(request) => {
                let response = self
                    .backend
                    .candidate_search(&request, context.user_id)
                    .await
                    .map_err(IntelligenceToolError::from_media)?;
                Ok(output_candidate_search(&request.query, response))
            }
            ParsedToolInput::MediaQuery(input) => {
                let rows = self
                    .backend
                    .query_media(&input.query)
                    .await
                    .map_err(IntelligenceToolError::from_media)?;
                Ok(output_media_query(rows))
            }
            ParsedToolInput::ItemContext(request) => {
                let response = self
                    .backend
                    .item_context(&request, context.user_id)
                    .await
                    .map_err(IntelligenceToolError::from_media)?;
                Ok(output_item_context(response))
            }
            ParsedToolInput::RelatedContext(input) => {
                if let Some(library_id) = input.library_id {
                    let guard = IntelligenceItemContextRequest {
                        media_id: input.media_id,
                        library_id: Some(library_id),
                        caps: input.caps,
                    };
                    self.backend
                        .item_context(&guard, context.user_id)
                        .await
                        .map_err(IntelligenceToolError::from_media)?;
                }
                let request = IntelligenceRelatedContextRequest {
                    media_id: input.media_id,
                    relationship_kinds: input.relationship_kinds,
                    pagination: input.pagination,
                    caps: input.caps,
                };
                let response = self
                    .backend
                    .related_context(&request, context.user_id)
                    .await
                    .map_err(IntelligenceToolError::from_media)?;
                Ok(output_related_context(response))
            }
            ParsedToolInput::WatchContext(input) => {
                let user_id = context.user_id.ok_or_else(|| {
                    IntelligenceToolError::new(
                        IntelligenceToolErrorCode::ScopeViolation,
                        "watch context requires an authenticated user scope",
                    )
                    .with_details(json!({"tool": name.as_str()}))
                })?;
                let rows = match input.kind {
                    IntelligenceWatchContextKind::InProgress => {
                        self.backend
                            .query_in_progress_media(user_id, &input.query)
                            .await
                    }
                    IntelligenceWatchContextKind::Completed => {
                        self.backend
                            .query_completed_media(user_id, &input.query)
                            .await
                    }
                    IntelligenceWatchContextKind::Unwatched => {
                        self.backend
                            .query_unwatched_media(user_id, &input.query)
                            .await
                    }
                    IntelligenceWatchContextKind::RecentlyWatched => {
                        self.backend
                            .query_recently_watched_media(
                                user_id,
                                input.recent_days,
                                &input.query,
                            )
                            .await
                    }
                }
                .map_err(IntelligenceToolError::from_media)?;
                Ok(output_watch_context(input.kind, rows))
            }
            ParsedToolInput::ArtifactSearch(request) => {
                let response = self
                    .backend
                    .artifact_search(&request, context.user_id)
                    .await
                    .map_err(IntelligenceToolError::from_media)?;
                Ok(output_artifact_search(response, "artifact summaries"))
            }
            ParsedToolInput::ArtifactDetailSample(request) => {
                let response = self
                    .backend
                    .artifact_search(&request, context.user_id)
                    .await
                    .map_err(IntelligenceToolError::from_media)?;
                Ok(output_artifact_search(response, "artifact detail samples"))
            }
            ParsedToolInput::ArtifactFacets(request) => {
                let response = self
                    .backend
                    .artifact_search(&request, context.user_id)
                    .await
                    .map_err(IntelligenceToolError::from_media)?;
                Ok(output_artifact_facets(response))
            }
            ParsedToolInput::CreateDraft(input) => {
                let create = IntelligenceDraftArtifactCreate {
                    artifact_id: input.artifact_id,
                    kind: input.kind,
                    scope: match context.user_id {
                        Some(user_id) => {
                            IntelligenceArtifactScope::User(user_id)
                        }
                        None => IntelligenceArtifactScope::Global,
                    },
                    library_id: input.library_id,
                    media_id: input.media_id,
                    run_id: Some(context.run_id),
                    title: input.title,
                    summary: input.summary,
                    excerpt: input.excerpt,
                    content: input.content,
                    metadata: object_or_empty(input.metadata)?,
                    source_revision: input.source_revision.unwrap_or(1),
                };
                let artifact_id = self
                    .backend
                    .create_draft_artifact(create)
                    .await
                    .map_err(IntelligenceToolError::from_media)?;

                if !input.sources.is_empty() {
                    let mut sources = input.sources;
                    for source in &mut sources {
                        if source.source_tool_call_id.is_none() {
                            source.source_tool_call_id = Some(tool_call_id);
                        }
                        if source.source_run_id.is_none() {
                            source.source_run_id = Some(context.run_id);
                        }
                    }
                    self.backend
                        .replace_artifact_sources(
                            artifact_id,
                            context.user_id,
                            sources,
                        )
                        .await
                        .map_err(IntelligenceToolError::from_media)?;
                }

                if let Some(payload) = self
                    .backend
                    .get_draft_artifact(artifact_id, context.user_id)
                    .await
                    .map_err(IntelligenceToolError::from_media)?
                {
                    Ok(output_create_draft_from_payload(payload))
                } else {
                    Ok(output_create_draft_minimal(artifact_id))
                }
            }
        }
    }
}

fn cancelled_tool_error(name: IntelligenceToolName) -> IntelligenceToolError {
    IntelligenceToolError::new(
        IntelligenceToolErrorCode::Cancelled,
        "Ferrex tool execution was cancelled",
    )
    .with_details(json!({"tool": name.as_str()}))
}

fn definition_for(name: IntelligenceToolName) -> IntelligenceToolDefinition {
    IntelligenceToolDefinition {
        name: name.as_str().to_string(),
        description: name.description().to_string(),
        input_schema: name.input_schema(),
        budget: name.budget(),
        side_effect: name.side_effect(),
    }
}

fn parse_tool_input(
    name: IntelligenceToolName,
    arguments: Value,
) -> std::result::Result<ParsedToolInput, IntelligenceToolError> {
    ensure_object_with_allowed_keys(name, &arguments)?;
    match name {
        IntelligenceToolName::LibraryOverview => {
            parse_arguments(arguments).map(ParsedToolInput::LibraryOverview)
        }
        IntelligenceToolName::Facets => {
            parse_arguments(arguments).map(ParsedToolInput::Facets)
        }
        IntelligenceToolName::CandidateSearch => {
            parse_arguments(arguments).map(ParsedToolInput::CandidateSearch)
        }
        IntelligenceToolName::MediaQuery => {
            parse_arguments(arguments).map(ParsedToolInput::MediaQuery)
        }
        IntelligenceToolName::ItemContext => {
            parse_arguments(arguments).map(ParsedToolInput::ItemContext)
        }
        IntelligenceToolName::RelatedContext => {
            parse_arguments(arguments).map(ParsedToolInput::RelatedContext)
        }
        IntelligenceToolName::WatchContext => {
            parse_arguments(arguments).map(ParsedToolInput::WatchContext)
        }
        IntelligenceToolName::ArtifactSearch => {
            parse_arguments(arguments).map(ParsedToolInput::ArtifactSearch)
        }
        IntelligenceToolName::ArtifactDetailSample => {
            let sample: IntelligenceArtifactDetailSampleToolInput =
                parse_arguments(arguments)?;
            Ok(ParsedToolInput::ArtifactDetailSample(
                sample.into_search_request(),
            ))
        }
        IntelligenceToolName::ArtifactFacets => {
            let facets: IntelligenceArtifactFacetsToolInput =
                parse_arguments(arguments)?;
            Ok(ParsedToolInput::ArtifactFacets(
                facets.into_search_request(),
            ))
        }
        IntelligenceToolName::CreateDraft => {
            parse_arguments(arguments).map(ParsedToolInput::CreateDraft)
        }
    }
}

fn parse_arguments<T: for<'de> Deserialize<'de>>(
    arguments: Value,
) -> std::result::Result<T, IntelligenceToolError> {
    serde_json::from_value(arguments).map_err(|_| {
        IntelligenceToolError::new(
            IntelligenceToolErrorCode::MalformedArguments,
            "tool arguments did not match the declared JSON schema",
        )
    })
}

fn object_or_empty(
    value: Value,
) -> std::result::Result<Value, IntelligenceToolError> {
    match value {
        Value::Null => Ok(json!({})),
        Value::Object(_) => Ok(value),
        _ => Err(IntelligenceToolError::new(
            IntelligenceToolErrorCode::MalformedArguments,
            "draft metadata must be a JSON object",
        )),
    }
}

fn ensure_object_with_allowed_keys(
    name: IntelligenceToolName,
    value: &Value,
) -> std::result::Result<(), IntelligenceToolError> {
    let Some(object) = value.as_object() else {
        return Err(IntelligenceToolError::new(
            IntelligenceToolErrorCode::MalformedArguments,
            "tool arguments must be a JSON object",
        )
        .with_details(json!({"tool": name.as_str()})));
    };
    let allowed = name.allowed_keys();
    let unknown: Vec<&String> = object
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(IntelligenceToolError::new(
            IntelligenceToolErrorCode::MalformedArguments,
            "tool arguments contain fields outside the declared schema",
        )
        .with_details(json!({
            "tool": name.as_str(),
            "unknown_fields": unknown,
        })))
    }
}

fn apply_scope_and_caps(
    name: IntelligenceToolName,
    input: ParsedToolInput,
    context: &IntelligenceToolCallContext,
) -> std::result::Result<ParsedToolInput, IntelligenceToolError> {
    let budget = name.budget();
    match input {
        ParsedToolInput::LibraryOverview(mut request) => {
            scope_library_ids(&mut request.library_ids, context, name)?;
            request.pagination = clamp_pagination(request.pagination, budget);
            request.caps = clamp_caps_for_tool(request.caps, budget);
            Ok(ParsedToolInput::LibraryOverview(request))
        }
        ParsedToolInput::Facets(mut request) => {
            scope_library_ids(&mut request.library_ids, context, name)?;
            request.pagination = clamp_pagination(request.pagination, budget);
            request.caps = clamp_caps_for_tool(request.caps, budget);
            Ok(ParsedToolInput::Facets(request))
        }
        ParsedToolInput::CandidateSearch(mut request) => {
            scope_library_ids(&mut request.library_ids, context, name)?;
            request.pagination = clamp_pagination(request.pagination, budget);
            request.caps = clamp_caps_for_tool(request.caps, budget);
            Ok(ParsedToolInput::CandidateSearch(request))
        }
        ParsedToolInput::MediaQuery(mut input) => {
            scope_raw_library_ids(
                &mut input.query.filters.library_ids,
                context,
                name,
            )?;
            input.query.pagination =
                clamp_query_pagination(input.query.pagination, budget);
            if let Some(user_id) = context.user_id {
                input.query.user_context = Some(user_id);
            }
            input.caps = clamp_caps_for_tool(input.caps, budget);
            Ok(ParsedToolInput::MediaQuery(input))
        }
        ParsedToolInput::ItemContext(mut request) => {
            validate_or_infer_library_id(
                &mut request.library_id,
                context,
                name,
                true,
            )?;
            request.caps = clamp_caps_for_tool(request.caps, budget);
            Ok(ParsedToolInput::ItemContext(request))
        }
        ParsedToolInput::RelatedContext(mut input) => {
            validate_or_infer_library_id(
                &mut input.library_id,
                context,
                name,
                true,
            )?;
            input.pagination = clamp_pagination(input.pagination, budget);
            input.caps = clamp_caps_for_tool(input.caps, budget);
            Ok(ParsedToolInput::RelatedContext(input))
        }
        ParsedToolInput::WatchContext(mut input) => {
            scope_library_ids(&mut input.library_ids, context, name)?;
            input.pagination = clamp_pagination(input.pagination, budget);
            input.caps = clamp_caps_for_tool(input.caps, budget);
            input.recent_days = input.recent_days.clamp(1, 365);
            input.query = watch_media_query(&input, context.user_id, budget);
            Ok(ParsedToolInput::WatchContext(input))
        }
        ParsedToolInput::ArtifactSearch(mut request) => {
            scope_library_ids(&mut request.library_ids, context, name)?;
            request.pagination = clamp_pagination(request.pagination, budget);
            request.caps = clamp_caps_for_tool(request.caps, budget);
            Ok(ParsedToolInput::ArtifactSearch(request))
        }
        ParsedToolInput::ArtifactDetailSample(mut request) => {
            if request.artifact_ids.is_empty() {
                return Err(IntelligenceToolError::new(
                    IntelligenceToolErrorCode::MalformedArguments,
                    "artifact detail sampling requires at least one artifact id",
                )
                .with_details(json!({"tool": name.as_str()})));
            }
            scope_library_ids(&mut request.library_ids, context, name)?;
            request.pagination = clamp_pagination(request.pagination, budget);
            request.caps = clamp_caps_for_tool(request.caps, budget);
            request.artifact_ids.truncate(budget.max_rows as usize);
            Ok(ParsedToolInput::ArtifactDetailSample(request))
        }
        ParsedToolInput::ArtifactFacets(mut request) => {
            scope_library_ids(&mut request.library_ids, context, name)?;
            request.pagination = clamp_pagination(request.pagination, budget);
            request.caps = clamp_caps_for_tool(request.caps, budget);
            Ok(ParsedToolInput::ArtifactFacets(request))
        }
        ParsedToolInput::CreateDraft(mut input) => {
            validate_or_infer_library_id(
                &mut input.library_id,
                context,
                name,
                true,
            )?;
            input.title = truncate_chars(input.title.trim(), 512);
            if input.title.is_empty() {
                return Err(IntelligenceToolError::new(
                    IntelligenceToolErrorCode::MalformedArguments,
                    "draft creation requires a non-empty title",
                )
                .with_details(json!({"tool": name.as_str()})));
            }
            input.summary = input
                .summary
                .map(|value| truncate_chars(value.trim(), 4_000))
                .filter(|value| !value.is_empty());
            input.excerpt = input
                .excerpt
                .map(|value| truncate_chars(value.trim(), 2_048))
                .filter(|value| !value.is_empty());
            input.sources.truncate(usize::from(clamp_limit(
                0,
                DEFAULT_INTELLIGENCE_GROUNDING_LIMIT,
                MAX_INTELLIGENCE_GROUNDING_LIMIT,
            )));
            Ok(ParsedToolInput::CreateDraft(input))
        }
    }
}

fn scope_library_ids(
    library_ids: &mut Vec<LibraryId>,
    context: &IntelligenceToolCallContext,
    name: IntelligenceToolName,
) -> std::result::Result<(), IntelligenceToolError> {
    let Some(allowed) = context.allowed_library_ids.as_deref() else {
        return Ok(());
    };
    if allowed.is_empty() {
        return Err(scope_error(name, "no libraries are available in scope"));
    }
    if library_ids.is_empty() {
        library_ids.extend_from_slice(allowed);
        return Ok(());
    }
    if library_ids.iter().all(|id| allowed.contains(id)) {
        Ok(())
    } else {
        Err(scope_error(
            name,
            "requested library is outside the tool-call scope",
        ))
    }
}

fn scope_raw_library_ids(
    library_ids: &mut Vec<Uuid>,
    context: &IntelligenceToolCallContext,
    name: IntelligenceToolName,
) -> std::result::Result<(), IntelligenceToolError> {
    let Some(allowed) = context.allowed_library_ids.as_deref() else {
        return Ok(());
    };
    if allowed.is_empty() {
        return Err(scope_error(name, "no libraries are available in scope"));
    }
    let allowed_raw: Vec<Uuid> = allowed.iter().map(|id| id.0).collect();
    if library_ids.is_empty() {
        library_ids.extend(allowed_raw);
        return Ok(());
    }
    if library_ids.iter().all(|id| allowed_raw.contains(id)) {
        Ok(())
    } else {
        Err(scope_error(
            name,
            "requested library is outside the tool-call scope",
        ))
    }
}

fn validate_or_infer_library_id(
    library_id: &mut Option<LibraryId>,
    context: &IntelligenceToolCallContext,
    name: IntelligenceToolName,
    require_when_restricted: bool,
) -> std::result::Result<(), IntelligenceToolError> {
    let Some(allowed) = context.allowed_library_ids.as_deref() else {
        return Ok(());
    };
    if allowed.is_empty() {
        return Err(scope_error(name, "no libraries are available in scope"));
    }
    match library_id {
        Some(id) if allowed.contains(id) => Ok(()),
        Some(_) => Err(scope_error(
            name,
            "requested library is outside the tool-call scope",
        )),
        None if allowed.len() == 1 => {
            *library_id = Some(allowed[0]);
            Ok(())
        }
        None if require_when_restricted => Err(scope_error(
            name,
            "library_id is required when multiple libraries are in scope",
        )),
        None => Ok(()),
    }
}

fn scope_error(
    name: IntelligenceToolName,
    message: &'static str,
) -> IntelligenceToolError {
    IntelligenceToolError::new(
        IntelligenceToolErrorCode::ScopeViolation,
        message,
    )
    .with_details(json!({"tool": name.as_str()}))
}

fn clamp_caps_for_tool(
    caps: IntelligenceCaps,
    budget: IntelligenceToolBudget,
) -> IntelligenceCaps {
    IntelligenceCaps {
        candidate_limit: clamp_limit(
            caps.candidate_limit,
            DEFAULT_INTELLIGENCE_CANDIDATE_LIMIT,
            MAX_INTELLIGENCE_CANDIDATE_LIMIT.min(budget.max_rows),
        ),
        artifact_limit: clamp_limit(
            caps.artifact_limit,
            DEFAULT_INTELLIGENCE_ARTIFACT_LIMIT,
            MAX_INTELLIGENCE_ARTIFACT_LIMIT.min(budget.max_rows),
        ),
        related_limit: clamp_limit(
            caps.related_limit,
            DEFAULT_INTELLIGENCE_RELATED_LIMIT,
            MAX_INTELLIGENCE_RELATED_LIMIT.min(budget.max_rows),
        ),
        facet_limit: clamp_limit(
            caps.facet_limit,
            DEFAULT_INTELLIGENCE_FACET_LIMIT,
            MAX_INTELLIGENCE_FACET_LIMIT.min(budget.max_rows),
        ),
        grounding_limit: clamp_limit(
            caps.grounding_limit,
            DEFAULT_INTELLIGENCE_GROUNDING_LIMIT,
            MAX_INTELLIGENCE_GROUNDING_LIMIT.min(budget.max_rows),
        ),
        tool_call_limit: clamp_limit(
            caps.tool_call_limit,
            DEFAULT_INTELLIGENCE_TOOL_CALL_LIMIT,
            MAX_INTELLIGENCE_TOOL_CALL_LIMIT,
        ),
        summary_max_chars: clamp_limit(
            caps.summary_max_chars,
            DEFAULT_INTELLIGENCE_SUMMARY_CHARS,
            MAX_INTELLIGENCE_SUMMARY_CHARS,
        ),
        timed_text_snippet_limit: clamp_limit(
            caps.timed_text_snippet_limit,
            DEFAULT_TIMED_TEXT_SNIPPET_LIMIT,
            MAX_TIMED_TEXT_SNIPPET_LIMIT.min(budget.max_rows),
        ),
        timed_text_segment_limit: clamp_limit(
            caps.timed_text_segment_limit,
            DEFAULT_TIMED_TEXT_SEGMENT_LIMIT,
            MAX_TIMED_TEXT_SEGMENT_LIMIT,
        ),
        timed_text_snippet_max_chars: clamp_limit(
            caps.timed_text_snippet_max_chars,
            DEFAULT_TIMED_TEXT_SNIPPET_CHARS,
            MAX_TIMED_TEXT_SNIPPET_CHARS,
        ),
    }
}

const fn clamp_limit(limit: u16, default: u16, max: u16) -> u16 {
    if limit == 0 {
        if default > max { max } else { default }
    } else if limit > max {
        max
    } else {
        limit
    }
}

fn clamp_pagination(
    pagination: IntelligencePagination,
    budget: IntelligenceToolBudget,
) -> IntelligencePagination {
    IntelligencePagination {
        cursor: pagination.cursor,
        limit: clamp_limit(
            pagination.limit,
            DEFAULT_INTELLIGENCE_PAGE_LIMIT,
            MAX_INTELLIGENCE_PAGE_LIMIT.min(budget.max_rows),
        ),
    }
}

fn clamp_query_pagination(
    pagination: Pagination,
    budget: IntelligenceToolBudget,
) -> Pagination {
    Pagination {
        offset: pagination.offset,
        limit: clamp_usize_limit(
            pagination.limit,
            usize::from(DEFAULT_INTELLIGENCE_PAGE_LIMIT),
            usize::from(budget.max_rows),
        ),
    }
}

fn clamp_usize_limit(limit: usize, default: usize, max: usize) -> usize {
    if limit == 0 {
        default.min(max)
    } else {
        limit.min(max)
    }
}

fn watch_media_query(
    input: &IntelligenceWatchContextToolInput,
    user_id: Option<Uuid>,
    budget: IntelligenceToolBudget,
) -> MediaQuery {
    MediaQuery {
        filters: MediaFilters {
            media_type: None,
            watch_status: None,
            genres: Vec::new(),
            year_range: None,
            rating_range: None,
            resolution_range: None,
            library_ids: input.library_ids.iter().map(|id| id.0).collect(),
        },
        sort: SortCriteria {
            primary: crate::query::types::SortBy::LastWatched,
            order: SortOrder::Descending,
            secondary: None,
        },
        search: None,
        pagination: clamp_query_pagination(
            Pagination {
                offset: 0,
                limit: usize::from(input.pagination.limit),
            },
            budget,
        ),
        user_context: user_id,
    }
}

fn finalize_tool_output(
    name: IntelligenceToolName,
    tool_call_id: Uuid,
    output: RawToolOutput,
    budget: IntelligenceToolBudget,
) -> std::result::Result<IntelligenceToolExecution, IntelligenceToolError> {
    if output.row_count > budget.max_rows {
        return Err(IntelligenceToolError::new(
            IntelligenceToolErrorCode::BudgetExceeded,
            "Ferrex tool returned more rows than its budget allows",
        )
        .with_details(json!({
            "tool": name.as_str(),
            "rows_returned": output.row_count,
            "max_rows": budget.max_rows,
        })));
    }

    let visible = redact_json_for_model(json!({
        "tool": name.as_str(),
        "summary": output.summary,
        "data": output.data,
        "media_ids": output.media_ids,
        "artifact_ids": output.artifact_ids,
    }));
    let visible_bytes = serde_json::to_vec(&visible)
        .map_err(|_| {
            IntelligenceToolError::new(
                IntelligenceToolErrorCode::Internal,
                "Ferrex tool visible output could not be serialized",
            )
        })?
        .len();

    if visible_bytes > budget.max_bytes as usize {
        return Err(IntelligenceToolError::new(
            IntelligenceToolErrorCode::BudgetExceeded,
            "Ferrex tool visible output exceeded its byte budget",
        )
        .with_details(json!({
            "tool": name.as_str(),
            "visible_bytes": visible_bytes,
            "max_bytes": budget.max_bytes,
        })));
    }

    Ok(IntelligenceToolExecution {
        tool_call_id,
        tool_name: name.as_str().to_string(),
        summary: serde_json::from_value(
            visible
                .get("summary")
                .cloned()
                .unwrap_or_else(|| json!(IntelligenceSummary::new(""))),
        )
        .unwrap_or_else(|_| IntelligenceSummary::new("")),
        visible,
        grounding: output.grounding,
        media_ids: output.media_ids,
        artifact_ids: output.artifact_ids,
        row_count: output.row_count,
        visible_bytes: visible_bytes as u32,
    })
}

fn output_library_overview(
    response: IntelligenceLibraryOverviewResponse,
) -> RawToolOutput {
    let row_count = saturating_u16(response.libraries.len());
    let mut artifact_ids = Vec::new();
    let mut media_ids = Vec::new();
    let mut grounding = Vec::new();

    for library in &response.libraries {
        push_artifact_ids(&mut artifact_ids, &library.artifact_ids);
        grounding.push(IntelligenceGroundingRef {
            source: IntelligenceGroundingSource::FerrexLibrary,
            media_id: None,
            artifact_id: None,
            field: Some(format!("library:{}", library.library_id.0)),
            label: library.name.clone(),
            evidence: library.summary.clone(),
        });
        for facet in &library.facets {
            collect_facet_media_ids(facet, &mut media_ids);
        }
    }
    for facet in &response.facets {
        collect_facet_media_ids(facet, &mut media_ids);
    }

    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Returned {} library overviews with {} global facet groups.",
            response.libraries.len(),
            response.facets.len()
        )),
        data: json!({
            "libraries": response.libraries,
            "facets": response.facets,
            "page": response.page,
            "caps": response.caps,
        }),
        grounding,
        media_ids,
        artifact_ids,
        row_count,
    }
}

fn output_facets(
    response: IntelligenceLibraryOverviewResponse,
) -> RawToolOutput {
    let mut facets = response.facets;
    for library in &response.libraries {
        facets.extend(library.facets.clone());
    }
    let mut media_ids = Vec::new();
    for facet in &facets {
        collect_facet_media_ids(facet, &mut media_ids);
    }
    let row_count = saturating_u16(facets.len());
    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Returned {} facet groups across {} libraries.",
            facets.len(),
            response.libraries.len()
        )),
        data: json!({
            "facets": facets,
            "page": response.page,
            "caps": response.caps,
        }),
        grounding: Vec::new(),
        media_ids,
        artifact_ids: Vec::new(),
        row_count,
    }
}

fn output_candidate_search(
    query: &str,
    response: IntelligenceCandidateSearchResponse,
) -> RawToolOutput {
    let mut media_ids = Vec::new();
    let mut artifact_ids = Vec::new();
    let mut grounding = Vec::new();
    for candidate in &response.candidates {
        push_media_id(&mut media_ids, candidate.media.media_id);
        push_artifact_ids(&mut artifact_ids, &candidate.artifact_ids);
        push_artifact_ids(&mut artifact_ids, &candidate.media.artifact_ids);
        grounding.extend(candidate.grounding.clone());
    }
    let row_count = saturating_u16(response.candidates.len());
    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Returned {} candidate media matches for query '{}'.",
            response.candidates.len(),
            truncate_chars(query, 80)
        )),
        data: json!({
            "candidates": response.candidates,
            "page": response.page,
            "caps": response.caps,
        }),
        grounding,
        media_ids,
        artifact_ids,
        row_count,
    }
}

fn output_media_query(rows: Vec<MediaWithStatus>) -> RawToolOutput {
    let mut media_ids = Vec::new();
    let mut grounding = Vec::new();
    for row in &rows {
        push_media_id(&mut media_ids, row.id);
        grounding.push(IntelligenceGroundingRef {
            source: IntelligenceGroundingSource::FerrexLibrary,
            media_id: Some(row.id),
            artifact_id: None,
            field: Some("media_query".to_string()),
            label: "Media query result".to_string(),
            evidence: None,
        });
    }
    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Returned {} media query results.",
            rows.len()
        )),
        data: json!({"results": rows}),
        grounding,
        media_ids,
        artifact_ids: Vec::new(),
        row_count: saturating_u16(rows.len()),
    }
}

fn output_item_context(
    response: IntelligenceItemContextResponse,
) -> RawToolOutput {
    let mut media_ids = Vec::new();
    let mut artifact_ids = Vec::new();
    let mut grounding = response.grounding.clone();

    push_media_id(&mut media_ids, response.item.media.media_id);
    push_artifact_ids(&mut artifact_ids, &response.item.artifact_ids);
    push_artifact_ids(&mut artifact_ids, &response.item.media.artifact_ids);
    for related in &response.related {
        push_media_id(&mut media_ids, related.media.media_id);
        push_artifact_ids(&mut artifact_ids, &related.artifact_ids);
        grounding.extend(related.grounding.clone());
    }
    for artifact in &response.artifacts {
        push_artifact_id(&mut artifact_ids, artifact.artifact_id);
        if let Some(media) = &artifact.media {
            push_media_id(&mut media_ids, media.media_id);
        }
        grounding.extend(artifact.grounding.clone());
    }

    let row_count = 1usize
        .saturating_add(response.related.len())
        .saturating_add(response.artifacts.len());
    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Returned context for '{}' with {} related items and {} artifacts.",
            response.item.media.title,
            response.related.len(),
            response.artifacts.len()
        )),
        data: json!({
            "item": response.item,
            "related": response.related,
            "artifacts": response.artifacts,
            "caps": response.caps,
        }),
        grounding,
        media_ids,
        artifact_ids,
        row_count: saturating_u16(row_count),
    }
}

fn output_related_context(
    response: IntelligenceRelatedContextResponse,
) -> RawToolOutput {
    let mut media_ids = Vec::new();
    let mut artifact_ids = Vec::new();
    let mut grounding = Vec::new();
    push_media_id(&mut media_ids, response.seed.media_id);
    for related in &response.related {
        push_media_id(&mut media_ids, related.media.media_id);
        push_artifact_ids(&mut artifact_ids, &related.artifact_ids);
        grounding.extend(related.grounding.clone());
    }
    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Returned {} related items for '{}'.",
            response.related.len(),
            response.seed.title
        )),
        data: json!({
            "seed": response.seed,
            "related": response.related,
            "page": response.page,
            "caps": response.caps,
        }),
        grounding,
        media_ids,
        artifact_ids,
        row_count: saturating_u16(response.related.len()),
    }
}

fn output_watch_context(
    kind: IntelligenceWatchContextKind,
    rows: Vec<MediaWithStatus>,
) -> RawToolOutput {
    let mut media_ids = Vec::new();
    let mut grounding = Vec::new();
    for row in &rows {
        push_media_id(&mut media_ids, row.id);
        grounding.push(IntelligenceGroundingRef {
            source: IntelligenceGroundingSource::WatchState,
            media_id: Some(row.id),
            artifact_id: None,
            field: Some(kind.as_str().to_string()),
            label: kind.label().to_string(),
            evidence: row
                .watch_status
                .as_ref()
                .map(|status| IntelligenceSummary::new(format!("{status:?}"))),
        });
    }
    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Returned {} {} watch-state media entries.",
            rows.len(),
            kind.as_str()
        )),
        data: json!({
            "kind": kind,
            "results": rows,
        }),
        grounding,
        media_ids,
        artifact_ids: Vec::new(),
        row_count: saturating_u16(rows.len()),
    }
}

fn output_artifact_search(
    response: IntelligenceArtifactSearchResponse,
    label: &str,
) -> RawToolOutput {
    let mut media_ids = Vec::new();
    let mut artifact_ids = Vec::new();
    let mut grounding = Vec::new();
    for artifact in &response.artifacts {
        push_artifact_id(&mut artifact_ids, artifact.artifact_id);
        if let Some(media) = &artifact.media {
            push_media_id(&mut media_ids, media.media_id);
            push_artifact_ids(&mut artifact_ids, &media.artifact_ids);
        }
        grounding.extend(artifact.grounding.clone());
    }
    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Returned {} {}.",
            response.artifacts.len(),
            label
        )),
        data: json!({
            "artifacts": response.artifacts,
            "page": response.page,
            "caps": response.caps,
        }),
        grounding,
        media_ids,
        artifact_ids,
        row_count: saturating_u16(response.artifacts.len()),
    }
}

fn output_artifact_facets(
    response: IntelligenceArtifactSearchResponse,
) -> RawToolOutput {
    let mut kind_counts: Vec<(IntelligenceArtifactKind, u64)> = Vec::new();
    let mut media_kind_counts: Vec<(IntelligenceMediaKind, u64)> = Vec::new();
    let mut artifact_ids = Vec::new();
    let mut media_ids = Vec::new();

    for artifact in &response.artifacts {
        push_artifact_id(&mut artifact_ids, artifact.artifact_id);
        increment_kind_count(&mut kind_counts, artifact.kind);
        if let Some(media) = &artifact.media {
            push_media_id(&mut media_ids, media.media_id);
            increment_media_kind_count(
                &mut media_kind_counts,
                media.media_kind,
            );
        }
    }

    let kind_values: Vec<Value> = kind_counts
        .into_iter()
        .map(|(kind, count)| json!({"key": kind, "count": count}))
        .collect();
    let media_kind_values: Vec<Value> = media_kind_counts
        .into_iter()
        .map(|(kind, count)| json!({"key": kind, "count": count}))
        .collect();

    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Built artifact facets from {} sampled artifacts.",
            response.artifacts.len()
        )),
        data: json!({
            "facets": [
                {"name": "artifact_kind", "values": kind_values},
                {"name": "media_kind", "values": media_kind_values}
            ],
            "sample_size": response.artifacts.len(),
            "page": response.page,
            "caps": response.caps,
        }),
        grounding: Vec::new(),
        media_ids,
        artifact_ids,
        row_count: saturating_u16(response.artifacts.len()),
    }
}

fn output_create_draft_from_payload(
    payload: IntelligenceDraftArtifactPayload,
) -> RawToolOutput {
    let mut media_ids = Vec::new();
    let mut artifact_ids = Vec::new();
    let mut grounding = Vec::new();
    push_artifact_id(&mut artifact_ids, payload.artifact_id);
    if let Some(media_id) = payload.media_id {
        push_media_id(&mut media_ids, media_id);
    }
    for source in &payload.sources {
        if let Some(media_id) = source.source_media_id {
            push_media_id(&mut media_ids, media_id);
        }
        if let Some(artifact_id) = source.source_artifact_id {
            push_artifact_id(&mut artifact_ids, artifact_id);
        }
        grounding.push(IntelligenceGroundingRef {
            source: IntelligenceGroundingSource::ToolCall,
            media_id: source.source_media_id,
            artifact_id: source.source_artifact_id,
            field: Some(format!("source:{}", source.source_ordinal)),
            label: format!("Draft source {:?}", source.source_kind),
            evidence: source.source_excerpt.clone(),
        });
    }
    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Created draft artifact '{}' ({})",
            payload.title, payload.artifact_id
        )),
        data: json!({
            "artifact_id": payload.artifact_id,
            "kind": payload.kind,
            "status": payload.status,
            "library_id": payload.library_id,
            "media_id": payload.media_id,
            "title": payload.title,
            "summary": payload.summary,
            "excerpt": payload.excerpt,
            "source_count": payload.sources.len(),
        }),
        grounding,
        media_ids,
        artifact_ids,
        row_count: 1,
    }
}

fn output_create_draft_minimal(artifact_id: Uuid) -> RawToolOutput {
    RawToolOutput {
        summary: IntelligenceSummary::new(format!(
            "Created draft artifact {artifact_id}."
        )),
        data: json!({"artifact_id": artifact_id}),
        grounding: Vec::new(),
        media_ids: Vec::new(),
        artifact_ids: vec![artifact_id],
        row_count: 1,
    }
}

fn increment_kind_count(
    counts: &mut Vec<(IntelligenceArtifactKind, u64)>,
    kind: IntelligenceArtifactKind,
) {
    if let Some((_, count)) = counts.iter_mut().find(|(k, _)| *k == kind) {
        *count += 1;
    } else {
        counts.push((kind, 1));
    }
}

fn increment_media_kind_count(
    counts: &mut Vec<(IntelligenceMediaKind, u64)>,
    kind: IntelligenceMediaKind,
) {
    if let Some((_, count)) = counts.iter_mut().find(|(k, _)| *k == kind) {
        *count += 1;
    } else {
        counts.push((kind, 1));
    }
}

fn collect_facet_media_ids(
    facet: &IntelligenceFacetGroup,
    media_ids: &mut Vec<MediaID>,
) {
    for value in &facet.values {
        for media_id in &value.sample_media_ids {
            push_media_id(media_ids, *media_id);
        }
    }
}

fn push_media_id(media_ids: &mut Vec<MediaID>, media_id: MediaID) {
    if !media_ids.contains(&media_id) {
        media_ids.push(media_id);
    }
}

fn push_artifact_id(artifact_ids: &mut Vec<Uuid>, artifact_id: Uuid) {
    if !artifact_ids.contains(&artifact_id) {
        artifact_ids.push(artifact_id);
    }
}

fn push_artifact_ids(artifact_ids: &mut Vec<Uuid>, ids: &[Uuid]) {
    for id in ids {
        push_artifact_id(artifact_ids, *id);
    }
}

fn saturating_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn approved_tool_names() -> Vec<&'static str> {
    IntelligenceToolName::ALL
        .into_iter()
        .map(IntelligenceToolName::as_str)
        .collect()
}

fn stable_json_hash(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

fn redact_json_for_model(value: Value) -> Value {
    redact_value(value, REDACTION_MAX_STRING_CHARS, REDACTION_MAX_ARRAY_ITEMS)
}

fn redact_value(
    value: Value,
    max_string_chars: usize,
    max_array_items: usize,
) -> Value {
    match value {
        Value::Object(map) => {
            let mut redacted = serde_json::Map::new();
            for (key, value) in map {
                if is_sensitive_key(&key) {
                    redacted.insert(key, Value::String(REDACTED.to_string()));
                } else {
                    redacted.insert(
                        key,
                        redact_value(value, max_string_chars, max_array_items),
                    );
                }
            }
            Value::Object(redacted)
        }
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .take(max_array_items)
                .map(|value| {
                    redact_value(value, max_string_chars, max_array_items)
                })
                .collect(),
        ),
        Value::String(text) => {
            Value::String(truncate_chars(&text, max_string_chars))
        }
        other => other,
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("password")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("authorization")
        || lower.contains("cookie")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("private_key")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => text[..idx].to_string(),
        None => text.to_string(),
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone)]
enum ParsedToolInput {
    LibraryOverview(IntelligenceLibraryOverviewRequest),
    Facets(IntelligenceLibraryOverviewRequest),
    CandidateSearch(IntelligenceCandidateSearchRequest),
    MediaQuery(IntelligenceMediaQueryToolInput),
    ItemContext(IntelligenceItemContextRequest),
    RelatedContext(IntelligenceRelatedContextToolInput),
    WatchContext(IntelligenceWatchContextToolInput),
    ArtifactSearch(IntelligenceArtifactSearchRequest),
    ArtifactDetailSample(IntelligenceArtifactSearchRequest),
    ArtifactFacets(IntelligenceArtifactSearchRequest),
    CreateDraft(IntelligenceDraftCreateToolInput),
}

/// Typed input for the `media_query` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntelligenceMediaQueryToolInput {
    #[serde(default)]
    pub query: MediaQuery,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Typed input for the `related_context` tool. The library id is used to guard
/// the seed item before delegating to the Phase 1 related-context port.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntelligenceRelatedContextToolInput {
    pub media_id: MediaID,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_id: Option<LibraryId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationship_kinds: Vec<IntelligenceRelationshipKind>,
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

/// Watch-state classes exposed through `watch_context`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceWatchContextKind {
    InProgress,
    Completed,
    Unwatched,
    RecentlyWatched,
}

impl IntelligenceWatchContextKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Unwatched => "unwatched",
            Self::RecentlyWatched => "recently_watched",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::InProgress => "In-progress watch state",
            Self::Completed => "Completed watch state",
            Self::Unwatched => "Unwatched context",
            Self::RecentlyWatched => "Recently watched context",
        }
    }
}

const fn default_recent_days() -> u32 {
    30
}

/// Typed input for the `watch_context` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntelligenceWatchContextToolInput {
    pub kind: IntelligenceWatchContextKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_ids: Vec<LibraryId>,
    #[serde(default = "default_recent_days")]
    pub recent_days: u32,
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
    #[serde(skip)]
    query: MediaQuery,
}

/// Typed input for `artifact_detail_sample`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntelligenceArtifactDetailSampleToolInput {
    #[serde(default)]
    pub artifact_ids: Vec<Uuid>,
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

impl IntelligenceArtifactDetailSampleToolInput {
    fn into_search_request(self) -> IntelligenceArtifactSearchRequest {
        IntelligenceArtifactSearchRequest {
            artifact_ids: self.artifact_ids,
            media_ids: Vec::new(),
            library_ids: Vec::new(),
            kinds: Vec::new(),
            pagination: self.pagination,
            caps: self.caps,
        }
    }
}

/// Typed input for `artifact_facets`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntelligenceArtifactFacetsToolInput {
    #[serde(default)]
    pub media_ids: Vec<MediaID>,
    #[serde(default)]
    pub library_ids: Vec<LibraryId>,
    #[serde(default)]
    pub kinds: Vec<IntelligenceArtifactKind>,
    #[serde(default)]
    pub pagination: IntelligencePagination,
    #[serde(default)]
    pub caps: IntelligenceCaps,
}

impl IntelligenceArtifactFacetsToolInput {
    fn into_search_request(self) -> IntelligenceArtifactSearchRequest {
        IntelligenceArtifactSearchRequest {
            artifact_ids: Vec::new(),
            media_ids: self.media_ids,
            library_ids: self.library_ids,
            kinds: self.kinds,
            pagination: self.pagination,
            caps: self.caps,
        }
    }
}

/// Typed input for `create_draft`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntelligenceDraftCreateToolInput {
    #[serde(default)]
    pub artifact_id: Option<Uuid>,
    pub kind: IntelligenceArtifactKind,
    #[serde(default)]
    pub library_id: Option<LibraryId>,
    #[serde(default)]
    pub media_id: Option<MediaID>,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub excerpt: Option<String>,
    #[serde(default)]
    pub content: Value,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub sources: Vec<IntelligenceArtifactSourceEdge>,
    #[serde(default)]
    pub source_revision: Option<i64>,
}

fn string_array_schema(description: &str) -> Value {
    json!({
        "type": "array",
        "description": description,
        "items": {"type": "string", "format": "uuid"},
    })
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
        "required": required,
    })
}

fn pagination_schema(max_rows: u16) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "cursor": {"type": ["string", "null"]},
            "limit": {"type": "integer", "minimum": 1, "maximum": max_rows}
        }
    })
}

fn caps_schema(max_rows: u16) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "candidate_limit": {"type": "integer", "minimum": 1, "maximum": max_rows},
            "artifact_limit": {"type": "integer", "minimum": 1, "maximum": max_rows},
            "related_limit": {"type": "integer", "minimum": 1, "maximum": max_rows},
            "facet_limit": {"type": "integer", "minimum": 1, "maximum": max_rows},
            "grounding_limit": {"type": "integer", "minimum": 1, "maximum": max_rows},
            "tool_call_limit": {"type": "integer", "minimum": 1, "maximum": MAX_INTELLIGENCE_TOOL_CALL_LIMIT},
            "summary_max_chars": {"type": "integer", "minimum": 1, "maximum": MAX_INTELLIGENCE_SUMMARY_CHARS}
        }
    })
}

fn media_id_schema() -> Value {
    json!({
        "description": "Ferrex MediaID encoded with the stable API serde shape.",
        "oneOf": [
            {"type": "object"},
            {"type": "string"}
        ]
    })
}

fn media_kind_schema() -> Value {
    json!({
        "type": "string",
        "enum": ["movie", "series", "season", "episode"]
    })
}

fn artifact_kind_schema() -> Value {
    json!({
        "type": "string",
        "enum": [
            "summary",
            "embedding_chunk",
            "transcript_segment",
            "user_note",
            "generated_answer",
            "recommendation",
            "audit_record"
        ]
    })
}

fn schema_library_overview() -> Value {
    let budget = IntelligenceToolName::LibraryOverview.budget();
    object_schema(
        json!({
            "library_ids": string_array_schema("Optional library allowlist; omitted values are filled from runtime scope when present."),
            "pagination": pagination_schema(budget.max_rows),
            "caps": caps_schema(budget.max_rows)
        }),
        &[],
    )
}

fn schema_facets() -> Value {
    let budget = IntelligenceToolName::Facets.budget();
    object_schema(
        json!({
            "library_ids": string_array_schema("Optional library allowlist; omitted values are filled from runtime scope when present."),
            "pagination": pagination_schema(budget.max_rows),
            "caps": caps_schema(budget.max_rows)
        }),
        &[],
    )
}

fn schema_candidate_search() -> Value {
    let budget = IntelligenceToolName::CandidateSearch.budget();
    object_schema(
        json!({
            "query": {"type": "string", "minLength": 1, "maxLength": 512},
            "library_ids": string_array_schema("Optional scoped library ids."),
            "media_kinds": {"type": "array", "items": media_kind_schema()},
            "pagination": pagination_schema(budget.max_rows),
            "caps": caps_schema(budget.max_rows),
            "include_artifacts": {"type": "boolean"}
        }),
        &["query"],
    )
}

fn schema_media_query() -> Value {
    let budget = IntelligenceToolName::MediaQuery.budget();
    object_schema(
        json!({
            "query": {
                "type": "object",
                "description": "Bounded Ferrex MediaQuery. The executor clamps pagination.limit and injects runtime user/library scope."
            },
            "caps": caps_schema(budget.max_rows)
        }),
        &[],
    )
}

fn schema_item_context() -> Value {
    let budget = IntelligenceToolName::ItemContext.budget();
    object_schema(
        json!({
            "media_id": media_id_schema(),
            "library_id": {"type": ["string", "null"], "format": "uuid"},
            "caps": caps_schema(budget.max_rows)
        }),
        &["media_id"],
    )
}

fn schema_related_context() -> Value {
    let budget = IntelligenceToolName::RelatedContext.budget();
    object_schema(
        json!({
            "media_id": media_id_schema(),
            "library_id": {"type": ["string", "null"], "format": "uuid"},
            "relationship_kinds": {
                "type": "array",
                "items": {"type": "string"}
            },
            "pagination": pagination_schema(budget.max_rows),
            "caps": caps_schema(budget.max_rows)
        }),
        &["media_id"],
    )
}

fn schema_watch_context() -> Value {
    let budget = IntelligenceToolName::WatchContext.budget();
    object_schema(
        json!({
            "kind": {
                "type": "string",
                "enum": ["in_progress", "completed", "unwatched", "recently_watched"]
            },
            "library_ids": string_array_schema("Optional scoped library ids."),
            "recent_days": {"type": "integer", "minimum": 1, "maximum": 365},
            "pagination": pagination_schema(budget.max_rows),
            "caps": caps_schema(budget.max_rows)
        }),
        &["kind"],
    )
}

fn schema_artifact_search() -> Value {
    let budget = IntelligenceToolName::ArtifactSearch.budget();
    object_schema(
        json!({
            "artifact_ids": string_array_schema("Optional artifact ids."),
            "media_ids": {"type": "array", "items": media_id_schema()},
            "library_ids": string_array_schema("Optional scoped library ids."),
            "kinds": {"type": "array", "items": artifact_kind_schema()},
            "pagination": pagination_schema(budget.max_rows),
            "caps": caps_schema(budget.max_rows)
        }),
        &[],
    )
}

fn schema_artifact_detail_sample() -> Value {
    let budget = IntelligenceToolName::ArtifactDetailSample.budget();
    object_schema(
        json!({
            "artifact_ids": string_array_schema("Artifact ids to sample."),
            "pagination": pagination_schema(budget.max_rows),
            "caps": caps_schema(budget.max_rows)
        }),
        &["artifact_ids"],
    )
}

fn schema_artifact_facets() -> Value {
    let budget = IntelligenceToolName::ArtifactFacets.budget();
    object_schema(
        json!({
            "media_ids": {"type": "array", "items": media_id_schema()},
            "library_ids": string_array_schema("Optional scoped library ids."),
            "kinds": {"type": "array", "items": artifact_kind_schema()},
            "pagination": pagination_schema(budget.max_rows),
            "caps": caps_schema(budget.max_rows)
        }),
        &[],
    )
}

fn schema_create_draft() -> Value {
    object_schema(
        json!({
            "artifact_id": {"type": ["string", "null"], "format": "uuid"},
            "kind": artifact_kind_schema(),
            "library_id": {"type": ["string", "null"], "format": "uuid"},
            "media_id": media_id_schema(),
            "title": {"type": "string", "minLength": 1, "maxLength": 512},
            "summary": {"type": ["string", "null"], "maxLength": 4000},
            "excerpt": {"type": ["string", "null"], "maxLength": 2048},
            "content": {"type": ["object", "array", "string", "number", "boolean", "null"]},
            "metadata": {"type": ["object", "null"]},
            "sources": {"type": "array", "items": {"type": "object"}},
            "source_revision": {"type": ["integer", "null"], "minimum": 0}
        }),
        &["kind", "title"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::types::intelligence::{
            IntelligenceArtifactStatus, IntelligenceCandidate,
            IntelligenceContextItem, IntelligenceMediaRef,
            IntelligencePageInfo,
        },
        query::types::{SearchField, SearchQuery},
    };
    use ferrex_model::{MovieID, SeriesID};
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeBackend {
        calls: Mutex<Vec<String>>,
        creates: Mutex<Vec<IntelligenceToolCallCreate>>,
        updates: Mutex<Vec<(Uuid, IntelligenceToolCallUpdate)>>,
        candidate_response: Mutex<Option<IntelligenceCandidateSearchResponse>>,
        draft_payload: Mutex<Option<IntelligenceDraftArtifactPayload>>,
        last_candidate_request:
            Mutex<Option<IntelligenceCandidateSearchRequest>>,
        last_media_query: Mutex<Option<MediaQuery>>,
    }

    impl FakeBackend {
        fn registry(self: &Arc<Self>) -> IntelligenceToolRegistry {
            IntelligenceToolRegistry::new(self.clone())
        }

        fn creates(&self) -> Vec<IntelligenceToolCallCreate> {
            self.creates.lock().unwrap().clone()
        }

        fn updates(&self) -> Vec<(Uuid, IntelligenceToolCallUpdate)> {
            self.updates.lock().unwrap().clone()
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl IntelligenceToolBackend for FakeBackend {
        async fn library_overview(
            &self,
            request: &IntelligenceLibraryOverviewRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceLibraryOverviewResponse> {
            self.calls
                .lock()
                .unwrap()
                .push("library_overview".to_string());
            Ok(IntelligenceLibraryOverviewResponse {
                libraries: request
                    .library_ids
                    .iter()
                    .map(|library_id| crate::api::types::intelligence::IntelligenceLibraryOverview {
                        library_id: *library_id,
                        name: format!("Library {}", library_id.0),
                        summary: Some(IntelligenceSummary::new("Scoped library")),
                        counts: Default::default(),
                        facets: Vec::new(),
                        artifact_ids: Vec::new(),
                    })
                    .collect(),
                facets: Vec::new(),
                page: IntelligencePageInfo::default(),
                caps: request.caps,
                generated_at_epoch_seconds: None,
            })
        }

        async fn candidate_search(
            &self,
            request: &IntelligenceCandidateSearchRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceCandidateSearchResponse> {
            self.calls
                .lock()
                .unwrap()
                .push("candidate_search".to_string());
            *self.last_candidate_request.lock().unwrap() =
                Some(request.clone());
            Ok(self
                .candidate_response
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| IntelligenceCandidateSearchResponse {
                    candidates: Vec::new(),
                    page: IntelligencePageInfo::default(),
                    caps: request.caps,
                }))
        }

        async fn item_context(
            &self,
            request: &IntelligenceItemContextRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceItemContextResponse> {
            self.calls.lock().unwrap().push("item_context".to_string());
            let mut media = IntelligenceMediaRef::new(request.media_id, "Seed");
            media.library_id = request.library_id;
            Ok(IntelligenceItemContextResponse {
                item: IntelligenceContextItem {
                    media,
                    summary: Some(IntelligenceSummary::new("Seed context")),
                    facets: Vec::new(),
                    artifact_ids: Vec::new(),
                    provenance: Vec::new(),
                },
                related: Vec::new(),
                artifacts: Vec::new(),
                grounding: Vec::new(),
                caps: request.caps,
            })
        }

        async fn related_context(
            &self,
            request: &IntelligenceRelatedContextRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceRelatedContextResponse> {
            self.calls
                .lock()
                .unwrap()
                .push("related_context".to_string());
            Ok(IntelligenceRelatedContextResponse {
                seed: IntelligenceMediaRef::new(request.media_id, "Seed"),
                related: Vec::new(),
                page: IntelligencePageInfo::default(),
                caps: request.caps,
            })
        }

        async fn artifact_search(
            &self,
            request: &IntelligenceArtifactSearchRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceArtifactSearchResponse> {
            self.calls
                .lock()
                .unwrap()
                .push("artifact_search".to_string());
            Ok(IntelligenceArtifactSearchResponse {
                artifacts: Vec::new(),
                page: IntelligencePageInfo::default(),
                caps: request.caps,
            })
        }

        async fn get_draft_artifact(
            &self,
            artifact_id: Uuid,
            _user_id: Option<Uuid>,
        ) -> Result<Option<IntelligenceDraftArtifactPayload>> {
            self.calls
                .lock()
                .unwrap()
                .push("get_draft_artifact".to_string());
            Ok(self
                .draft_payload
                .lock()
                .unwrap()
                .clone()
                .or_else(|| Some(draft_payload(artifact_id))))
        }

        async fn create_draft_artifact(
            &self,
            _create: IntelligenceDraftArtifactCreate,
        ) -> Result<Uuid> {
            self.calls
                .lock()
                .unwrap()
                .push("create_draft_artifact".to_string());
            Ok(Uuid::from_u128(900))
        }

        async fn replace_artifact_sources(
            &self,
            _artifact_id: Uuid,
            _user_id: Option<Uuid>,
            _sources: Vec<IntelligenceArtifactSourceEdge>,
        ) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push("replace_artifact_sources".to_string());
            Ok(())
        }

        async fn query_media(
            &self,
            query: &MediaQuery,
        ) -> Result<Vec<MediaWithStatus>> {
            self.calls.lock().unwrap().push("query_media".to_string());
            *self.last_media_query.lock().unwrap() = Some(query.clone());
            Ok(vec![MediaWithStatus {
                id: MediaID::Movie(MovieID(Uuid::from_u128(1))),
                watch_status: None,
            }])
        }

        async fn query_in_progress_media(
            &self,
            _user_id: Uuid,
            query: &MediaQuery,
        ) -> Result<Vec<MediaWithStatus>> {
            self.query_media(query).await
        }

        async fn query_completed_media(
            &self,
            _user_id: Uuid,
            query: &MediaQuery,
        ) -> Result<Vec<MediaWithStatus>> {
            self.query_media(query).await
        }

        async fn query_unwatched_media(
            &self,
            _user_id: Uuid,
            query: &MediaQuery,
        ) -> Result<Vec<MediaWithStatus>> {
            self.query_media(query).await
        }

        async fn query_recently_watched_media(
            &self,
            _user_id: Uuid,
            _recent_days: u32,
            query: &MediaQuery,
        ) -> Result<Vec<MediaWithStatus>> {
            self.query_media(query).await
        }

        async fn create_tool_call(
            &self,
            mut create: IntelligenceToolCallCreate,
        ) -> Result<Uuid> {
            let id = Uuid::from_u128(
                700 + self.creates.lock().unwrap().len() as u128,
            );
            create.tool_call_id = Some(id);
            self.creates.lock().unwrap().push(create);
            Ok(id)
        }

        async fn update_tool_call(
            &self,
            tool_call_id: Uuid,
            update: IntelligenceToolCallUpdate,
        ) -> Result<()> {
            self.updates.lock().unwrap().push((tool_call_id, update));
            Ok(())
        }
    }

    fn context() -> IntelligenceToolCallContext {
        IntelligenceToolCallContext {
            run_id: Uuid::from_u128(100),
            sequence: 1,
            user_id: Some(Uuid::from_u128(200)),
            allowed_library_ids: None,
            idempotency_key: None,
        }
    }

    fn library(id: u128) -> LibraryId {
        LibraryId(Uuid::from_u128(id))
    }

    fn candidate(media_id: MediaID) -> IntelligenceCandidate {
        IntelligenceCandidate {
            media: IntelligenceMediaRef::new(media_id, "Candidate"),
            summary: Some(IntelligenceSummary::new("Candidate summary")),
            match_reason: Some(IntelligenceSummary::new("Matched query")),
            score: Some(0.9),
            artifact_ids: Vec::new(),
            grounding: Vec::new(),
            transcript_grounding: Vec::new(),
        }
    }

    fn draft_payload(artifact_id: Uuid) -> IntelligenceDraftArtifactPayload {
        IntelligenceDraftArtifactPayload {
            artifact_id,
            kind: IntelligenceArtifactKind::GeneratedAnswer,
            status: IntelligenceArtifactStatus::Draft,
            library_id: Some(library(1)),
            owner_user_id: Some(Uuid::from_u128(200)),
            media_id: None,
            run_id: Some(Uuid::from_u128(100)),
            title: "Draft".to_string(),
            summary: Some(IntelligenceSummary::new("Draft summary")),
            excerpt: None,
            content: json!({"body": "hidden from model output"}),
            metadata: json!({}),
            sources: Vec::new(),
            created_at_epoch_seconds: None,
            updated_at_epoch_seconds: None,
        }
    }

    #[test]
    fn registry_exposes_only_approved_tool_definitions() {
        let backend = Arc::new(FakeBackend::default());
        let registry = backend.registry();
        let definitions = registry.definitions();
        let names: Vec<_> =
            definitions.iter().map(|d| d.name.as_str()).collect();

        assert_eq!(names, approved_tool_names());
        assert!(definitions.iter().all(|definition| {
            definition.input_schema["additionalProperties"] == json!(false)
                && definition.budget.max_rows > 0
                && definition.budget.max_bytes > 0
                && definition.budget.max_time_ms > 0
        }));
        assert_eq!(
            definitions
                .iter()
                .filter(|definition| definition.side_effect
                    == IntelligenceToolSideEffect::CreateDraft)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn candidate_search_clamps_caps_and_page_limits() {
        let backend = Arc::new(FakeBackend::default());
        let registry = backend.registry();
        let mut args =
            serde_json::to_value(IntelligenceCandidateSearchRequest {
                query: "arrival".to_string(),
                library_ids: Vec::new(),
                media_kinds: Vec::new(),
                pagination: IntelligencePagination::new(None, 500),
                caps: IntelligenceCaps {
                    candidate_limit: 500,
                    artifact_limit: 500,
                    related_limit: 500,
                    facet_limit: 500,
                    grounding_limit: 500,
                    tool_call_limit: 500,
                    summary_max_chars: 5_000,
                    timed_text_snippet_limit: 500,
                    timed_text_segment_limit: 500,
                    timed_text_snippet_max_chars: 5_000,
                },
                include_artifacts: true,
                include_transcript_grounding: false,
            })
            .unwrap();
        args["pagination"]["limit"] = json!(500);
        args["caps"]["candidate_limit"] = json!(500);

        let output = registry
            .execute(&context(), "candidate_search", args)
            .await
            .unwrap();

        assert_eq!(output.row_count, 0);
        let request = backend
            .last_candidate_request
            .lock()
            .unwrap()
            .clone()
            .unwrap();
        assert_eq!(request.pagination.limit, 50);
        assert_eq!(request.caps.candidate_limit, 50);
        assert_eq!(
            request.caps.summary_max_chars,
            MAX_INTELLIGENCE_SUMMARY_CHARS
        );
    }

    #[tokio::test]
    async fn media_query_injects_library_scope_and_caps_limit() {
        let backend = Arc::new(FakeBackend::default());
        let registry = backend.registry();
        let mut scoped = context();
        scoped.allowed_library_ids = Some(vec![library(55)]);
        let args = serde_json::to_value(IntelligenceMediaQueryToolInput {
            query: MediaQuery {
                filters: MediaFilters::default(),
                sort: SortCriteria::default(),
                search: Some(SearchQuery {
                    text: "seed".to_string(),
                    fields: vec![SearchField::Title],
                    fuzzy: false,
                }),
                pagination: Pagination {
                    offset: 0,
                    limit: 5_000,
                },
                user_context: None,
            },
            caps: IntelligenceCaps::default(),
        })
        .unwrap();

        registry
            .execute(&scoped, "media_query", args)
            .await
            .unwrap();
        let query = backend.last_media_query.lock().unwrap().clone().unwrap();

        assert_eq!(query.pagination.limit, 50);
        assert_eq!(query.filters.library_ids, vec![library(55).0]);
        assert_eq!(query.user_context, scoped.user_id);
    }

    #[tokio::test]
    async fn scope_isolation_rejects_out_of_scope_library() {
        let backend = Arc::new(FakeBackend::default());
        let registry = backend.registry();
        let mut scoped = context();
        scoped.allowed_library_ids = Some(vec![library(1)]);

        let err = registry
            .execute(
                &scoped,
                "library_overview",
                json!({"library_ids": [library(2)]}),
            )
            .await
            .unwrap_err();

        assert_eq!(err.code, IntelligenceToolErrorCode::ScopeViolation);
        assert!(!backend.calls().contains(&"library_overview".to_string()));
        assert!(
            backend.updates().iter().any(|(_, update)| update.status
                == Some(ToolCallStatusInternal::Failed))
        );
    }

    #[tokio::test]
    async fn unknown_tools_are_rejected_before_audit_creation() {
        let backend = Arc::new(FakeBackend::default());
        let registry = backend.registry();

        let err = registry
            .execute(&context(), "shell", json!({"command": "echo nope"}))
            .await
            .unwrap_err();

        assert_eq!(err.code, IntelligenceToolErrorCode::UnknownTool);
        assert!(backend.creates().is_empty());
        assert!(backend.updates().is_empty());
    }

    #[tokio::test]
    async fn malformed_arguments_fail_deterministically_and_audit_failure() {
        let backend = Arc::new(FakeBackend::default());
        let registry = backend.registry();

        let err = registry
            .execute(&context(), "candidate_search", json!({}))
            .await
            .unwrap_err();

        assert_eq!(err.code, IntelligenceToolErrorCode::MalformedArguments);
        assert_eq!(backend.creates().len(), 1);
        assert!(
            backend.updates().iter().any(|(_, update)| update.status
                == Some(ToolCallStatusInternal::Failed))
        );
    }

    #[tokio::test]
    async fn over_budget_results_are_rejected() {
        let backend = Arc::new(FakeBackend::default());
        let candidates: Vec<_> = (0..55)
            .map(|idx| {
                candidate(MediaID::Movie(MovieID(Uuid::from_u128(idx + 1))))
            })
            .collect();
        *backend.candidate_response.lock().unwrap() =
            Some(IntelligenceCandidateSearchResponse {
                candidates,
                page: IntelligencePageInfo::default(),
                caps: IntelligenceCaps::default(),
            });
        let registry = backend.registry();

        let err = registry
            .execute(&context(), "candidate_search", json!({"query": "many"}))
            .await
            .unwrap_err();

        assert_eq!(err.code, IntelligenceToolErrorCode::BudgetExceeded);
        assert!(
            backend.updates().iter().any(|(_, update)| update.status
                == Some(ToolCallStatusInternal::Failed))
        );
    }

    #[tokio::test]
    async fn audit_arguments_are_redacted_for_draft_creation() {
        let backend = Arc::new(FakeBackend::default());
        let registry = backend.registry();
        let mut scoped = context();
        scoped.allowed_library_ids = Some(vec![library(1)]);

        let output = registry
            .execute(
                &scoped,
                "create_draft",
                json!({
                    "kind": "generated_answer",
                    "library_id": library(1),
                    "title": "Draft with sensitive inputs",
                    "content": {"answer": "safe", "api_key": "sk-live-secret"},
                    "metadata": {"refresh_token": "rt-secret"}
                }),
            )
            .await
            .unwrap();

        assert_eq!(output.row_count, 1);
        let creates = backend.creates();
        let arguments = creates[0].arguments.to_string();
        assert!(!arguments.contains("sk-live-secret"));
        assert!(!arguments.contains("rt-secret"));
        assert!(arguments.contains(REDACTED));

        let succeeded_result = backend
            .updates()
            .into_iter()
            .filter_map(|(_, update)| update.result)
            .last()
            .unwrap()
            .to_string();
        assert!(!succeeded_result.contains("hidden from model output"));
        assert!(
            !output
                .visible
                .to_string()
                .contains("hidden from model output")
        );
    }

    #[tokio::test]
    async fn watch_context_requires_user_scope() {
        let backend = Arc::new(FakeBackend::default());
        let registry = backend.registry();
        let mut no_user = context();
        no_user.user_id = None;

        let err = registry
            .execute(
                &no_user,
                "watch_context",
                json!({"kind": "recently_watched"}),
            )
            .await
            .unwrap_err();

        assert_eq!(err.code, IntelligenceToolErrorCode::ScopeViolation);
    }

    #[test]
    fn redaction_catches_nested_sensitive_keys_without_hiding_ids() {
        let value = redact_json_for_model(json!({
            "artifact_ids": [Uuid::from_u128(1)],
            "nested": {"password": "hunter2", "title": "Arrival"}
        }));
        let text = value.to_string();

        assert!(text.contains("artifact_ids"));
        assert!(text.contains("Arrival"));
        assert!(!text.contains("hunter2"));
        assert!(text.contains(REDACTED));
    }

    #[test]
    fn media_id_schema_accepts_stable_serde_shapes() {
        let media_id = MediaID::Series(SeriesID(Uuid::from_u128(42)));
        let value = serde_json::to_value(media_id).unwrap();
        assert!(value.is_object() || value.is_string());
    }
}
