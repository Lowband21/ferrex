use crate::error::Result;
use crate::image::records::MediaImageVariantKey;
use crate::{MediaImageKind, types::LibraryID};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use uuid::Uuid;

use super::config::PriorityWeights;

/// Unique identifier for orchestrator jobs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct JobId(pub Uuid);

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl JobId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Priority bands map to token buckets and fairness guarantees.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum JobPriority {
    P0 = 0,
    P1 = 1,
    P2 = 2,
    P3 = 3,
}

impl JobPriority {
    pub fn weight(&self, weights: &PriorityWeights) -> u8 {
        match self {
            JobPriority::P0 => weights.p0,
            JobPriority::P1 => weights.p1,
            JobPriority::P2 => weights.p2,
            JobPriority::P3 => weights.p3,
        }
    }

    pub fn elevate(self, target: JobPriority) -> JobPriority {
        if target as u8 <= self as u8 {
            target
        } else {
            self
        }
    }
}

/// Scheduler-visible job states. Ready/Leased map directly to queue presence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum JobState {
    Ready,
    Deferred,
    Leased,
    Completed,
    Failed,
    DeadLetter,
}

/// Distinguishes the work queues described in the requirements doc.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum JobKind {
    FolderScan,
    MediaAnalyze,
    MetadataEnrich,
    IndexUpsert,
    ImageFetch,
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobKind::FolderScan => write!(f, "scan"),
            JobKind::MediaAnalyze => write!(f, "analyze"),
            JobKind::MetadataEnrich => write!(f, "metadata"),
            JobKind::IndexUpsert => write!(f, "index"),
            JobKind::ImageFetch => write!(f, "image"),
        }
    }
}

/// Structured payload per job kind.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum JobPayload {
    FolderScan(FolderScanJob),
    MediaAnalyze(MediaAnalyzeJob),
    MetadataEnrich(MetadataEnrichJob),
    IndexUpsert(IndexUpsertJob),
    ImageFetch(ImageFetchJob),
}

impl JobPayload {
    pub fn kind(&self) -> JobKind {
        match self {
            JobPayload::FolderScan(_) => JobKind::FolderScan,
            JobPayload::MediaAnalyze(_) => JobKind::MediaAnalyze,
            JobPayload::MetadataEnrich(_) => JobKind::MetadataEnrich,
            JobPayload::IndexUpsert(_) => JobKind::IndexUpsert,
            JobPayload::ImageFetch(_) => JobKind::ImageFetch,
        }
    }

    pub fn library_id(&self) -> LibraryID {
        match self {
            JobPayload::FolderScan(job) => job.library_id,
            JobPayload::MediaAnalyze(job) => job.library_id,
            JobPayload::MetadataEnrich(job) => job.library_id,
            JobPayload::IndexUpsert(job) => job.library_id,
            JobPayload::ImageFetch(job) => job.library_id,
        }
    }

    /// A structural dedupe key extracted from payload content.
    pub fn dedupe_key(&self) -> DedupeKey {
        match self {
            JobPayload::FolderScan(job) => DedupeKey::FolderScan {
                library_id: job.library_id,
                folder_path_norm: job.folder_path_norm.clone(),
            },
            JobPayload::MediaAnalyze(job) => {
                DedupeKey::MediaAnalyze(job.fingerprint.clone())
            }
            JobPayload::MetadataEnrich(job) => DedupeKey::MetadataEnrich {
                candidate_id: job.logical_candidate_id.clone(),
            },
            JobPayload::IndexUpsert(job) => DedupeKey::IndexUpsert {
                library_id: job.library_id,
                file_path_norm: job.path_norm.clone(),
            },
            JobPayload::ImageFetch(job) => DedupeKey::ImageFetch {
                media_type: job.key.media_type.clone(),
                media_id: job.key.media_id,
                image_type: job.key.image_type.clone(),
                order_index: job.key.order_index,
                variant: job.key.variant.clone(),
            },
        }
    }
}

/// Envelope stored in persistence for each job.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: JobId,
    pub payload: JobPayload,
    pub priority: JobPriority,
    pub state: JobState,
    pub attempts: u16,
    pub available_at: DateTime<Utc>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub backoff_until: Option<DateTime<Utc>>,
    pub dedupe_key: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl JobRecord {
    pub fn new(payload: JobPayload, priority: JobPriority) -> Self {
        let now = Utc::now();
        let dedupe_key = payload.dedupe_key().to_string();
        Self {
            id: JobId::new(),
            payload,
            priority,
            state: JobState::Ready,
            attempts: 0,
            available_at: now,
            lease_owner: None,
            lease_expires_at: None,
            backoff_until: None,
            dedupe_key,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Domain view over dedupe identity per job kind.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum DedupeKey {
    FolderScan {
        library_id: LibraryID,
        folder_path_norm: String,
    },
    MediaAnalyze(MediaFingerprint),
    MetadataEnrich {
        candidate_id: String,
    },
    IndexUpsert {
        library_id: LibraryID,
        file_path_norm: String,
    },
    ImageFetch {
        media_type: String,
        media_id: uuid::Uuid,
        image_type: MediaImageKind,
        order_index: i32,
        variant: String,
    },
}

impl fmt::Display for DedupeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DedupeKey::FolderScan {
                library_id,
                folder_path_norm,
            } => write!(f, "scan:{}:{}", library_id, folder_path_norm),
            DedupeKey::MediaAnalyze(fp) => {
                write!(f, "analyze:{}", fp.hash_repr())
            }
            DedupeKey::MetadataEnrich { candidate_id } => {
                write!(f, "metadata:{}", candidate_id)
            }
            DedupeKey::IndexUpsert {
                library_id,
                file_path_norm,
            } => write!(f, "index:{}:{}", library_id, file_path_norm),
            DedupeKey::ImageFetch {
                media_type,
                media_id,
                image_type,
                order_index,
                variant,
            } => write!(
                f,
                "image:{}:{}:{}:{}:{}",
                media_type, media_id, image_type, order_index, variant
            ),
        }
    }
}

