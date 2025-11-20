//! Video backend abstraction layer
//!
//! This module provides a unified interface for video playback that:
//! - Uses standard implementation for non-Wayland systems
//! - Uses native Wayland subsurface implementation on Wayland
//! - Maintains API compatibility with existing code

use std::panic;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use glib::ffi::G_CONVERT_ERROR_NOT_ABSOLUTE_PATH;
use iced::Error;
// Import both implementations
use iced_video_player as standard_video;
use iced_video_player_wayland as wayland_video;

// Re-export types from the standard implementation
pub use standard_video::{
    AlgorithmParams, AudioTrack, SubtitleTrack, ToneMappingAlgorithm, ToneMappingConfig,
    ToneMappingPreset, VideoPlayer,
};

/// Check if we're running on a Wayland compositor
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

/// Unified error type for video operations
#[derive(Debug, Clone)]
pub enum VideoError {
    Wayland(String),
    Standard(String),
    Unsupported(String),
}

impl std::fmt::Display for VideoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoError::Wayland(msg) => write!(f, "Wayland video error: {}", msg),
            VideoError::Standard(msg) => write!(f, "Standard video error: {}", msg),
            VideoError::Unsupported(msg) => write!(f, "Unsupported operation: {}", msg),
        }
    }
}

impl std::error::Error for VideoError {}

/// Video backend wrapper that provides a unified interface
///
/// Automatically selects the appropriate backend based on the platform
#[derive(Debug)]
pub struct Video {
    inner: VideoInner,
}

pub enum VideoInner {
    /// Standard cross-platform implementation (already thread-safe via internal RwLock)
    Standard(Arc<standard_video::Video>),
    /// Wayland implementation - matches simple_player.rs pattern
    Wayland(Arc<Mutex<Option<Box<wayland_video::Video>>>>),
}

// Manual Debug implementation since wayland_video::Video doesn't implement Debug
impl std::fmt::Debug for VideoInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoInner::Standard(video) => f.debug_tuple("Standard").field(video).finish(),
            VideoInner::Wayland(_) => f.debug_tuple("Wayland").field(&"<wayland_video>").finish(),
        }
    }
}

// Mark as Send/Sync since we handle thread safety internally
unsafe impl Send for Video {}
unsafe impl Sync for Video {}

impl Video {
    /// Create a new video from a URL
    ///
    /// Automatically selects the appropriate backend based on the platform
    pub fn new(uri: &url::Url) -> Result<Arc<Self>, VideoError> {
        if is_wayland() {
            Self::new_wayland(uri)
        } else {
            Self::new_standard(uri)
        }
    }

    /// Create a Wayland video (following simple_player.rs pattern)
    fn new_wayland(uri: &url::Url) -> Result<Arc<Self>, VideoError> {
        log::info!("Creating Wayland video for URI: {}", uri);

        // Initialize GStreamer if needed (matches simple_player.rs)
        if let Err(e) = wayland_video::init() {
            log::warn!("GStreamer init returned: {:?}", e);
        }

        // Create the video following the simple_player.rs pattern
        match wayland_video::Video::new(uri) {
            Ok(video) => {
                let video_box = Arc::new(Mutex::new(Some(Box::new(video))));
                Ok(Arc::new(Video {
                    inner: VideoInner::Wayland(video_box),
                }))
            }
            Err(e) => Err(VideoError::Wayland(format!(
                "Failed to create video: {:?}",
                e
            ))),
        }
    }

    /// Create a standard video (cross-platform, thread-safe)
    fn new_standard(uri: &url::Url) -> Result<Arc<Self>, VideoError> {
        match standard_video::Video::new(uri) {
            Ok(video) => Ok(Arc::new(Video {
                inner: VideoInner::Standard(Arc::new(video)),
            })),
            Err(e) => Err(VideoError::Standard(format!("{:?}", e))),
        }
    }

