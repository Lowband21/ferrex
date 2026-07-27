//! Transcoding job status and progress response DTOs.
//!
//! Server APIs use these shapes to report queued/running/completed transcode
//! work and clients use them to render playback preparation progress.

use std::{fmt, str::FromStr};

/// Server-supported HLS quality profiles.
///
/// These names are part of the v1 HTTP contract. Keeping the set closed makes
/// cache paths and FFmpeg arguments server-owned instead of accepting raw
/// encoder/filter strings from clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TranscodeQualityProfile {
    #[cfg_attr(feature = "serde", serde(rename = "original"))]
    Original,
    #[cfg_attr(feature = "serde", serde(rename = "1080p"))]
    P1080,
    #[cfg_attr(feature = "serde", serde(rename = "720p"))]
    P720,
    #[cfg_attr(feature = "serde", serde(rename = "480p"))]
    P480,
    #[cfg_attr(feature = "serde", serde(rename = "360p"))]
    P360,
}

impl TranscodeQualityProfile {
    pub const ALL: [Self; 5] = [
        Self::Original,
        Self::P1080,
        Self::P720,
        Self::P480,
        Self::P360,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
            Self::P480 => "480p",
            Self::P360 => "360p",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
            Self::P480 => "480p",
            Self::P360 => "360p",
        }
    }
}

impl fmt::Display for TranscodeQualityProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for TranscodeQualityProfile {
    type Err = ParseTranscodeQualityProfileError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "original" => Ok(Self::Original),
            "1080p" => Ok(Self::P1080),
            "720p" => Ok(Self::P720),
            "480p" => Ok(Self::P480),
            "360p" => Ok(Self::P360),
            _ => Err(ParseTranscodeQualityProfileError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseTranscodeQualityProfileError;

impl fmt::Display for ParseTranscodeQualityProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unsupported transcode quality profile")
    }
}

impl std::error::Error for ParseTranscodeQualityProfileError {}

/// Request body for starting a server-generated HLS rendition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StartTranscodeRequest {
    pub profile: TranscodeQualityProfile,
}

/// Stable public lifecycle states for an HLS transcode job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum TranscodeJobState {
    Queued,
    Running,
    Completed,
    Failed,
}

/// Privacy-safe v1 response for both transcode start and status endpoints.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TranscodeJobStatusResponse {
    pub job_id: String,
    pub media_id: String,
    pub profile: TranscodeQualityProfile,
    pub state: TranscodeJobState,
    pub progress: Option<f32>,
    pub message: Option<String>,
    /// Credential-free API path to the protected media playlist.
    pub playback_path: Option<String>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TranscodingJobResponse {
    pub id: String,
    pub media_id: String,
    pub media_path: String,
    pub profile: String,
    pub status: TranscodingStatus,
    pub created_at: u64, // Unix timestamp
    pub output_path: Option<String>,
    pub playlist_path: Option<String>,
    pub error: Option<String>,
    pub progress_details: Option<TranscodingProgressDetails>,
    pub duration: Option<f64>, // Video duration in seconds
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TranscodingStatus {
    Pending,
    Queued,
    Processing { progress: f32 },
    Completed,
    Failed { error: String },
    Cancelled,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TranscodingProgressDetails {
    pub percentage: f32,
    pub time_elapsed: Option<f64>,
    pub estimated_time_remaining: Option<f64>,
    pub frames_processed: Option<u64>,
    pub current_fps: Option<f64>,
    pub current_bitrate: Option<u64>,
}
