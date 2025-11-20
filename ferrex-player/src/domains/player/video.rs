use std::sync::Arc;
use std::time::Instant;

use iced::{Point, Task};

use crate::domains::player::video_backend::Video;

use gstreamer as gst;

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
    state.domains.player.state.position = 0.0;
    state.domains.player.state.duration = 0.0;
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
pub fn load_external_video(state: &mut State) -> Task<crate::domains::media::messages::Message> {
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

pub fn load_video(state: &mut State) -> Task<crate::domains::media::messages::Message> {
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
                    state.domains.player.state.duration = duration;
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
    log::info!(
        "GStreamer version: {}.{}.{}",
        gst::version().0,
        gst::version().1,
        gst::version().2
    );

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

    // Create the loading task
    let video_url = url.to_string();

    Task::perform(
        async move {
            log::info!("Starting async video creation");

            // Use spawn_blocking since Video::new might block
            let result = tokio::task::spawn_blocking(move || {
                log::info!("Creating video for URL: {}", video_url);

                //// Get tone mapping config from state (passed via the closure context)
                //let tone_mapping_config = tone_mapping_config_final.clone();

                //if use_hdr_pipeline_final {
                //    log::info!("Attempting HDR pipeline with tone mapping config");
                //    match Video::new_with_config(&url, tone_mapping_config) {
                //        Ok(video) => {
                //            log::info!("HDR pipeline created successfully");
                //            Ok(video)
                //        }
                //        Err(e) => {
                //            log::error!("HDR pipeline failed: {:?}", e);
                //            log::warn!("Falling back to standard pipeline");
                //            // Try standard pipeline as fallback
                //            Video::new_with_config(&url, tone_mapping_config)
                //        }
                //    }
                //} else {
                //    Video::new_with_config(&url, tone_mapping_config)
                //}
                Video::new(&url)
            })
            .await;

            match result {
                Ok(video_arc) => {
                    // Video::new already returns Result<Arc<Video>, VideoError>
                    video_arc
                }
                Err(e) => {
                    // Convert JoinError to VideoError
                    Err(crate::domains::player::video_backend::VideoError::Standard(
                        format!("Task error: {:?}", e),
                    ))
                }
            }
        },
        |result| {
            // Convert to media domain message with video object
            // Convert VideoError to String for the message
            let result_string = result.map_err(|e| format!("{:?}", e));
            crate::domains::media::messages::Message::VideoCreated(result_string)
        },
    )
}

fn update_controls(state: &mut State, show: bool) {
    state.domains.player.state.controls = show;
    if show {
        state.domains.player.state.controls_time = Instant::now();
    }
}
