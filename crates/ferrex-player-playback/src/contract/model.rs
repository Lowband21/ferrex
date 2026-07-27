use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;
use url::Url;
use zeroize::Zeroize;

/// Monotonically increasing identity for a playback session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionGeneration(u64);

impl SessionGeneration {
    pub const INITIAL: Self = Self(1);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Ordering assigned by the serialized backend event owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventSequence(u64);

impl EventSequence {
    pub const FIRST: Self = Self(1);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// A value that must not reveal its contents through normal diagnostics.
#[derive(Clone, PartialEq, Eq)]
pub struct SensitiveValue(String);

impl SensitiveValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Deliberately expose this value to a backend adapter.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SensitiveValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

impl Drop for SensitiveValue {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PlaybackHttpHeader {
    pub name: String,
    pub value: SensitiveValue,
}

impl PlaybackHttpHeader {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: SensitiveValue::new(value),
        }
    }
}

impl fmt::Debug for PlaybackHttpHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlaybackHttpHeader")
            .field("name", &self.name)
            .field("value", &self.value)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PlaybackCookie {
    pub name: String,
    pub value: SensitiveValue,
}

impl PlaybackCookie {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: SensitiveValue::new(value),
        }
    }
}

impl fmt::Debug for PlaybackCookie {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlaybackCookie")
            .field("name", &self.name)
            .field("value", &self.value)
            .finish()
    }
}

/// Media source plus authentication material.
///
/// `Debug` intentionally omits the path, query, fragment, header values, and
/// cookie values. Adapters must pass this structure directly instead of
/// reconstructing a URL in a loggable command line.
#[derive(Clone, PartialEq, Eq)]
pub struct PlaybackSource {
    uri: Url,
    title: Option<String>,
    headers: Vec<PlaybackHttpHeader>,
    cookies: Vec<PlaybackCookie>,
}

