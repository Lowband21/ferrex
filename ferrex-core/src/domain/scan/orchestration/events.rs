use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ferrex_model::SubjectKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::scan::context::FolderScanContext, error::Result,
    types::ids::LibraryId,
};

use super::{
    job::{JobHandle, JobId, JobKind, JobPayload, JobPriority},
    lease::LeaseId,
};

/// Metadata envelope attached to every orchestrator job event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventMeta {
    pub version: u16,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub library_id: LibraryId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_key: Option<SubjectKey>,
}

impl EventMeta {
    pub fn new(
        correlation_id: Option<Uuid>,
        library_id: LibraryId,
        idempotency_key: impl Into<String>,
        path_key: Option<SubjectKey>,
    ) -> Self {
        let correlation_id = correlation_id.unwrap_or_else(Uuid::now_v7);
        Self {
            version: 1,
            correlation_id,
            idempotency_key: idempotency_key.into(),
            library_id,
            path_key,
        }
    }
}

/// Event payload emitted by the orchestrator for job lifecycle transitions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum JobEventPayload {
    Enqueued {
        job_id: JobId,
        kind: JobKind,
        priority: JobPriority,
    },
    Merged {
        existing_job_id: JobId,
        merged_job_id: JobId,
        kind: JobKind,
        priority: JobPriority,
    },
    Dequeued {
        job_id: JobId,
        kind: JobKind,
        priority: JobPriority,
        lease_id: LeaseId,
    },
    LeaseRenewed {
        job_id: JobId,
        lease_id: LeaseId,
        renewals: u32,
    },
    LeaseExpired {
        job_id: JobId,
        lease_id: LeaseId,
    },
    Completed {
        job_id: JobId,
        kind: JobKind,
        priority: JobPriority,
    },
    Failed {
        job_id: JobId,
        kind: JobKind,
        priority: JobPriority,
        retryable: bool,
    },
    DeadLettered {
        job_id: JobId,
        kind: JobKind,
        priority: JobPriority,
    },
    ThroughputTick {
        queue_depths: Vec<(JobKind, usize)>,
        sampled_at: DateTime<Utc>,
    },
}

/// Fully qualified job event with metadata and payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobEvent {
    pub meta: EventMeta,
    pub payload: JobEventPayload,
}

impl JobEvent {
    pub fn from_handle(
        handle: &JobHandle,
        correlation_id: Option<Uuid>,
        payload: JobEventPayload,
        path_key: Option<SubjectKey>,
    ) -> Self {
        let meta = EventMeta::new(
            correlation_id,
            handle.library_id,
            handle.dedupe_key.clone(),
            path_key,
        );
        Self { meta, payload }
    }

    pub fn from_job(
        correlation_id: Option<Uuid>,
        library_id: LibraryId,
        idempotency_key: String,
        path_key: Option<SubjectKey>,
        payload: JobEventPayload,
    ) -> Self {
        let meta = EventMeta::new(
            correlation_id,
            library_id,
            idempotency_key,
            path_key,
        );
        Self { meta, payload }
    }
}

#[async_trait]
pub trait JobEventPublisher: Send + Sync {
    async fn publish(&self, event: JobEvent) -> Result<()>;
}

// Domain-level events linking the scan/analyze/index provider.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ScanEvent {
    FolderDiscovered {
        context: Box<FolderScanContext>,
        /// Why this folder should be scanned; used to determine priority
        reason: ScanReason,
    },
    MediaFileDiscovered(Box<MediaFileDiscovered>),
    FolderScanCompleted(FolderScanSummary),
    // Pipeline progression events
    MediaAnalyzed(Box<MediaAnalyzed>),
    MediaReadyForIndex(Box<MediaReadyForIndex>),
    Indexed(Box<IndexingOutcome>),
}

#[async_trait]
pub trait ScanEventPublisher: Send + Sync {
    async fn publish_scan_event(&self, event: ScanEvent) -> Result<()>;
}

// Marker trait for an event bus capable of publishing both job and scan events.
pub trait ScanEventBus: JobEventPublisher + ScanEventPublisher {}
impl<T> ScanEventBus for T where T: JobEventPublisher + ScanEventPublisher {}

use crate::domain::scan::actors::index::IndexingOutcome;
use crate::domain::scan::actors::metadata::MediaReadyForIndex;
use crate::domain::scan::{
    FolderScanSummary, MediaFileDiscovered, ScanReason, analyze::MediaAnalyzed,
};
#[cfg(feature = "compat")]
pub use ScanEvent as DomainEvent;

#[cfg(feature = "compat")]
#[async_trait]
pub trait DomainEventPublisher: Send + Sync {
    async fn publish_domain(&self, event: DomainEvent) -> Result<()>;
}

#[cfg(feature = "compat")]
#[async_trait]
impl<T> DomainEventPublisher for T
where
    T: ScanEventPublisher + Send + Sync,
{
    async fn publish_domain(&self, event: DomainEvent) -> Result<()> {
        self.publish_scan_event(event).await
    }
}

#[cfg(feature = "compat")]
pub trait EventBus: ScanEventBus {}

#[cfg(feature = "compat")]
impl<T> EventBus for T where T: ScanEventBus {}

/// Simplified message for manual enqueue debug endpoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManualEnqueueRequest {
    pub priority: JobPriority,
    pub payload: JobPayload,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManualEnqueueResponse {
    pub handle: JobHandle,
}

pub fn stable_path_key(payload: &JobPayload) -> Option<SubjectKey> {
    match payload {
        JobPayload::FolderScan(job) => {
            SubjectKey::path(job.context.folder_path_norm().to_string()).ok()
        }
        JobPayload::MediaAnalyze(job) => {
            SubjectKey::path(job.path_norm.clone()).ok()
        }
        JobPayload::SeriesResolve(job) => {
            SubjectKey::path(job.series_root_path.as_str().to_string()).ok()
        }
        JobPayload::MetadataEnrich(job) => {
            SubjectKey::path(job.path_norm.to_string()).ok()
        }
        JobPayload::IndexUpsert(job) => {
            SubjectKey::path(job.path_norm.clone()).ok()
        }
        JobPayload::ImageFetch(job) => {
            SubjectKey::opaque(job.iid.to_string()).ok()
        }
        JobPayload::EpisodeMatch(job) => {
            SubjectKey::path(job.path_norm.clone()).ok()
        }
    }
}
