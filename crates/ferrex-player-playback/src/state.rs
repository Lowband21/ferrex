//! Playback state container for media, stream, controls, and track selection.
//!
//! The state intentionally separates runtime video handles from serializable-ish
//! UI/control flags so reducers and app shells can reason about playback without
//! coupling every caller to the concrete video backend.

use crate::{
    contract::{
        AudioTrack, BackendKind, BackendRequest, EndReason,
        PlaybackCapabilities, PlaybackSnapshot, PlaybackSource, PlaybackState,
        PlaybackTarget, SessionGeneration, SubtitleTrack, TrackId,
    },
    diagnostics::{PlaybackDiagnosticSnapshot, redact_playback_url},
    messages::{PlaybackExitDestination, PlaybackRequestId},
    session::PlaybackSession,
};
use ferrex_core::player_prelude::{MediaFile, MediaID};
use iced::ContentFit;
use std::{
    fmt,
    time::{Duration, Instant},
};

// Seek bar interaction constants
/// Visible height of the seek bar in pixels.
pub const SEEK_BAR_VISUAL_HEIGHT: f32 = 4.0;
/// Click tolerance multiplier around the visible seek bar height.
pub const SEEK_BAR_CLICK_TOLERANCE_MULTIPLIER: f32 = 7.0;

/// Mutable playback state owned by the playback domain.
pub struct PlayerDomainState {
    // Current media
    pub current_media: Option<MediaFile>,
    pub current_media_id: Option<MediaID>,
    /// Compatibility URI used by legacy streaming/external-player code.
    /// In-process backends consume `current_source` so credentials can travel
    /// in headers instead of this URL.
    pub current_url: Option<url::Url>,
    pub current_source: Option<PlaybackSource>,
    pub is_resolving_stream_url: bool,
    pub stream_url_resolution_failed: bool,
    /// Last allocated source-request identity. This counter is intentionally
    /// preserved by [`Self::reset`] so a delayed completion can never match a
    /// later playback request.
    pub playback_request_counter: PlaybackRequestId,
    /// Latest media/source request allowed to mutate playback state.
    pub active_playback_request: Option<PlaybackRequestId>,
    /// Active request whose authenticated source has been accepted.
    pub resolved_playback_request: Option<PlaybackRequestId>,
    /// Request that owns the currently open in-process backend session.
    pub session_playback_request: Option<PlaybackRequestId>,
    /// Media identity owned by the currently open in-process backend session.
    ///
    /// A replacement request may update `current_media_id` while the previous
    /// session remains visible during authorization. Terminal progress must
    /// remain attributed to the session that produced the snapshot.
    pub session_media_id: Option<MediaID>,
    /// Request that entered the shell's integrated single-window handoff.
    ///
    /// This provenance survives an in-session mpv-to-embedded fallback so the
    /// shell can restore itself even if the replacement snapshot has no mpv
    /// fallback chain of its own.
    pub integrated_playback_request: Option<PlaybackRequestId>,
    /// A playback root owner is being destroyed away from the UI reducer. No shell
    /// restoration or replacement backend may begin while this is true.
    pub root_shutdown_in_progress: bool,
    /// Root owner destruction failed without positive absence proof. This
    /// latch blocks every later backend launch until the process is restarted.
    pub root_shutdown_failed: bool,
    /// Request that owned the hidden retained shell when native teardown
    /// started. Completion uses this to retire or transfer shell ownership.
    pub root_shutdown_retired_request: Option<PlaybackRequestId>,
    /// A user exit received while teardown is already in flight. It overrides
    /// an older replacement/fallback continuation once completion arrives.
    pub root_shutdown_exit_destination: Option<PlaybackExitDestination>,
    /// Current request that explicitly selected the external-process backend.
    /// This intent is allocated atomically with the source request.
    pub external_playback_intent_request: Option<PlaybackRequestId>,
    /// Request that owns the live external process and its snapshot.
    pub external_playback_request: Option<PlaybackRequestId>,
    /// Media identity owned by the live external process. A newer request may
    /// replace `current_media_id` while the old process is still shutting down.
    pub external_media_id: Option<MediaID>,

