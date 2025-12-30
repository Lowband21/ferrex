//! Playback section update handlers

use super::messages::PlaybackMessage;
use super::state::{PlaybackQuality, ResumeBehavior};
use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Main message handler for playback section
pub fn handle_message(
    state: &mut State,
    message: PlaybackMessage,
) -> DomainUpdateResult {
    match message {
        // General
        PlaybackMessage::SetAutoPlayNext(enabled) => {
            set_auto_play_next(state, enabled)
        }
        PlaybackMessage::SetResumeBehavior(behavior) => {
            set_resume_behavior(state, behavior)
        }
        PlaybackMessage::SetPreferredQuality(quality) => {
            set_preferred_quality(state, quality)
        }

        // Seeking
        PlaybackMessage::SetSeekForwardCoarse(secs) => {
            set_seek_forward_coarse(state, secs)
        }
        PlaybackMessage::SetSeekBackwardCoarse(secs) => {
            set_seek_backward_coarse(state, secs)
        }
        PlaybackMessage::SetSeekForwardFine(secs) => {
            set_seek_forward_fine(state, secs)
        }
        PlaybackMessage::SetSeekBackwardFine(secs) => {
            set_seek_backward_fine(state, secs)
        }

        // Skip
        PlaybackMessage::SetSkipIntroDuration(secs) => {
            set_skip_intro_duration(state, secs)
        }
        PlaybackMessage::SetSkipCreditsDuration(secs) => {
            set_skip_credits_duration(state, secs)
        }

        // Subtitles
        PlaybackMessage::SetSubtitlesEnabled(enabled) => {
            set_subtitles_enabled(state, enabled)
        }
        PlaybackMessage::SetSubtitleLanguage(lang) => {
            set_subtitle_language(state, lang)
        }
        PlaybackMessage::SetSubtitleFontScale(scale) => {
            set_subtitle_font_scale(state, scale)
        }
    }
}

// General handlers
fn set_auto_play_next(state: &mut State, enabled: bool) -> DomainUpdateResult {
    // TODO: Update playback state and mark for persistence
    let _ = (state, enabled);
    DomainUpdateResult::none()
}

fn set_resume_behavior(
    state: &mut State,
    behavior: ResumeBehavior,
) -> DomainUpdateResult {
    let _ = (state, behavior);
    DomainUpdateResult::none()
}

fn set_preferred_quality(
    state: &mut State,
    quality: PlaybackQuality,
) -> DomainUpdateResult {
    let _ = (state, quality);
    DomainUpdateResult::none()
}

// Seeking handlers - accept String, parse and validate
fn set_seek_forward_coarse(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(secs) = value.parse::<f64>()
        && secs > 0.0
        && secs <= 120.0
    {
        state.domains.settings.playback.seek_forward_coarse = secs;
    }
    DomainUpdateResult::none()
}

fn set_seek_backward_coarse(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(secs) = value.parse::<f64>()
        && secs > 0.0
        && secs <= 120.0
    {
        state.domains.settings.playback.seek_backward_coarse = secs;
    }
    DomainUpdateResult::none()
}

fn set_seek_forward_fine(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(secs) = value.parse::<f64>()
        && secs > 0.0
        && secs <= 60.0
    {
        state.domains.settings.playback.seek_forward_fine = secs;
    }
    DomainUpdateResult::none()
}

fn set_seek_backward_fine(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(secs) = value.parse::<f64>()
        && secs > 0.0
        && secs <= 60.0
    {
        state.domains.settings.playback.seek_backward_fine = secs;
    }
    DomainUpdateResult::none()
}

// Skip handlers
fn set_skip_intro_duration(state: &mut State, secs: u32) -> DomainUpdateResult {
    let _ = (state, secs);
    DomainUpdateResult::none()
}

fn set_skip_credits_duration(
    state: &mut State,
    secs: u32,
) -> DomainUpdateResult {
    let _ = (state, secs);
    DomainUpdateResult::none()
}

// Subtitle handlers
fn set_subtitles_enabled(
    state: &mut State,
    enabled: bool,
) -> DomainUpdateResult {
    let _ = (state, enabled);
    DomainUpdateResult::none()
}

fn set_subtitle_language(
    state: &mut State,
    lang: Option<String>,
) -> DomainUpdateResult {
    let _ = (state, lang);
    DomainUpdateResult::none()
}

fn set_subtitle_font_scale(
    state: &mut State,
    scale: f32,
) -> DomainUpdateResult {
    let _ = (state, scale);
    DomainUpdateResult::none()
}
