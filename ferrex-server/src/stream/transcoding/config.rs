use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscodingConfig {
    /// FFmpeg binary path
    pub ffmpeg_path: String,
    /// FFprobe binary path
    pub ffprobe_path: String,
    /// Directory for storing transcoded segments
    pub transcode_cache_dir: PathBuf,
    /// Default segment duration in seconds
    pub segment_duration: u32,
    /// Number of segments to generate ahead
    pub segments_ahead: u32,
    /// Enable hardware acceleration
    pub hw_accel_enabled: bool,
    /// Hardware acceleration type (vaapi, nvenc, qsv)
    pub hw_accel_type: Option<String>,
    /// Maximum concurrent transcoding jobs
    pub max_concurrent_jobs: usize,
    /// Worker thread count
    pub worker_count: usize,
    /// Maximum cache size in MB
    pub max_cache_size_mb: u64,
    /// Enable adaptive bitrate streaming
    pub enable_adaptive_bitrate: bool,
    /// Segments to pre-generate
    pub pregenerate_segments: usize,
}

impl Default for TranscodingConfig {
    fn default() -> Self {
        Self {
            ffmpeg_path: "ffmpeg".to_string(),
            ffprobe_path: "ffprobe".to_string(),
            transcode_cache_dir: PathBuf::from("./cache/transcode"),
            segment_duration: 4,
            segments_ahead: 3,
            hw_accel_enabled: false,
            hw_accel_type: None,
            max_concurrent_jobs: 4,
            worker_count: 2,
            max_cache_size_mb: 10_000, // 10GB
            enable_adaptive_bitrate: true,
            pregenerate_segments: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToneMappingConfig {
    /// Tone mapping algorithm (reinhard, hable, mobius)
    pub algorithm: String,
    /// Peak brightness for tone mapping
    pub peak_brightness: f32,
    /// Desaturation amount (0.0 to 1.0)
    pub desat: f32,
    /// Target color space (bt709, bt2020)
    pub target_colorspace: String,
    /// Enable GPU tone mapping if available
    pub use_gpu: bool,
}

impl Default for ToneMappingConfig {
    fn default() -> Self {
        Self {
            algorithm: "hable".to_string(),
            peak_brightness: 100.0,
            desat: 0.0,
            target_colorspace: "bt709".to_string(),
            use_gpu: true,
        }
    }
}

impl ToneMappingConfig {
    /// Configuration for bright SDR displays
    pub fn for_bright_sdr_display() -> Self {
        Self {
            algorithm: "hable".to_string(),
            peak_brightness: 500.0,
            desat: 0.0,
            target_colorspace: "bt709".to_string(),
            use_gpu: true,
        }
    }
}
