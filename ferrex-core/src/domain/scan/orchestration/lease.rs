use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    JobKind,
    job::{JobPriority, JobRecord},
};
use crate::types::ids::LibraryId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct LeaseId(pub Uuid);

impl Default for LeaseId {
    fn default() -> Self {
        Self::new()
    }
}

impl LeaseId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

/// Lease metadata returned when a worker dequeues a job.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobLease {
    pub lease_id: LeaseId,
    pub job: JobRecord,
    pub lease_owner: String,
    pub expires_at: DateTime<Utc>,
    pub renewals: u32,
}

impl JobLease {
    pub fn new(job: JobRecord, owner: String, ttl: chrono::Duration) -> Self {
        Self {
            lease_id: LeaseId::new(),
            expires_at: Utc::now() + ttl,
            lease_owner: owner,
            job,
            renewals: 0,
        }
    }
}

/// Request object to dequeue work from a specific queue.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DequeueRequest {
    pub kind: JobKind,
    pub worker_id: String,
    pub lease_ttl: chrono::Duration,
    #[serde(default)]
    pub selector: Option<QueueSelector>,
}

/// Scheduler-provided hint to bias the queue towards a library/priority.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct QueueSelector {
    pub library_id: LibraryId,
    pub priority: JobPriority,
}

/// Payload provided when renewing a lease.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaseRenewal {
    pub lease_id: LeaseId,
    pub worker_id: String,
    pub extend_by: chrono::Duration,
}

/// Completion outcome used to report job execution results.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CompletionOutcome {
    Completed,
    Retry {
        retryable: bool,
        error: Option<String>,
    },
    DeadLetter {
        error: Option<String>,
    },
}
