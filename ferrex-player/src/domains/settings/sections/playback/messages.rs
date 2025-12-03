//! Playback section messages
//!
//! All messages related to playback settings.

use super::state::{PlaybackQuality, ResumeBehavior};

/// Messages for the playback settings section
#[derive(Debug, Clone)]
pub enum PlaybackMessage {
    // General subsection
    /// Toggle auto-play next episode
    SetAutoPlayNext(bool),
    /// Set resume behavior
    SetResumeBehavior(ResumeBehavior),
    /// Set preferred quality
    SetPreferredQuality(PlaybackQuality),

    // Seeking subsection (String for validation in domain handler)
    /// Set coarse forward seek amount (seconds)
    SetSeekForwardCoarse(String),
    /// Set coarse backward seek amount (seconds)
    SetSeekBackwardCoarse(String),
    /// Set fine forward seek amount (seconds)
    SetSeekForwardFine(String),
    /// Set fine backward seek amount (seconds)
    SetSeekBackwardFine(String),

    // Skip subsection
    /// Set intro skip duration (seconds)
    SetSkipIntroDuration(u32),
    /// Set credits skip duration (seconds)
    SetSkipCreditsDuration(u32),

    // Subtitles subsection
    /// Toggle subtitles enabled by default
    SetSubtitlesEnabled(bool),
    /// Set preferred subtitle language
    SetSubtitleLanguage(Option<String>),
    /// Set subtitle font scale
    SetSubtitleFontScale(f32),
}

impl PlaybackMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::SetAutoPlayNext(_) => "Playback::SetAutoPlayNext",
            Self::SetResumeBehavior(_) => "Playback::SetResumeBehavior",
            Self::SetPreferredQuality(_) => "Playback::SetPreferredQuality",
            Self::SetSeekForwardCoarse(_) => "Playback::SetSeekForwardCoarse",
            Self::SetSeekBackwardCoarse(_) => "Playback::SetSeekBackwardCoarse",
            Self::SetSeekForwardFine(_) => "Playback::SetSeekForwardFine",
            Self::SetSeekBackwardFine(_) => "Playback::SetSeekBackwardFine",
            Self::SetSkipIntroDuration(_) => "Playback::SetSkipIntroDuration",
            Self::SetSkipCreditsDuration(_) => {
                "Playback::SetSkipCreditsDuration"
            }
            Self::SetSubtitlesEnabled(_) => "Playback::SetSubtitlesEnabled",
            Self::SetSubtitleLanguage(_) => "Playback::SetSubtitleLanguage",
            Self::SetSubtitleFontScale(_) => "Playback::SetSubtitleFontScale",
        }
    }
}
