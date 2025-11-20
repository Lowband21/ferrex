use crate::{
    media_library::MediaFile,
    player::state::{AspectRatio, TranscodingStatus},
};
use iced::Point;
use iced_video_player::{ToneMappingAlgorithm, ToneMappingPreset};
pub mod subscriptions;

#[derive(Clone, Debug)]
pub enum Message {
    // Playback control
    Play,
    Pause,
    PlayPause,
    Stop,
    PlayMedia(MediaFile),
    PlayMediaWithId(MediaFile, ferrex_core::api_types::MediaId), // includes MediaId for watch tracking
    BackToLibrary,

    // Seeking
    Seek(f64),
    SeekRelative(f64),
    SeekRelease,
    SeekBarPressed,
    SeekBarMoved(Point),
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

    // Video state
    VideoLoaded(bool), // Success flag
    VideoCreated(Result<std::sync::Arc<iced_video_player::Video>, String>),
    MediaAvailabilityChecked(MediaFile),
    MediaUnavailable(String, String), // reason, message
    TranscodingStarted,

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
    SetAspectRatio(AspectRatio),

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
    #[doc(hidden)]
    _EmitCrossDomainEvent(crate::messages::CrossDomainEvent),
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
            Self::BackToLibrary => "Media::BackToLibrary",

            // Seeking
            Self::Seek(_) => "Media::Seek",
            Self::SeekRelative(_) => "Media::SeekRelative",
            Self::SeekRelease => "Media::SeekRelease",
            Self::SeekBarPressed => "Media::SeekBarPressed",
            Self::SeekBarMoved(_) => "Media::SeekBarMoved",
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

            // Video state
            Self::VideoLoaded(_) => "Media::VideoLoaded",
            Self::VideoCreated(_) => "Media::VideoCreated",
            Self::MediaAvailabilityChecked(_) => "Media::MediaAvailabilityChecked",
            Self::MediaUnavailable(_, _) => "Media::MediaUnavailable",
            Self::TranscodingStarted => "Media::TranscodingStarted",

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
            Self::SetAspectRatio(_) => "Media::SetAspectRatio",

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
            Self::_EmitCrossDomainEvent(_) => "Media::_EmitCrossDomainEvent",
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
