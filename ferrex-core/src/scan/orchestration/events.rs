use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::Result, types::ids::LibraryID};

use super::{
    job::{ImageFetchSource, JobHandle, JobId, JobKind, JobPayload, JobPriority},
    lease::LeaseId,
};

/// Metadata envelope attached to every orchestrator job event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventMeta {
    pub version: u16,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub library_id: LibraryID,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_key: Option<String>,
}

impl EventMeta {
    pub fn new(
        correlation_id: Option<Uuid>,
        library_id: LibraryID,
        idempotency_key: impl Into<String>,
        path_key: Option<String>,
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
        path_key: Option<String>,
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
        library_id: LibraryID,
        idempotency_key: String,
        path_key: Option<String>,
        payload: JobEventPayload,
    ) -> Self {
        let meta = EventMeta::new(correlation_id, library_id, idempotency_key, path_key);
        Self { meta, payload }
    }
}

#[async_trait]
pub trait JobEventPublisher: Send + Sync {
    async fn publish(&self, event: JobEvent) -> Result<()>;
}

// Domain-level events linking the scan/analyze/index pipeline.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ScanEvent {
    FolderDiscovered {
        library_id: LibraryID,
        folder_path: String,
        parent: super::actors::messages::ParentDescriptors,
        /// Why this folder should be scanned; used to determine priority
        reason: super::job::ScanReason,
    },
    MediaFileDiscovered(super::actors::messages::MediaFileDiscovered),
    FolderScanCompleted(super::actors::messages::FolderScanSummary),
    // Pipeline progression events
    MediaAnalyzed(super::actors::pipeline::MediaAnalyzed),
    MediaReadyForIndex(super::actors::pipeline::MediaReadyForIndex),
    Indexed(super::actors::pipeline::IndexingOutcome),
}

#[async_trait]
pub trait ScanEventPublisher: Send + Sync {
    async fn publish_scan_event(&self, event: ScanEvent) -> Result<()>;
}

// Marker trait for an event bus capable of publishing both job and scan events.
pub trait ScanEventBus: JobEventPublisher + ScanEventPublisher {}
impl<T> ScanEventBus for T where T: JobEventPublisher + ScanEventPublisher {}

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

pub fn stable_path_key(payload: &JobPayload) -> Option<String> {
    match payload {
        JobPayload::FolderScan(job) => Some(job.folder_path_norm.clone()),
        JobPayload::MediaAnalyze(job) => Some(job.fingerprint.hash_repr()),
        JobPayload::MetadataEnrich(job) => Some(job.logical_candidate_id.clone()),
        JobPayload::IndexUpsert(job) => Some(job.path_norm.clone()),
        JobPayload::ImageFetch(job) => match &job.source {
            ImageFetchSource::Tmdb { tmdb_path } => Some(tmdb_path.clone()),
            ImageFetchSource::EpisodeThumbnail { image_key, .. } => Some(image_key.clone()),
        },
    }
}
