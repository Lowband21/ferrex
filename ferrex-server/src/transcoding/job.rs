use super::config::ToneMappingConfig;
use super::profiles::{ProfileVariant, TranscodingProfile};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum JobType {
    /// Regular single-profile transcoding job
    Regular,
    /// Master job for adaptive bitrate streaming that tracks variant jobs
    Master { variant_job_ids: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct TranscodingJob {
    pub id: String,
    /// The media file path (misleadingly named, but contains the actual file path)
    pub media_id: String,
    pub profile: TranscodingProfile,
    pub status: TranscodingStatus,
    pub output_dir: PathBuf,
    pub playlist_path: PathBuf,
    pub segments: HashMap<u32, String>,
    pub error: Option<String>,
    pub created_at: Instant,
    pub updated_at: Instant,
    pub priority: JobPriority,
    pub retry_count: u32,
    pub process_pid: Option<u32>,
    pub tone_mapping_config: Option<ToneMappingConfig>,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
    /// Source video metadata for accurate progress tracking
    pub source_metadata: Option<SourceVideoMetadata>,
    /// Job type - regular or master adaptive
    pub job_type: JobType,
}

#[derive(Debug, Clone)]
pub struct SourceVideoMetadata {
    pub duration: f64,
    pub framerate: f64,
    pub total_frames: u64,
    pub width: u32,
    pub height: u32,
    pub codec: String,
}

#[derive(Debug, Clone)]
pub struct AdaptiveTranscodingJob {
    pub id: String,
    pub media_id: String,
    pub variants: Vec<VariantJob>,
    pub master_playlist_path: PathBuf,
    pub status: TranscodingStatus,
    pub created_at: Instant,
    pub priority: JobPriority,
}

#[derive(Debug, Clone)]
pub struct VariantJob {
    pub variant: ProfileVariant,
    pub job_id: String,
    pub playlist_path: PathBuf,
    pub output_dir: PathBuf,
    pub status: TranscodingStatus,
}

// Use the shared TranscodingStatus from ferrex-core
pub use ferrex_core::TranscodingStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum JobPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl Default for JobPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Segment generation request for on-the-fly transcoding
#[derive(Debug, Clone)]
pub struct SegmentRequest {
    pub job_id: String,
    pub segment_number: u32,
    pub priority: JobPriority,
    pub requested_at: Instant,
}

/// Job statistics for monitoring
#[derive(Debug, Clone, Serialize)]
pub struct JobStatistics {
    pub total_jobs: usize,
    pub pending_jobs: usize,
    pub processing_jobs: usize,
    pub completed_jobs: usize,
    pub failed_jobs: usize,
    pub average_processing_time: Duration,
    pub jobs_per_minute: f64,
}

/// Job progress information
#[derive(Debug, Clone, Serialize)]
pub struct JobProgress {
    pub job_id: String,
    pub status: TranscodingStatus,
    pub current_frame: Option<u64>,
    pub total_frames: Option<u64>,
    pub fps: Option<f32>,
    pub bitrate: Option<String>,
    pub speed: Option<f32>,
    pub eta: Option<Duration>,
}

impl TranscodingJob {
    pub fn new(
        id: String,
        media_id: String,
        profile: TranscodingProfile,
        output_dir: PathBuf,
        priority: JobPriority,
    ) -> Self {
        let playlist_path = output_dir.join("playlist.m3u8");
        let now = Instant::now();

        Self {
            id,
            media_id,
            profile,
            status: TranscodingStatus::Pending,
            output_dir,
            playlist_path,
            segments: HashMap::new(),
            error: None,
            created_at: now,
            updated_at: now,
            priority,
            retry_count: 0,
            process_pid: None,
            tone_mapping_config: Some(ToneMappingConfig::default()),
            started_at: None,
            completed_at: None,
            source_metadata: None,
            job_type: JobType::Regular,
        }
    }

    pub fn new_master(
        id: String,
        media_id: String,
        output_dir: PathBuf,
        priority: JobPriority,
        variant_job_ids: Vec<String>,
    ) -> Self {
        let playlist_path = output_dir.join("master.m3u8");
        let now = Instant::now();

        Self {
            id,
            media_id,
            profile: TranscodingProfile {
                name: "adaptive_master".to_string(),
                video_codec: "".to_string(),
                audio_codec: "".to_string(),
                video_bitrate: "".to_string(),
                audio_bitrate: "".to_string(),
                resolution: None,
                preset: "".to_string(),
                apply_tone_mapping: false,
            },
            status: TranscodingStatus::Pending,
            output_dir,
            playlist_path,
            segments: HashMap::new(),
            error: None,
            created_at: now,
            updated_at: now,
            priority,
            retry_count: 0,
            process_pid: None,
            tone_mapping_config: None,
            started_at: None,
            completed_at: None,
            source_metadata: None,
            job_type: JobType::Master { variant_job_ids },
        }
    }

    pub fn update_status(&mut self, status: TranscodingStatus) {
        self.status = status;
        self.updated_at = Instant::now();
    }

    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
        self.updated_at = Instant::now();
    }

    pub fn processing_time(&self) -> Duration {
        self.updated_at.duration_since(self.created_at)
    }
}

/// Job queue message types
#[derive(Debug, Clone)]
pub enum JobMessage {
    /// Submit a new job
    Submit(TranscodingJob),
    /// Cancel a job
    Cancel(String),
    /// Update job priority
    UpdatePriority {
        job_id: String,
        priority: JobPriority,
    },
    /// Get job status
    GetStatus(String),
    /// Request a specific segment
    RequestSegment(SegmentRequest),
    /// Update job status (for when a job completes)
    UpdateStatus {
        job_id: String,
        status: TranscodingStatus,
    },
}

/// Response from job queue
#[derive(Debug, Clone)]
pub enum JobResponse {
    /// Job submitted successfully
    Submitted(String),
    /// Job status
    Status(Option<TranscodingJob>),
    /// Job cancelled
    Cancelled,
    /// Error response
    Error(String),
}
