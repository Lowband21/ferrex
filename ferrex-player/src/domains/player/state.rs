use ferrex_core::player_prelude::{MediaFile, MediaID};
use iced::ContentFit;
use std::time::{Duration, Instant};
use subwave_core::video::types::{AudioTrack, SubtitleTrack};
use subwave_unified::video::SubwaveVideo;

// Seek bar interaction constants
pub const SEEK_BAR_VISUAL_HEIGHT: f32 = 4.0; // The visible bar height
pub const SEEK_BAR_CLICK_TOLERANCE_MULTIPLIER: f32 = 7.0; // Allow clicks within 7x the visual bar height

#[derive(Debug)]
pub struct PlayerDomainState {
    // Current media
    pub current_media: Option<MediaFile>,
    pub current_media_id: Option<MediaID>,
    pub current_url: Option<url::Url>,

    // Video instance (unified)
    pub video_opt: Option<SubwaveVideo>,

    // Watch progress tracking
    pub last_progress_update: Option<Instant>,
    pub last_progress_sent: f64,
    pub pending_resume_position: Option<f32>, // Position to resume at when video loads

    // Playback state
    pub buffered_percentage: f64, // Percentage of video buffered (0.0 to 1.0)
    pub dragging: bool,
    pub last_seek_position: Option<f64>,
    pub last_mouse_y: Option<f32>, // Track vertical mouse position for seek bar validation
    pub seek_bar_hovered: bool,    // Track if mouse is hovering over the seek bar
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
    pub content_fit: ContentFit,

    // Playlist control (NEW - for Phase 2 direct commands)
    pub is_shuffle_enabled: bool,
    pub is_repeat_enabled: bool,

    // Settings panel
    pub show_settings: bool,

    // Click tracking for double-click
    pub last_click_time: Option<Instant>,

    // Track selection (NEW)
    pub available_audio_tracks: Vec<AudioTrack>,
    pub current_audio_track: i32,
    pub available_subtitle_tracks: Vec<SubtitleTrack>,
    pub current_subtitle_track: Option<i32>,
    pub last_subtitle_track: Option<i32>,
    pub subtitles_enabled: bool,

    pub track_notification: Option<TrackNotification>,

    pub show_subtitle_menu: bool,

    pub show_quality_menu: bool,
    pub current_quality_profile: Option<String>,

    pub last_seek_time: Option<Instant>,
    pub pending_seek_position: Option<f64>,

    pub last_valid_position: f64,
    pub last_valid_duration: f64,

    pub is_hdr_content: bool,
    pub is_loading_video: bool, // Flag to prevent duplicate video loading TODO: refactor out
    pub source_duration: Option<f64>, // Original source video duration (never changes)

    pub external_mpv_handle: Option<Box<super::external_mpv::ExternalMpvHandle>>,
    pub external_mpv_active: bool,
}

#[derive(Debug, Clone)]
pub struct TrackNotification {
    pub message: String,
    pub show_time: Instant,
}

impl Default for PlayerDomainState {
    fn default() -> Self {
        Self {
            current_media: None,
            current_media_id: None,
            current_url: None,
            video_opt: None,
            last_progress_update: None,
            last_progress_sent: 0.0,
            pending_resume_position: None,
            buffered_percentage: 0.0, // Start with no buffer
            dragging: false,
            last_seek_position: None,
            last_mouse_y: None,
            seek_bar_hovered: false,
            seeking: false,
            seek_started_time: None,
            controls: true,
            controls_time: Instant::now(),
            is_fullscreen: false,
            volume: 1.0,
            is_muted: false,
            playback_speed: 1.0,
            content_fit: ContentFit::Contain,
            is_shuffle_enabled: false,
            is_repeat_enabled: false,
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
            last_valid_position: 0.0,
            last_valid_duration: 0.0,
            is_hdr_content: false,
            is_loading_video: false,
            source_duration: None,
            external_mpv_handle: None,
            external_mpv_active: false,
        }
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl PlayerDomainState {
    pub fn reset(&mut self) {
        self.current_media = None;
        self.current_media_id = None;
        self.current_url = None;
        self.video_opt = None;
        self.last_progress_update = None;
        self.last_progress_sent = 0.0;
        self.pending_resume_position = None;
        self.last_valid_position = 0.0;
        self.last_valid_duration = 0.0;
        self.buffered_percentage = 0.0; // Start with no buffer
        self.dragging = false;
        self.last_seek_position = None;
        self.last_mouse_y = None;
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
        self.is_loading_video = false;
        self.source_duration = None;
        self.content_fit = ContentFit::Contain;
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
        if let Some(notification) = &self.track_notification
            && notification.show_time.elapsed() > Duration::from_secs(2)
        {
            self.track_notification = None;
        }
    }

    /// Stop native/internal playback and release the video handle without resetting all state
    pub fn stop_native_playback(&mut self) {
        if let Some(mut video) = self.video_opt.take() {
            video.set_paused(true);
            drop(video);
        }
        self.seeking = false;
        self.dragging = false;
        self.last_seek_position = None;
        self.pending_seek_position = None;
        self.last_seek_time = None;
    }
}
