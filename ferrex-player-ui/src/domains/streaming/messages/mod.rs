use ferrex_core::player_prelude::TranscodingStatus;

#[derive(Clone)]
pub enum StreamingMessage {
    // Transcoding
    TranscodingStarted(Result<String, String>), // job_id or error
    TranscodingStatusUpdate(
        Result<(TranscodingStatus, Option<f64>, Option<String>), String>,
    ),
    CheckTranscodingStatus, // Periodic check for transcoding status

    // HLS Streaming
    MasterPlaylistLoaded(
        Option<crate::domains::library::server::hls::MasterPlaylist>,
    ), // Master playlist from server
    MasterPlaylistReady(
        Option<crate::domains::library::server::hls::MasterPlaylist>,
    ), // Master playlist exists and ready for playback

    // Segment management
    StartSegmentPrefetch(usize), // Start prefetching segment at index
    SegmentPrefetched(usize, Result<Vec<u8>, String>), // segment index, data or error

    // Bandwidth adaptation
    BandwidthMeasured(u64), // bits per second
}

impl std::fmt::Debug for StreamingMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Transcoding
            Self::TranscodingStarted(result) => {
                write!(f, "TranscodingStarted({:?})", result)
            }
            Self::TranscodingStatusUpdate(result) => {
                write!(f, "TranscodingStatusUpdate({:?})", result)
            }
            Self::CheckTranscodingStatus => write!(f, "CheckTranscodingStatus"),

            // HLS Streaming
            Self::MasterPlaylistLoaded(playlist) => {
                write!(f, "MasterPlaylistLoaded({:?})", playlist.is_some())
            }
            Self::MasterPlaylistReady(playlist) => {
                write!(f, "MasterPlaylistReady({:?})", playlist.is_some())
            }

            // Segment management
            Self::StartSegmentPrefetch(index) => {
                write!(f, "StartSegmentPrefetch({})", index)
            }
            Self::SegmentPrefetched(index, result) => write!(
                f,
                "SegmentPrefetched({}, {:?})",
                index,
                result.as_ref().map(|v| v.len())
            ),

            // Bandwidth adaptation
            Self::BandwidthMeasured(bandwidth) => {
                write!(f, "BandwidthMeasured({})", bandwidth)
            }
        }
    }
}

impl StreamingMessage {
    pub fn name(&self) -> &'static str {
        match self {
            // Transcoding
            Self::TranscodingStarted(_) => "Streaming::TranscodingStarted",
            Self::TranscodingStatusUpdate(_) => {
                "Streaming::TranscodingStatusUpdate"
            }
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
