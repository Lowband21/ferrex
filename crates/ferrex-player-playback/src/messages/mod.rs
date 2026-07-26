//! Playback domain messages and subscription payloads.
//!
//! Messages describe media loading, controls, seek/track interactions, stream URL
//! resolution, and progress updates independent of the final app state.

/// Playback subscription DTOs.
pub mod subscriptions;

use crate::contract::{ChapterId, EditionId, PlaybackSource, TrackId};
use ferrex_core::player_prelude::{MediaFile, MediaID};
use ferrex_player_api::services::streaming::TranscodeQualityProfile;
use iced::ContentFit;
use std::fmt;
use std::time::Duration;

/// Monotonic identity for one media-source request.
///
/// This is deliberately distinct from a playback session generation: URL
/// authorization exists before a backend session, and one request may replace
/// or fall back across multiple backend sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlaybackRequestId(u64);

impl PlaybackRequestId {
    /// Reserved initial value before the first request is allocated.
    pub const INITIAL: Self = Self(0);

    /// Construct an identity for deterministic state-machine tests.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the numeric identity for diagnostics and tests.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advance to a never-reused request identity.
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Navigation performed only after playback teardown is positively complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackExitDestination {
    None,
    Back,
    Home,
}

#[derive(Clone)]
pub enum PlayerMessage {
    // Media control
    PlayMedia(MediaFile),
    PlayMediaWithId(MediaFile, MediaID),
    /// Atomically allocate a media request whose resolved source must be
    /// handed to the explicit external-process backend.
    PlayMediaWithIdExternally(MediaFile, MediaID),
    NavigateBack, // Navigate to previous view
    NavigateHome, // Navigate to home/library view

    // Playback control
    Play,
    Pause,
    PlayPause,
    Stop,
    /// Complete teardown for the request captured when exit began. A delayed
    /// reset must never erase a newer playback request.
    ResetAfterStop(Option<PlaybackRequestId>),
    /// Finish reset/navigation after any root teardown barrier.
    CompletePlaybackExit {
        request: Option<PlaybackRequestId>,
        destination: PlaybackExitDestination,
    },
    /// Restore the retained shell after a root teardown that must preserve
    /// the current error/request projection instead of resetting playback.
    RestoreShellAfterRootShutdown {
        request: Option<PlaybackRequestId>,
    },
    /// Positive completion from the root owner reaper. The wrapped action
    /// cannot be reduced until the in-flight shutdown phase is cleared.
    RootShutdownCompleted {
        request: Option<PlaybackRequestId>,
        continuation: Box<PlayerMessage>,
    },

    // Seeking
    Seek(f64),
    SeekTo(Duration), // Direct command for seeking to specific duration
    SeekRelative(f64),
    SeekRelease,
    SeekBarPressed,
    SeekDone, // Seek operation completed

    // Volume
    SetVolume(f64),
    ToggleMute,

    // Playlist control (NEW - for Phase 2 direct commands)
    ToggleShuffle,
    ToggleRepeat,

    // Episode navigation
    NextEpisode,
    PreviousEpisode,

    // Video events
    /// Backend-open completion scoped to the source request that created it.
    VideoLoaded {
        request: PlaybackRequestId,
        success: bool,
    },
    VideoReadyToPlay, // Video is ready to be loaded and played (from streaming domain)
    EndOfStream,
    /// Synchronize a legacy adapter snapshot on the bounded controls timer,
    /// independently of decoded-frame presentation.
    PlaybackSnapshotTick,
    /// Drain copied events from asynchronous native backends without tying
    /// state updates to decoded-frame redraws.
    PlaybackEventsReady,
    /// Capture the renderer window's raw host on the Iced event-loop thread.
    CaptureNativeVideoHost(iced::window::Id),
    /// Completion of one raw-host capture. The native handle itself never
    /// enters the message channel.
    NativeVideoHostCaptured {
        window_id: iced::window::Id,
        result: Result<(), String>,
    },
    /// Drain UI-thread-local presenter effects into `PlaybackSnapshot`.
    NativePresenterUpdated,
    /// Refresh native-root geometry/visibility independently of mpv events.
    NativePresenterRefresh,
    Reload,

