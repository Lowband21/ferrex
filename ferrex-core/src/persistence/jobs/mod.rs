//! Persistence contracts for orchestrator job storage.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::Result;
use crate::orchestration::{
    JobKind,
    job::{DedupeKey, JobId, JobPriority, JobRecord, JobState},
    lease::{CompletionOutcome, LeaseId},
};

/// Describes the persisted lease metadata attached to a job row.
#[derive(Clone, Debug)]
pub struct LeaseRecord {
    pub lease_id: LeaseId,
    pub lease_owner: String,
    pub lease_expires_at: DateTime<Utc>,
    pub renewals: u32,
}

/// Captures an error attached to a job attempt.
#[derive(Clone, Debug)]
pub struct JobErrorRecord {
    pub job_id: JobId,
    pub attempt: u16,
    pub error_class: String,
    pub message: Option<String>,
    pub context_json: Option<serde_json::Value>,
    pub occurred_at: DateTime<Utc>,
}

/// Snapshot returned when loading a queue for scheduling decisions.
#[derive(Clone, Debug)]
pub struct QueueRow {
    pub record: JobRecord,
    pub lease: Option<LeaseRecord>,
}

/// Repository abstraction for durable orchestrator storage.
#[async_trait]
pub trait JobsRepository: Send + Sync {
    type TransactionCtx: Send + Sync;

    async fn insert_job(&self, job: JobRecord) -> Result<JobRecord>;

    async fn find_active_by_dedupe(
        &self,
        kind: JobKind,
        dedupe_key: &DedupeKey,
    ) -> Result<Option<JobRecord>>;

    async fn update_job_state(
        &self,
        job_id: JobId,
        state: JobState,
        lease: Option<LeaseRecord>,
        attempts: Option<u16>,
    ) -> Result<()>;

    async fn record_error(&self, error: JobErrorRecord) -> Result<()>;

    async fn next_ready_job(
        &self,
        kind: JobKind,
        priority: JobPriority,
        now: DateTime<Utc>,
    ) -> Result<Option<JobRecord>>;

    async fn attach_lease(
        &self,
        job_id: JobId,
        lease_owner: String,
        lease_ttl: chrono::Duration,
    ) -> Result<LeaseRecord>;

    async fn renew_lease(
        &self,
        lease_id: LeaseId,
        extend_by: chrono::Duration,
    ) -> Result<LeaseRecord>;

    async fn clear_lease(
        &self,
        lease_id: LeaseId,
        completion: CompletionOutcome,
        now: DateTime<Utc>,
    ) -> Result<()>;

    async fn list_queue_rows(&self, kind: JobKind, limit: usize) -> Result<Vec<QueueRow>>;

    async fn queue_depth(&self, kind: JobKind) -> Result<usize>;
}
