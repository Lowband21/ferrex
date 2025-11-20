use super::state::AspectRatio;
use crate::media_library::MediaFile;
use crate::messages::media::Message;
use iced::Point;

#[derive(Debug, Clone)]
pub enum PlayerMessage {
    // Media control
    PlayMedia(MediaFile),
    BackToLibrary,

    // Playback control
    Play,
    Pause,
    PlayPause,
    Stop,

    // Seeking
    Seek(f64),
    SeekRelative(f64),
    SeekRelease,
    SeekBarPressed,
    SeekBarMoved(Point),
    SeekForward,  // +15s
    SeekBackward, // -15s
    SeekDone,     // Seek operation completed

    // Volume
    SetVolume(f64),
    ToggleMute,

    // Video events
    VideoLoaded(bool), // Success flag
    EndOfStream,
    NewFrame,
    Reload,

    // UI control
    ShowControls,
    ToggleFullscreen,
    ToggleSettings,
    MouseMoved,
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
}

impl PlayerMessage {
    /// Check if this is a player message that should be handled by the player module
    pub fn is_player_message(msg: &Message) -> bool {
        matches!(
            msg,
            Message::PlayMedia(_) |
            Message::BackToLibrary |
            Message::Play |
            Message::Pause |
            Message::PlayPause |
            Message::Stop |
            Message::Seek(_) |
            Message::SeekRelative(_) |
            Message::SeekRelease |
            Message::SeekBarPressed |
            // Message::SeekBarMoved(_) | // Handled in update_media.rs where we have window dimensions
            Message::SeekDone |
            Message::SeekForward |
            Message::SeekBackward |
            Message::SetVolume(_) |
            Message::ToggleMute |
            Message::VideoLoaded(_) |
            Message::EndOfStream |
            Message::NewFrame |
            Message::Reload |
            Message::ShowControls |
            Message::ToggleFullscreen |
            Message::ToggleSettings |
            Message::MouseMoved |
            Message::VideoClicked |
            Message::VideoDoubleClicked |
            Message::SetPlaybackSpeed(_) |
            Message::SetAspectRatio(_) |
            Message::AudioTrackSelected(_) |
            Message::SubtitleTrackSelected(_) |
            Message::ToggleSubtitles |
            Message::ToggleSubtitleMenu |
            Message::CycleAudioTrack |
            Message::CycleSubtitleTrack |
            Message::CycleSubtitleSimple |
            Message::TracksLoaded |
            Message::ToggleToneMapping(_) |
            Message::SetToneMappingPreset(_) |
            Message::SetToneMappingAlgorithm(_) |
            Message::SetToneMappingWhitePoint(_) |
            Message::SetToneMappingExposure(_) |
            Message::SetToneMappingSaturation(_) |
            Message::SetHableShoulderStrength(_) |
            Message::SetHableLinearStrength(_) |
            Message::SetHableLinearAngle(_) |
            Message::SetHableToeStrength(_) |
            Message::SetMonitorBrightness(_) |
            Message::SetToneMappingBrightness(_) |
            Message::SetToneMappingContrast(_) |
            Message::SetToneMappingSaturationBoost(_)
        )
    }

