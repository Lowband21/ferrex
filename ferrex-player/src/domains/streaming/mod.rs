//! Streaming/Transcoding domain
//!
//! Contains all streaming-related state and logic

pub mod messages;
pub mod update;
pub mod update_handlers;

use self::messages::StreamingMessage;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::infra::services::api::ApiService;
use ferrex_core::player_prelude::{LibraryID, TranscodingStatus};
use iced::Task;
use std::sync::Arc;

use crate::infra::repository::accessor::{Accessor, ReadOnly};

/// Streaming domain state
pub struct StreamingDomainState {
    // References needed by streaming domain
    pub api_service: Arc<dyn ApiService>,
    pub current_library_id: Option<LibraryID>,

    pub repo_accessor: Accessor<ReadOnly>,

    // Streaming-specific service trait
    pub streaming_service:
        Arc<dyn crate::infra::services::streaming::StreamingApiService>,

    // Streaming state moved from PlayerState
    pub using_hls: bool,
    pub transcoding_status: Option<TranscodingStatus>,
    pub transcoding_job_id: Option<String>,
    pub transcoding_duration: Option<f64>, // Duration from transcoding job
    pub transcoding_check_count: u32,      // Number of status checks performed
    pub hls_client: Option<crate::domains::library::server::hls::HlsClient>,
    pub master_playlist:
        Option<crate::domains::library::server::hls::MasterPlaylist>,
    pub current_variant_playlist:
        Option<crate::domains::library::server::hls::VariantPlaylist>,
    pub current_segment_index: usize,
    pub segment_buffer: Vec<Vec<u8>>, // Prefetched segments
    pub last_bandwidth_measurement: Option<u64>, // bits per second
    pub quality_switch_count: u32,
}

impl std::fmt::Debug for StreamingDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingDomain")
            .field("state", &self.state)
            .finish()
    }
}

impl std::fmt::Debug for StreamingDomainState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingDomainState")
            .field("api_service", &"ApiService(..)")
            .field("current_library_id", &self.current_library_id)
            .field("streaming_service", &"StreamingApiService(..)")
            .field("using_hls", &self.using_hls)
            .field("transcoding_status", &self.transcoding_status)
            .field("transcoding_job_id", &self.transcoding_job_id)
            .field("transcoding_duration", &self.transcoding_duration)
            .field("transcoding_check_count", &self.transcoding_check_count)
            .field("hls_client", &self.hls_client.is_some())
            .field("master_playlist", &self.master_playlist.is_some())
            .field(
                "current_variant_playlist",
                &self.current_variant_playlist.is_some(),
            )
            .field("current_segment_index", &self.current_segment_index)
            .field("segment_buffer_count", &self.segment_buffer.len())
            .field(
                "last_bandwidth_measurement",
                &self.last_bandwidth_measurement,
            )
            .field("quality_switch_count", &self.quality_switch_count)
            .finish()
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl StreamingDomainState {
    pub fn new(
        api_service: Arc<dyn ApiService>,
        streaming_service: Arc<
            dyn crate::infra::services::streaming::StreamingApiService,
        >,
        repo_accessor: Accessor<ReadOnly>,
    ) -> Self {
        Self {
            api_service,
            current_library_id: None,
            repo_accessor,
            streaming_service,

            // Initialize streaming state
            using_hls: false,
            transcoding_status: None,
            transcoding_job_id: None,
            transcoding_duration: None,
            transcoding_check_count: 0,
            hls_client: None,
            master_playlist: None,
            current_variant_playlist: None,
            current_segment_index: 0,
            segment_buffer: Vec::new(),
            last_bandwidth_measurement: None,
            quality_switch_count: 0,
        }
    }

    /// Reset streaming state (equivalent to PlayerState::reset_stream_state)
    pub fn reset_stream_state(&mut self) {
        self.using_hls = false;
        self.transcoding_status = None;
        self.transcoding_job_id = None;
        self.hls_client = None;
        self.master_playlist = None;
        self.current_variant_playlist = None;
        self.current_segment_index = 0;
        self.segment_buffer.clear();
        self.last_bandwidth_measurement = None;
        self.quality_switch_count = 0;
        self.transcoding_duration = None;
        self.transcoding_check_count = 0;
    }
}

pub struct StreamingDomain {
    pub state: StreamingDomainState,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl StreamingDomain {
    pub fn new(state: StreamingDomainState) -> Self {
        Self { state }
    }

    /// Update function - delegates to existing update_streaming logic
    pub fn update(
        &mut self,
        _message: StreamingMessage,
    ) -> Task<DomainMessage> {
        // This will call the existing update_streaming function
        Task::none()
    }

    pub fn handle_event(
        &mut self,
        event: &CrossDomainEvent,
    ) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::LibraryChanged(library_id) => {
                self.state.current_library_id = Some(*library_id);
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