    // External player control
    PlayExternal,
    /// Resume an explicit external-player handoff after root owner teardown.
    ResumeExternalPlaybackAfterRootShutdown {
        request: PlaybackRequestId,
    },
    /// Shell-ordered continuation after the retained main window is hidden and
    /// any integrated controls donor has been closed without restoration.
    OpenExternalStreamSource {
        request: PlaybackRequestId,
    },
    /// Continue with an in-process backend only after a failed external spawn
    /// has restored the request-owned retained shell.
    ResumeInternalPlaybackAfterExternalLaunchFailure {
        request: PlaybackRequestId,
    },
    // Internal/current-domain replacement: allocate a fresh request identity,
    // set a resolved, redacted source, and trigger playback.
    SetStreamSource(PlaybackSource),
    /// Completion of one asynchronous authenticated stream-source request.
    StreamSourceResolved {
        request: PlaybackRequestId,
        source: PlaybackSource,
    },
    /// Internal shell-ordered continuation after the retained main window has
    /// completed its integrated-playback hide action.
    OpenResolvedStreamSource {
        request: PlaybackRequestId,
    },
    /// Resume source selection after an older root is fully destroyed.
    ResumeResolvedStreamSourceAfterRootShutdown {
        request: PlaybackRequestId,
        retired_request: Option<PlaybackRequestId>,
    },
    /// Fail closed when root teardown could not establish completion.
    RootShutdownFailed {
        request: Option<PlaybackRequestId>,
        message: String,
    },
    // Internal: surface stream authorization failures before opening a renderer
    StreamUrlResolutionFailed {
        request: PlaybackRequestId,
        message: String,
    },

    // UI control
    ShowControls,
    ToggleFullscreen,
    DisableFullscreen,
    ToggleSettings,
    MouseMoved(iced::Point),
    /// Begin an AppKit-managed drag of mpv's retained native root window.
    ///
    /// This is emitted only by the central video/background surface in the
    /// integrated macOS presentation path. Other targets keep `VideoClicked`.
    BeginNativeRootDrag,
    VideoClicked,
    VideoDoubleClicked,

    // Settings
    SetPlaybackSpeed(f64),
    SetContentFit(ContentFit),
    QualityProfileSelected(TranscodeQualityProfile),

    // Track selection
    AudioTrackSelected(TrackId),
    SubtitleTrackSelected(Option<TrackId>),
    ChapterSelected(ChapterId),
    EditionSelected(EditionId),
    ToggleSubtitles,
    ToggleSubtitleMenu,
    ToggleQualityMenu,
    ToggleAppsinkBackend,
    CycleAudioTrack,
    CycleSubtitleTrack,
    CycleSubtitleSimple, // Simple subtitle cycling for left-click
    TracksLoaded,

    // Overlay hide timer
    CheckControlsVisibility,