    /// Convert from main Message to PlayerMessage
    pub fn from_main_message(msg: Message) -> Option<Self> {
        match msg {
            Message::PlayMedia(media) => Some(PlayerMessage::PlayMedia(media)),
            Message::BackToLibrary => Some(PlayerMessage::BackToLibrary),
            Message::Play => Some(PlayerMessage::Play),
            Message::Pause => Some(PlayerMessage::Pause),
            Message::PlayPause => Some(PlayerMessage::PlayPause),
            Message::Stop => Some(PlayerMessage::Stop),
            Message::Seek(pos) => Some(PlayerMessage::Seek(pos)),
            Message::SeekRelative(delta) => Some(PlayerMessage::SeekRelative(delta)),
            Message::SeekRelease => Some(PlayerMessage::SeekRelease),
            Message::SeekBarPressed => Some(PlayerMessage::SeekBarPressed),
            // Message::SeekBarMoved(point) => Some(PlayerMessage::SeekBarMoved(point)), // Handled in update_media.rs
            Message::SeekDone => Some(PlayerMessage::SeekDone),
            Message::SeekForward => Some(PlayerMessage::SeekForward),
            Message::SeekBackward => Some(PlayerMessage::SeekBackward),
            Message::SetVolume(vol) => Some(PlayerMessage::SetVolume(vol)),
            Message::ToggleMute => Some(PlayerMessage::ToggleMute),
            Message::VideoLoaded(success) => Some(PlayerMessage::VideoLoaded(success)),
            Message::EndOfStream => Some(PlayerMessage::EndOfStream),
            Message::NewFrame => Some(PlayerMessage::NewFrame),
            Message::Reload => Some(PlayerMessage::Reload),
            Message::ShowControls => Some(PlayerMessage::ShowControls),
            Message::ToggleFullscreen => Some(PlayerMessage::ToggleFullscreen),
            Message::ToggleSettings => Some(PlayerMessage::ToggleSettings),
            Message::MouseMoved => Some(PlayerMessage::MouseMoved),
            Message::VideoClicked => Some(PlayerMessage::VideoClicked),
            Message::VideoDoubleClicked => Some(PlayerMessage::VideoDoubleClicked),
            Message::SetPlaybackSpeed(speed) => Some(PlayerMessage::SetPlaybackSpeed(speed)),
            Message::SetAspectRatio(ratio) => Some(PlayerMessage::SetAspectRatio(ratio)),
            Message::AudioTrackSelected(index) => Some(PlayerMessage::AudioTrackSelected(index)),
            Message::SubtitleTrackSelected(index) => {
                Some(PlayerMessage::SubtitleTrackSelected(index))
            }
            Message::ToggleSubtitles => Some(PlayerMessage::ToggleSubtitles),
            Message::ToggleSubtitleMenu => Some(PlayerMessage::ToggleSubtitleMenu),
            Message::CycleAudioTrack => Some(PlayerMessage::CycleAudioTrack),
            Message::CycleSubtitleTrack => Some(PlayerMessage::CycleSubtitleTrack),
            Message::CycleSubtitleSimple => Some(PlayerMessage::CycleSubtitleSimple),
            Message::TracksLoaded => Some(PlayerMessage::TracksLoaded),

            // Tone mapping messages
            Message::ToggleToneMapping(enabled) => Some(PlayerMessage::ToggleToneMapping(enabled)),
            Message::SetToneMappingPreset(preset) => {
                Some(PlayerMessage::SetToneMappingPreset(preset))
            }
            Message::SetToneMappingAlgorithm(algo) => {
                Some(PlayerMessage::SetToneMappingAlgorithm(algo))
            }
            Message::SetToneMappingWhitePoint(value) => {
                Some(PlayerMessage::SetToneMappingWhitePoint(value))
            }
            Message::SetToneMappingExposure(value) => {
                Some(PlayerMessage::SetToneMappingExposure(value))
            }
            Message::SetToneMappingSaturation(value) => {
                Some(PlayerMessage::SetToneMappingSaturation(value))
            }
            Message::SetHableShoulderStrength(value) => {
                Some(PlayerMessage::SetHableShoulderStrength(value))
            }
            Message::SetHableLinearStrength(value) => {
                Some(PlayerMessage::SetHableLinearStrength(value))
            }
            Message::SetHableLinearAngle(value) => Some(PlayerMessage::SetHableLinearAngle(value)),
            Message::SetHableToeStrength(value) => Some(PlayerMessage::SetHableToeStrength(value)),
            Message::SetMonitorBrightness(value) => {
                Some(PlayerMessage::SetMonitorBrightness(value))
            }
            Message::SetToneMappingBrightness(value) => {
                Some(PlayerMessage::SetToneMappingBrightness(value))
            }
            Message::SetToneMappingContrast(value) => {
                Some(PlayerMessage::SetToneMappingContrast(value))
            }
            Message::SetToneMappingSaturationBoost(value) => {
                Some(PlayerMessage::SetToneMappingSaturationBoost(value))
            }

            _ => None,
        }
    }
}
