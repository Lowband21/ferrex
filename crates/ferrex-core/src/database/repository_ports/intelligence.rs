//! Repository port for the Phase 1 intelligence bounded context.
//!
//! The [`IntelligenceRepository`] trait exposes the read-model refresh
//! builders, bounded query services, artifact CRUD, and run/tool-call audit
//! persistence used by downstream intelligence handlers. Implementations
//! enforce caps and user scope internally so callers cannot accidentally
//! exceed the stable API budgets or read another user's scoped rows.
//!
//! Storage contracts live in migration `007_intelligence_foundation.sql` and
//! the DTO boundary lives in [`crate::api::types::intelligence`].

use async_trait::async_trait;
use ferrex_model::{LibraryId, MediaID};
use uuid::Uuid;

use crate::{
    api::types::intelligence::{
        IntelligenceArtifactKind, IntelligenceArtifactSearchRequest,
        IntelligenceArtifactSearchResponse, IntelligenceCandidateSearchRequest,
        IntelligenceCandidateSearchResponse, IntelligenceItemContextRequest,
        IntelligenceItemContextResponse, IntelligenceLibraryOverviewRequest,
        IntelligenceLibraryOverviewResponse, IntelligenceRelatedContextRequest,
        IntelligenceRelatedContextResponse, IntelligenceRunAuditRequest,
        IntelligenceRunAuditResponse,
    },
    error::Result,
};

/// Scope for an intelligence artifact. `Global` artifacts are shared across
/// users; `User` artifacts are owned by a single user and must stay isolated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelligenceArtifactScope {
    Global,
    User(Uuid),
}

impl IntelligenceArtifactScope {
    /// Returns the owning user id, or `None` for global artifacts.
    pub fn user_id(&self) -> Option<Uuid> {
        match self {
            IntelligenceArtifactScope::Global => None,
            IntelligenceArtifactScope::User(id) => Some(*id),
        }
    }
}

/// Operational kind for an intelligence run. Matches the
/// `intelligence_runs.run_kind` database enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelligenceRunKind {
    Index,
    Search,
    Summarize,
    Recommend,
    Answer,
    Maintenance,
}

impl IntelligenceRunKind {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            IntelligenceRunKind::Index => "index",
            IntelligenceRunKind::Search => "search",
            IntelligenceRunKind::Summarize => "summarize",
            IntelligenceRunKind::Recommend => "recommend",
            IntelligenceRunKind::Answer => "answer",
            IntelligenceRunKind::Maintenance => "maintenance",
        }
    }
}

/// Lifecycle status for an intelligence run. Matches the
/// `intelligence_runs.status` database enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            IntelligenceRunStatus::Queued => "queued",
            IntelligenceRunStatus::Running => "running",
            IntelligenceRunStatus::Succeeded => "succeeded",
            IntelligenceRunStatus::Failed => "failed",
            IntelligenceRunStatus::Cancelled => "cancelled",
        }
    }
}

/// Operational kind for a single tool call within a run. Matches the
/// `intelligence_tool_calls.tool_kind` database enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelligenceToolKind {
    Search,
    ReadModel,
    Artifact,
    External,
    System,
}

impl IntelligenceToolKind {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            IntelligenceToolKind::Search => "search",
            IntelligenceToolKind::ReadModel => "read_model",
            IntelligenceToolKind::Artifact => "artifact",
            IntelligenceToolKind::External => "external",
            IntelligenceToolKind::System => "system",
        }
    }
}

/// Lifecycle status for a single tool call. Matches the
/// `intelligence_tool_calls.status` database enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelligenceToolCallStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}

impl IntelligenceToolCallStatus {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            IntelligenceToolCallStatus::Queued => "queued",
            IntelligenceToolCallStatus::Running => "running",
            IntelligenceToolCallStatus::Succeeded => "succeeded",
            IntelligenceToolCallStatus::Failed => "failed",
            IntelligenceToolCallStatus::Skipped => "skipped",
            IntelligenceToolCallStatus::Cancelled => "cancelled",
        }
    }
}

