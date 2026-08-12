use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ferrex_model::SubjectKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;

use super::{
    context::SeriesRootPath,
    job::{EnqueueRequest, JobHandle, JobId, JobKind, JobPriority, JobState},
    lease::{DequeueRequest, JobLease, LeaseRenewal},
};
use crate::domain::scan::actors::index::IndexingChange;
use crate::types::{LibraryId, MediaID, SeriesID};

/// Aggregated schedulable state grouped by queue dimensions.
///
/// `ready` contains only jobs whose backoff has elapsed. `leased` contains
/// leases that have not expired. Together they let the in-memory scheduler
/// rebuild both advertised work and per-library capacity from PostgreSQL.
#[derive(Clone, Debug)]
pub struct ReadyQueueCount {
    pub kind: JobKind,
    pub library_id: LibraryId,
    pub priority: JobPriority,
    pub ready: usize,
    pub leased: usize,
}

/// Compact durable job view used to repair in-memory progress after event loss.
///
/// PostgreSQL remains the authority for lifecycle state; broadcast events are
/// only a low-latency notification path. Consumers select the exact durable
/// enrollments recorded for a correlation and can also provide already-tracked
/// job IDs for legacy runs. This keeps unrelated work that merely shares an old
/// correlation out of the enrolling run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableJobState {
    pub job_id: JobId,
    pub kind: JobKind,
    /// Stable catalog identity carried by durable media jobs. This lets
    /// observers reconstruct a lost live projection without parsing dedupe
    /// strings or relying on the bounded broadcast event.
    pub media_id: Option<MediaID>,
    /// Exact catalog mutation semantics captured in an index-upsert payload.
    pub indexing_change: Option<IndexingChange>,
    /// Compact series hierarchy extracted while decoding the durable payload.
    /// Keeping only the identity needed by projections avoids retaining the
    /// full (and potentially large) job payload in reconciliation snapshots.
    pub series_identity: Option<DurableSeriesIdentity>,
    pub state: JobState,
    pub attempts: u16,
    pub dedupe_key: String,
    pub correlation_id: Option<Uuid>,
    pub path_key: Option<SubjectKey>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Minimal durable series hierarchy needed to rebuild a dropped bundle event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableSeriesIdentity {
    /// Stable series folder represented by the durable payload hierarchy.
    pub series_root_path: SeriesRootPath,
    /// Catalog identity when the payload hierarchy had resolved the series.
    pub series_id: Option<SeriesID>,
    /// Logical season number carried by season or episode hierarchy.
    pub season_number: Option<u16>,
}

impl DurableJobState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            JobState::Completed | JobState::Failed | JobState::DeadLetter
        )
    }

    pub fn is_terminal_failure(&self) -> bool {
        matches!(self.state, JobState::Failed | JobState::DeadLetter)
    }
}

/// Whether a requested durable queue transition changed the leased row.
///
/// A missing lease is distinct from an applied transition: callers must not
/// publish a terminal event or terminal actor notification unless the durable
/// transition was applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueTransitionOutcome {
    Applied,
    /// The requested terminal transition was superseded by newer durable work
    /// and the same job row was made runnable again instead.
    Requeued,
    Missing,
}

