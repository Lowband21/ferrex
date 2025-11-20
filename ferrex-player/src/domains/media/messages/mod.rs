use crate::domains::player::video_backend::{ToneMappingAlgorithm, ToneMappingPreset};

use crate::infrastructure::api_types::MediaId;

use super::library::MediaFile;
pub mod subscriptions;

#[derive(Clone)]
pub enum Message {
    // Playback control
    Play,
    Pause,
    PlayPause,
    Stop,
    PlayMedia(MediaFile),
    PlayMediaWithId(MediaFile, ferrex_core::api_types::MediaId), // includes MediaId for watch tracking
    LoadMediaById(ferrex_core::api_types::MediaId), // Load track by ID (NEW - for Phase 2 direct commands)

    // Seeking
    Seek(f64),
    SeekRelative(f64),
    SeekRelease,
    SeekBarPressed,
    SeekDone,
    SeekForward,  // +15s
    SeekBackward, // -15s

    // Volume control
    SetVolume(f64),
    ToggleMute,

    // Playback events
    EndOfStream,
    NewFrame,
    Reload,
    ShowControls,
    CheckControlsVisibility, // Check if controls should be hidden based on inactivity

    // Watch progress tracking
    ProgressUpdateSent(MediaId, f64, f64), // Position that was successfully sent to server
    ProgressUpdateFailed,                  // Failed to send progress update
    SendProgressUpdateWithData(MediaId, f64, f64), // position, duration - captures data at message creation time
    WatchProgressFetched(MediaId, Option<f32>),    // Media ID and resume position

    // Video state
    VideoLoaded(bool), // Success flag
    VideoCreated(Result<std::sync::Arc<crate::domains::player::video_backend::Video>, String>),
    MediaAvailabilityChecked(MediaFile),
    MediaUnavailable(String, String), // reason, message

    // Track selection
    AudioTrackSelected(i32),
    SubtitleTrackSelected(Option<i32>),
    ToggleSubtitles,
    ToggleSubtitleMenu,
    CycleAudioTrack,
    CycleSubtitleTrack,
    CycleSubtitleSimple,
    TracksLoaded,

    // Quality control
    ToggleQualityMenu,
    QualityVariantSelected(String), // profile name

    // Playback settings
    SetPlaybackSpeed(f64),
    ToggleSettings,

    // Fullscreen control
    ToggleFullscreen,
    ExitFullscreen,

    // Mouse/UI events
    MouseMoved,
    VideoClicked,
    VideoDoubleClicked,

    // Tone mapping controls
    ToggleToneMapping(bool),
    SetToneMappingPreset(ToneMappingPreset),
    SetToneMappingAlgorithm(ToneMappingAlgorithm),
    SetToneMappingWhitePoint(f32),
    SetToneMappingExposure(f32),
    SetToneMappingSaturation(f32),
    SetHableShoulderStrength(f32),
    SetHableLinearStrength(f32),
    SetHableLinearAngle(f32),
    SetHableToeStrength(f32),
    SetMonitorBrightness(f32),
    SetToneMappingBrightness(f32),
    SetToneMappingContrast(f32),
    SetToneMappingSaturationBoost(f32),

    // Internal messages for cross-domain coordination
    #[doc(hidden)]
    _LoadVideo, // Internal message to trigger video loading from cross-domain events

    // No-op message for task chaining
    Noop,

    // Auto-play next episode
    PlayNextEpisode,
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // For VideoCreated, don't debug the entire Video object
            Self::VideoCreated(result) => match result {
                Ok(_) => write!(f, "Message::VideoCreated(Ok(<Video>))"),
                Err(e) => write!(f, "Message::VideoCreated(Err({:?}))", e),
            },
            // For variants with MediaFile, provide concise debug output
            Self::PlayMedia(media) => write!(f, "Message::PlayMedia({})", media.filename),
            Self::PlayMediaWithId(media, id) => {
                write!(f, "Message::PlayMediaWithId({}, {:?})", media.filename, id)
            }
            Self::MediaAvailabilityChecked(media) => {
                write!(f, "Message::MediaAvailabilityChecked({})", media.filename)
            }

            // Simple variants can use their name
            Self::Play => write!(f, "Message::Play"),
            Self::Pause => write!(f, "Message::Pause"),
            Self::PlayPause => write!(f, "Message::PlayPause"),
            Self::Stop => write!(f, "Message::Stop"),
            Self::LoadMediaById(id) => write!(f, "Message::LoadMediaById({:?})", id),

            // Seeking
            Self::Seek(pos) => write!(f, "Message::Seek({})", pos),
            Self::SeekRelative(delta) => write!(f, "Message::SeekRelative({})", delta),
            Self::SeekRelease => write!(f, "Message::SeekRelease"),
            Self::SeekBarPressed => write!(f, "Message::SeekBarPressed"),
            Self::SeekDone => write!(f, "Message::SeekDone"),
            Self::SeekForward => write!(f, "Message::SeekForward"),
            Self::SeekBackward => write!(f, "Message::SeekBackward"),

            // Volume
            Self::SetVolume(vol) => write!(f, "Message::SetVolume({})", vol),
            Self::ToggleMute => write!(f, "Message::ToggleMute"),