    // Video instance (unified)
    pub video_opt: Option<PlaybackSession>,
    pub playback_generation: SessionGeneration,
    /// Explicit backend request for the next/current load. Auto preserves the
    /// migration default; native-window mpv is opt-in.
    pub backend_request: BackendRequest,
    /// Prevent repeated terminal handling while a polling subscription remains
    /// alive for one final turn of the UI event loop.
    pub terminal_generation_handled: Option<SessionGeneration>,
    /// Playback generation for which the shell has already received the
    /// native-presenter attached handoff. Presenter snapshots can be delivered
    /// repeatedly without repeating the window side effect.
    pub native_presenter_attached_generation: Option<SessionGeneration>,
    /// Playback generation for which the shell has already received the
    /// native-presenter unavailable handoff. This remains separate from the
    /// attached marker because an attached presenter may still fail over later
    /// in the same playback generation.
    pub native_presenter_unavailable_generation: Option<SessionGeneration>,

    // Watch progress tracking
    pub last_progress_update: Option<Instant>,
    pub last_progress_sent: f64,
    pub pending_resume_position: Option<f32>, // Position to resume at when video loads

    // Playback state
    pub buffered_percentage: f64, // Percentage of video buffered (0.0 to 1.0)
    pub dragging: bool,
    pub last_seek_position: Option<f64>,
    pub last_mouse_y: Option<f32>, // Track vertical mouse position for seek bar validation
    pub seek_bar_hovered: bool, // Track if mouse is hovering over the seek bar
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
    /// Generation that established the mirrored track catalog. Selection
    /// notices are emitted only for later changes in this same generation.
    pub track_catalog_generation: Option<SessionGeneration>,
    pub current_audio_track: Option<TrackId>,
    pub available_subtitle_tracks: Vec<SubtitleTrack>,
    pub current_subtitle_track: Option<TrackId>,
    pub last_subtitle_track: Option<TrackId>,
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
    pub is_loading_video: bool, // Flag to prevent duplicate video loading
    pub source_duration: Option<f64>, // Original source video duration (never changes)

    /// Process owner for the explicit legacy external-mpv compatibility path.
    /// Playback state exposed to domain/view policy lives in
    /// `external_mpv_snapshot`, not in this native handle.
    pub external_mpv_handle:
        Option<Box<crate::external_mpv::ExternalMpvHandle>>,
    /// Backend-neutral projection of the retained external process. Keeping a
    /// snapshot beside the process owner lets views, progress persistence, and
    /// episode policy use the same state model as in-process backends.
    pub external_mpv_snapshot: Option<PlaybackSnapshot>,
}

