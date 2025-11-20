use std::time::Instant;

use iced::{Point, Task};

use super::messages::Message;
use crate::domains::ui::types::ViewState;
use crate::state_refactored::State;

// Helper functions
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn close_video(state: &mut State) {
    if let Some(mut video) = state.domains.player.state.video_opt.take() {
        log::info!("Closing video");
        video.set_paused(true);
        drop(video);
    }
    state.domains.player.state.last_valid_position = 0.0;
    state.domains.player.state.last_valid_duration = 0.0;
    state.domains.player.state.dragging = false;
    state.domains.player.state.last_seek_position = None;
    state.domains.player.state.seeking = false;
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
#[cfg(feature = "external-mpv-player")]
pub fn load_external_video(state: &mut State) -> Task<crate::domains::player::messages::Message> {
    use super::external_mpv;
    use iced::window;

    // Check if video is already loaded or loading
    if state.domains.player.state.external_mpv_handle.is_some() {
        log::warn!("External MPV already running, skipping duplicate load");
        return Task::none();
    }

    let url = match &state.domains.player.state.current_url {
        Some(url) => url.clone(),
        None => {
            state.domains.ui.state.view = ViewState::VideoError {
                message: "No URL provided".to_string(),
            };
            return Task::none();
        }
    };

    log::info!("Starting external MPV playback for: {}", url);

    // Get current window settings
    let is_fullscreen = state.domains.player.state.is_fullscreen;
    let window_size = Some((
        state.window_size.width as u32,
        state.window_size.height as u32,
    ));

    let window_position = state
        .window_position
        .map(|pos| (pos.x as i32, pos.y as i32));

    log::info!(
        "Launching MPV with fullscreen={}, window_size={:?}, window_position={:?}",
        is_fullscreen,
        window_size,
        window_position
    );

    // Start position polling subscription first
    state.domains.player.state.external_mpv_active = true;
    state.domains.ui.state.view = ViewState::Player;

    // Get resume position from player state
    let resume_position = state.domains.player.state.pending_resume_position;

    // Spawn external MPV with window settings, position, and resume position
    match external_mpv::start_external_playback(
        url.as_str(),
        is_fullscreen,
        window_size,
        window_position,
        resume_position,
    ) {
        Ok(handle) => {
            state.domains.player.state.external_mpv_handle = Some(Box::new(handle));
            state.domains.player.state.is_loading_video = false;

            // Clear pending resume position after use
            state.domains.player.state.pending_resume_position = None;

            log::info!("External MPV started successfully, emitting HideWindow event");

            // Return no task - window hiding will be handled via domain event
            Task::none()
        }
        Err(e) => {
            log::error!("Failed to start external MPV: {}", e);
            state.domains.ui.state.view = ViewState::VideoError {
                message: format!("Failed to start MPV: {}", e),
            };
            Task::none()
        }
    }
}

pub fn load_video(state: &mut State) -> Task<crate::domains::player::messages::Message> {
    // Check if video is already loaded or loading
    if state.domains.player.state.video_opt.is_some() {
        log::warn!("Video already loaded, skipping duplicate load");
        return Task::none();
    }

    // Check if we're already in the process of loading
    if state.domains.player.state.is_loading_video {
        log::warn!("Video is already being loaded, skipping duplicate load");
        return Task::none();
    }

    // Video loading is now handled directly, while transcoding cases are handled by streaming domain

    // Mark that we're loading
    state.domains.player.state.is_loading_video = true;

    // Close existing video if any (should not happen due to guard above)
    close_video(state);

    let url = match &state.domains.player.state.current_url {
        Some(url) => url.clone(),
        None => {
            state.domains.ui.state.view = ViewState::VideoError {
                message: "No URL provided".to_string(),
            };
            state.domains.player.state.is_loading_video = false;
            return Task::none();
        }
    };

    log::info!("=== VIDEO LOADING DEBUG ===");
    log::info!("Loading video URL: {}", url);
    log::info!("URL scheme: {}", url.scheme());
    log::info!("URL host: {:?}", url.host());
    log::info!("URL path: {}", url.path());

    // Check if this is HDR content based on server metadata
    let (use_hdr_pipeline, needs_metadata_fetch) =
        if let Some(current_media) = &state.domains.player.state.current_media {
            if let Some(metadata) = &current_media.media_file_metadata {
                if let Some(duration) = metadata.duration {
                    state.domains.player.state.last_valid_duration = duration;
                }
            }
            // Always log metadata for debugging
            log::info!("Checking HDR status for: {}", current_media.filename);

            let has_color_metadata = if let Some(metadata) = &current_media.media_file_metadata {
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
                log::warn!("  No color metadata for potential HDR file, metadata fetch needed!");
            }

            (false, needs_fetch)
        } else {
            (false, false)
        };

    // Override HDR decision if filename suggests HDR but metadata is missing
    let use_hdr_pipeline_final = if needs_metadata_fetch {
        log::warn!("No HDR metadata available, using filename heuristics for pipeline selection");
        true // Use HDR pipeline for likely HDR content even without metadata
    } else {
        use_hdr_pipeline
    };

    /* // Performed in iced_video_player, redundant to do here
    // Initialize GStreamer if needed
    if let Err(e) = gst::init() {
        log::warn!("GStreamer init returned: {:?}", e);
    } else {
        log::info!("GStreamer initialized successfully");
    } */

    // Check GStreamer version

    // Validate URL is valid UTF-8 before using
    let url_string = url.as_str();
    if !url_string.is_ascii() {
        log::warn!("URL contains non-ASCII characters: {}", url_string);
        // Check each byte
        for (i, byte) in url_string.bytes().enumerate() {
            if byte > 127 {
                log::warn!("Non-ASCII byte at position {}: 0x{:02x}", i, byte);
            }
        }
    }

    log::info!(
        "Creating Video object with URL: {} (HDR: {})",
        url_string,
        use_hdr_pipeline_final
    );

    // Log URL bytes for debugging UTF-8 issues
    log::debug!("URL bytes: {:?}", url_string.as_bytes());

    // Store tone mapping config for the async task
    //let tone_mapping_config_final = state.domains.player.state.tone_mapping_config.clone();

    state.domains.ui.state.view = ViewState::Player;

    // Create video synchronously on the UI thread and update state immediately
    match subwave_unified::video::SubwaveVideo::new(&url) {
        Ok(mut video) => {
            // Update duration if available
            let duration = video.duration().as_secs_f64();
            if duration > 0.0 {
                state.domains.player.state.last_valid_duration = duration;
                state.domains.player.state.last_valid_duration = duration;
            }

            // Resume position if any
            if let Some(resume_pos) = state.domains.player.state.pending_resume_position {
                let _ = video.seek(std::time::Duration::from_secs_f32(resume_pos), false);
                state.domains.player.state.pending_resume_position = None;
            }

            // Store and start playback
            state.domains.player.state.video_opt = Some(video);
            if let Some(video) = &mut state.domains.player.state.video_opt {
                // Start playback
                video.set_paused(false);
                // If Wayland backend and subtitles enabled, create overlay
                if video.backend() == subwave_unified::video::BackendPreference::ForceWayland
                    && video.subtitles_enabled()
                {
                    // Use window size for overlay; can refine to video size later
                    let (w, h) = (
                        state.window_size.width as i32,
                        state.window_size.height as i32,
                    );
                    if let Ok(mut overlay) =
                        subwave_overlay::SubtitleOverlay::new(&url, w.max(1), h.max(1))
                    {
                        // Share clock from main pipeline for rough sync
                        let main_pipe = video.pipeline();
                        overlay.adopt_clock_from(&main_pipe);
                        // Apply current subtitle index if any
                        overlay.select_subtitle_index(video.current_subtitle_track());
                        let _ = overlay.start();
                        state.domains.player.state.overlay = Some(overlay);
                    }
                } else {
                    state.domains.player.state.overlay = None;
                }
            }
            state.domains.player.state.is_loading_video = false;
            state.domains.ui.state.view = ViewState::Player;

            Task::done(crate::domains::player::messages::Message::VideoLoaded(true))
        }
        Err(e) => {
            log::error!("Failed to create video: {}", e);
            state.domains.ui.state.view = ViewState::VideoError {
                message: format!("{}", e),
            };
            state.domains.player.state.is_loading_video = false;
            Task::done(crate::domains::player::messages::Message::VideoLoaded(
                false,
            ))
        }
    }
}

fn update_controls(state: &mut State, show: bool) {
    state.domains.player.state.controls = show;
    if show {
        state.domains.player.state.controls_time = Instant::now();
    }
}
