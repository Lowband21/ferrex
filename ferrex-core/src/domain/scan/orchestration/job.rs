use ferrex_model::{ImageSize, MediaID, VideoMediaType};
use tracing::error;

use crate::{
    error::{MediaError, Result},
    types::LibraryId,
};

use super::config::PriorityWeights;
use crate::domain::scan::orchestration::context::{
    EpisodeScanHierarchy, FolderScanContext, MovieScanHierarchy, ScanNodeKind,
    SeasonScanHierarchy, SeriesHint, SeriesRootPath, SeriesScanHierarchy,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, result::Result as StdResult, str::FromStr};
use uuid::Uuid;

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
#[repr(u8)]
pub enum JobKind {
    FolderScan = 0,
    SeriesResolve = 1,
    MediaAnalyze = 2,
    MetadataEnrich = 3,
    IndexUpsert = 4,
    ImageFetch = 5,
    EpisodeMatch = 6,
}

impl JobKind {
    pub fn from_i16(v: i16) -> Result<Self> {
        match v {
            0 => Ok(JobKind::FolderScan),
            1 => Ok(JobKind::SeriesResolve),
            2 => Ok(JobKind::MediaAnalyze),
            3 => Ok(JobKind::MetadataEnrich),
            4 => Ok(JobKind::IndexUpsert),
            5 => Ok(JobKind::ImageFetch),
            6 => Ok(JobKind::EpisodeMatch),
            _ => Err(MediaError::NotFound(
                "Invalid JobKind provided".to_string(),
            )),
        }
    }

    pub fn all_kinds() -> &'static [Self] {
        &[
            JobKind::FolderScan,
            JobKind::SeriesResolve,
            JobKind::MediaAnalyze,
            JobKind::MetadataEnrich,
            JobKind::IndexUpsert,
            JobKind::ImageFetch,
            JobKind::EpisodeMatch,
        ]
    }
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobKind::FolderScan => write!(f, "scan folder"),
            JobKind::SeriesResolve => write!(f, "resolve series"),
            JobKind::MediaAnalyze => write!(f, "analyze media"),
            JobKind::MetadataEnrich => write!(f, "enrich metadata"),
            JobKind::IndexUpsert => write!(f, "index upsert"),
            JobKind::ImageFetch => write!(f, "fetch image"),
            JobKind::EpisodeMatch => write!(f, "match episode"),
        }
    }
}

/// Structured payload per job kind.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum JobPayload {
    FolderScan(FolderScanJob),
    SeriesResolve(SeriesResolveJob),
    MediaAnalyze(MediaAnalyzeJob),
    MetadataEnrich(MetadataEnrichJob),
    IndexUpsert(IndexUpsertJob),
    ImageFetch(ImageFetchJob),
    EpisodeMatch(EpisodeMatchJob),
}

impl JobPayload {
    pub fn kind(&self) -> JobKind {
        match self {
            JobPayload::FolderScan(_) => JobKind::FolderScan,
            JobPayload::SeriesResolve(_) => JobKind::SeriesResolve,
            JobPayload::MediaAnalyze(_) => JobKind::MediaAnalyze,
            JobPayload::MetadataEnrich(_) => JobKind::MetadataEnrich,
            JobPayload::IndexUpsert(_) => JobKind::IndexUpsert,
            JobPayload::ImageFetch(_) => JobKind::ImageFetch,
            JobPayload::EpisodeMatch(_) => JobKind::EpisodeMatch,
        }
    }

    pub fn library_id(&self) -> LibraryId {
        match self {
            JobPayload::FolderScan(job) => job.context.library_id(),
            JobPayload::SeriesResolve(job) => job.library_id,
            JobPayload::MediaAnalyze(job) => job.library_id,
            JobPayload::MetadataEnrich(job) => job.library_id,
            JobPayload::IndexUpsert(job) => job.library_id,
            JobPayload::ImageFetch(job) => job.library_id,
            JobPayload::EpisodeMatch(job) => job.library_id,
        }
    }

