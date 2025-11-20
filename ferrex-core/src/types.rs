use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TranscodingStatus {
    Pending,
    Queued,
    Processing { progress: f32 },
    Completed,
    Failed { error: String },
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscodingProgressDetails {
    pub percentage: f32,
    pub time_elapsed: Option<f64>,
    pub estimated_time_remaining: Option<f64>,
    pub frames_processed: Option<u64>,
    pub current_fps: Option<f64>,
    pub current_bitrate: Option<u64>,
}