impl fmt::Debug for PlayerDomainState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let current_url = self
            .current_url
            .as_ref()
            .map(|url| redact_playback_url(url.as_str()));
        let current_source = self.current_source.as_ref();
        let video_opt = self.video_opt.as_ref().map(|_| "PlaybackSession(..)");

        f.debug_struct("PlayerDomainState")
            .field("current_media", &self.current_media)
            .field("current_media_id", &self.current_media_id)
            .field("current_url", &current_url)
            .field("current_source", &current_source)
            .field("is_resolving_stream_url", &self.is_resolving_stream_url)
            .field(
                "stream_url_resolution_failed",
                &self.stream_url_resolution_failed,
            )
            .field("playback_request_counter", &self.playback_request_counter)
            .field("active_playback_request", &self.active_playback_request)
            .field("resolved_playback_request", &self.resolved_playback_request)
            .field("session_playback_request", &self.session_playback_request)
            .field("session_media_id", &self.session_media_id)
            .field(
                "integrated_playback_request",
                &self.integrated_playback_request,
            )
            .field("root_shutdown_in_progress", &self.root_shutdown_in_progress)
            .field("root_shutdown_failed", &self.root_shutdown_failed)
            .field(
                "root_shutdown_retired_request",
                &self.root_shutdown_retired_request,
            )
            .field(
                "root_shutdown_exit_destination",
                &self.root_shutdown_exit_destination,
            )
            .field(
                "external_playback_intent_request",
                &self.external_playback_intent_request,
            )
            .field("external_playback_request", &self.external_playback_request)
            .field("external_media_id", &self.external_media_id)
            .field("video_opt", &video_opt)
            .field("playback_generation", &self.playback_generation)
            .field("backend_request", &self.backend_request)
            .field(
                "terminal_generation_handled",
                &self.terminal_generation_handled,
            )
            .field(
                "native_presenter_attached_generation",
                &self.native_presenter_attached_generation,
            )
            .field(
                "native_presenter_unavailable_generation",
                &self.native_presenter_unavailable_generation,
            )
            .field("last_progress_update", &self.last_progress_update)
            .field("last_progress_sent", &self.last_progress_sent)
            .field("pending_resume_position", &self.pending_resume_position)
            .field("buffered_percentage", &self.buffered_percentage)
            .field("dragging", &self.dragging)
            .field("last_seek_position", &self.last_seek_position)
            .field("last_mouse_y", &self.last_mouse_y)
            .field("seek_bar_hovered", &self.seek_bar_hovered)
            .field("seeking", &self.seeking)
            .field("seek_started_time", &self.seek_started_time)
            .field("controls", &self.controls)
            .field("controls_time", &self.controls_time)
            .field("is_fullscreen", &self.is_fullscreen)
            .field("volume", &self.volume)
            .field("is_muted", &self.is_muted)
            .field("playback_speed", &self.playback_speed)
            .field("content_fit", &self.content_fit)
            .field("is_shuffle_enabled", &self.is_shuffle_enabled)
            .field("is_repeat_enabled", &self.is_repeat_enabled)
            .field("show_settings", &self.show_settings)
            .field("last_click_time", &self.last_click_time)
            .field("available_audio_tracks", &self.available_audio_tracks)
            .field("track_catalog_generation", &self.track_catalog_generation)
            .field("current_audio_track", &self.current_audio_track)
            .field("available_subtitle_tracks", &self.available_subtitle_tracks)
            .field("current_subtitle_track", &self.current_subtitle_track)
            .field("last_subtitle_track", &self.last_subtitle_track)
            .field("subtitles_enabled", &self.subtitles_enabled)
            .field("track_notification", &self.track_notification)
            .field("show_subtitle_menu", &self.show_subtitle_menu)
            .field("show_quality_menu", &self.show_quality_menu)
            .field("current_quality_profile", &self.current_quality_profile)
            .field("last_seek_time", &self.last_seek_time)
            .field("pending_seek_position", &self.pending_seek_position)
            .field("last_valid_position", &self.last_valid_position)
            .field("last_valid_duration", &self.last_valid_duration)
            .field("is_hdr_content", &self.is_hdr_content)
            .field("is_loading_video", &self.is_loading_video)
            .field("source_duration", &self.source_duration)
            .field("external_mpv_handle", &self.external_mpv_handle)
            .field("external_mpv_snapshot", &self.external_mpv_snapshot)
            .finish()
    }
}

/// Short-lived notification shown after track selection changes.
#[derive(Debug, Clone)]
pub struct TrackNotification {
    /// Human-readable notification text.
    pub message: String,
    /// Instant the notification became visible.
    pub show_time: Instant,
}