/// Write payload for upserting an intelligence artifact.
///
/// When `artifact_id` is `None` a new artifact is created; otherwise the
/// existing row is updated. `content_hash` is computed deterministically from
/// the bounded payload by the implementation.
#[derive(Debug, Clone)]
pub struct IntelligenceArtifactUpsert {
    /// Existing artifact id to update, or `None` to create a new artifact.
    pub artifact_id: Option<Uuid>,
    pub kind: IntelligenceArtifactKind,
    pub scope: IntelligenceArtifactScope,
    pub library_id: Option<LibraryId>,
    pub media_id: Option<MediaID>,
    pub run_id: Option<Uuid>,
    /// Optional artifact id superseded by this one. The superseded artifact is
    /// marked `superseded` by the implementation.
    pub supersedes_artifact_id: Option<Uuid>,
    pub title: String,
    pub summary: Option<String>,
    pub excerpt: Option<String>,
    pub content: serde_json::Value,
    pub metadata: serde_json::Value,
    pub source_revision: i64,
}

/// Write payload for creating an intelligence run.
#[derive(Debug, Clone)]
pub struct IntelligenceRunCreate {
    /// Optional run id; when `None` the database generates one.
    pub run_id: Option<Uuid>,
    pub run_kind: IntelligenceRunKind,
    pub library_id: Option<LibraryId>,
    pub user_id: Option<Uuid>,
    pub media_id: Option<MediaID>,
    /// Optional idempotency key; when set the implementation returns the
    /// existing run id instead of creating a duplicate.
    pub idempotency_key: Option<String>,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    /// Optional SHA-256 hex request hash.
    pub request_hash: Option<String>,
    pub prompt_excerpt: Option<String>,
    pub metadata: serde_json::Value,
}

/// Update payload for an intelligence run. `None` fields are left unchanged.
#[derive(Debug, Clone, Default)]
pub struct IntelligenceRunUpdate {
    pub status: Option<IntelligenceRunStatus>,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    pub result_summary: Option<String>,
    pub error_excerpt: Option<String>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<serde_json::Value>,
}

/// Filter for listing intelligence runs.
#[derive(Debug, Clone, Default)]
pub struct IntelligenceRunListFilter {
    pub library_id: Option<LibraryId>,
    pub user_id: Option<Uuid>,
    pub run_kind: Option<IntelligenceRunKind>,
    pub status: Option<IntelligenceRunStatus>,
    pub limit: u16,
}

/// Bounded summary of an intelligence run.
#[derive(Debug, Clone)]
pub struct IntelligenceRunSummary {
    pub run_id: Uuid,
    pub run_kind: IntelligenceRunKind,
    pub status: IntelligenceRunStatus,
    pub library_id: Option<LibraryId>,
    pub user_id: Option<Uuid>,
    pub media_id: Option<MediaID>,
    pub correlation_id: Uuid,
    pub idempotency_key: Option<String>,
    pub model_name: Option<String>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Write payload for creating a tool-call audit record within a run.
#[derive(Debug, Clone)]
pub struct IntelligenceToolCallCreate {
    pub tool_call_id: Option<Uuid>,
    pub run_id: Uuid,
    pub sequence: i32,
    pub tool_kind: IntelligenceToolKind,
    pub tool_name: String,
    pub idempotency_key: Option<String>,
    pub input_hash: Option<String>,
    pub arguments: serde_json::Value,
}

/// Update payload for a tool-call audit record. `None` fields are unchanged.
#[derive(Debug, Clone, Default)]
pub struct IntelligenceToolCallUpdate {
    pub status: Option<IntelligenceToolCallStatus>,
    pub output_hash: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error_excerpt: Option<String>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Bounded summary of a single tool-call audit record.
#[derive(Debug, Clone)]
pub struct IntelligenceToolCallSummary {
    pub tool_call_id: Uuid,
    pub run_id: Uuid,
    pub sequence: i32,
    pub tool_kind: IntelligenceToolKind,
    pub tool_name: String,
    pub status: IntelligenceToolCallStatus,
    pub idempotency_key: Option<String>,
    pub input_hash: Option<String>,
    pub output_hash: Option<String>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Repository port for Phase 1 intelligence read models and audit persistence.
///
/// All query methods take the requesting user's id (when available) and enforce
/// user scope internally: user-scoped artifacts and watch-state context are only
/// visible to their owner, while global/library rows are visible to everyone.
/// Caps from the request DTOs are clamped by the implementation before querying
/// so result counts stay within the stable API budgets.
#[async_trait]
pub trait IntelligenceRepository: Send + Sync {
    // ----- Read-model refresh / builders -----

