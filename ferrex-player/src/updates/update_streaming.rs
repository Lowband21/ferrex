use crate::{messages::streaming, state::State};
use iced::Task;

/// Handle streaming domain messages
///
/// This handler is part of the domain message architecture migration.
/// To integrate into the main update loop:
///
/// 1. In update.rs, add a match arm for DomainMessage::Streaming:
///    ```
///    DomainMessage::Streaming(msg) => {
///        super::update_streaming::update_streaming(state, msg)
///            .map(|msg| DomainMessage::Streaming(msg))
///    }
///    ```
///
/// 2. Or for legacy integration, handle streaming messages directly:
///    ```
///    Message::TranscodingStarted(r) => {
///        update_streaming(state, streaming::Message::TranscodingStarted(r))
///            .map(convert_streaming_to_legacy)
///    }
///    ```
pub fn update_streaming(
    state: &mut State,
    message: streaming::Message,
) -> Task<streaming::Message> {
    match message {
        // Transcoding messages
        streaming::Message::TranscodingStarted(result) => {
            super::transcoding::handle_transcoding_started(state, result)
        }

        streaming::Message::TranscodingStatusUpdate(result) => {
            super::transcoding::handle_transcoding_status_update(state, result)
        }

        streaming::Message::CheckTranscodingStatus => {
            super::transcoding::handle_check_transcoding_status(state)
        }

        // HLS Streaming messages
        streaming::Message::MasterPlaylistLoaded(playlist_opt) => {
            handle_master_playlist_loaded(state, playlist_opt)
        }

        streaming::Message::MasterPlaylistReady(playlist_opt) => {
            handle_master_playlist_ready(state, playlist_opt)
        }

        // Segment management
        streaming::Message::StartSegmentPrefetch(segment_index) => {
            handle_start_segment_prefetch(state, segment_index)
        }

        streaming::Message::SegmentPrefetched(index, result) => {
            handle_segment_prefetched(state, index, result)
        }

        // Bandwidth adaptation
        streaming::Message::BandwidthMeasured(bandwidth) => {
            handle_bandwidth_measured(state, bandwidth)
        }

        // Cross-domain event emission (internal coordination)
        streaming::Message::_EmitCrossDomainEvent(event) => {
            // This is handled by the cross-domain coordinator
            Task::done(streaming::Message::_EmitCrossDomainEvent(event))
        }
    }
}

/// Handle master playlist loaded
fn handle_master_playlist_loaded(
    state: &mut State,
    playlist_opt: Option<crate::server::hls::MasterPlaylist>,
) -> Task<streaming::Message> {
    if let Some(playlist) = playlist_opt {
        log::info!(
            "Master playlist loaded with {} quality variants",
            playlist.variants.len()
        );
        for variant in &playlist.variants {
            log::info!(
                "  - {} ({}p, {:.1} Mbps)",
                variant.profile,
                variant.resolution.map(|(_, h)| h).unwrap_or(0),
                variant.bandwidth as f64 / 1_000_000.0
            );
        }
        state.player.master_playlist = Some(playlist);
    }
    Task::none()
}

/// Handle master playlist ready for playback
fn handle_master_playlist_ready(
    state: &mut State,
    playlist_opt: Option<crate::server::hls::MasterPlaylist>,
) -> Task<streaming::Message> {
    if let Some(playlist) = playlist_opt {
        log::info!(
            "Master playlist is ready - loading video with {} quality variants",
            playlist.variants.len()
        );
        state.player.master_playlist = Some(playlist);

        // Now that we confirmed the playlist exists, emit cross-domain event to load the video
        Task::done(streaming::Message::_EmitCrossDomainEvent(
            crate::messages::CrossDomainEvent::VideoReadyToPlay,
        ))
    } else {
        log::error!("Master playlist check failed - retrying in 2 seconds");
        // Retry checking after a delay
        Task::perform(
            async {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            },
            |_| streaming::Message::CheckTranscodingStatus,
        )
    }
}

/// Handle segment prefetch request
fn handle_start_segment_prefetch(
    _state: &mut State,
    _segment_index: usize,
) -> Task<streaming::Message> {
    // TODO: Implement segment prefetching when needed
    // For now, GStreamer handles buffering internally
    Task::none()
}

/// Handle prefetched segment data
fn handle_segment_prefetched(
    _state: &mut State,
    _index: usize,
    _result: Result<Vec<u8>, String>,
) -> Task<streaming::Message> {
    // TODO: Handle prefetched segment data
    Task::none()
}

/// Handle bandwidth measurement update
fn handle_bandwidth_measured(state: &mut State, bandwidth: u64) -> Task<streaming::Message> {
    log::debug!("Bandwidth measured: {} bps", bandwidth);
    state.player.last_bandwidth_measurement = Some(bandwidth);

    // Check if we should switch quality based on bandwidth
    // TODO: Fix type inference issue and implement automatic quality switching
    /*
    if let Some(ref mut hls_client) = state.player.hls_client {
        if let Some(ref master_playlist) = state.player.master_playlist {
            if let Some(new_variant) = hls_client.should_switch_variant(master_playlist) {
                log::info!("Switching to quality variant: {}", new_variant.profile);
                // TODO: Implement automatic quality switching
            }
        }
    }
    */

    Task::none()
}
