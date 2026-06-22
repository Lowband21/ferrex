//! Repository port for subtitle-first timed-text corpus storage.
//!
//! The transcript repository owns durable source manifests, timestamped segment
//! upserts, processing status, invalidation/purge paths, and bounded snippet
//! search. Public query payloads return timestamped snippets and source/artifact
//! ids only; whole transcript bodies remain internal storage details.

use async_trait::async_trait;
use ferrex_model::{LibraryId, MediaID};
use uuid::Uuid;

use crate::{
    api::types::intelligence::{
        TimedTextSnippetSearchRequest, TimedTextSnippetSearchResponse,
        TimedTextSourceKind,
    },
    error::Result,
};

/// Lifecycle status for a transcript source row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptSourceStatus {
    Pending,
    Active,
    Stale,
    Invalidated,
    Purged,
    Failed,
    Skipped,
}

impl TranscriptSourceStatus {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            TranscriptSourceStatus::Pending => "pending",
            TranscriptSourceStatus::Active => "active",
            TranscriptSourceStatus::Stale => "stale",
            TranscriptSourceStatus::Invalidated => "invalidated",
            TranscriptSourceStatus::Purged => "purged",
            TranscriptSourceStatus::Failed => "failed",
            TranscriptSourceStatus::Skipped => "skipped",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "pending" => TranscriptSourceStatus::Pending,
            "stale" => TranscriptSourceStatus::Stale,
            "invalidated" => TranscriptSourceStatus::Invalidated,
            "purged" => TranscriptSourceStatus::Purged,
            "failed" => TranscriptSourceStatus::Failed,
            "skipped" => TranscriptSourceStatus::Skipped,
            _ => TranscriptSourceStatus::Active,
        }
    }
}

/// Lifecycle status for transcript extraction/refresh processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptProcessingState {
    Pending,
    Queued,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
    Invalidated,
    Purged,
}

impl TranscriptProcessingState {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            TranscriptProcessingState::Pending => "pending",
            TranscriptProcessingState::Queued => "queued",
            TranscriptProcessingState::Running => "running",
            TranscriptProcessingState::Succeeded => "succeeded",
            TranscriptProcessingState::Failed => "failed",
            TranscriptProcessingState::Skipped => "skipped",
            TranscriptProcessingState::Cancelled => "cancelled",
            TranscriptProcessingState::Invalidated => "invalidated",
            TranscriptProcessingState::Purged => "purged",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "queued" => TranscriptProcessingState::Queued,
            "running" => TranscriptProcessingState::Running,
            "succeeded" => TranscriptProcessingState::Succeeded,
            "failed" => TranscriptProcessingState::Failed,
            "skipped" => TranscriptProcessingState::Skipped,
            "cancelled" => TranscriptProcessingState::Cancelled,
            "invalidated" => TranscriptProcessingState::Invalidated,
            "purged" => TranscriptProcessingState::Purged,
            _ => TranscriptProcessingState::Pending,
        }
    }
}

/// Source manifest upsert payload. `source_key` is the stable, safe locator for
/// idempotency (for example an embedded stream id or a sidecar path hash) and
/// must not contain a local filesystem path.
#[derive(Debug, Clone)]
pub struct TranscriptSourceUpsert {
    pub source_id: Option<Uuid>,
    pub library_id: LibraryId,
    pub media_id: MediaID,
    pub media_file_id: Uuid,
    pub source_kind: TimedTextSourceKind,
    pub language_code: String,
    pub source_key: String,
    pub source_name: Option<String>,
    pub stream_index: Option<i32>,
    pub source_path_hash: Option<String>,
    pub source_content_hash: String,
    pub normalized_content_hash: Option<String>,
    pub artifact_id: Option<Uuid>,
    pub duration_ms: Option<i64>,
    pub source_locator: serde_json::Value,
    pub metadata: serde_json::Value,
}

/// Timestamped cue upsert payload. Segment hashes are computed by the
/// implementation from cue index, time range, and text.
#[derive(Debug, Clone)]
pub struct TranscriptSegmentUpsert {
    pub cue_index: i32,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub metadata: serde_json::Value,
}