    /// Refresh bounded context and search read models for every available
    /// media item in a library.
    ///
    /// When `user_id` is `None`, global `metadata`/`combined` rows are upserted
    /// for movies, series, seasons, and episodes that have available media.
    /// When `user_id` is `Some`, user-scoped `watch_state` rows are upserted
    /// for items the user has watch progress for. Returns the number of media
    /// items refreshed.
    async fn refresh_library_read_models(
        &self,
        library_id: LibraryId,
        user_id: Option<Uuid>,
    ) -> Result<u64>;

    /// Refresh read models for a single media item, if it has available media.
    async fn refresh_media_read_model(
        &self,
        library_id: LibraryId,
        media_id: MediaID,
        user_id: Option<Uuid>,
    ) -> Result<()>;

    /// Invalidate read models for a media item by marking them `invalidated`.
    async fn invalidate_media_read_model(
        &self,
        library_id: LibraryId,
        media_id: MediaID,
        user_id: Option<Uuid>,
        reason: &str,
    ) -> Result<()>;

    // ----- Bounded query services -----

    /// Build a bounded library overview with counts, facets, and artifact ids.
    async fn library_overview(
        &self,
        request: &IntelligenceLibraryOverviewRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceLibraryOverviewResponse>;

    /// Search for bounded candidate media documents to ground a task.
    async fn candidate_search(
        &self,
        request: &IntelligenceCandidateSearchRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceCandidateSearchResponse>;

    /// Build a bounded item context packet for a single media item.
    async fn item_context(
        &self,
        request: &IntelligenceItemContextRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceItemContextResponse>;

    /// Build bounded related-item context around a seed media item.
    async fn related_context(
        &self,
        request: &IntelligenceRelatedContextRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRelatedContextResponse>;

    // ----- Artifacts -----

    /// Search/list artifact summaries by id, media, library, or kind.
    async fn artifact_search(
        &self,
        request: &IntelligenceArtifactSearchRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceArtifactSearchResponse>;

    /// Look up a single artifact summary by id.
    async fn get_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<
        Option<crate::api::types::intelligence::IntelligenceArtifactSummary>,
    >;

    /// Upsert a global or user-scoped artifact and return its id.
    async fn upsert_artifact(
        &self,
        upsert: IntelligenceArtifactUpsert,
    ) -> Result<Uuid>;

    /// Invalidate an artifact by marking it `invalidated`.
    async fn invalidate_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
        reason: &str,
    ) -> Result<()>;

    // ----- Run / tool-call audit -----

    /// Create an intelligence run and return its id. Idempotent on
    /// `idempotency_key`.
    async fn create_run(&self, create: IntelligenceRunCreate) -> Result<Uuid>;

    /// Update an intelligence run's status and audit fields.
    async fn update_run(
        &self,
        run_id: Uuid,
        update: IntelligenceRunUpdate,
    ) -> Result<()>;

    /// List bounded intelligence run summaries matching a filter.
    async fn list_runs(
        &self,
        filter: IntelligenceRunListFilter,
    ) -> Result<Vec<IntelligenceRunSummary>>;

    /// Build a bounded audit response for a single run and its tool calls.
    async fn run_audit(
        &self,
        request: &IntelligenceRunAuditRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRunAuditResponse>;

    /// Create a tool-call audit record within a run and return its id.
    async fn create_tool_call(
        &self,
        create: IntelligenceToolCallCreate,
    ) -> Result<Uuid>;

    /// Update a tool-call audit record's status and result fields.
    async fn update_tool_call(
        &self,
        tool_call_id: Uuid,
        update: IntelligenceToolCallUpdate,
    ) -> Result<()>;

    /// List bounded tool-call audit summaries for a run, ordered by sequence.
    async fn list_tool_calls(
        &self,
        run_id: Uuid,
    ) -> Result<Vec<IntelligenceToolCallSummary>>;
}