    /// A structural dedupe key extracted from payload content.
    pub fn dedupe_key(&self) -> DedupeKey {
        match self {
            JobPayload::FolderScan(job) => DedupeKey::FolderScan {
                candidate: MediaCandidate::new(
                    job.context.library_id(),
                    job.context.folder_path_norm().to_string(),
                ),
            },
            JobPayload::SeriesResolve(job) => DedupeKey::SeriesResolve {
                candidate: MediaCandidate::new(
                    job.library_id,
                    job.series_root_path.as_str().to_string(),
                ),
            },
            JobPayload::MediaAnalyze(job) => DedupeKey::MediaAnalyze {
                candidate: MediaCandidate::new(
                    job.library_id,
                    job.path_norm.clone(),
                ),
            },
            JobPayload::MetadataEnrich(job) => DedupeKey::MetadataEnrich {
                candidate: MediaCandidate::new(
                    job.library_id,
                    job.path_norm.clone(),
                ),
            },
            JobPayload::IndexUpsert(job) => DedupeKey::IndexUpsert {
                candidate: MediaCandidate::new(
                    job.library_id,
                    job.path_norm.clone(),
                ),
            },
            JobPayload::ImageFetch(job) => DedupeKey::ImageFetch {
                library_id: job.library_id.to_uuid(),
                image_id: job.iid,
                image_size: job.imz,
            },
            JobPayload::EpisodeMatch(job) => DedupeKey::EpisodeMatch {
                candidate: MediaCandidate::new(
                    job.library_id,
                    job.path_norm.clone(),
                ),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct DependencyKey(String);

impl DependencyKey {
    pub fn series_root(series_root_path: &SeriesRootPath) -> Self {
        Self(format!("series_root:{}", series_root_path.as_str()))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for DependencyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for DependencyKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for DependencyKey {
    fn from(value: &str) -> Self {
        Self(value.to_string())
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
    pub dependency_key: Option<DependencyKey>,
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
            dependency_key: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct MediaCandidate {
    pub library_id: LibraryId,
    pub path_norm: String,
}

impl MediaCandidate {
    pub fn new(library_id: LibraryId, path_norm: String) -> Self {
        MediaCandidate {
            library_id,
            path_norm,
        }
    }
}

impl fmt::Display for MediaCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.library_id, self.path_norm)
    }
}

/// Domain view over dedupe identity per job kind.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum DedupeKey {
    FolderScan {
        candidate: MediaCandidate,
    },
    SeriesResolve {
        candidate: MediaCandidate,
    },
    MediaAnalyze {
        candidate: MediaCandidate,
    },
    MetadataEnrich {
        candidate: MediaCandidate,
    },
    EpisodeMatch {
        candidate: MediaCandidate,
    },
    IndexUpsert {
        candidate: MediaCandidate,
    },
    ImageFetch {
        library_id: Uuid,
        image_id: Uuid,
        image_size: ImageSize,
    },
}

impl fmt::Display for DedupeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DedupeKey::FolderScan { candidate } => write!(
                f,
                "scan:{}:{}",
                candidate.library_id, candidate.path_norm
            ),
            DedupeKey::SeriesResolve { candidate } => write!(
                f,
                "series_resolve:{}:{}",
                candidate.library_id, candidate.path_norm
            ),
            DedupeKey::MediaAnalyze { candidate } => {
                write!(f, "analyze:{}", candidate)
            }
            DedupeKey::MetadataEnrich { candidate } => {
                write!(f, "metadata:{}", candidate)
            }
            DedupeKey::EpisodeMatch { candidate } => {
                write!(f, "episode_match:{}", candidate)
            }
            DedupeKey::IndexUpsert { candidate } => write!(
                f,
                "index:{}:{}",
                candidate.library_id, candidate.path_norm
            ),
            DedupeKey::ImageFetch {
                library_id,
                image_id,
                image_size,
            } => write!(
                f,
                "image:{}:{}:{}:{}",
                library_id,
                image_id,
                image_size.image_variant(),
                image_size.width_name(),
            ),
        }
    }
}

/// Background image fetch task for media imagery.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageFetchJob {
    pub library_id: LibraryId,
    pub iid: Uuid,
    pub imz: ImageSize,
    pub priority_hint: ImageFetchPriority,
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
    pub context: FolderScanContext,
    pub scan_reason: ScanReason,
    pub enqueue_time: DateTime<Utc>,
    pub device_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScanReason {
    HotChange,
    UserRequested,
    BulkSeed,
    MaintenanceSweep,
    WatcherOverflow,
}

impl FromStr for ScanReason {
    type Err = &'static str;

    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        match s {
            "HotChange" => Ok(ScanReason::HotChange),
            "UserRequested" => Ok(ScanReason::UserRequested),
            "BulkSeed" => Ok(ScanReason::BulkSeed),
            "MaintenanceSweep" => Ok(ScanReason::MaintenanceSweep),
            "WatcherOverflow" => Ok(ScanReason::WatcherOverflow),
            _ => Err("unrecognized scan reason"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AnalyzeScanHierarchy {
    Movie(MovieScanHierarchy),
    Series(SeriesScanHierarchy),
    Season(SeasonScanHierarchy),
    Episode(EpisodeScanHierarchy),
}

/// Analyze job payload (typically ffprobe + thumbnails).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaAnalyzeJob {
    pub library_id: LibraryId,
    pub path_norm: String,
    pub fingerprint: MediaFingerprint,
    pub discovered_at: DateTime<Utc>,
    pub media_id: MediaID,
    pub variant: VideoMediaType,
    pub hierarchy: AnalyzeScanHierarchy,
    pub node: ScanNodeKind,
    #[serde(default = "default_scan_reason")]
    pub scan_reason: ScanReason,
}

fn default_scan_reason() -> ScanReason {
    ScanReason::BulkSeed
}

/// Series resolution payload (first-class pipeline stage).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeriesResolveJob {
    pub library_id: LibraryId,
    pub series_root_path: SeriesRootPath,
    pub hint: Option<SeriesHint>,
    pub folder_name: String,
    pub scan_reason: ScanReason,
}

/// Metadata enrichment payload (normalize/match/fetch).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetadataEnrichJob {
    pub library_id: LibraryId,
    pub media_id: MediaID,
    pub variant: VideoMediaType,
    pub hierarchy: AnalyzeScanHierarchy,
    pub node: ScanNodeKind,
    pub path_norm: String,
    pub fingerprint: MediaFingerprint,
    pub scan_reason: ScanReason,
}

/// Episode match job (blocks until series mapping is resolved).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpisodeMatchJob {
    pub library_id: LibraryId,
    pub media_id: MediaID,
    pub path_norm: String,
    pub fingerprint: MediaFingerprint,
    pub hierarchy: EpisodeScanHierarchy,
    pub node: ScanNodeKind,
    pub scan_reason: ScanReason,
}

