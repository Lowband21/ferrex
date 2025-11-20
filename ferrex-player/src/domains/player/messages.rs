use super::state::AspectRatio;
use crate::domains::media::library::MediaFile;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum Message {
    // Media control
    PlayMedia(MediaFile),
    NavigateBack,   // Navigate to previous view
    NavigateHome,   // Navigate to home/library view

    // Playback control
    Play,
    Pause,
    PlayPause,
    Stop,
    ResetAfterStop,  // Internal message to reset state after progress update

    // Seeking
    Seek(f64),
    SeekTo(Duration),  // Direct command for seeking to specific duration
    SeekRelative(f64),
    SeekRelease,
    SeekBarPressed,
    SeekForward,  // +15s
    SeekBackward, // -15s
    SeekDone,     // Seek operation completed

    // Volume
    SetVolume(f64),
    ToggleMute,
    
    // Playlist control (NEW - for Phase 2 direct commands)
    ToggleShuffle,
    ToggleRepeat,
    LoadTrack(ferrex_core::api_types::MediaId),

    // Video events
    VideoLoaded(bool), // Success flag
    VideoReadyToPlay,  // Video is ready to be loaded and played (from streaming domain)
    EndOfStream,
    NewFrame,
    Reload,

    // UI control
    ShowControls,
    ToggleFullscreen,
    ToggleSettings,
    MouseMoved(iced::Point),
    VideoClicked,
    VideoDoubleClicked,

    // Settings
    SetPlaybackSpeed(f64),
    SetAspectRatio(AspectRatio),

    // Track selection (NEW)
    AudioTrackSelected(i32),
    SubtitleTrackSelected(Option<i32>),
    ToggleSubtitles,
    ToggleSubtitleMenu,
    ToggleQualityMenu,
    CycleAudioTrack,
    CycleSubtitleTrack,
    CycleSubtitleSimple, // Simple subtitle cycling for left-click
    TracksLoaded,

    // Tone mapping controls
    ToggleToneMapping(bool),
    SetToneMappingPreset(iced_video_player::ToneMappingPreset),
    SetToneMappingAlgorithm(iced_video_player::ToneMappingAlgorithm),
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
    
    // External MPV player messages
    #[cfg(feature = "external-mpv-player")]
    ExternalPlaybackStarted,
    #[cfg(feature = "external-mpv-player")]
    ExternalPlaybackUpdate { position: f64, duration: f64 },
    #[cfg(feature = "external-mpv-player")]
    ExternalPlaybackEnded,
    #[cfg(feature = "external-mpv-player")]
    PollExternalMpv,
}
