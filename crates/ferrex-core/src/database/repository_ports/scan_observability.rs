use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::Result, types::ids::LibraryId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanRunSource {
    Manual,
    Maintenance,
    Watcher,
    Retry,
    Orchestrator,
}

impl ScanRunSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Maintenance => "maintenance",
            Self::Watcher => "watcher",
            Self::Retry => "retry",
            Self::Orchestrator => "orchestrator",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "maintenance" => Some(Self::Maintenance),
            "watcher" => Some(Self::Watcher),
            "retry" => Some(Self::Retry),
            "orchestrator" => Some(Self::Orchestrator),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanRunStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Canceled,
}

impl ScanRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "canceled" => Some(Self::Canceled),
            _ => None,
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Running | Self::Paused)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRunRecord {
    pub id: Uuid,
    pub library_id: LibraryId,
    pub source: ScanRunSource,
    pub status: ScanRunStatus,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub sequence: i64,
    pub started_at: DateTime<Utc>,
    pub last_event_at: DateTime<Utc>,
    pub terminal_at: Option<DateTime<Utc>>,
    pub current_path: Option<String>,
    pub completed_items: i64,
    pub total_items: i64,
    pub retrying_items: i64,
    pub dead_lettered_items: i64,
    pub terminal_summary: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRunUpdate {
    pub id: Uuid,
    pub status: ScanRunStatus,
    pub idempotency_key: String,
    pub last_event_at: DateTime<Utc>,
    pub terminal_at: Option<DateTime<Utc>>,
    pub current_path: Option<String>,
    pub completed_items: i64,
    pub total_items: i64,
    pub retrying_items: i64,
    pub dead_lettered_items: i64,
    pub terminal_summary: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewScanRunEvent {
    pub run_id: Uuid,
    pub library_id: LibraryId,
    pub event_kind: String,
    pub status: String,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub subject_key: Option<String>,
    pub current_path: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub completed_items: i64,
    pub total_items: i64,
    pub retrying_items: i64,
    pub dead_lettered_items: i64,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRunEventRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub library_id: LibraryId,
    pub event_version: i32,
    pub event_kind: String,
    pub status: String,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub sequence: i64,
    pub subject_key: Option<String>,
    pub current_path: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub completed_items: i64,
    pub total_items: i64,
    pub retrying_items: i64,
    pub dead_lettered_items: i64,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRunFailureSummary {
    pub run_id: Uuid,
    pub library_id: LibraryId,
    pub subject_key: String,
    pub category: String,
    pub message_code: String,
    pub raw_debug_details: serde_json::Value,
    pub last_error: Option<String>,
    pub occurrences: i32,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub retryable: bool,
    pub job_id: Option<Uuid>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ScanRunRetentionPolicy {
    pub terminal_before: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy)]
pub struct ScanRunPageRequest {
    pub library_id: Option<LibraryId>,
    pub status: Option<ScanRunStatus>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRunPage {
    pub runs: Vec<ScanRunRecord>,
    pub total: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct ScanRunEventPageRequest {
    pub run_id: Uuid,
    pub after_sequence: Option<i64>,
    pub limit: i64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ScanRunEventSequenceBounds {
    pub min_sequence: Option<i64>,
    pub max_sequence: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub struct ScanRunFailurePageRequest {
    pub run_id: Uuid,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRunFailurePage {
    pub failures: Vec<ScanRunFailureSummary>,
    pub total: i64,
}

#[async_trait]
pub trait ScanObservabilityRepository: Send + Sync {
    async fn create_run(&self, run: &ScanRunRecord) -> Result<bool>;

    async fn update_run(&self, update: &ScanRunUpdate) -> Result<()>;

    async fn append_event(
        &self,
        event: &NewScanRunEvent,
    ) -> Result<ScanRunEventRecord>;

    async fn upsert_failure_summary(
        &self,
        failure: &ScanRunFailureSummary,
    ) -> Result<()>;

    async fn get_run(&self, run_id: Uuid) -> Result<Option<ScanRunRecord>>;

    async fn active_runs(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<ScanRunRecord>>;

    async fn active_runs_all(&self) -> Result<Vec<ScanRunRecord>>;

    async fn recent_runs(
        &self,
        library_id: Option<LibraryId>,
        limit: i64,
    ) -> Result<Vec<ScanRunRecord>>;

    async fn runs_page(
        &self,
        request: ScanRunPageRequest,
    ) -> Result<ScanRunPage>;

    async fn events_for_run(
        &self,
        run_id: Uuid,
    ) -> Result<Vec<ScanRunEventRecord>>;

    async fn events_page_for_run(
        &self,
        request: ScanRunEventPageRequest,
    ) -> Result<Vec<ScanRunEventRecord>>;

    async fn event_sequence_bounds(
        &self,
        run_id: Uuid,
    ) -> Result<ScanRunEventSequenceBounds>;

    async fn failure_summaries_for_run(
        &self,
        run_id: Uuid,
    ) -> Result<Vec<ScanRunFailureSummary>>;

    async fn failure_summaries_page_for_run(
        &self,
        request: ScanRunFailurePageRequest,
    ) -> Result<ScanRunFailurePage>;

    async fn prune(&self, policy: ScanRunRetentionPolicy) -> Result<u64>;
}