/// Index upsert payload (DB + search index writes).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexUpsertJob {
    pub library_id: LibraryId,
    pub media_id: MediaID,
    pub variant: VideoMediaType,
    pub hierarchy: AnalyzeScanHierarchy,
    pub node: ScanNodeKind,
    pub path_norm: String,
    pub idempotency_key: String,
}

/// Stable media fingerprint.
#[derive(
    Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Default,
)]
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
    pub library_id: LibraryId,
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
    #[serde(default)]
    pub dependency_key: Option<DependencyKey>,
}

impl EnqueueRequest {
    pub fn new(priority: JobPriority, payload: JobPayload) -> Self {
        Self {
            priority,
            payload,
            allow_merge: true,
            requested_at: Utc::now(),
            correlation_id: None,
            dependency_key: None,
        }
    }

    pub fn dedupe_key(&self) -> DedupeKey {
        self.payload.dedupe_key()
    }

    pub fn with_dependency(mut self, dependency_key: DependencyKey) -> Self {
        self.dependency_key = Some(dependency_key);
        self
    }

    pub fn validate(&self) -> Result<()> {
        match &self.payload {
            JobPayload::EpisodeMatch(_job) => {
                if self.dependency_key.is_none() {
                    let msg = "episode match requires dependency gating";
                    error!(msg);
                    return Err(MediaError::InvalidMedia(msg.into()));
                }
            }
            JobPayload::MetadataEnrich(job) => {
                if job.variant == VideoMediaType::Episode {
                    use crate::domain::scan::orchestration::context::WithSeriesHierarchy;

                    let AnalyzeScanHierarchy::Episode(hierarchy) =
                        &job.hierarchy
                    else {
                        return Err(MediaError::InvalidMedia(
                            "episode metadata enrich requires episode hierarchy"
                                .into(),
                        ));
                    };

                    if hierarchy.series_id().is_none() {
                        return Err(MediaError::InvalidMedia(
                            "episode metadata enrich requires resolved series id"
                                .into(),
                        ));
                    }
                }
            }
            JobPayload::SeriesResolve(job) => {
                if job.folder_name.trim().is_empty() && job.hint.is_none() {
                    return Err(MediaError::InvalidMedia(
                        "series resolve requires folder name or hint".into(),
                    ));
                }
            }
            _ => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::orchestration::context::MovieRootPath;

    #[test]
    fn media_analyze_dedupe_key_is_path_scoped() {
        let library_id =
            LibraryId(Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa));

        let fingerprint = MediaFingerprint {
            device_id: None,
            inode: None,
            size: 0,
            mtime: 123,
            weak_hash: None,
        };

        let hierarchy_a = AnalyzeScanHierarchy::Movie(MovieScanHierarchy {
            movie_root_path: MovieRootPath::try_new("/demo/A")
                .expect("valid movie root"),
            movie_id: None,
            extra_tag: None,
        });

        let hierarchy_b = AnalyzeScanHierarchy::Movie(MovieScanHierarchy {
            movie_root_path: MovieRootPath::try_new("/demo/B")
                .expect("valid movie root"),
            movie_id: None,
            extra_tag: None,
        });

        let key_a = JobPayload::MediaAnalyze(MediaAnalyzeJob {
            library_id,
            path_norm: "/demo/A/A.mkv".into(),
            fingerprint: fingerprint.clone(),
            discovered_at: Utc::now(),
            media_id: MediaID::new(VideoMediaType::Movie),
            variant: VideoMediaType::Movie,
            hierarchy: hierarchy_a,
            node: ScanNodeKind::MovieFolder,
            scan_reason: ScanReason::BulkSeed,
        })
        .dedupe_key();

        let key_b = JobPayload::MediaAnalyze(MediaAnalyzeJob {
            library_id,
            path_norm: "/demo/B/B.mkv".into(),
            fingerprint,
            discovered_at: Utc::now(),
            media_id: MediaID::new(VideoMediaType::Movie),
            variant: VideoMediaType::Movie,
            hierarchy: hierarchy_b,
            node: ScanNodeKind::MovieFolder,
            scan_reason: ScanReason::BulkSeed,
        })
        .dedupe_key();

        assert_ne!(key_a, key_b);
    }
}
