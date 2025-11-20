use crate::media_library::MediaFile;
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

    // Current subtitle text to display (raw text for processing)

    // Seek throttling
    pub last_seek_time: Option<Instant>,
    pub pending_seek_position: Option<f64>,
}

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
            buffered_percentage: 0.7, // Demo value - shows 70% buffered
            dragging: false,
            last_seek_position: None,
            seeking: false,
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
            last_seek_time: None,
            pending_seek_position: None,
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
        self.buffered_percentage = 0.7; // Demo value - shows 70% buffered
        self.dragging = false;
        self.last_seek_position = None;
        self.seeking = false;
        self.available_audio_tracks.clear();
        self.current_audio_track = 0;
        self.available_subtitle_tracks.clear();
        self.current_subtitle_track = None;
        self.last_subtitle_track = None;
        self.subtitles_enabled = false;
        self.track_notification = None;
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