    /// Play the video
    pub fn play(&self) {
        match &self.inner {
            VideoInner::Standard(video) => {
                video.set_paused(false);
            }
            VideoInner::Wayland(video_mutex) => {
                if let Ok(guard) = video_mutex.lock() {
                    if let Some(video) = guard.as_ref() {
                        if let Err(e) = video.play() {
                            log::error!("Failed to play Wayland video: {}", e);
                        }
                    }
                }
            }
        }
    }

    /// Pause the video
    pub fn pause(&self) {
        match &self.inner {
            VideoInner::Standard(video) => {
                video.set_paused(true);
            }
            VideoInner::Wayland(video_mutex) => {
                if let Ok(guard) = video_mutex.lock() {
                    if let Some(video) = guard.as_ref() {
                        if let Err(e) = video.pause() {
                            log::error!("Failed to pause Wayland video: {}", e);
                        }
                    }
                }
            }
        }
    }

    /// Check if the video is paused
    pub fn paused(&self) -> bool {
        match &self.inner {
            VideoInner::Standard(video) => video.paused(),
            VideoInner::Wayland(video_mutex) => {
                if let Ok(guard) = video_mutex.lock() {
                    guard.as_ref().map(|v| v.is_paused()).unwrap_or(true)
                } else {
                    true
                }
            }
        }
    }

    /// Check if the video is paused (alias for compatibility)
    pub fn is_paused(&self) -> bool {
        self.paused()
    }

    /// Get the duration of the video
    pub fn duration(&self) -> Option<Duration> {
        match &self.inner {
            VideoInner::Standard(video) => Some(video.duration()),
            VideoInner::Wayland(video_mutex) => {
                if let Ok(guard) = video_mutex.lock() {
                    guard.as_ref().and_then(|v| v.duration())
                } else {
                    None
                }
            }
        }
    }

    /// Get the current position in the video
    pub fn position(&self) -> Duration {
        match &self.inner {
            VideoInner::Standard(video) => video.position(),
            VideoInner::Wayland(video_mutex) => {
                if let Ok(guard) = video_mutex.lock() {
                    if let Some(video) = guard.as_ref() {
                        if let Some(position) = video.position() {
                            //log::info!("[VideoBackend]: Position: {:?}", position);
                            position
                        } else {
                            log::error!("Failed to get position from video");
                            Duration::ZERO
                        }
                    } else {
                        log::error!("Failed to get reference to video to query position");
                        Duration::ZERO
                    }
                } else {
                    log::error!("Failed to get lock on video to query position");
                    Duration::ZERO
                }
            }
        }
    }

    /// Set the volume (0.0 to 1.0)
    pub fn set_volume(&self, volume: f64) {
        match &self.inner {
            VideoInner::Standard(video) => {
                video.set_volume(volume);
            }
            VideoInner::Wayland(video_mutex) => {
                if let Ok(guard) = video_mutex.lock() {
                    if let Some(video) = guard.as_ref() {
                        if let Err(e) = video.set_volume(volume) {
                            log::error!("Failed to set volume: {}", e);
                        }
                    }
                }
            }
        }
    }

    /// Get the current volume
    pub fn volume(&self) -> f64 {
        match &self.inner {
            VideoInner::Standard(video) => video.volume(),
            VideoInner::Wayland(_video_mutex) => {
                // Wayland video doesn't expose volume getter yet, return default
                1.0
            }
        }
    }

    /// Set playback speed
    pub fn set_speed(&self, speed: f64) {
        match &self.inner {
            VideoInner::Standard(video) => {
                video.set_speed(speed);
            }
            VideoInner::Wayland(video_mutex) => {
                if let Ok(guard) = video_mutex.lock() {
                    if let Some(video) = guard.as_ref() {
                        if let Err(e) = video.set_playback_rate(speed) {
                            log::error!("Failed to set playback rate: {}", e);
                        }
                    }
                }
            }
        }
    }

