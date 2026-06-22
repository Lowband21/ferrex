//! Video backend wiring and stream URL handling.
//!
//! This module coordinates the unified video backend, stream URL redaction,
//! loading flags, and playback state transitions around media file playback.

use crate::{
    PlayerMessage,
    diagnostics::redact_playback_url,
    state::PlayerDomainState,
    update::{PlaybackUiShell, PlaybackUpdatePort},
};

use iced::Task;
use subwave_unified::video::SubwaveVideo;

// Helper functions
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn close_video(state: &mut PlayerDomainState) {
    if let Some(mut video) = state.video_opt.take() {
        log::info!("Closing video");
        video.set_paused(true);
        drop(video);
    }
    state.last_valid_position = 0.0;
    state.last_valid_duration = 0.0;
    state.dragging = false;
    state.last_seek_position = None;
    state.seeking = false;
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn load_video<P>(
    state: &mut PlayerDomainState,
    ui: &mut dyn PlaybackUiShell,
) -> Task<P::AppMessage>
where
    P: PlaybackUpdatePort + 'static,
{
    // Check if video is already loaded or loading
    if state.video_opt.is_some() {
        log::warn!("Video already loaded, skipping duplicate load");
        return Task::none();
    }

    // Check if we're already in the process of loading
    if state.is_loading_video {
        log::warn!("Video is already being loaded, skipping duplicate load");
        return Task::none();
    }

    // Video loading is now handled directly, while transcoding cases are handled by streaming domain

    // Mark that we're loading
    state.is_loading_video = true;

    // Preserve any resume/duration hints before closing the current provider
    let pending_resume_hint = state.pending_resume_position;
    let duration_hint_before_close = state.last_valid_duration;

    // Close existing video if any (should not happen due to guard above)
    close_video(state);

    // Restore playback hints immediately so UI elements reflect intended progress
    if let Some(resume) = pending_resume_hint {
        state.last_valid_position = resume as f64;
        state.pending_resume_position = Some(resume);
    } else {
        state.last_valid_position = 0.0;
        state.pending_resume_position = None;
    }

    if duration_hint_before_close > 0.0 {
        state.last_valid_duration = duration_hint_before_close;
    }

    let url = match &state.current_url {
        Some(url) => url.clone(),
        None => {
            ui.set_video_error("No URL provided".to_string());
            state.is_loading_video = false;
            return Task::none();
        }
    };

    let redacted_url = redact_playback_url(url.as_str());

    log::info!("=== VIDEO LOADING DEBUG ===");
    log::info!("Loading video URL: {}", redacted_url);
    log::info!("URL scheme: {}", url.scheme());
    log::info!("URL host: {:?}", url.host());
    log::info!("URL path: {}", url.path());

    // Check if this is HDR content based on server metadata
    let (use_hdr_pipeline, needs_metadata_fetch) = if let Some(current_media) =
        &state.current_media
    {
        if let Some(metadata) = &current_media.media_file_metadata
            && let Some(duration) = metadata.duration
        {
            state.last_valid_duration = duration;
        }
        // Always log metadata for debugging
        log::info!("Checking HDR status for: {}", current_media.filename);

        let has_color_metadata =
            if let Some(metadata) = &current_media.media_file_metadata {
                log::info!("  Color transfer: {:?}", metadata.color_transfer);
                log::info!("  Color space: {:?}", metadata.color_space);
                log::info!("  Color primaries: {:?}", metadata.color_primaries);
                log::info!("  Bit depth: {:?}", metadata.bit_depth);

                // Check if we have any color metadata
                metadata.color_transfer.is_some()
                    || metadata.color_space.is_some()
                    || metadata.color_primaries.is_some()
                    || metadata.bit_depth.is_some()
            } else {
                log::warn!("  No metadata available from server!");
                false
            };

        // If no color metadata and filename suggests HDR, we need to fetch metadata
        let filename_suggests_hdr = current_media.filename.contains("2160p")
            || current_media.filename.contains("UHD")
            || current_media.filename.contains("HDR")
            || current_media.filename.contains("DV");

        let needs_fetch = !has_color_metadata && filename_suggests_hdr;

        if needs_fetch {
            log::warn!(
                "  No color metadata for potential HDR file, metadata fetch needed!"
            );
        }

        (false, needs_fetch)
    } else {
        (false, false)
    };

    // Override HDR decision if filename suggests HDR but metadata is missing
    let use_hdr_pipeline_final = if needs_metadata_fetch {
        log::warn!(
            "No HDR metadata available, using filename heuristics for provider selection"
        );
        true // Use HDR provider for likely HDR content even without metadata
    } else {
        use_hdr_pipeline
    };

    // Validate URL is valid UTF-8 before using
    let url_string = url.as_str();
    if !url_string.is_ascii() {
        log::warn!(
            "URL contains non-ASCII characters: {}",
            redact_playback_url(url_string)
        );
        // Check each byte
        for (i, byte) in url_string.bytes().enumerate() {
            if byte > 127 {
                log::warn!("Non-ASCII byte at position {}: 0x{:02x}", i, byte);
            }
        }
    }

    log::info!(
        "Creating Video object with URL: {} (HDR: {})",
        redact_playback_url(url_string),
        use_hdr_pipeline_final
    );

    ui.set_player_view();

    let res_pos: f64 = state.pending_resume_position.unwrap_or(0.0).into();

    // Create video synchronously on the UI thread and update state immediately
    match SubwaveVideo::open_at_seconds(&url, res_pos) {
        Ok(video) => {
            let duration = video.duration().as_secs_f64();
            if duration > 0.0 {
                state.last_valid_duration = duration;
            }

            state.video_opt = Some(video);

            state.is_loading_video = false;
            ui.set_player_view();

            Task::done(P::playback_message(PlayerMessage::VideoLoaded(true)))
        }
        Err(e) => {
            log::error!("Failed to create video: {}", e);
            ui.set_video_error(format!("{}", e));
            state.is_loading_video = false;
            Task::done(P::playback_message(PlayerMessage::VideoLoaded(false)))
        }
    }
}