            // Events
            Self::EndOfStream => write!(f, "Message::EndOfStream"),
            Self::NewFrame => write!(f, "Message::NewFrame"),
            Self::Reload => write!(f, "Message::Reload"),
            Self::ShowControls => write!(f, "Message::ShowControls"),
            Self::CheckControlsVisibility => write!(f, "Message::CheckControlsVisibility"),

            // Progress tracking
            Self::ProgressUpdateSent(id, pos, dur) => {
                write!(f, "Message::ProgressUpdateSent({:?}, {}, {})", id, pos, dur)
            }
            Self::ProgressUpdateFailed => write!(f, "Message::ProgressUpdateFailed"),
            Self::SendProgressUpdateWithData(id, pos, dur) => {
                write!(
                    f,
                    "Message::SendProgressUpdateWithData({:?}, {}, {})",
                    id, pos, dur
                )
            }
            Self::WatchProgressFetched(id, pos) => {
                write!(f, "Message::WatchProgressFetched({:?}, {:?})", id, pos)
            }

            // Video state
            Self::VideoLoaded(success) => write!(f, "Message::VideoLoaded({})", success),
            Self::MediaUnavailable(reason, msg) => {
                write!(f, "Message::MediaUnavailable({}, {})", reason, msg)
            }

            // Track selection
            Self::AudioTrackSelected(idx) => write!(f, "Message::AudioTrackSelected({})", idx),
            Self::SubtitleTrackSelected(idx) => {
                write!(f, "Message::SubtitleTrackSelected({:?})", idx)
            }
            Self::ToggleSubtitles => write!(f, "Message::ToggleSubtitles"),
            Self::ToggleSubtitleMenu => write!(f, "Message::ToggleSubtitleMenu"),
            Self::CycleAudioTrack => write!(f, "Message::CycleAudioTrack"),
            Self::CycleSubtitleTrack => write!(f, "Message::CycleSubtitleTrack"),
            Self::CycleSubtitleSimple => write!(f, "Message::CycleSubtitleSimple"),
            Self::TracksLoaded => write!(f, "Message::TracksLoaded"),

            // Quality
            Self::ToggleQualityMenu => write!(f, "Message::ToggleQualityMenu"),
            Self::QualityVariantSelected(profile) => {
                write!(f, "Message::QualityVariantSelected({})", profile)
            }

            // Settings
            Self::SetPlaybackSpeed(speed) => write!(f, "Message::SetPlaybackSpeed({})", speed),
            Self::ToggleSettings => write!(f, "Message::ToggleSettings"),

            // Fullscreen
            Self::ToggleFullscreen => write!(f, "Message::ToggleFullscreen"),
            Self::ExitFullscreen => write!(f, "Message::ExitFullscreen"),

            // Mouse/UI
            Self::MouseMoved => write!(f, "Message::MouseMoved"),
            Self::VideoClicked => write!(f, "Message::VideoClicked"),
            Self::VideoDoubleClicked => write!(f, "Message::VideoDoubleClicked"),

            // Tone mapping - simplified output
            Self::ToggleToneMapping(enabled) => {
                write!(f, "Message::ToggleToneMapping({})", enabled)
            }
            Self::SetToneMappingPreset(_) => write!(f, "Message::SetToneMappingPreset"),
            Self::SetToneMappingAlgorithm(_) => write!(f, "Message::SetToneMappingAlgorithm"),
            Self::SetToneMappingWhitePoint(val) => {
                write!(f, "Message::SetToneMappingWhitePoint({})", val)
            }
            Self::SetToneMappingExposure(val) => {
                write!(f, "Message::SetToneMappingExposure({})", val)
            }
            Self::SetToneMappingSaturation(val) => {
                write!(f, "Message::SetToneMappingSaturation({})", val)
            }
            Self::SetHableShoulderStrength(val) => {
                write!(f, "Message::SetHableShoulderStrength({})", val)
            }
            Self::SetHableLinearStrength(val) => {
                write!(f, "Message::SetHableLinearStrength({})", val)
            }
            Self::SetHableLinearAngle(val) => write!(f, "Message::SetHableLinearAngle({})", val),
            Self::SetHableToeStrength(val) => write!(f, "Message::SetHableToeStrength({})", val),
            Self::SetMonitorBrightness(val) => write!(f, "Message::SetMonitorBrightness({})", val),
            Self::SetToneMappingBrightness(val) => {
                write!(f, "Message::SetToneMappingBrightness({})", val)
            }
            Self::SetToneMappingContrast(val) => {
                write!(f, "Message::SetToneMappingContrast({})", val)
            }
            Self::SetToneMappingSaturationBoost(val) => {
                write!(f, "Message::SetToneMappingSaturationBoost({})", val)
            }