    /// Set playback rate (alias for set_speed)
    pub fn set_playback_rate(&self, rate: f64) {
        self.set_speed(rate);
    }

    /// Get video width
    pub fn width(&self) -> i32 {
        match &self.inner {
            VideoInner::Standard(video) => video.size().0,
            VideoInner::Wayland(video_mutex) => {
                if let Ok(guard) = video_mutex.lock() {
                    guard.as_ref().and_then(|v| v.width()).unwrap_or(1920)
                } else {
                    1920
                }
            }
        }
    }

    /// Get video height
    pub fn height(&self) -> i32 {
        match &self.inner {
            VideoInner::Standard(video) => video.size().1,
            VideoInner::Wayland(video_mutex) => {
                if let Ok(guard) = video_mutex.lock() {
                    guard.as_ref().and_then(|v| v.height()).unwrap_or(1080)
                } else {
                    1080
                }
            }
        }
    }

    /// Check if end of stream has been reached
    pub fn eos(&self) -> bool {
        match &self.inner {
            VideoInner::Standard(video) => video.eos(),
            VideoInner::Wayland(_video_mutex) => {
                // TODO: Implement EOS detection in Wayland
                false
            }
        }
    }

    /// Restart the video from the beginning
    pub fn restart(&self) -> Result<(), VideoError> {
        match &self.inner {
            VideoInner::Standard(video) => video
                .restart_stream()
                .map_err(|err| VideoError::Standard(err.to_string())),
            VideoInner::Wayland(_) => {
                // For Wayland, restart by seeking to the beginning
                self.seek(Duration::ZERO, false)
            }
        }
    }