impl PlaybackSource {
    pub fn new(uri: Url) -> Self {
        Self {
            uri,
            title: None,
            headers: Vec::new(),
            cookies: Vec::new(),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.headers.push(PlaybackHttpHeader::new(name, value));
        self
    }

    pub fn with_cookie(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.cookies.push(PlaybackCookie::new(name, value));
        self
    }

    pub fn uri(&self) -> &Url {
        &self.uri
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn headers(&self) -> &[PlaybackHttpHeader] {
        &self.headers
    }

    pub fn cookies(&self) -> &[PlaybackCookie] {
        &self.cookies
    }

    fn redacted_uri(&self) -> String {
        match self.uri.host() {
            Some(host) => {
                let port = self
                    .uri
                    .port()
                    .map(|port| format!(":{port}"))
                    .unwrap_or_default();
                format!("{}://{host}{port}/<redacted>", self.uri.scheme())
            }
            None => format!("{}:<redacted>", self.uri.scheme()),
        }
    }
}

impl fmt::Debug for PlaybackSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlaybackSource")
            .field("uri", &self.redacted_uri())
            .field("title", &self.title)
            .field("headers", &self.headers)
            .field("cookies", &self.cookies)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    GStreamer,
    Mpv,
    ExternalMpv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationMode {
    /// Native video surface integrated with the Iced host.
    IntegratedNative,
    /// Decoded frames presented by the legacy Iced/wgpu path.
    EmbeddedFrames,
    /// Backend-owned ordinary top-level window.
    NativeWindow,
    /// Window owned by a separate player process.
    ExternalWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct PlaybackTarget {
    pub backend: BackendKind,
    pub presentation: PresentationMode,
}

impl PlaybackTarget {
    pub const GSTREAMER_INTEGRATED: Self = Self {
        backend: BackendKind::GStreamer,
        presentation: PresentationMode::IntegratedNative,
    };
    pub const GSTREAMER_EMBEDDED: Self = Self {
        backend: BackendKind::GStreamer,
        presentation: PresentationMode::EmbeddedFrames,
    };
    pub const MPV_INTEGRATED: Self = Self {
        backend: BackendKind::Mpv,
        presentation: PresentationMode::IntegratedNative,
    };
    pub const MPV_NATIVE_WINDOW: Self = Self {
        backend: BackendKind::Mpv,
        presentation: PresentationMode::NativeWindow,
    };
    pub const EXTERNAL_MPV: Self = Self {
        backend: BackendKind::ExternalMpv,
        presentation: PresentationMode::ExternalWindow,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackContentFit {
    Contain,
    Cover,
    Fill,
    None,
    ScaleDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationDelta {
    Forward(Duration),
    Backward(Duration),
}

impl DurationDelta {
    pub fn from_seconds(seconds: f64) -> Option<Self> {
        if !seconds.is_finite() {
            return None;
        }

        let duration = Duration::try_from_secs_f64(seconds.abs()).ok()?;
        Some(if seconds.is_sign_negative() {
            Self::Backward(duration)
        } else {
            Self::Forward(duration)
        })
    }

    pub const fn magnitude(self) -> Duration {
        match self {
            Self::Forward(duration) | Self::Backward(duration) => duration,
        }
    }

    pub fn as_seconds_f64(self) -> f64 {
        match self {
            Self::Forward(duration) => duration.as_secs_f64(),
            Self::Backward(duration) => -duration.as_secs_f64(),
        }
    }
}

/// Local path passed intentionally to a playback backend.
///
/// Debug output is redacted because home-directory components and filenames
/// can contain private account or media information. Backends must likewise
/// avoid copying the raw path into normal logs or diagnostics.
#[derive(Clone, PartialEq, Eq)]
pub struct PlaybackFilePath(PathBuf);

impl PlaybackFilePath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl fmt::Debug for PlaybackFilePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted-local-path>")
    }
}

/// Name of a backend video profile selected explicitly by the user.
///
/// Profile names are omitted from normal debug output because they originate
/// in trusted user configuration and are not needed in issue diagnostics.
#[derive(Clone, PartialEq, Eq)]
pub struct VideoProfileName(String);

impl VideoProfileName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for VideoProfileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted-video-profile>")
    }
}

/// Portion of native video output included in a screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackScreenshotMode {
    /// Video after scaling/color processing, without rendered subtitles.
    VideoOnly,
    /// Video plus backend-rendered subtitles. OSD inclusion remains a
    /// backend/video-output detail; use `Window` when it is required.
    VideoWithSubtitles,
    /// Complete native playback window when the backend supports it.
    Window,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackCommand {
    Load(PlaybackSource),
    SetPaused(bool),
    SeekAbsolute(Duration),
    SeekRelative(DurationDelta),
    SetVolume(f64),
    SetMuted(bool),
    SetSpeed(f64),
    SelectAudio(TrackId),
    SelectSubtitle(Option<TrackId>),
    /// Add a local sidecar subtitle to the current media generation.
    AddExternalSubtitle {
        source: PlaybackFilePath,
        select: bool,
    },
    SelectChapter(ChapterId),
    SelectEdition(EditionId),
    SetContentFit(PlaybackContentFit),
    SetFullscreen(bool),
    /// Apply a named backend video profile. Capability-gated because user
    /// profiles may depend on explicitly trusted backend configuration.
    ApplyVideoProfile(VideoProfileName),
    /// Replace the ordered local shader chain used by native video output.
    SetVideoShaders(Vec<PlaybackFilePath>),
    /// Capture one native-output screenshot at an explicit local path.
    CaptureScreenshot {
        output: PlaybackFilePath,
        mode: PlaybackScreenshotMode,
    },
    Stop,
    /// Terminate the backend owner after ordered stop/cleanup.
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackState {
    Idle,
    Loading,
    Playing,
    Paused,
    Buffering,
    Seeking,
    Stopping,
    Ended,
    Failed,
    Terminated,
}

impl PlaybackState {
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Loading
                | Self::Playing
                | Self::Paused
                | Self::Buffering
                | Self::Seeking
                | Self::Stopping
        )
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BufferState {
    pub buffering: bool,
    pub percentage: Option<f64>,
    pub cached_duration: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackId(String);

impl TrackId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TrackId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrack {
    pub id: TrackId,
    pub title: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub channels: Option<u16>,
    pub sample_rate: Option<u32>,
    pub is_default: bool,
    pub is_forced: bool,
}

impl AudioTrack {
    pub fn display_name(&self) -> &str {
        self.title
            .as_deref()
            .or(self.language.as_deref())
            .unwrap_or(self.id.as_str())
    }
}

impl fmt::Display for AudioTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleKind {
    Text,
    Bitmap,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleTrack {
    pub id: TrackId,
    pub title: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub kind: SubtitleKind,
    pub is_default: bool,
    pub is_forced: bool,
    pub is_external: bool,
}

impl SubtitleTrack {
    pub fn display_name(&self) -> &str {
        self.title
            .as_deref()
            .or(self.language.as_deref())
            .unwrap_or(self.id.as_str())
    }
}

impl fmt::Display for SubtitleTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackCatalog {
    pub audio: Vec<AudioTrack>,
    pub subtitles: Vec<SubtitleTrack>,
    pub selected_audio: Option<TrackId>,
    pub selected_subtitle: Option<TrackId>,
}

impl TrackCatalog {
    /// Drop selections that do not exist in this catalog.
    pub fn normalize_selections(&mut self) {
        if self.selected_audio.as_ref().is_some_and(|selected| {
            !self.audio.iter().any(|track| &track.id == selected)
        }) {
            self.selected_audio = None;
        }

        if self.selected_subtitle.as_ref().is_some_and(|selected| {
            !self.subtitles.iter().any(|track| &track.id == selected)
        }) {
            self.selected_subtitle = None;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChapterId(String);

impl ChapterId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    pub id: ChapterId,
    pub title: Option<String>,
    pub start: Duration,
    pub end: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EditionId(String);

impl EditionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edition {
    pub id: EditionId,
    pub title: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct VideoParameters {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codec: Option<String>,
    pub pixel_format: Option<String>,
    pub bit_depth: Option<u8>,
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_matrix: Option<String>,
    pub hardware_decoder: Option<String>,
    pub hdr_metadata_observed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct PlaybackCapabilities {
    pub seek: bool,
    pub audio_track_selection: bool,
    pub subtitle_track_selection: bool,
    pub external_subtitle_loading: bool,
    pub chapter_selection: bool,
    pub edition_selection: bool,
    pub speed: bool,
    pub content_fit: bool,
    pub fullscreen: bool,
    pub screenshot: bool,
    pub video_shader_passthrough: bool,
    pub video_profile_passthrough: bool,
    pub integrated_presentation: bool,
    pub native_window_fallback: bool,
    pub native_hdr: bool,
    pub fractional_scaling: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Eof,
    Stopped,
    Replaced,
    /// The backend-owned native window requested an orderly quit.
    Closed,
    /// The backend core terminated without an earlier, more specific reason.
    BackendTerminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackErrorKind {
    InvalidSource,
    Authentication,
    UnsupportedMedia,
    UnsupportedOperation,
    BackendUnavailable,
    BackendInitialization,
    Command,
    Presenter,
    Protocol,
    Shutdown,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct PlaybackError {
    pub kind: PlaybackErrorKind,
    pub code: Option<i64>,
    pub message: String,
    pub recoverable: bool,
    pub backend: Option<BackendKind>,
}

impl PlaybackError {
    pub fn new(kind: PlaybackErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            code: None,
            message: message.into(),
            recoverable: false,
            backend: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReasonCode {
    RequestedUnavailable,
    MissingCapability,
    BackendDisabled,
    RuntimeIncompatible,
    InitializationFailed,
    PresenterFailed,
    UnsupportedPlatform,
    Policy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FallbackReason {
    pub code: FallbackReasonCode,
    pub from: Option<PlaybackTarget>,
    pub to: PlaybackTarget,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenterState {
    Detached,
    AwaitingHost,
    AwaitingVideoOutput,
    Attached,
    Hidden,
    Suspended,
    Failed,
}

/// Monotonically increasing host geometry revision.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize,
)]
pub struct GeometryRevision(u64);

impl GeometryRevision {
    /// First valid geometry revision.
    pub const INITIAL: Self = Self(1);

    /// Construct a geometry revision.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the numeric revision.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advance without wrapping.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Axis-aligned rectangle in host logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl LogicalRect {
    /// Construct a logical rectangle.
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn is_finite(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
    }

    fn has_nonnegative_extent(self) -> bool {
        self.width >= 0.0 && self.height >= 0.0
    }

    fn has_area(self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}

/// Validated geometry reserved by the host layout for native video.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SurfaceGeometry {
    /// Revision assigned by the host layout/redraw owner.
    pub revision: GeometryRevision,
    /// Complete logical slot bounds.
    pub logical_bounds: LogicalRect,
    /// Slot after inherited clipping; `None` means fully clipped.
    pub visible_bounds: Option<LogicalRect>,
    /// Logical-to-physical host scale.
    pub scale_factor: f64,
}

impl SurfaceGeometry {
    /// Construct one geometry observation.
    pub const fn new(
        revision: GeometryRevision,
        logical_bounds: LogicalRect,
        visible_bounds: Option<LogicalRect>,
        scale_factor: f64,
    ) -> Self {
        Self {
            revision,
            logical_bounds,
            visible_bounds,
            scale_factor,
        }
    }

    /// Validate finite coordinates, nonnegative extents, and positive scale.
    pub fn validate(self) -> Result<(), SurfaceGeometryError> {
        if !self.logical_bounds.is_finite()
            || self
                .visible_bounds
                .is_some_and(|bounds| !bounds.is_finite())
        {
            return Err(SurfaceGeometryError::NonFiniteBounds);
        }
        if !self.logical_bounds.has_nonnegative_extent()
            || self
                .visible_bounds
                .is_some_and(|bounds| !bounds.has_nonnegative_extent())
        {
            return Err(SurfaceGeometryError::NegativeExtent);
        }
        if !self.scale_factor.is_finite() || self.scale_factor <= 0.0 {
            return Err(SurfaceGeometryError::InvalidScale);
        }
        Ok(())
    }

    /// Whether the slot has nonzero logical and clipped area.
    pub fn is_visible(self) -> bool {
        self.validate().is_ok()
            && self.logical_bounds.has_area()
            && self.visible_bounds.is_some_and(LogicalRect::has_area)
    }

    pub(crate) fn same_layout(self, other: Self) -> bool {
        self.logical_bounds == other.logical_bounds
            && self.visible_bounds == other.visible_bounds
            && self.scale_factor == other.scale_factor
    }
}

/// Invalid host geometry that cannot cross into a native API safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SurfaceGeometryError {
    #[error("presenter geometry contains a non-finite coordinate")]
    NonFiniteBounds,
    #[error("presenter geometry contains a negative extent")]
    NegativeExtent,
    #[error("presenter scale factor must be finite and positive")]
    InvalidScale,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PresenterEvent {
    StateChanged(PresenterState),
    /// Latest accepted host geometry, or `None` after host detach/loss.
    GeometryChanged(Option<SurfaceGeometry>),
    FullscreenChanged(bool),
    Failure(PlaybackError),
    FallbackRequested(FallbackReason),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackEvent {
    StateChanged(PlaybackState),
    PositionChanged(Duration),
    DurationChanged(Option<Duration>),
    BufferChanged(BufferState),
    TracksChanged(TrackCatalog),
    ChaptersChanged(Vec<Chapter>),
    ChapterChanged(Option<ChapterId>),
    EditionsChanged(Vec<Edition>),
    EditionChanged(Option<EditionId>),
    VideoParametersChanged(Option<VideoParameters>),
    CapabilitiesChanged(PlaybackCapabilities),
    VolumeChanged(f64),
    MutedChanged(bool),
    SpeedChanged(f64),
    ContentFitChanged(PlaybackContentFit),
    FullscreenChanged(bool),
    Ended(EndReason),
    Error(PlaybackError),
    Presenter(PresenterEvent),
    Fallback(FallbackReason),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackEventEnvelope {
    pub generation: SessionGeneration,
    pub sequence: EventSequence,
    pub event: PlaybackEvent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackSnapshot {
    pub generation: SessionGeneration,
    pub last_sequence: Option<EventSequence>,
    pub target: PlaybackTarget,
    pub state: PlaybackState,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub buffer: BufferState,
    pub tracks: TrackCatalog,
    pub chapters: Vec<Chapter>,
    pub current_chapter: Option<ChapterId>,
    pub editions: Vec<Edition>,
    pub current_edition: Option<EditionId>,
    pub video: Option<VideoParameters>,
    pub capabilities: PlaybackCapabilities,
    pub volume: f64,
    pub muted: bool,
    pub speed: f64,
    pub content_fit: PlaybackContentFit,
    pub fullscreen: bool,
    pub presenter: PresenterState,
    pub presenter_geometry: Option<SurfaceGeometry>,
    pub end_reason: Option<EndReason>,
    pub last_error: Option<PlaybackError>,
    pub last_fallback: Option<FallbackReason>,
    pub fallback_chain: Vec<FallbackReason>,
}

impl PlaybackSnapshot {
    pub fn new(
        generation: SessionGeneration,
        target: PlaybackTarget,
        capabilities: PlaybackCapabilities,
    ) -> Self {
        Self {
            generation,
            last_sequence: None,
            target,
            state: PlaybackState::Idle,
            position: Duration::ZERO,
            duration: None,
            buffer: BufferState::default(),
            tracks: TrackCatalog::default(),
            chapters: Vec::new(),
            current_chapter: None,
            editions: Vec::new(),
            current_edition: None,
            video: None,
            capabilities,
            volume: 1.0,
            muted: false,
            speed: 1.0,
            content_fit: PlaybackContentFit::Contain,
            fullscreen: false,
            presenter: PresenterState::Detached,
            presenter_geometry: None,
            end_reason: None,
            last_error: None,
            last_fallback: None,
            fallback_chain: Vec::new(),
        }
    }

    pub const fn is_playing(&self) -> bool {
        matches!(self.state, PlaybackState::Playing)
    }

    pub const fn is_paused(&self) -> bool {
        matches!(self.state, PlaybackState::Paused)
    }

    pub const fn has_active_session(&self) -> bool {
        self.state.is_active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_extension_inputs_are_redacted_from_debug_output() {
        let path = PlaybackFilePath::new(
            "/home/private-user/Videos/private-title.screenshot.png",
        );
        let profile = VideoProfileName::new("private-user-profile");
        let command = PlaybackCommand::CaptureScreenshot {
            output: path.clone(),
            mode: PlaybackScreenshotMode::VideoWithSubtitles,
        };

        assert_eq!(format!("{path:?}"), "<redacted-local-path>");
        assert_eq!(format!("{profile:?}"), "<redacted-video-profile>");
        let debug = format!("{command:?}");
        assert!(debug.contains("CaptureScreenshot"));
        assert!(!debug.contains("private-user"));
        assert!(!debug.contains("private-title"));
    }

    #[test]
    fn source_debug_redacts_all_authentication_material() {
        let source = PlaybackSource::new(
            Url::parse(
                "https://user:password@example.test/private/media-id?access_token=query-secret#fragment",
            )
            .unwrap(),
        )
        .with_header("Authorization", "Bearer header-secret")
        .with_cookie("session", "cookie-secret");

        let debug = format!("{source:?}");

        assert!(debug.contains("example.test"));
        assert!(debug.contains("Authorization"));
        assert!(debug.contains("session"));
        for secret in [
            "user",
            "password",
            "private",
            "media-id",
            "access_token",
            "query-secret",
            "fragment",
            "header-secret",
            "cookie-secret",
        ] {
            assert!(!debug.contains(secret), "debug output leaked {secret}");
        }
    }

    #[test]
    fn pause_intent_is_not_inferred_from_transient_non_playing_states() {
        let mut snapshot = PlaybackSnapshot::new(
            SessionGeneration::INITIAL,
            PlaybackTarget::MPV_NATIVE_WINDOW,
            PlaybackCapabilities::default(),
        );

        for state in [
            PlaybackState::Loading,
            PlaybackState::Buffering,
            PlaybackState::Seeking,
        ] {
            snapshot.state = state;
            assert!(!snapshot.is_paused());
        }
        snapshot.state = PlaybackState::Paused;
        assert!(snapshot.is_paused());
    }

    #[test]
    fn signed_duration_rejects_non_finite_values() {
        assert!(DurationDelta::from_seconds(f64::NAN).is_none());
        assert!(DurationDelta::from_seconds(f64::INFINITY).is_none());
        assert_eq!(
            DurationDelta::from_seconds(-2.5).unwrap(),
            DurationDelta::Backward(Duration::from_millis(2_500))
        );
    }
}