impl Default for PlayerDomainState {
    fn default() -> Self {
        Self {
            current_media: None,
            current_media_id: None,
            current_url: None,
            current_source: None,
            is_resolving_stream_url: false,
            stream_url_resolution_failed: false,
            playback_request_counter: PlaybackRequestId::INITIAL,
            active_playback_request: None,
            resolved_playback_request: None,
            session_playback_request: None,
            session_media_id: None,
            integrated_playback_request: None,
            root_shutdown_in_progress: false,
            root_shutdown_failed: false,
            root_shutdown_retired_request: None,
            root_shutdown_exit_destination: None,
            external_playback_intent_request: None,
            external_playback_request: None,
            external_media_id: None,
            video_opt: None,
            playback_generation: SessionGeneration::new(0),
            backend_request: BackendRequest::Auto,
            terminal_generation_handled: None,
            native_presenter_attached_generation: None,
            native_presenter_unavailable_generation: None,
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
            track_catalog_generation: None,
            current_audio_track: None,
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
            external_mpv_snapshot: None,
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
        // Preserve the monotonic counter while retiring every request-scoped
        // capability. Delayed async messages are rejected by identity.
        self.active_playback_request = None;
        self.resolved_playback_request = None;
        self.session_playback_request = None;
        self.session_media_id = None;
        self.integrated_playback_request = None;
        self.external_playback_intent_request = None;
        self.external_playback_request = None;
        self.external_media_id = None;
        self.current_media = None;
        self.current_media_id = None;
        self.current_url = None;
        self.current_source = None;
        self.is_resolving_stream_url = false;
        self.stream_url_resolution_failed = false;
        self.video_opt = None;
        self.backend_request = BackendRequest::Auto;
        self.terminal_generation_handled = None;
        self.native_presenter_attached_generation = None;
        self.native_presenter_unavailable_generation = None;
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
        self.track_catalog_generation = None;
        self.current_audio_track = None;
        self.available_subtitle_tracks.clear();
        self.current_subtitle_track = None;
        self.last_subtitle_track = None;
        self.subtitles_enabled = false;
        self.track_notification = None;
        self.is_hdr_content = false;
        self.is_loading_video = false;
        self.source_duration = None;
        self.content_fit = ContentFit::Contain;
        self.clear_external_playback();
    }

    /// Allocate the next source request and invalidate every earlier async
    /// source completion. Exhaustion fails closed instead of reusing an ID.
    pub fn begin_playback_request(&mut self) -> Option<PlaybackRequestId> {
        let next = self.playback_request_counter.next()?;
        self.playback_request_counter = next;
        self.active_playback_request = Some(next);
        self.resolved_playback_request = None;
        self.integrated_playback_request = None;
        self.external_playback_intent_request = None;
        Some(next)
    }

    /// Retire the current source request without resetting the monotonic
    /// counter or an already-open older session.
    pub fn invalidate_playback_request(&mut self) {
        self.active_playback_request = None;
        self.resolved_playback_request = None;
        self.external_playback_intent_request = None;
        self.is_resolving_stream_url = false;
    }

    /// Whether an async result still belongs to the newest media request.
    pub fn is_active_playback_request(
        &self,
        request: PlaybackRequestId,
    ) -> bool {
        self.active_playback_request == Some(request)
    }

    /// Mark one current request's authenticated source as accepted.
    pub fn resolve_playback_request(
        &mut self,
        request: PlaybackRequestId,
    ) -> bool {
        if !self.is_active_playback_request(request) {
            return false;
        }
        self.resolved_playback_request = Some(request);
        true
    }

    /// Whether a shell/backend continuation is still authorized.
    pub fn is_resolved_playback_request(
        &self,
        request: PlaybackRequestId,
    ) -> bool {
        self.active_playback_request == Some(request)
            && self.resolved_playback_request == Some(request)
    }

    /// Whether native teardown still lacks positive completion proof.
    pub fn root_shutdown_blocks_launch(&self) -> bool {
        self.root_shutdown_in_progress || self.root_shutdown_failed
    }

    /// Whether this request still owns an explicit external-process launch.
    pub fn is_external_playback_intent(
        &self,
        request: PlaybackRequestId,
    ) -> bool {
        self.active_playback_request == Some(request)
            && self.external_playback_intent_request == Some(request)
    }

    /// Bind the current request to the external-process launch path.
    pub fn request_external_playback(
        &mut self,
        request: PlaybackRequestId,
    ) -> bool {
        if self.active_playback_request != Some(request) {
            return false;
        }
        self.external_playback_intent_request = Some(request);
        true
    }

    /// Request that owns the visible backend or, before backend creation, the
    /// resolved single-window handoff.
    pub fn playback_handoff_request(&self) -> Option<PlaybackRequestId> {
        match self.session_playback_request {
            Some(request) => Some(request),
            None => match self.external_playback_request {
                Some(request) => Some(request),
                None => self.resolved_playback_request,
            },
        }
    }

    /// Replace the current in-process source and keep the compatibility URI
    /// synchronized for code that does not yet understand authenticated
    /// headers.
    pub fn set_playback_source(&mut self, source: PlaybackSource) {
        self.current_url = Some(source.uri().clone());
        self.current_source = Some(source);
    }

    /// Set a URI-only source for unauthenticated compatibility paths.
    /// Credential-bearing HTTP streams must use [`Self::set_playback_source`]
    /// so authentication cannot be lost or reconstructed in the URI.
    pub fn set_playback_url(&mut self, url: url::Url) {
        self.set_playback_source(PlaybackSource::new(url));
    }

    pub fn playback_snapshot(&self) -> Option<&PlaybackSnapshot> {
        self.video_opt
            .as_ref()
            .map(PlaybackSession::snapshot)
            .or(self.external_mpv_snapshot.as_ref())
    }

    /// Whether an existing or not-yet-proven-absent root still owns the
    /// backend-neutral progress projection.
    ///
    /// UI request routing may stage a replacement resume hint while this is
    /// true, but must not overwrite `last_valid_*`: terminal actions still
    /// attribute those values to the visible session's media.
    pub fn has_observable_playback_root(&self) -> bool {
        self.playback_snapshot().is_some() || self.root_shutdown_blocks_launch()
    }

    /// Whether an in-process backend currently owns a presentation session.
    pub fn has_internal_session(&self) -> bool {
        self.video_opt.is_some()
    }

    /// Build the backend-owned presentation element without exposing its
    /// session handle to player views. Playback state still comes exclusively
    /// from `playback_snapshot`.
    #[cfg(feature = "ui")]
    pub fn playback_widget<'a>(
        &'a self,
        native_host_window: Option<iced::window::Id>,
    ) -> Option<
        iced::Element<
            'a,
            crate::PlayerMessage,
            iced::Theme,
            iced_wgpu::Renderer,
        >,
    > {
        self.video_opt
            .as_ref()
            .map(|session| session.widget(self.content_fit, native_host_window))
    }