    /// Set paused state (convenience method)
    pub fn set_paused(&self, paused: bool) {
        if paused {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Get available audio tracks
    pub fn audio_tracks(&self) -> Vec<AudioTrack> {
        match &self.inner {
            VideoInner::Standard(video) => video.audio_tracks(),
            VideoInner::Wayland(_video_mutex) => {
                // TODO: Implement audio track querying in Wayland
                vec![]
            }
        }
    }

    /// Get current audio track
    pub fn current_audio_track(&self) -> i32 {
        match &self.inner {
            VideoInner::Standard(video) => video.current_audio_track(),
            VideoInner::Wayland(_video_mutex) => {
                // Get current audio track from Wayland video implementation
                if let Some(video) = _video_mutex.lock().unwrap().as_ref() {
                    video.current_audio_track()
                } else {
                    0 // Default to first track if video not initialized
                }
            }
        }
    }

    /// Set audio track
    pub fn set_audio_track(&self, track: i32) {
        match &self.inner {
            VideoInner::Standard(video) => {
                video.select_audio_track(track);
            }
            VideoInner::Wayland(_video_mutex) => {
                // TODO: Implement audio track selection in Wayland
            }
        }
    }

    /// Get available subtitle tracks
    pub fn subtitle_tracks(&self) -> Vec<SubtitleTrack> {
        match &self.inner {
            VideoInner::Standard(video) => video.subtitle_tracks(),
            VideoInner::Wayland(_video_mutex) => {
                // TODO: Implement subtitle track querying in Wayland
                vec![]
            }
        }
    }

    /// Get current subtitle track
    pub fn current_subtitle_track(&self) -> Option<i32> {
        match &self.inner {
            VideoInner::Standard(video) => video.current_subtitle_track(),
            VideoInner::Wayland(_video_mutex) => {
                // TODO: Implement current subtitle track in Wayland
                None
            }
        }
    }

    /// Set subtitle track
    pub fn set_subtitle_track(&self, track: Option<i32>) {
        match &self.inner {
            VideoInner::Standard(video) => {
                video.select_subtitle_track(track);
            }
            VideoInner::Wayland(_video_mutex) => {
                // TODO: Implement subtitle track selection in Wayland
            }
        }
    }

    /// Check if subtitles are enabled
    pub fn subtitles_enabled(&self) -> bool {
        match &self.inner {
            VideoInner::Standard(video) => video.subtitles_enabled(),
            VideoInner::Wayland(_video_mutex) => {
                // TODO: Implement subtitle enabled state in Wayland
                false
            }
        }
    }

    /// Enable or disable subtitles
    pub fn set_subtitles_enabled(&self, enabled: bool) {
        match &self.inner {
            VideoInner::Standard(video) => video.set_subtitles_enabled(enabled),
            VideoInner::Wayland(_video_mutex) => {
                // TODO: Implement subtitle enable/disable in Wayland
            }
        }
    }

    /// Set muted state
    pub fn set_muted(&self, muted: bool) {
        match &self.inner {
            VideoInner::Standard(video) => video.set_muted(muted),
            VideoInner::Wayland(video_mutex) => {
                // TODO: Implement proper muting in Wayland
                if muted {
                    self.set_volume(0.0);
                }
            }
        }
    }

    /// Seek to position
    pub fn seek(&self, position: Duration, accurate: bool) -> Result<(), VideoError> {
        match &self.inner {
            VideoInner::Standard(video) => video
                .seek(position, accurate)
                .map_err(|err| VideoError::Standard(err.to_string())),
            VideoInner::Wayland(video_mutex) => {
                if let Ok(guard) = video_mutex.lock() {
                    if let Some(video) = guard.as_ref() {
                        video
                            .seek(position, accurate)
                            .map_err(|err| VideoError::Wayland(err.to_string()))
                    } else {
                        Err(VideoError::Wayland("Video not initialized".into()))
                    }
                } else {
                    Err(VideoError::Wayland("Failed to lock video".into()))
                }
            }
        }
    }

    /// Set tone mapping configuration
    pub fn set_tone_mapping_config(&self, config: ToneMappingConfig) {
        match &self.inner {
            VideoInner::Standard(video) => video.set_tone_mapping_config(config),
            VideoInner::Wayland(_video_mutex) => {
                // TODO: Wayland implementation auto-configures tone mapping
            }
        }
    }

    /// Check if video has a video stream
    pub fn has_video(&self) -> bool {
        match &self.inner {
            VideoInner::Standard(video) => video.has_video(),
            VideoInner::Wayland(video_mutex) => {
                // Assume video stream exists if initialized
                video_mutex.lock().unwrap().is_some()
            }
        }
    }

    /// Clone the underlying standard video if this is a standard implementation
    /// Used by the video_player widget
    pub fn clone_standard(&self) -> Option<Arc<standard_video::Video>> {
        match &self.inner {
            VideoInner::Standard(video) => {
                // Clone the video from the mutex
                // This is safe because iced_video_player::Video internally uses Arc
                Some(video.clone())
            }
            VideoInner::Wayland(_) => None,
        }
    }

    /// Get the inner video implementation
    pub fn inner(&self) -> &VideoInner {
        &self.inner
    }

    /// Check if this is a Wayland video
    pub fn is_wayland_video(&self) -> bool {
        matches!(&self.inner, VideoInner::Wayland(_))
    }

    /// Get a reference to the underlying Wayland video Arc for widget creation
    /// This is used by the view layer to create the VideoPlayer widget
    pub fn get_wayland_video_ref(&self) -> Option<&Arc<Mutex<Option<Box<wayland_video::Video>>>>> {
        match &self.inner {
            VideoInner::Wayland(video) => Some(video),
            _ => None,
        }
    }
}

/// Unified video player widget that handles both standard and Wayland backends
/// This properly delegates to the appropriate widget implementation
pub enum UnifiedVideoPlayer<'a, Message> {
    Standard(&'a standard_video::Video, std::marker::PhantomData<Message>),
    Wayland(&'a wayland_video::Video, std::marker::PhantomData<Message>),
}

/// A video player widget that wraps both standard and Wayland implementations
/// This avoids the need to leak memory by properly managing the video lifetime
pub struct VideoPlayerWidget<Message> {
    video: Arc<Video>,
    width: iced::Length,
    height: iced::Length,
    on_new_frame: Option<Message>,
    on_seek_done: Option<Message>,
    on_end_of_stream: Option<Message>,
}

impl<Message: Clone> VideoPlayerWidget<Message> {
    pub fn new(video: Arc<Video>) -> Self {
        Self {
            video,
            width: iced::Length::Fill,
            height: iced::Length::Fill,
            on_new_frame: None,
            on_seek_done: None,
            on_end_of_stream: None,
        }
    }

    pub fn width(mut self, width: iced::Length) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: iced::Length) -> Self {
        self.height = height;
        self
    }

    pub fn on_new_frame(mut self, msg: Message) -> Self {
        self.on_new_frame = Some(msg);
        self
    }

    pub fn on_seek_done(mut self, msg: Message) -> Self {
        self.on_seek_done = Some(msg);
        self
    }

    pub fn on_end_of_stream(mut self, msg: Message) -> Self {
        self.on_end_of_stream = Some(msg);
        self
    }
}

impl<'a, Message, Theme, Renderer> iced::advanced::Widget<Message, Theme, Renderer>
    for VideoPlayerWidget<Message>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size::new(self.width, self.height)
    }

