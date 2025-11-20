use crate::{
    common::messages::{CrossDomainEvent, DomainUpdateResult},
    domains::media::messages::Message,
    domains::player::video::load_video,
    domains::ui::types::ViewState,
    state_refactored::State,
};
use ferrex_core::MediaFile;
use iced::Task;

/// Handle play media request
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_play_media(state: &mut State, media: MediaFile) -> DomainUpdateResult {
    log::info!("Playing media: {} (id: {})", media.filename, media.id);
    log::info!("Media path: {:?}", media.path);
    log::info!("Server URL: {}", state.server_url);

    // Save current scroll position before playing media
    // TODO: Implement save_scroll_position for domain state

    state.domains.player.state.current_media = Some(media.clone());

    if let Some(metadata) = &media.media_file_metadata {
        if let Some(duration) = metadata.duration {
            log::info!("Setting duration from media metadata: {:.1}s", duration);
            state.domains.player.state.duration = duration;
        }
    }

    // Reset watch progress tracking for new media
    state.domains.player.state.last_progress_update = None;
    state.domains.player.state.last_progress_sent = 0.0;

    // Pass any pending resume position to player domain
    state.domains.player.state.pending_resume_position =
        state.domains.media.state.pending_resume_position;

    // Clear the pending resume position from media domain after transferring
    state.domains.media.state.pending_resume_position = None;

    // Note: current_media_id is set by PlayMediaWithId message handler if available

    // Check if this is HDR content
    let is_hdr_content = if let Some(metadata) = &media.media_file_metadata {
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

    let media_id = media.id.to_string();

    let video_url = {
        let encoded_media_id = urlencoding::encode(&media_id);
        let video_url = format!("{}/stream/{}", state.server_url, encoded_media_id);

        video_url
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
            state.domains.player.state.current_url = Some(url);
            // Set loading state
            state.domains.ui.state.view = ViewState::LoadingVideo {
                url: video_url.clone(),
            };
            state.domains.ui.state.error_message = None;

            {
                // For direct streaming, send message to load video
                DomainUpdateResult::task(Task::done(crate::common::messages::DomainMessage::Media(
                    Message::_LoadVideo,
                )))
            }
        }
        Err(e) => {
            state.domains.ui.state.error_message = Some(format!("Invalid URL: {}", e));
            state.domains.ui.state.view = ViewState::VideoError {
                message: format!("Invalid URL: {}", e),
            };
            DomainUpdateResult::task(Task::none())
        }
    }
}

/// Handle media unavailable event
pub fn handle_media_unavailable(
    state: &mut State,
    reason: String,
    message: String,
) -> DomainUpdateResult {
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

    state.domains.ui.state.error_message = Some(error_msg.clone());
    state.domains.ui.state.view = ViewState::VideoError { message: error_msg };
    DomainUpdateResult::task(Task::none())
}
