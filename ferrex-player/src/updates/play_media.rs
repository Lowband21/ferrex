use crate::{
    media_library::MediaFile, messages::media::Message, player::video::load_video, prelude::*,
    state::State,
};
use iced::Task;

/// Handle play media request
pub fn handle_play_media(state: &mut State, media: MediaFile) -> Task<Message> {
    log::info!("Playing media: {} (id: {})", media.filename, media.id);
    log::info!("Media path: {:?}", media.path);
    log::info!("Server URL: {}", state.server_url);

    // Save current scroll position before playing media
    state.save_scroll_position();

    state.player.current_media = Some(media.clone());

    // Reset watch progress tracking for new media
    state.player.last_progress_update = None;
    state.player.last_progress_sent = 0.0;

    // Note: current_media_id is set by PlayMediaWithId message handler if available

    // Check if this is HDR content
    let is_hdr_content = if let Some(metadata) = &media.metadata {
        // Check bit depth
        if let Some(bit_depth) = metadata.bit_depth {
            if bit_depth > 8 {
                log::info!("HDR detected: bit depth = {}", bit_depth);
                true
            } else {
                false
            }
        } else if let Some(color_transfer) = &metadata.color_transfer {
            // Check color transfer characteristics
            let hdr_transfers = ["smpte2084", "arib-std-b67", "smpte2086"];
            let is_hdr = hdr_transfers.iter().any(|&t| color_transfer.contains(t));
            if is_hdr {
                log::info!("HDR detected: color transfer = {}", color_transfer);
            }
            is_hdr
        } else if let Some(color_primaries) = &metadata.color_primaries {
            // Check color primaries
            let is_hdr = color_primaries.contains("bt2020");
            if is_hdr {
                log::info!("HDR detected: color primaries = {}", color_primaries);
            }
            is_hdr
        } else {
            false
        }
    } else {
        // Fallback to filename detection if no metadata
        let filename_suggests_hdr = media.filename.contains("2160p")
            || media.filename.contains("UHD")
            || media.filename.contains("HDR")
            || media.filename.contains("DV");
        if filename_suggests_hdr {
            log::info!("HDR suggested by filename: {}", media.filename);
        }
        filename_suggests_hdr
    };

    // Determine if we should use adaptive streaming
    // Use HLS for HDR content that requires transcoding
    let use_adaptive_streaming = false; // Only use adaptive for HDR content

    // Initialize HLS client if using adaptive streaming
    if use_adaptive_streaming {
        state.player.hls_client =
            Some(crate::server::hls::HlsClient::new(state.server_url.clone()));
        state.player.using_hls = true;
    }

    let (video_url, start_transcoding_task) = if media.path.starts_with("http") {
        (media.path.clone(), None)
    } else if use_adaptive_streaming && is_hdr_content {
        // Use adaptive streaming for all content
        log::info!("Using adaptive streaming for media: {}", media.id);

        // Store transcoding state
        state.player.is_hdr_content = is_hdr_content;
        state.player.using_hls = true;
        state.player.transcoding_status = Some(ferrex_core::TranscodingStatus::Pending);

        // Create HLS client
        let hls_client = crate::server::hls::HlsClient::new(state.server_url.clone());
        state.player.hls_client = Some(hls_client);

        // Use master playlist URL for HLS playback
        log::debug!(
            "Building master URL - server: {}, media.id: {}",
            state.server_url,
            media.id
        );
        log::debug!("Media ID bytes: {:?}", media.id.as_bytes());
        // Percent-encode the media ID to handle special characters
        let encoded_media_id = urlencoding::encode(&media.id);
        let master_url = format!(
            "{}/transcode/{}/master.m3u8",
            state.server_url, encoded_media_id
        );
        log::debug!("Encoded media ID: {}", encoded_media_id);
        log::debug!("Constructed master URL: {}", master_url);
        log::debug!("Master URL bytes: {:?}", master_url.as_bytes());

        // Create task to start transcoding only if we don't already have a job
        let start_task = if state.player.transcoding_job_id.is_none() {
            let server_url = state.server_url.clone();
            let media_id = media.id.clone();

            log::info!("Starting new adaptive transcoding for media: {}", media_id);
            log::info!(
                "Current transcoding status: {:?}",
                state.player.transcoding_status
            );

            Some(Task::perform(
                async move {
                    let client = crate::server::hls::HlsClient::new(server_url);
                    // Use retry logic with 3 retries
                    match client
                        .start_adaptive_transcoding_with_retry(&media_id, 3)
                        .await
                    {
                        Ok(job_id) => {
                            log::info!(
                                "Adaptive transcoding started successfully with master job ID: {}",
                                job_id
                            );
                            Ok(job_id)
                        }
                        Err(e) => {
                            log::error!("Failed to start adaptive transcoding: {}", e);
                            Err(e)
                        }
                    }
                },
                |_result| Message::TranscodingStarted,
            ))
        } else {
            log::warn!(
                "Transcoding job already exists: {:?}, skipping duplicate start request",
                state.player.transcoding_job_id
            );
            log::warn!(
                "Current transcoding status: {:?}",
                state.player.transcoding_status
            );
            None
        };

        (master_url, start_task)
    } else {
        // Fallback to direct streaming (old behavior)
        let video_url = if is_hdr_content {
            /*
            let profile = if let Some(metadata) = &media.metadata {
                if let Some(height) = metadata.height {
                    if height >= 2160 {
                        "hdr_to_sdr_4k"
                    } else {
                        "hdr_to_sdr_1080p"
                    }
                } else {
                    "hdr_to_sdr_1080p"
                }
            } else {
                "hdr_to_sdr_1080p"
            }; */

            let encoded_media_id = urlencoding::encode(&media.id);
            let transcode_url = format!("{}/stream/{}", state.server_url, encoded_media_id);
            log::info!("Using direct transcode stream: {}", transcode_url);

            state.player.is_hdr_content = true;
            state.player.using_hls = false;
            state.player.transcoding_status =
                Some(crate::player::state::TranscodingStatus::Processing { progress: 0.0 });

            transcode_url
        } else {
            let encoded_media_id = urlencoding::encode(&media.id);
            let stream_url = format!("{}/stream/{}", state.server_url, encoded_media_id);
            log::info!("Using direct stream: {}", stream_url);

            state.player.is_hdr_content = false;
            state.player.using_hls = false;
            state.player.transcoding_status = None;

            stream_url
        };

        // For direct playback, we'll get duration from the video object itself
        // This ensures we have the actual playable duration, not just metadata

        (video_url, None)
    };

    log::info!("Final video URL: {}", video_url);

    // Check for UTF-8 validity before parsing
    if let Err(e) = std::str::from_utf8(video_url.as_bytes()) {
        log::error!("Video URL contains invalid UTF-8: {:?}", e);
        log::error!("URL bytes: {:?}", video_url.as_bytes());
    }

    // Parse URL and load video
    match url::Url::parse(&video_url) {
        Ok(url) => {
            state.player.current_url = Some(url);
            // Set loading state
            state.view = ViewState::LoadingVideo {
                url: video_url.clone(),
            };
            state.error_message = None;

            // If we're using adaptive streaming, don't load video yet
            // Wait for transcoding to be ready first
            if use_adaptive_streaming && is_hdr_content {
                match start_transcoding_task {
                    Some(transcode_task) => transcode_task,
                    None => {
                        log::error!("No transcoding task for adaptive streaming!");
                        Task::none()
                    }
                }
            } else {
                // For direct streaming, load video immediately
                load_video(state)
            }
        }
        Err(e) => {
            state.error_message = Some(format!("Invalid URL: {}", e));
            state.view = ViewState::VideoError {
                message: format!("Invalid URL: {}", e),
            };
            Task::none()
        }
    }
}

/// Handle media unavailable event
pub fn handle_media_unavailable(
    state: &mut State,
    reason: String,
    message: String,
) -> Task<Message> {
    log::error!("Media unavailable: {} - {}", reason, message);

    let error_msg = match reason.as_str() {
                "library_offline" => {
                    "Media Library Offline\n\nThe media library storage is currently unavailable. Please ensure the storage device is connected and mounted properly.".to_string()
                }
                "file_missing" => {
                    "Media File Not Found\n\nThis media file has been moved or deleted from the library. You may need to rescan the library to update the database.".to_string()
                }
                _ => message.clone()
            };

    state.error_message = Some(error_msg.clone());
    state.view = ViewState::VideoError { message: error_msg };
    Task::none()
}
