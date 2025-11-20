pub mod subscriptions;

use ferrex_core::player_prelude::{MediaFile, MediaID};
use iced::ContentFit;
use std::fmt;
use std::time::Duration;

#[derive(Clone)]
pub enum PlayerMessage {
    // Media control
    PlayMedia(MediaFile),
    PlayMediaWithId(MediaFile, MediaID),
    NavigateBack, // Navigate to previous view
    NavigateHome, // Navigate to home/library view

    // Playback control
    Play,
    Pause,
    PlayPause,
    Stop,
    ResetAfterStop, // Internal message to reset state after progress update

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
    VideoLoaded(bool), // Success flag
    VideoReadyToPlay, // Video is ready to be loaded and played (from streaming domain)
    EndOfStream,
    NewFrame,
    Reload,

    // External player control
    PlayExternal,
    // Internal: set resolved stream URL and trigger playback
    SetStreamUrl(String),

    // UI control
    ShowControls,
    ToggleFullscreen,
    DisableFullscreen,
    ToggleSettings,
    MouseMoved(iced::Point),
    VideoClicked,
    VideoDoubleClicked,

    // Settings
    SetPlaybackSpeed(f64),
    SetContentFit(ContentFit),

    // Track selection
    AudioTrackSelected(i32),
    SubtitleTrackSelected(Option<i32>),
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
    ExternalPlaybackStarted,
    ExternalPlaybackUpdate { position: f64, duration: f64 },
    ExternalPlaybackEnded,
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
            PlayerMessage::NavigateBack => write!(f, "NavigateBack"),
            PlayerMessage::NavigateHome => write!(f, "NavigateHome"),

            // Playback control - grouping simple variants
            PlayerMessage::Play => write!(f, "Play"),
            PlayerMessage::Pause => write!(f, "Pause"),
            PlayerMessage::PlayPause => write!(f, "PlayPause"),
            PlayerMessage::Stop => write!(f, "Stop"),
            PlayerMessage::ResetAfterStop => write!(f, "ResetAfterStop"),

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
            PlayerMessage::VideoLoaded(success) => {
                write!(f, "VideoLoaded({})", success)
            }
            PlayerMessage::VideoReadyToPlay => write!(f, "VideoReadyToPlay"),
            PlayerMessage::EndOfStream => write!(f, "EndOfStream"),
            PlayerMessage::NewFrame => write!(f, "NewFrame"),
            PlayerMessage::Reload => write!(f, "Reload"),

            // External player control
            PlayerMessage::PlayExternal => write!(f, "PlayExternal"),
            PlayerMessage::SetStreamUrl(_) => {
                write!(f, "SetStreamUrl(<redacted>)")
            }

            // UI control
            PlayerMessage::ShowControls => write!(f, "ShowControls"),
            PlayerMessage::ToggleFullscreen => write!(f, "ToggleFullscreen"),
            PlayerMessage::DisableFullscreen => write!(f, "DisableFullscreen"),
            PlayerMessage::ToggleSettings => write!(f, "ToggleSettings"),
            PlayerMessage::MouseMoved(point) => {
                write!(f, "MouseMoved({:?})", point)
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

            // Track selection
            PlayerMessage::AudioTrackSelected(track) => {
                write!(f, "AudioTrackSelected({})", track)
            }
            PlayerMessage::SubtitleTrackSelected(track) => match track {
                Some(t) => write!(f, "SubtitleTrackSelected(Some({}))", t),
                None => write!(f, "SubtitleTrackSelected(None)"),
            },
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
            PlayerMessage::ExternalPlaybackStarted => {
                write!(f, "ExternalPlaybackStarted")
            }
            PlayerMessage::ProgressHeartbeat => write!(f, "ProgressHeartbeat"),
            PlayerMessage::ExternalPlaybackUpdate { position, duration } => {
                write!(
                    f,
                    "ExternalPlaybackUpdate {{ position: {}, duration: {} }}",
                    position, duration
                )
            }
            PlayerMessage::ExternalPlaybackEnded => {
                write!(f, "ExternalPlaybackEnded")
            }
            PlayerMessage::PollExternalMpv => write!(f, "PollExternalMpv"),
        }
    }
}
