pub mod subscriptions;

use ferrex_core::MediaFile;
use iced::ContentFit;
use std::fmt;
use std::time::Duration;

#[derive(Clone)]
pub enum Message {
    // Media control
    PlayMedia(MediaFile),
    PlayMediaWithId(MediaFile, ferrex_core::MediaID),
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

    // Video events
    VideoLoaded(bool), // Success flag
    VideoReadyToPlay,  // Video is ready to be loaded and played (from streaming domain)
    EndOfStream,
    NewFrame,
    Reload,

    // External player control
    PlayExternal,

    // UI control
    ShowControls,
    ToggleFullscreen,
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
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Using write! macro directly is more efficient than the derived version
        // which builds up intermediate structures
        match self {
            // Media control
            Message::PlayMedia(media) => write!(f, "PlayMedia({:?})", media),
            Message::PlayMediaWithId(media, id) => {
                write!(f, "PlayMediaWithId({:?}, {:?})", media, id)
            }
            Message::NavigateBack => write!(f, "NavigateBack"),
            Message::NavigateHome => write!(f, "NavigateHome"),

            // Playback control - grouping simple variants
            Message::Play => write!(f, "Play"),
            Message::Pause => write!(f, "Pause"),
            Message::PlayPause => write!(f, "PlayPause"),
            Message::Stop => write!(f, "Stop"),
            Message::ResetAfterStop => write!(f, "ResetAfterStop"),

            // Seeking
            Message::Seek(pos) => write!(f, "Seek({})", pos),
            Message::SeekTo(duration) => write!(f, "SeekTo({:?})", duration),
            Message::SeekRelative(delta) => write!(f, "SeekRelative({})", delta),
            Message::SeekRelease => write!(f, "SeekRelease"),
            Message::SeekBarPressed => write!(f, "SeekBarPressed"),
            Message::SeekDone => write!(f, "SeekDone"),

            // Volume
            Message::SetVolume(vol) => write!(f, "SetVolume({})", vol),
            Message::ToggleMute => write!(f, "ToggleMute"),

            // Playlist control
            Message::ToggleShuffle => write!(f, "ToggleShuffle"),
            Message::ToggleRepeat => write!(f, "ToggleRepeat"),

            // Video events
            Message::VideoLoaded(success) => write!(f, "VideoLoaded({})", success),
            Message::VideoReadyToPlay => write!(f, "VideoReadyToPlay"),
            Message::EndOfStream => write!(f, "EndOfStream"),
            Message::NewFrame => write!(f, "NewFrame"),
            Message::Reload => write!(f, "Reload"),

            // External player control
            Message::PlayExternal => write!(f, "PlayExternal"),

            // UI control
            Message::ShowControls => write!(f, "ShowControls"),
            Message::ToggleFullscreen => write!(f, "ToggleFullscreen"),
            Message::ToggleSettings => write!(f, "ToggleSettings"),
            Message::MouseMoved(point) => write!(f, "MouseMoved({:?})", point),
            Message::VideoClicked => write!(f, "VideoClicked"),
            Message::VideoDoubleClicked => write!(f, "VideoDoubleClicked"),

            // Settings
            Message::SetPlaybackSpeed(speed) => write!(f, "SetPlaybackSpeed({})", speed),
            Message::SetContentFit(fit) => write!(f, "SetContentFit({:?})", fit),

            // Track selection
            Message::AudioTrackSelected(track) => write!(f, "AudioTrackSelected({})", track),
            Message::SubtitleTrackSelected(track) => match track {
                Some(t) => write!(f, "SubtitleTrackSelected(Some({}))", t),
                None => write!(f, "SubtitleTrackSelected(None)"),
            },
            Message::ToggleSubtitles => write!(f, "ToggleSubtitles"),
            Message::ToggleSubtitleMenu => write!(f, "ToggleSubtitleMenu"),
            Message::ToggleQualityMenu => write!(f, "ToggleQualityMenu"),
            Message::ToggleAppsinkBackend => write!(f, "ToggleAppsinkBackend"),
            Message::CycleAudioTrack => write!(f, "CycleAudioTrack"),
            Message::CycleSubtitleTrack => write!(f, "CycleSubtitleTrack"),
            Message::CycleSubtitleSimple => write!(f, "CycleSubtitleSimple"),
            Message::TracksLoaded => write!(f, "TracksLoaded"),
            Message::CheckControlsVisibility => write!(f, "CheckControlsVisibility"),
            Message::ExternalPlaybackStarted => write!(f, "ExternalPlaybackStarted"),
            Message::ExternalPlaybackUpdate { position, duration } => {
                write!(
                    f,
                    "ExternalPlaybackUpdate {{ position: {}, duration: {} }}",
                    position, duration
                )
            }
            Message::ExternalPlaybackEnded => write!(f, "ExternalPlaybackEnded"),
            Message::PollExternalMpv => write!(f, "PollExternalMpv"),

            // External MPV player messages
            #[cfg(feature = "external-mpv-player")]
            Message::ExternalPlaybackStarted => write!(f, "ExternalPlaybackStarted"),
            #[cfg(feature = "external-mpv-player")]
            Message::ExternalPlaybackUpdate { position, duration } => {
                write!(
                    f,
                    "ExternalPlaybackUpdate {{ position: {}, duration: {} }}",
                    position, duration
                )
            }
            #[cfg(feature = "external-mpv-player")]
            Message::ExternalPlaybackEnded => write!(f, "ExternalPlaybackEnded"),
            #[cfg(feature = "external-mpv-player")]
            Message::PollExternalMpv => write!(f, "PollExternalMpv"),
        }
    }
}
