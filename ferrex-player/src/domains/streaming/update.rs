use super::messages::StreamingMessage;
use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::state::State;
use iced::Task;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update_streaming(
    state: &mut State,
    message: StreamingMessage,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infra::profiling_scopes::scopes::STREAMING_UPDATE);

    match message {
        // Transcoding messages
        StreamingMessage::TranscodingStarted(result) => {
            super::update_handlers::transcoding::handle_transcoding_started(state, result)
        }

        StreamingMessage::TranscodingStatusUpdate(result) => {
            super::update_handlers::transcoding::handle_transcoding_status_update(state, result)
        }

        StreamingMessage::CheckTranscodingStatus => {
            super::update_handlers::transcoding::handle_check_transcoding_status(state)
        }

        // HLS Streaming messages
        StreamingMessage::MasterPlaylistLoaded(playlist_opt) => {
            handle_master_playlist_loaded(state, playlist_opt)
        }

        StreamingMessage::MasterPlaylistReady(playlist_opt) => {
            handle_master_playlist_ready(state, playlist_opt)
        }

        // Segment management
        StreamingMessage::StartSegmentPrefetch(segment_index) => {
            handle_start_segment_prefetch(state, segment_index)
        }

        StreamingMessage::SegmentPrefetched(index, result) => {
            handle_segment_prefetched(state, index, result)
        }

        // Bandwidth adaptation
        StreamingMessage::BandwidthMeasured(bandwidth) => handle_bandwidth_measured(state, bandwidth),
    }
}

/// Handle master playlist loaded
fn handle_master_playlist_loaded(
    state: &mut State,
    playlist_opt: Option<crate::domains::library::server::hls::MasterPlaylist>,
) -> DomainUpdateResult {
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
        state.domains.streaming.state.master_playlist = Some(playlist);
    }
    DomainUpdateResult::task(Task::none())
}

/// Handle master playlist ready for playback
fn handle_master_playlist_ready(
    state: &mut State,
    playlist_opt: Option<crate::domains::library::server::hls::MasterPlaylist>,
) -> DomainUpdateResult {
    if let Some(playlist) = playlist_opt {
        log::info!(
            "Master playlist is ready - loading video with {} quality variants",
            playlist.variants.len()
        );
        state.domains.streaming.state.master_playlist = Some(playlist);

        // Now that we confirmed the playlist exists, send direct message to Player domain
        DomainUpdateResult::task(Task::done(
            crate::common::messages::DomainMessage::Player(
                crate::domains::player::messages::PlayerMessage::VideoReadyToPlay,
            ),
        ))
    } else {
        log::error!("Master playlist check failed - retrying in 2 seconds");
        // Retry checking after a delay
        DomainUpdateResult::task(Task::perform(
            async {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            },
            |_| {
                DomainMessage::Streaming(
                    StreamingMessage::CheckTranscodingStatus,
                )
            },
        ))
    }
}

/// Handle segment prefetch request
fn handle_start_segment_prefetch(
    _state: &mut State,
    _segment_index: usize,
) -> DomainUpdateResult {
    // TODO: Implement segment prefetching when needed
    // For now, GStreamer handles buffering internally
    DomainUpdateResult::task(Task::none())
}

/// Handle prefetched segment data
fn handle_segment_prefetched(
    _state: &mut State,
    _index: usize,
    _result: Result<Vec<u8>, String>,
) -> DomainUpdateResult {
    // TODO: Handle prefetched segment data
    DomainUpdateResult::task(Task::none())
}

/// Handle bandwidth measurement update
fn handle_bandwidth_measured(
    state: &mut State,
    bandwidth: u64,
) -> DomainUpdateResult {
    log::debug!("Bandwidth measured: {} bps", bandwidth);
    state.domains.streaming.state.last_bandwidth_measurement = Some(bandwidth);

    // Check if we should switch quality based on bandwidth
    // TODO: Fix type inference issue and implement automatic quality switching
    /*
    if let Some(ref mut hls_client) = state.domains.streaming.state.hls_client {
        if let Some(ref master_playlist) = state.domains.streaming.state.master_playlist {
            if let Some(new_variant) = hls_client.should_switch_variant(master_playlist) {
                log::info!("Switching to quality variant: {}", new_variant.profile);
                // TODO: Implement automatic quality switching
            }
        }
    }
    */

    DomainUpdateResult::task(Task::none())
}
