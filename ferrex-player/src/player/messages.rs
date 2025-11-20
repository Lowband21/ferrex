use super::state::AspectRatio;
use crate::media_library::MediaFile;
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
    // Subtitle text from video
}

impl PlayerMessage {
    /// Check if this is a player message that should be handled by the player module
    pub fn is_player_message(msg: &crate::Message) -> bool {
        matches!(
            msg,
            crate::Message::PlayMedia(_) |
            crate::Message::BackToLibrary |
            crate::Message::Play |
            crate::Message::Pause |
            crate::Message::PlayPause |
            crate::Message::Stop |
            crate::Message::Seek(_) |
            crate::Message::SeekRelative(_) |
            crate::Message::SeekRelease |
            crate::Message::SeekBarPressed |
            //crate::Message::SeekBarMoved(_) |
            crate::Message::SeekForward |
            crate::Message::SeekBackward |
            crate::Message::SetVolume(_) |
            crate::Message::ToggleMute |
            crate::Message::VideoLoaded(_) |
            crate::Message::EndOfStream |
            crate::Message::NewFrame |
            crate::Message::Reload |
            crate::Message::ShowControls |
            crate::Message::ToggleFullscreen |
            crate::Message::ToggleSettings |
            crate::Message::MouseMoved |
            crate::Message::VideoClicked |
            crate::Message::VideoDoubleClicked |
            crate::Message::SetPlaybackSpeed(_) |
            crate::Message::SetAspectRatio(_) |
            crate::Message::AudioTrackSelected(_) |
            crate::Message::SubtitleTrackSelected(_) |
            crate::Message::ToggleSubtitles |
            crate::Message::ToggleSubtitleMenu |
            crate::Message::CycleAudioTrack |
            crate::Message::CycleSubtitleTrack |
            crate::Message::CycleSubtitleSimple |
            crate::Message::TracksLoaded
        )
    }

    /// Convert from main Message to PlayerMessage
    pub fn from_main_message(msg: crate::Message) -> Option<Self> {
        match msg {
            crate::Message::PlayMedia(media) => Some(PlayerMessage::PlayMedia(media)),
            crate::Message::BackToLibrary => Some(PlayerMessage::BackToLibrary),
            crate::Message::Play => Some(PlayerMessage::Play),
            crate::Message::Pause => Some(PlayerMessage::Pause),
            crate::Message::PlayPause => Some(PlayerMessage::PlayPause),
            crate::Message::Stop => Some(PlayerMessage::Stop),
            crate::Message::Seek(pos) => Some(PlayerMessage::Seek(pos)),
            crate::Message::SeekRelative(delta) => Some(PlayerMessage::SeekRelative(delta)),
            crate::Message::SeekRelease => Some(PlayerMessage::SeekRelease),
            crate::Message::SeekBarPressed => Some(PlayerMessage::SeekBarPressed),
            //crate::Message::SeekBarMoved(point) => Some(PlayerMessage::SeekBarMoved(point)),
            crate::Message::SeekForward => Some(PlayerMessage::SeekForward),
            crate::Message::SeekBackward => Some(PlayerMessage::SeekBackward),
            crate::Message::SetVolume(vol) => Some(PlayerMessage::SetVolume(vol)),
            crate::Message::ToggleMute => Some(PlayerMessage::ToggleMute),
            crate::Message::VideoLoaded(success) => Some(PlayerMessage::VideoLoaded(success)),
            crate::Message::EndOfStream => Some(PlayerMessage::EndOfStream),
            crate::Message::NewFrame => Some(PlayerMessage::NewFrame),
            crate::Message::Reload => Some(PlayerMessage::Reload),
            crate::Message::ShowControls => Some(PlayerMessage::ShowControls),
            crate::Message::ToggleFullscreen => Some(PlayerMessage::ToggleFullscreen),
            crate::Message::ToggleSettings => Some(PlayerMessage::ToggleSettings),
            crate::Message::MouseMoved => Some(PlayerMessage::MouseMoved),
            crate::Message::VideoClicked => Some(PlayerMessage::VideoClicked),
            crate::Message::VideoDoubleClicked => Some(PlayerMessage::VideoDoubleClicked),
            crate::Message::SetPlaybackSpeed(speed) => Some(PlayerMessage::SetPlaybackSpeed(speed)),
            crate::Message::SetAspectRatio(ratio) => Some(PlayerMessage::SetAspectRatio(ratio)),
            crate::Message::AudioTrackSelected(index) => {
                Some(PlayerMessage::AudioTrackSelected(index))
            }
            crate::Message::SubtitleTrackSelected(index) => {
                Some(PlayerMessage::SubtitleTrackSelected(index))
            }
            crate::Message::ToggleSubtitles => Some(PlayerMessage::ToggleSubtitles),
            crate::Message::ToggleSubtitleMenu => Some(PlayerMessage::ToggleSubtitleMenu),
            crate::Message::CycleAudioTrack => Some(PlayerMessage::CycleAudioTrack),
            crate::Message::CycleSubtitleTrack => Some(PlayerMessage::CycleSubtitleTrack),
            crate::Message::CycleSubtitleSimple => Some(PlayerMessage::CycleSubtitleSimple),
            crate::Message::TracksLoaded => Some(PlayerMessage::TracksLoaded),
            _ => None,
        }
    }
}