    fn layout(
        &self,
        _tree: &mut iced::advanced::widget::Tree,
        _renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        let size = limits.width(self.width).height(self.height).resolve(
            self.width,
            self.height,
            iced::Size::ZERO,
        );

        iced::advanced::layout::Node::new(size)
    }

    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        _renderer: &mut Renderer,
        _theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        _layout: iced::advanced::Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        // The actual drawing is handled by the underlying video implementation
        // For now, this is a placeholder - the real implementation would delegate
        // to the appropriate video backend
    }

    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<VideoPlayerState>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(VideoPlayerState::new(self.video.clone()))
    }
}

impl<'a, Message, Theme> From<VideoPlayerWidget<Message>>
    for iced::Element<'a, Message, Theme, iced::Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
{
    fn from(widget: VideoPlayerWidget<Message>) -> Self {
        iced::Element::new(widget)
    }
}

/// State for the video player widget that holds the video
/// This implementation avoids memory leaks by properly managing video references
#[derive(Debug, Clone)]
pub struct VideoPlayerState {
    video: Arc<Video>,
    // Track whether we're using standard or Wayland
    is_wayland: bool,
}

impl VideoPlayerState {
    fn new(video: Arc<Video>) -> Self {
        // Check if this is a Wayland video
        let is_wayland = video.clone_standard().is_none();

        Self { video, is_wayland }
    }

    /// Get the video reference
    pub fn video(&self) -> &Arc<Video> {
        &self.video
    }
}

