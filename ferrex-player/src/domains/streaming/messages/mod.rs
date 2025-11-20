use ferrex_core::TranscodingStatus;

#[derive(Clone, Debug)]
pub enum Message {
    // Transcoding
    TranscodingStarted(Result<String, String>), // job_id or error
    TranscodingStatusUpdate(Result<(TranscodingStatus, Option<f64>, Option<String>), String>),
    CheckTranscodingStatus, // Periodic check for transcoding status

    // HLS Streaming
    MasterPlaylistLoaded(Option<crate::domains::library::server::hls::MasterPlaylist>), // Master playlist from server
    MasterPlaylistReady(Option<crate::domains::library::server::hls::MasterPlaylist>), // Master playlist exists and ready for playback

    // Segment management
    StartSegmentPrefetch(usize), // Start prefetching segment at index
    SegmentPrefetched(usize, Result<Vec<u8>, String>), // segment index, data or error

    // Bandwidth adaptation
    BandwidthMeasured(u64), // bits per second
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            // Transcoding
            Self::TranscodingStarted(_) => "Streaming::TranscodingStarted",
            Self::TranscodingStatusUpdate(_) => "Streaming::TranscodingStatusUpdate",
            Self::CheckTranscodingStatus => "Streaming::CheckTranscodingStatus",

            // HLS Streaming
            Self::MasterPlaylistLoaded(_) => "Streaming::MasterPlaylistLoaded",
            Self::MasterPlaylistReady(_) => "Streaming::MasterPlaylistReady",

            // Segment management
            Self::StartSegmentPrefetch(_) => "Streaming::StartSegmentPrefetch",
            Self::SegmentPrefetched(_, _) => "Streaming::SegmentPrefetched",

            // Bandwidth adaptation
            Self::BandwidthMeasured(_) => "Streaming::BandwidthMeasured",
        }
    }
}

/// Streaming domain events
#[derive(Clone, Debug)]
pub enum StreamingEvent {
    TranscodingStarted(String),   // job_id
    TranscodingCompleted(String), // job_id
    StreamReady(String),          // stream_url
    BandwidthChanged(u64),        // new bandwidth
}