    /// Whether the selected snapshot belongs to the retained external process.
    pub fn is_external_playback(&self) -> bool {
        self.playback_snapshot().is_some_and(|snapshot| {
            snapshot.target.backend == BackendKind::ExternalMpv
        })
    }

    /// Whether external-mpv polling should remain active.
    pub fn external_playback_active(&self) -> bool {
        self.external_mpv_snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.target.backend == BackendKind::ExternalMpv
                && snapshot.state.is_active()
        })
    }

    /// Whether any backend-neutral playback lifecycle is currently active.
    pub fn has_active_playback(&self) -> bool {
        self.playback_snapshot()
            .is_some_and(PlaybackSnapshot::has_active_session)
    }

    /// Start the reduced external-player lifecycle after process creation.
    pub fn begin_external_playback(
        &mut self,
        request: PlaybackRequestId,
        media_id: Option<MediaID>,
        generation: SessionGeneration,
        position: f64,
        duration: f64,
        fullscreen: bool,
    ) {
        let mut snapshot = PlaybackSnapshot::new(
            generation,
            PlaybackTarget::EXTERNAL_MPV,
            PlaybackCapabilities {
                seek: true,
                fullscreen: true,
                native_window_fallback: true,
                ..PlaybackCapabilities::default()
            },
        );
        snapshot.state = PlaybackState::Loading;
        snapshot.position =
            valid_external_duration(position).unwrap_or(Duration::ZERO);
        snapshot.duration = valid_external_duration(duration)
            .filter(|duration| *duration > Duration::ZERO);
        snapshot.fullscreen = fullscreen;
        self.external_mpv_snapshot = Some(snapshot);
        self.external_playback_request = Some(request);
        self.external_media_id = media_id;
    }

    /// Mark the spawned process ready without exposing its native handle.
    pub fn mark_external_playback_started(&mut self) {
        if let Some(snapshot) = self.external_mpv_snapshot.as_mut()
            && snapshot.state == PlaybackState::Loading
        {
            snapshot.state = PlaybackState::Playing;
            snapshot.end_reason = None;
        }
    }

    /// Reduce one copied IPC observation into the external snapshot.
    pub fn update_external_playback_snapshot(
        &mut self,
        position: f64,
        duration: f64,
    ) {
        let Some(snapshot) = self.external_mpv_snapshot.as_mut() else {
            return;
        };
        if let Some(position) = valid_external_duration(position) {
            snapshot.position = position;
        }
        if let Some(duration) = valid_external_duration(duration)
            && duration > Duration::ZERO
        {
            snapshot.duration = Some(duration);
        }
        if snapshot.state.is_active() {
            snapshot.state = PlaybackState::Playing;
        }
    }

    /// Capture the final copied IPC state before dropping the process owner.
    pub fn finish_external_playback(
        &mut self,
        position: f64,
        duration: f64,
        fullscreen: bool,
        reason: EndReason,
    ) {
        self.update_external_playback_snapshot(position, duration);
        if let Some(snapshot) = self.external_mpv_snapshot.as_mut() {
            snapshot.state = PlaybackState::Ended;
            snapshot.end_reason = Some(reason);
            snapshot.fullscreen = fullscreen;
        }
    }

    /// Drop the external process owner and its reduced lifecycle together.
    pub fn clear_external_playback(&mut self) {
        debug_assert!(
            self.external_mpv_handle.is_none(),
            "live external process must pass through the root shutdown barrier"
        );
        self.external_mpv_handle = None;
        self.external_mpv_snapshot = None;
        self.external_playback_request = None;
        self.external_media_id = None;
    }

    /// Retire a positively absent external root only when the completion still
    /// belongs to that process owner.
    pub fn clear_external_playback_for_request(
        &mut self,
        request: Option<PlaybackRequestId>,
    ) -> bool {
        if self.external_mpv_handle.is_some()
            || (request.is_some() && self.external_playback_request != request)
        {
            return false;
        }
        self.clear_external_playback();
        true
    }

    pub fn playback_diagnostics(&self) -> Option<PlaybackDiagnosticSnapshot> {
        self.video_opt
            .as_ref()
            .map(PlaybackSession::diagnostics)
            .or_else(|| {
                self.external_mpv_snapshot.as_ref().map(|snapshot| {
                    PlaybackDiagnosticSnapshot::from_snapshot(
                        snapshot,
                        BackendRequest::Exact(PlaybackTarget::EXTERNAL_MPV),
                    )
                })
            })
    }

    pub fn is_playing(&self) -> bool {
        self.playback_snapshot()
            .is_some_and(PlaybackSnapshot::is_playing)
    }

    pub fn update_controls(&mut self, in_use: bool) {
        if in_use || !self.has_video() {
            self.controls = true;
            self.controls_time = Instant::now();
        } else if self.controls
            && self.controls_time.elapsed() > Duration::from_secs(3)
        {
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
}

fn valid_external_duration(seconds: f64) -> Option<Duration> {
    (seconds.is_finite() && seconds >= 0.0)
        .then(|| Duration::try_from_secs_f64(seconds).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_does_not_reuse_session_generation() {
        let mut state = PlayerDomainState {
            playback_generation: SessionGeneration::new(42),
            native_presenter_attached_generation: Some(SessionGeneration::new(
                41,
            )),
            native_presenter_unavailable_generation: Some(
                SessionGeneration::new(41),
            ),
            current_source: Some(
                PlaybackSource::new(
                    "https://ferrex.example/api/v1/stream/media"
                        .parse()
                        .unwrap(),
                )
                .with_header("Authorization", "Bearer secret"),
            ),
            ..PlayerDomainState::default()
        };

        state.reset();

        assert_eq!(state.playback_generation, SessionGeneration::new(42));
        assert_eq!(state.backend_request, BackendRequest::Auto);
        assert_eq!(state.terminal_generation_handled, None);
        assert_eq!(state.native_presenter_attached_generation, None);
        assert_eq!(state.native_presenter_unavailable_generation, None);
        assert!(state.current_url.is_none());
        assert!(state.current_source.is_none());
    }

    #[test]
    fn external_process_lifecycle_reduces_into_the_neutral_snapshot() {
        let mut state = PlayerDomainState::default();
        let generation = SessionGeneration::new(9);
        let request = PlaybackRequestId::new(3);

        state.begin_external_playback(
            request,
            None,
            generation,
            f64::NAN,
            f64::INFINITY,
            true,
        );
        let snapshot = state.playback_snapshot().expect("external snapshot");
        assert_eq!(snapshot.generation, generation);
        assert_eq!(snapshot.target, PlaybackTarget::EXTERNAL_MPV);
        assert_eq!(snapshot.state, PlaybackState::Loading);
        assert_eq!(snapshot.position, Duration::ZERO);
        assert_eq!(snapshot.duration, None);
        assert!(snapshot.fullscreen);
        assert!(state.external_playback_active());

        state.mark_external_playback_started();
        state.update_external_playback_snapshot(12.5, 100.0);
        let snapshot = state.playback_snapshot().expect("updated snapshot");
        assert_eq!(snapshot.state, PlaybackState::Playing);
        assert_eq!(snapshot.position, Duration::from_millis(12_500));
        assert_eq!(snapshot.duration, Some(Duration::from_secs(100)));
        assert!(state.is_external_playback());
        assert!(state.has_active_playback());

        state.finish_external_playback(42.0, 100.0, false, EndReason::Eof);
        let snapshot = state.playback_snapshot().expect("terminal snapshot");
        assert_eq!(snapshot.state, PlaybackState::Ended);
        assert_eq!(snapshot.end_reason, Some(EndReason::Eof));
        assert!(!state.external_playback_active());
        let diagnostics = state
            .playback_diagnostics()
            .expect("external playback remains diagnosable after exit");
        assert_eq!(diagnostics.selected_target, PlaybackTarget::EXTERNAL_MPV);
        assert_eq!(
            diagnostics.requested_backend,
            BackendRequest::Exact(PlaybackTarget::EXTERNAL_MPV)
        );
        assert_eq!(
            diagnostics.summary().selected_backend,
            "mpv (external process)"
        );

        state.clear_external_playback();
        assert!(state.playback_snapshot().is_none());
    }

    #[test]
    fn reset_clears_external_snapshot_ownership() {
        let mut state = PlayerDomainState::default();
        state.begin_external_playback(
            PlaybackRequestId::new(4),
            None,
            SessionGeneration::new(4),
            3.0,
            20.0,
            false,
        );

        state.reset();

        assert!(state.external_mpv_snapshot.is_none());
        assert!(state.external_mpv_handle.is_none());
        assert!(!state.is_external_playback());
    }

    #[test]
    fn debug_redacts_current_stream_access_token() {
        let state = PlayerDomainState {
            current_url: Some(
                "https://ferrex.example/api/v1/stream/file?access_token=raw-secret"
                    .parse()
                    .expect("valid url"),
            ),
            current_source: Some(
                PlaybackSource::new(
                    "https://ferrex.example/api/v1/stream/file"
                        .parse()
                        .unwrap(),
                )
                .with_header("Authorization", "Bearer header-secret"),
            ),
            ..PlayerDomainState::default()
        };

        let debug = format!("{state:?}");

        assert!(debug.contains("access_token=<redacted>"));
        assert!(debug.contains("Authorization"));
        assert!(!debug.contains("raw-secret"));
        assert!(!debug.contains("header-secret"));
    }
}