/// Durable result of failing a leased job.
///
/// Retryable failures may either schedule another attempt or cross the retry
/// limit and become terminal. `Missing` means no leased row was changed and
/// must not be treated as either outcome by runtime accounting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailOutcome {
    RetryScheduled,
    /// A newer durable generation superseded terminalization and the same job
    /// row is immediately runnable again.
    Requeued,
    Terminal {
        state: JobState,
    },
    Missing,
}

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

    /// Complete a lease and report whether the durable transition was applied.
    ///
    /// The default preserves compatibility with queue implementations whose
    /// legacy `complete` method already returns an error for a missing lease.
    async fn complete_with_outcome(
        &self,
        lease_id: super::lease::LeaseId,
    ) -> Result<QueueTransitionOutcome> {
        self.complete(lease_id).await?;
        Ok(QueueTransitionOutcome::Applied)
    }

    async fn fail(
        &self,
        lease_id: super::lease::LeaseId,
        retryable: bool,
        error: Option<String>,
    ) -> Result<()>;

    /// Fail a lease and distinguish a scheduled retry from a terminal state.
    ///
    /// Durable implementations that enforce an attempt limit must override
    /// this method so a retry request that reaches the limit is reported as a
    /// terminal outcome.
    async fn fail_with_outcome(
        &self,
        lease_id: super::lease::LeaseId,
        retryable: bool,
        error: Option<String>,
    ) -> Result<FailOutcome> {
        self.fail(lease_id, retryable, error).await?;
        if retryable {
            Ok(FailOutcome::RetryScheduled)
        } else {
            Ok(FailOutcome::Terminal {
                state: JobState::Failed,
            })
        }
    }

    async fn dead_letter(
        &self,
        lease_id: super::lease::LeaseId,
        error: Option<String>,
    ) -> Result<()>;

    /// Dead-letter a lease and report whether the durable transition applied.
    ///
    /// Implementations that coordinate terminal job state with other durable
    /// state may return [`QueueTransitionOutcome::Requeued`] when a newer
    /// generation supersedes this terminal result. Callers must treat that as
    /// nonterminal retry work and must not publish a dead-letter event.
    /// PostgreSQL-backed SeriesResolve implementations also own the exact-root
    /// dependency release in this transition so failed series state, queue
    /// terminality, and episode promotion share one linearization boundary.
    async fn dead_letter_with_outcome(
        &self,
        lease_id: super::lease::LeaseId,
        error: Option<String>,
    ) -> Result<QueueTransitionOutcome> {
        self.dead_letter(lease_id, error).await?;
        Ok(QueueTransitionOutcome::Applied)
    }

    async fn cancel_job(&self, job_id: super::job::JobId) -> Result<()>;

    async fn queue_depth(&self, kind: JobKind) -> Result<usize>;

    /// Load durable job lifecycle state for progress reconciliation.
    ///
    /// The correlation selects exact durable enrollment records owned by the
    /// current run, while `job_ids` preserves compatibility with legacy runs
    /// that tracked shared jobs only in memory. Implementations must not broaden
    /// the selection to every job that shares an older correlation.
    /// Non-durable implementations may retain the empty default.
    async fn durable_job_states(
        &self,
        _correlation_id: Uuid,
        _job_ids: &[JobId],
    ) -> Result<Vec<DurableJobState>> {
        Ok(Vec::new())
    }

    async fn release_dependency(
        &self,
        library_id: crate::types::LibraryId,
        dependency_key: &super::job::DependencyKey,
    ) -> Result<u64>;

    /// Atomically promote deferred EpisodeMatch rows for one series root only
    /// when that root is still terminal in authoritative series state.
    ///
    /// Durable implementations must serialize this check with fresh resolver
    /// enrollment; callers must not perform a separate terminal-state read.
    async fn release_terminal_series_dependency(
        &self,
        _library_id: crate::types::LibraryId,
        _series_root_path: &super::context::SeriesRootPath,
    ) -> Result<u64> {
        Ok(0)
    }

    /// Repair deferred episode jobs whose series dependency is already
    /// terminal in durable state.
    ///
    /// Series resolution and episode enrollment are independent durable
    /// transitions. If resolution wins immediately before the deferred
    /// EpisodeMatch row is inserted, the one-shot dependency release cannot
    /// observe that row. Durable queue implementations should atomically
    /// promote those stranded rows back to ready. The default keeps
    /// non-durable queue implementations compatible.
    async fn repair_terminal_series_dependencies(&self) -> Result<u64> {
        Ok(0)
    }

    /// Fetch grouped ready and active-lease counts for scheduler reconciliation.
    ///
    /// Queue implementations without durable startup state can keep the default
    /// empty snapshot.
    async fn ready_counts_grouped(&self) -> Result<Vec<ReadyQueueCount>> {
        Ok(Vec::new())
    }

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
