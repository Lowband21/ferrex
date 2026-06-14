use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::{
    job::{EnqueueRequest, JobHandle, JobKind},
    lease::{DequeueRequest, JobLease, LeaseRenewal},
};

/// Abstracts the queue backend (persistence + scheduling) consumed by the orchestrator service.
#[async_trait]
pub trait QueueService: Send + Sync {
    async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle>;

    async fn dequeue(
        &self,
        request: DequeueRequest,
    ) -> Result<Option<JobLease>>;

    async fn renew(&self, renewal: LeaseRenewal) -> Result<JobLease>;

    async fn complete(&self, lease_id: super::lease::LeaseId) -> Result<()>;

    async fn fail(
        &self,
        lease_id: super::lease::LeaseId,
        retryable: bool,
        error: Option<String>,
    ) -> Result<()>;

    async fn dead_letter(
        &self,
        lease_id: super::lease::LeaseId,
        error: Option<String>,
    ) -> Result<()>;

    async fn cancel_job(&self, job_id: super::job::JobId) -> Result<()>;

    async fn queue_depth(&self, kind: JobKind) -> Result<usize>;

    async fn release_dependency(
        &self,
        library_id: crate::types::LibraryId,
        dependency_key: &super::job::DependencyKey,
    ) -> Result<u64>;

    /// Enqueue multiple jobs. Default implementation issues jobs one-by-one.
    /// Implementations backed by a transactional store should override this
    /// to insert all jobs atomically.
    async fn enqueue_many(
        &self,
        requests: Vec<EnqueueRequest>,
    ) -> Result<Vec<JobHandle>> {
        let mut out = Vec::with_capacity(requests.len());
        for req in requests {
            out.push(self.enqueue(req).await?);
        }
        Ok(out)
    }
}

/// Optional capability supported by durable queue implementations to scan and
/// resurrect expired leases.
#[async_trait]
pub trait LeaseExpiryScanner: Send + Sync {
    /// Returns number of jobs transitioned back to ready.
    async fn scan_expired_leases(&self) -> Result<u64>;
}

/// Optional instrumentation hook for queue implementations that can surface
/// observability data.
#[async_trait]
pub trait QueueInstrumentation: Send + Sync {
    async fn queue_snapshot(&self) -> Result<QueueSnapshot>;
}

/// Aggregated metrics for all queue kinds at a specific instant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueSnapshot {
    pub sampled_at: DateTime<Utc>,
    pub queues: HashMap<JobKind, QueueSnapshotEntry>,
}

impl QueueSnapshot {
    pub fn new(sampled_at: DateTime<Utc>) -> Self {
        Self {
            sampled_at,
            queues: HashMap::new(),
        }
    }

    pub fn entry_mut(&mut self, kind: JobKind) -> &mut QueueSnapshotEntry {
        self.queues.entry(kind).or_default()
    }
}

/// Per-queue counters plus instantaneous throughput measurements.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QueueSnapshotEntry {
    pub ready: usize,
    pub leased: usize,
    pub deferred: usize,
    pub failed: usize,
    pub dead_letter: usize,
    #[serde(default)]
    pub dequeue_per_minute: f64,
}