/// Result returned after a transactional source + segments upsert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptSourceUpsertResult {
    pub source_id: Uuid,
    pub segment_count: u64,
    pub source_content_hash: String,
}

/// Filter for listing safe transcript source manifest statuses.
#[derive(Debug, Clone, Default)]
pub struct TranscriptSourceStatusFilter {
    pub library_id: Option<LibraryId>,
    pub media_id: Option<MediaID>,
    pub media_file_id: Option<Uuid>,
    pub status: Option<TranscriptSourceStatus>,
    pub limit: u16,
}

/// Safe, bounded status row for a transcript source manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptSourceStatusSummary {
    pub source_id: Uuid,
    pub library_id: LibraryId,
    pub media_id: MediaID,
    pub media_file_id: Uuid,
    pub source_kind: TimedTextSourceKind,
    pub status: TranscriptSourceStatus,
    pub language_code: String,
    pub source_name: Option<String>,
    pub artifact_id: Option<Uuid>,
    pub segment_count: i32,
    pub duration_ms: Option<i64>,
    pub invalidated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub purged_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Filter for listing transcript processing status.
#[derive(Debug, Clone, Default)]
pub struct TranscriptStatusFilter {
    pub library_id: Option<LibraryId>,
    pub media_id: Option<MediaID>,
    pub media_file_id: Option<Uuid>,
    pub status: Option<TranscriptProcessingState>,
    pub limit: u16,
}

/// Bounded status row for transcript extraction/refresh processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptProcessingStatusSummary {
    pub status_id: Uuid,
    pub library_id: LibraryId,
    pub media_id: MediaID,
    pub media_file_id: Uuid,
    pub status: TranscriptProcessingState,
    pub source_count: i32,
    pub segment_count: i32,
    pub attempt_count: i32,
    pub last_error_excerpt: Option<String>,
    pub next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    pub invalidated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub purged_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Repository port for timed-text corpus persistence and bounded snippet search.
#[async_trait]
pub trait TranscriptRepository: Send + Sync {
    /// Upsert a transcript source and replace its segment set in one
    /// transaction. The upsert is idempotent on `(library, media_file,
    /// source_kind, language, source_key)`.
    async fn upsert_source_with_segments(
        &self,
        source: TranscriptSourceUpsert,
        segments: Vec<TranscriptSegmentUpsert>,
    ) -> Result<TranscriptSourceUpsertResult>;

    /// List bounded, safe source manifest status rows without hashes, local
    /// paths, or transcript bodies.
    async fn list_source_status(
        &self,
        filter: TranscriptSourceStatusFilter,
    ) -> Result<Vec<TranscriptSourceStatusSummary>>;

    /// List bounded transcript processing status rows.
    async fn list_processing_status(
        &self,
        filter: TranscriptStatusFilter,
    ) -> Result<Vec<TranscriptProcessingStatusSummary>>;

    /// Mark active transcript sources/segments and their linked source-level
    /// artifacts invalidated for a media item.
    async fn invalidate_media(
        &self,
        library_id: LibraryId,
        media_id: MediaID,
        reason: &str,
    ) -> Result<u64>;

    /// Purge transcript segment text for a media item, mark source manifests and
    /// source-level artifacts purged/deleted, and update processing status.
    async fn purge_media(
        &self,
        library_id: LibraryId,
        media_id: MediaID,
        reason: &str,
    ) -> Result<u64>;

    /// Mark one source and its segments invalidated.
    async fn invalidate_source(
        &self,
        source_id: Uuid,
        reason: &str,
    ) -> Result<()>;

    /// Purge segment text for one source and mark the source purged.
    async fn purge_source(&self, source_id: Uuid, reason: &str) -> Result<()>;

    /// Search active transcript segments and return bounded timestamped
    /// snippets. Results never include local paths, content hashes, or whole
    /// transcript bodies.
    async fn search_snippets(
        &self,
        request: &TimedTextSnippetSearchRequest,
    ) -> Result<TimedTextSnippetSearchResponse>;
}