/// Background image fetch task for media imagery.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageFetchJob {
    pub library_id: LibraryID,
    pub source: ImageFetchSource,
    pub key: MediaImageVariantKey,
    pub priority_hint: ImageFetchPriority,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ImageFetchSource {
    /// Remote TMDB asset identified by a path fragment.
    Tmdb { tmdb_path: String },
    /// Locally generated episode thumbnail sourced from a media file.
    EpisodeThumbnail {
        media_file_id: Uuid,
        image_key: String,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum ImageFetchPriority {
    Poster,
    Backdrop,
    Profile,
}

impl ImageFetchPriority {
    pub fn job_priority(&self) -> JobPriority {
        match self {
            ImageFetchPriority::Poster => JobPriority::P0,
            ImageFetchPriority::Backdrop => JobPriority::P1,
            ImageFetchPriority::Profile => JobPriority::P2,
        }
    }
}

/// Minimum contract for folder scan payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FolderScanJob {
    pub library_id: LibraryID,
    pub folder_path_norm: String,
    pub parent_context: Option<String>,
    pub scan_reason: ScanReason,
    pub enqueue_time: DateTime<Utc>,
    pub device_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ScanReason {
    HotChange,
    UserRequested,
    BulkSeed,
    MaintenanceSweep,
    WatcherOverflow,
}

/// Analyze job payload (typically ffprobe + thumbnails).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaAnalyzeJob {
    pub library_id: LibraryID,
    pub path_norm: String,
    pub fingerprint: MediaFingerprint,
    pub discovered_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub context: Value,
    #[serde(default = "default_scan_reason")]
    pub scan_reason: ScanReason,
}

fn default_scan_reason() -> ScanReason {
    ScanReason::BulkSeed
}

/// Metadata enrichment payload (normalize/match/fetch).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetadataEnrichJob {
    pub library_id: LibraryID,
    pub logical_candidate_id: String,
    pub parse_fields: serde_json::Value,
    pub external_ids: Option<serde_json::Value>,
}

/// Index upsert payload (DB + search index writes).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexUpsertJob {
    pub library_id: LibraryID,
    pub logical_entity: serde_json::Value,
    pub media_attrs: serde_json::Value,
    pub relations: serde_json::Value,
    pub path_norm: String,
    pub idempotency_key: String,
}

/// Stable media fingerprint.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct MediaFingerprint {
    pub device_id: Option<String>,
    pub inode: Option<u64>,
    pub size: u64,
    pub mtime: i64,
    pub weak_hash: Option<String>,
}

impl MediaFingerprint {
    pub fn hash_repr(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.device_id.as_deref().unwrap_or(""),
            self.inode.unwrap_or_default(),
            self.size,
            self.mtime,
            self.weak_hash.as_deref().unwrap_or("")
        )
    }
}

/// Lightweight summary returned to callers after enqueue.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobHandle {
    pub job_id: JobId,
    pub kind: JobKind,
    pub dedupe_key: String,
    pub library_id: LibraryID,
    pub priority: JobPriority,
    pub accepted: bool,
    pub merged_into: Option<JobId>,
}

impl JobHandle {
    pub fn accepted(
        job_id: JobId,
        payload: &JobPayload,
        priority: JobPriority,
    ) -> Self {
        Self {
            job_id,
            kind: payload.kind(),
            dedupe_key: payload.dedupe_key().to_string(),
            library_id: payload.library_id(),
            priority,
            accepted: true,
            merged_into: None,
        }
    }

    pub fn merged(
        existing: JobId,
        payload: &JobPayload,
        priority: JobPriority,
    ) -> Self {
        Self {
            job_id: existing,
            kind: payload.kind(),
            dedupe_key: payload.dedupe_key().to_string(),
            library_id: payload.library_id(),
            priority,
            accepted: false,
            merged_into: Some(existing),
        }
    }
}

/// High-level enqueue request used by upstream producers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnqueueRequest {
    pub priority: JobPriority,
    pub payload: JobPayload,
    pub allow_merge: bool,
    pub requested_at: DateTime<Utc>,
    #[serde(default)]
    pub correlation_id: Option<Uuid>,
}

impl EnqueueRequest {
    pub fn new(priority: JobPriority, payload: JobPayload) -> Self {
        Self {
            priority,
            payload,
            allow_merge: true,
            requested_at: Utc::now(),
            correlation_id: None,
        }
    }

    pub fn dedupe_key(&self) -> DedupeKey {
        self.payload.dedupe_key()
    }
}

/// Minimal validation hook; implementation left for future work.
pub trait JobValidator: Send + Sync {
    fn validate(&self, _request: &EnqueueRequest) -> Result<()> {
        Ok(())
    }
}
