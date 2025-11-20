use crate::media_library::MediaFile;
use crate::hls::{HlsClient, MasterPlaylist, VariantPlaylist};
use iced_video_player::{AudioTrack, SubtitleTrack, Video};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AspectRatio {
    Original,
    Fill,
    Fit,
    Stretch,
}

#[derive(Debug)]
pub struct PlayerState {
    // Current media
    pub current_media: Option<MediaFile>,
    pub current_url: Option<url::Url>,

    // Video instance
    pub video_opt: Option<Video>,

    // Playback state
    pub position: f64,
    pub duration: f64,
    pub buffered_percentage: f64, // Percentage of video buffered (0.0 to 1.0)
    pub dragging: bool,
    pub last_seek_position: Option<f64>,
    pub seeking: bool,
    pub seek_started_time: Option<Instant>,

    // Controls visibility
    pub controls: bool,
    pub controls_time: Instant,

    // Player settings
    pub is_fullscreen: bool,
    pub volume: f64,
    pub is_muted: bool,
    pub playback_speed: f64,
    pub aspect_ratio: AspectRatio,

    // Settings panel
    pub show_settings: bool,

    // Click tracking for double-click
    pub last_click_time: Option<Instant>,

    // Track selection (NEW)
    pub available_audio_tracks: Vec<AudioTrack>,
    pub current_audio_track: i32,
    pub available_subtitle_tracks: Vec<SubtitleTrack>,
    pub current_subtitle_track: Option<i32>,
    pub last_subtitle_track: Option<i32>, // Track the last used subtitle for toggling
    pub subtitles_enabled: bool,

    // Track selection notification
    pub track_notification: Option<TrackNotification>,

    // Subtitle menu state
    pub show_subtitle_menu: bool,
    
    // Quality selection menu state
    pub show_quality_menu: bool,
    pub current_quality_profile: Option<String>,

    // Current subtitle text to display (raw text for processing)

    // Seek throttling
    pub last_seek_time: Option<Instant>,
    pub pending_seek_position: Option<f64>,
    
    // HDR and transcoding state
    pub is_hdr_content: bool,
    pub using_hls: bool,
    pub transcoding_status: Option<TranscodingStatus>,
    pub transcoding_job_id: Option<String>,
    
    // HLS adaptive streaming
    pub hls_client: Option<HlsClient>,
    pub master_playlist: Option<MasterPlaylist>,
    pub current_variant_playlist: Option<VariantPlaylist>,
    pub current_segment_index: usize,
    pub segment_buffer: Vec<Vec<u8>>, // Prefetched segments
    
    // Performance metrics
    pub last_bandwidth_measurement: Option<u64>, // bits per second
    pub quality_switch_count: u32,
    pub transcoding_duration: Option<f64>, // Duration from transcoding job
    pub transcoding_check_count: u32, // Number of status checks performed
    pub is_loading_video: bool, // Flag to prevent duplicate video loading
    pub source_duration: Option<f64>, // Original source video duration (never changes)
}

// Use the shared TranscodingStatus from ferrex-core
pub use ferrex_core::TranscodingStatus;

#[derive(Debug, Clone)]
pub struct TrackNotification {
    pub message: String,
    pub show_time: Instant,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            current_media: None,
            current_url: None,
            video_opt: None,
            position: 0.0,
            duration: 0.0,
            buffered_percentage: 0.0, // Start with no buffer
            dragging: false,
            last_seek_position: None,
            seeking: false,
            seek_started_time: None,
            controls: true,
            controls_time: Instant::now(),
            is_fullscreen: false,
            volume: 1.0,
            is_muted: false,
            playback_speed: 1.0,
            aspect_ratio: AspectRatio::Fit,
            show_settings: false,
            last_click_time: None,
            available_audio_tracks: Vec::new(),
            current_audio_track: 0,
            available_subtitle_tracks: Vec::new(),
            current_subtitle_track: None,
            last_subtitle_track: None,
            subtitles_enabled: false,
            track_notification: None,
            show_subtitle_menu: false,
            show_quality_menu: false,
            current_quality_profile: None,
            last_seek_time: None,
            pending_seek_position: None,
            is_hdr_content: false,
            using_hls: false,
            transcoding_status: None,
            transcoding_job_id: None,
            hls_client: None,
            master_playlist: None,
            current_variant_playlist: None,
            current_segment_index: 0,
            segment_buffer: Vec::new(),
            last_bandwidth_measurement: None,
            quality_switch_count: 0,
            transcoding_duration: None,
            transcoding_check_count: 0,
            is_loading_video: false,
            source_duration: None,
        }
    }
}

impl PlayerState {
    pub fn reset(&mut self) {
        self.current_media = None;
        self.current_url = None;
        self.video_opt = None;
        self.position = 0.0;
        self.duration = 0.0;
        self.buffered_percentage = 0.0; // Start with no buffer
        self.dragging = false;
        self.last_seek_position = None;
        self.seeking = false;
        self.seek_started_time = None;
        self.available_audio_tracks.clear();
        self.current_audio_track = 0;
        self.available_subtitle_tracks.clear();
        self.current_subtitle_track = None;
        self.last_subtitle_track = None;
        self.subtitles_enabled = false;
        self.track_notification = None;
        self.is_hdr_content = false;
        self.using_hls = false;
        self.transcoding_status = None;
        self.transcoding_job_id = None;
        self.hls_client = None;
        self.master_playlist = None;
        self.current_variant_playlist = None;
        self.current_segment_index = 0;
        self.segment_buffer.clear();
        self.last_bandwidth_measurement = None;
        self.quality_switch_count = 0;
        self.transcoding_duration = None;
        self.transcoding_check_count = 0;
        self.is_loading_video = false;
        self.source_duration = None;
    }

    pub fn is_playing(&self) -> bool {
        self.video_opt
            .as_ref()
            .map(|v| !v.paused())
            .unwrap_or(false)
    }

    pub fn update_controls(&mut self, in_use: bool) {
        if in_use || !self.has_video() {
            self.controls = true;
            self.controls_time = Instant::now();
        } else if self.controls && self.controls_time.elapsed() > Duration::from_secs(3) {
            self.controls = false;
        }
    }

    pub fn has_video(&self) -> bool {
        self.video_opt
            .as_ref()
            .map(|v| v.has_video())
            .unwrap_or(false)
    }

    pub fn show_track_notification(&mut self, message: String) {
        self.track_notification = Some(TrackNotification {
            message,
            show_time: Instant::now(),
        });
    }

    pub fn update_track_notification(&mut self) {
        if let Some(notification) = &self.track_notification {
            if notification.show_time.elapsed() > Duration::from_secs(2) {
                self.track_notification = None;
            }
        }
    }
}