    // External player status messages
    ExternalPlaybackStarted {
        request: PlaybackRequestId,
    },
    ExternalPlaybackUpdate {
        position: f64,
        duration: f64,
    },
    ExternalPlaybackEnded {
        request: PlaybackRequestId,
    },
    PollExternalMpv,
    ProgressHeartbeat,
}

impl fmt::Debug for PlayerMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Using write! macro directly is more efficient than the derived version
        // which builds up intermediate structures
        match self {
            // Media control
            PlayerMessage::PlayMedia(media) => {
                write!(f, "PlayMedia({:?})", media)
            }
            PlayerMessage::PlayMediaWithId(media, id) => {
                write!(f, "PlayMediaWithId({:?}, {:?})", media, id)
            }
            PlayerMessage::PlayMediaWithIdExternally(media, id) => {
                write!(f, "PlayMediaWithIdExternally({:?}, {:?})", media, id)
            }
            PlayerMessage::NavigateBack => write!(f, "NavigateBack"),
            PlayerMessage::NavigateHome => write!(f, "NavigateHome"),

            // Playback control - grouping simple variants
            PlayerMessage::Play => write!(f, "Play"),
            PlayerMessage::Pause => write!(f, "Pause"),
            PlayerMessage::PlayPause => write!(f, "PlayPause"),
            PlayerMessage::Stop => write!(f, "Stop"),
            PlayerMessage::ResetAfterStop(_) => write!(f, "ResetAfterStop"),
            PlayerMessage::CompletePlaybackExit { .. } => {
                write!(f, "CompletePlaybackExit")
            }
            PlayerMessage::RestoreShellAfterRootShutdown { .. } => {
                write!(f, "RestoreShellAfterRootShutdown")
            }
            PlayerMessage::RootShutdownCompleted { .. } => {
                write!(f, "RootShutdownCompleted")
            }

            // Seeking
            PlayerMessage::Seek(pos) => write!(f, "Seek({})", pos),
            PlayerMessage::SeekTo(duration) => {
                write!(f, "SeekTo({:?})", duration)
            }
            PlayerMessage::SeekRelative(delta) => {
                write!(f, "SeekRelative({})", delta)
            }
            PlayerMessage::SeekRelease => write!(f, "SeekRelease"),
            PlayerMessage::SeekBarPressed => write!(f, "SeekBarPressed"),
            PlayerMessage::SeekDone => write!(f, "SeekDone"),

            // Volume
            PlayerMessage::SetVolume(vol) => write!(f, "SetVolume({})", vol),
            PlayerMessage::ToggleMute => write!(f, "ToggleMute"),

            // Playlist control
            PlayerMessage::ToggleShuffle => write!(f, "ToggleShuffle"),
            PlayerMessage::ToggleRepeat => write!(f, "ToggleRepeat"),

            // Episode navigation
            PlayerMessage::NextEpisode => write!(f, "NextEpisode"),
            PlayerMessage::PreviousEpisode => write!(f, "PreviousEpisode"),

            // Video events
            PlayerMessage::VideoLoaded { request, success } => {
                write!(f, "VideoLoaded({request:?}, {success})")
            }
            PlayerMessage::VideoReadyToPlay => write!(f, "VideoReadyToPlay"),
            PlayerMessage::EndOfStream => write!(f, "EndOfStream"),
            PlayerMessage::PlaybackSnapshotTick => {
                write!(f, "PlaybackSnapshotTick")
            }
            PlayerMessage::PlaybackEventsReady => {
                write!(f, "PlaybackEventsReady")
            }
            PlayerMessage::CaptureNativeVideoHost(_) => {
                write!(f, "CaptureNativeVideoHost(<window>)")
            }
            PlayerMessage::NativeVideoHostCaptured { result, .. } => {
                write!(
                    f,
                    "NativeVideoHostCaptured({})",
                    if result.is_ok() { "ok" } else { "error" }
                )
            }
            PlayerMessage::NativePresenterUpdated => {
                write!(f, "NativePresenterUpdated")
            }
            PlayerMessage::NativePresenterRefresh => {
                write!(f, "NativePresenterRefresh")
            }
            PlayerMessage::Reload => write!(f, "Reload"),

            // External player control
            PlayerMessage::PlayExternal => write!(f, "PlayExternal"),
            PlayerMessage::ResumeExternalPlaybackAfterRootShutdown { request } => {
                write!(f, "ResumeExternalPlaybackAfterRootShutdown({request:?})")
            }
            PlayerMessage::OpenExternalStreamSource { request } => {
                write!(f, "OpenExternalStreamSource({request:?})")
            }
            PlayerMessage::ResumeInternalPlaybackAfterExternalLaunchFailure {
                request,
            } => {
                write!(
                    f,
                    "ResumeInternalPlaybackAfterExternalLaunchFailure({request:?})"
                )
            }
            PlayerMessage::SetStreamSource(_) => {
                write!(f, "SetStreamSource(<redacted>)")
            }
            PlayerMessage::StreamSourceResolved { request, .. } => {
                write!(f, "StreamSourceResolved({request:?}, <redacted>)")
            }
            PlayerMessage::OpenResolvedStreamSource { request } => {
                write!(f, "OpenResolvedStreamSource({request:?})")
            }
            PlayerMessage::ResumeResolvedStreamSourceAfterRootShutdown {
                request,
                ..
            } => {
                write!(
                    f,
                    "ResumeResolvedStreamSourceAfterRootShutdown({request:?})"
                )
            }
            PlayerMessage::RootShutdownFailed { request, .. } => {
                write!(f, "RootShutdownFailed({request:?}, <redacted>)")
            }
            PlayerMessage::StreamUrlResolutionFailed { request, .. } => {
                write!(f, "StreamUrlResolutionFailed({request:?}, <redacted>)")
            }

            // UI control
            PlayerMessage::ShowControls => write!(f, "ShowControls"),
            PlayerMessage::ToggleFullscreen => write!(f, "ToggleFullscreen"),
            PlayerMessage::DisableFullscreen => write!(f, "DisableFullscreen"),
            PlayerMessage::ToggleSettings => write!(f, "ToggleSettings"),
            PlayerMessage::MouseMoved(point) => {
                write!(f, "MouseMoved({:?})", point)
            }
            PlayerMessage::BeginNativeRootDrag => {
                write!(f, "BeginNativeRootDrag")
            }
            PlayerMessage::VideoClicked => write!(f, "VideoClicked"),
            PlayerMessage::VideoDoubleClicked => {
                write!(f, "VideoDoubleClicked")
            }

            // Settings
            PlayerMessage::SetPlaybackSpeed(speed) => {
                write!(f, "SetPlaybackSpeed({})", speed)
            }
            PlayerMessage::SetContentFit(fit) => {
                write!(f, "SetContentFit({:?})", fit)
            }
            PlayerMessage::QualityProfileSelected(profile) => {
                write!(f, "QualityProfileSelected({profile})")
            }

            // Track selection
            PlayerMessage::AudioTrackSelected(track) => {
                write!(f, "AudioTrackSelected({})", track)
            }
            PlayerMessage::SubtitleTrackSelected(track) => match track {
                Some(t) => write!(f, "SubtitleTrackSelected(Some({}))", t),
                None => write!(f, "SubtitleTrackSelected(None)"),
            },
            PlayerMessage::ChapterSelected(chapter) => {
                write!(f, "ChapterSelected({})", chapter.as_str())
            }
            PlayerMessage::EditionSelected(edition) => {
                write!(f, "EditionSelected({})", edition.as_str())
            }
            PlayerMessage::ToggleSubtitles => write!(f, "ToggleSubtitles"),
            PlayerMessage::ToggleSubtitleMenu => {
                write!(f, "ToggleSubtitleMenu")
            }
            PlayerMessage::ToggleQualityMenu => write!(f, "ToggleQualityMenu"),
            PlayerMessage::ToggleAppsinkBackend => {
                write!(f, "ToggleAppsinkBackend")
            }
            PlayerMessage::CycleAudioTrack => write!(f, "CycleAudioTrack"),
            PlayerMessage::CycleSubtitleTrack => {
                write!(f, "CycleSubtitleTrack")
            }
            PlayerMessage::CycleSubtitleSimple => {
                write!(f, "CycleSubtitleSimple")
            }
            PlayerMessage::TracksLoaded => write!(f, "TracksLoaded"),
            PlayerMessage::CheckControlsVisibility => {
                write!(f, "CheckControlsVisibility")
            }
            PlayerMessage::ExternalPlaybackStarted { request } => {
                write!(f, "ExternalPlaybackStarted({request:?})")
            }
            PlayerMessage::ProgressHeartbeat => write!(f, "ProgressHeartbeat"),
            PlayerMessage::ExternalPlaybackUpdate { position, duration } => {
                write!(
                    f,
                    "ExternalPlaybackUpdate {{ position: {}, duration: {} }}",
                    position, duration
                )
            }
            PlayerMessage::ExternalPlaybackEnded { request } => {
                write!(f, "ExternalPlaybackEnded({request:?})")
            }
            PlayerMessage::PollExternalMpv => write!(f, "PollExternalMpv"),
        }
    }
}