            // Internal
            Self::_LoadVideo => write!(f, "Message::_LoadVideo"),
            Self::Noop => write!(f, "Message::Noop"),
            Self::PlayNextEpisode => write!(f, "Message::PlayNextEpisode"),
        }
    }
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            // Playback control
            Self::Play => "Media::Play",
            Self::Pause => "Media::Pause",
            Self::PlayPause => "Media::PlayPause",
            Self::Stop => "Media::Stop",
            Self::PlayMedia(_) => "Media::PlayMedia",
            Self::PlayMediaWithId(_, _) => "Media::PlayMediaWithId",
            Self::LoadMediaById(_) => "Media::LoadMediaById",

            // Seeking
            Self::Seek(_) => "Media::Seek",
            Self::SeekRelative(_) => "Media::SeekRelative",
            Self::SeekRelease => "Media::SeekRelease",
            Self::SeekBarPressed => "Media::SeekBarPressed",
            Self::SeekDone => "Media::SeekDone",
            Self::SeekForward => "Media::SeekForward",
            Self::SeekBackward => "Media::SeekBackward",

            // Volume control
            Self::SetVolume(_) => "Media::SetVolume",
            Self::ToggleMute => "Media::ToggleMute",

            // Playback events
            Self::EndOfStream => "Media::EndOfStream",
            Self::NewFrame => "Media::NewFrame",
            Self::Reload => "Media::Reload",
            Self::ShowControls => "Media::ShowControls",
            Self::CheckControlsVisibility => "Media::CheckControlsVisibility",

            // Watch progress tracking
            Self::ProgressUpdateSent(_, _, _) => "Media::ProgressUpdateSent",
            Self::ProgressUpdateFailed => "Media::ProgressUpdateFailed",

            Self::SendProgressUpdateWithData(_, _, _) => "Media::SendProgressUpdateWithData",
            Self::WatchProgressFetched(_, _) => "Media::WatchProgressFetched",

            // Video state
            Self::VideoLoaded(_) => "Media::VideoLoaded",
            Self::VideoCreated(_) => "Media::VideoCreated",
            Self::MediaAvailabilityChecked(_) => "Media::MediaAvailabilityChecked",
            Self::MediaUnavailable(_, _) => "Media::MediaUnavailable",

            // Track selection
            Self::AudioTrackSelected(_) => "Media::AudioTrackSelected",
            Self::SubtitleTrackSelected(_) => "Media::SubtitleTrackSelected",
            Self::ToggleSubtitles => "Media::ToggleSubtitles",
            Self::ToggleSubtitleMenu => "Media::ToggleSubtitleMenu",
            Self::CycleAudioTrack => "Media::CycleAudioTrack",
            Self::CycleSubtitleTrack => "Media::CycleSubtitleTrack",
            Self::CycleSubtitleSimple => "Media::CycleSubtitleSimple",
            Self::TracksLoaded => "Media::TracksLoaded",

            // Quality control
            Self::ToggleQualityMenu => "Media::ToggleQualityMenu",
            Self::QualityVariantSelected(_) => "Media::QualityVariantSelected",

            // Playback settings
            Self::SetPlaybackSpeed(_) => "Media::SetPlaybackSpeed",
            Self::ToggleSettings => "Media::ToggleSettings",

            // Fullscreen control
            Self::ToggleFullscreen => "Media::ToggleFullscreen",
            Self::ExitFullscreen => "Media::ExitFullscreen",

            // Mouse/UI events
            Self::MouseMoved => "Media::MouseMoved",
            Self::VideoClicked => "Media::VideoClicked",
            Self::VideoDoubleClicked => "Media::VideoDoubleClicked",

            // Tone mapping controls
            Self::ToggleToneMapping(_) => "Media::ToggleToneMapping",
            Self::SetToneMappingPreset(_) => "Media::SetToneMappingPreset",
            Self::SetToneMappingAlgorithm(_) => "Media::SetToneMappingAlgorithm",
            Self::SetToneMappingWhitePoint(_) => "Media::SetToneMappingWhitePoint",
            Self::SetToneMappingExposure(_) => "Media::SetToneMappingExposure",
            Self::SetToneMappingSaturation(_) => "Media::SetToneMappingSaturation",
            Self::SetHableShoulderStrength(_) => "Media::SetHableShoulderStrength",
            Self::SetHableLinearStrength(_) => "Media::SetHableLinearStrength",
            Self::SetHableLinearAngle(_) => "Media::SetHableLinearAngle",
            Self::SetHableToeStrength(_) => "Media::SetHableToeStrength",
            Self::SetMonitorBrightness(_) => "Media::SetMonitorBrightness",
            Self::SetToneMappingBrightness(_) => "Media::SetToneMappingBrightness",
            Self::SetToneMappingContrast(_) => "Media::SetToneMappingContrast",
            Self::SetToneMappingSaturationBoost(_) => "Media::SetToneMappingSaturationBoost",

            // Internal
            Self::_LoadVideo => "Media::_LoadVideo",
            Self::Noop => "Media::Noop",
            Self::PlayNextEpisode => "Media::PlayNextEpisode",
        }
    }
}

/// Media domain events
#[derive(Clone, Debug)]
pub enum MediaEvent {
    PlaybackStarted(MediaFile, ferrex_core::api_types::MediaId),
    PlaybackPaused,
    PlaybackStopped,
    PlaybackPositionChanged(f64),
    TrackChanged,
}