/// Unified video player widget that can hold either standard or Wayland widget
pub enum UnifiedVideoPlayerWidget<'a, Message> {
    Standard(iced_video_player::VideoPlayer<'a, Message>),
    Wayland(wayland_video::VideoPlayer<'a, Message>),
}

impl<'a, Message> UnifiedVideoPlayerWidget<'a, Message>
where
    Message: Clone + 'a,
{
    pub fn width(self, width: iced::Length) -> Self {
        match self {
            UnifiedVideoPlayerWidget::Standard(player) => {
                UnifiedVideoPlayerWidget::Standard(player.width(width))
            }
            UnifiedVideoPlayerWidget::Wayland(player) => {
                UnifiedVideoPlayerWidget::Wayland(player.width(width))
            }
        }
    }

    pub fn height(self, height: iced::Length) -> Self {
        match self {
            UnifiedVideoPlayerWidget::Standard(player) => {
                UnifiedVideoPlayerWidget::Standard(player.height(height))
            }
            UnifiedVideoPlayerWidget::Wayland(player) => {
                UnifiedVideoPlayerWidget::Wayland(player.height(height))
            }
        }
    }

    pub fn on_new_frame(self, msg: Message) -> Self {
        match self {
            UnifiedVideoPlayerWidget::Standard(player) => {
                UnifiedVideoPlayerWidget::Standard(player.on_new_frame(msg))
            }
            UnifiedVideoPlayerWidget::Wayland(player) => {
                // Wayland widget might not have this method yet
                UnifiedVideoPlayerWidget::Wayland(player)
            }
        }
    }

    pub fn on_seek_done(self, msg: Message) -> Self {
        match self {
            UnifiedVideoPlayerWidget::Standard(player) => {
                UnifiedVideoPlayerWidget::Standard(player.on_seek_done(msg))
            }
            UnifiedVideoPlayerWidget::Wayland(player) => {
                // Wayland widget might not have this method yet
                UnifiedVideoPlayerWidget::Wayland(player)
            }
        }
    }

    pub fn content_fit(self, fit: iced::ContentFit) -> Self {
        match self {
            UnifiedVideoPlayerWidget::Standard(player) => {
                UnifiedVideoPlayerWidget::Standard(player.content_fit(fit))
            }
            UnifiedVideoPlayerWidget::Wayland(player) => {
                UnifiedVideoPlayerWidget::Wayland(player.content_fit(fit))
            }
        }
    }
}

impl<'a, Message> From<UnifiedVideoPlayerWidget<'a, Message>> for iced::Element<'a, Message>
where
    Message: Clone + 'a,
{
    fn from(widget: UnifiedVideoPlayerWidget<'a, Message>) -> Self {
        match widget {
            UnifiedVideoPlayerWidget::Standard(player) => iced::Element::from(player),
            UnifiedVideoPlayerWidget::Wayland(player) => iced::Element::from(
                iced::widget::container(iced::widget::text("Wayland video requires wgpu renderer")),
            ),
        }
    }
}

/// Create a video_player widget for our Video type
///
/// This function bridges our Video abstraction with the appropriate video player widget.
/// Returns a unified widget that can handle both standard and Wayland backends.
///
/// NOTE: This still leaks memory for standard video. The proper solution is to
/// use the view layer's video_view method which handles both backends correctly.
pub fn video_player<'a, Message>(video: &Arc<Video>) -> VideoPlayer<'a, Message>
where
    Message: Clone + 'a,
{
    match &video.inner {
        VideoInner::Standard(std_video) => {
            // Clone the Arc to get an owned version
            let owned_video = std_video.clone();
            // Leak the owned Arc to get a 'static reference
            // TODO: This leaks memory and should be fixed
            let leaked: &'static standard_video::Video = Box::leak(Box::new(owned_video));
            iced_video_player::VideoPlayer::new(leaked)
        }
        VideoInner::Wayland(_) => {
            // Wayland video player should be created using the view layer's video_view method
            panic!("Use view layer's video_view method for Wayland video")
        }
    }
}

/*
/// Create a Wayland video_player widget
///
/// This creates the Wayland-specific video player widget.
pub fn video_player_wayland<'a, Message>(
    video: &Arc<Video>,
) -> wayland_video::VideoPlayer<'a, Message>
where
    Message: Clone + 'a,
{
    match &video.inner {
        VideoInner::Wayland(wayland_mutex) => {
            if let Some(wayland_video) = wayland_mutex.lock().unwrap().as_ref() {
                // We need to leak the reference here as well for now
                let leaked: &'static wayland_video::Video =
                    unsafe { std::mem::transmute(wayland_video.as_ref()) };
                wayland_video::VideoPlayer::new(leaked)
            } else {
                panic!("Wayland video not initialized")
            }
        }
        VideoInner::Standard(_) => {
            panic!("Standard video cannot use Wayland widget")
        }
    }
}
 */
