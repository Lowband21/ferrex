use ferrex_core::player_prelude::MediaID;

pub mod subscriptions;

#[derive(Clone)]
pub enum MediaMessage {
    // Watch progress tracking
    ProgressUpdateSent(MediaID, f64, f64), // Position that was successfully sent to server
    ProgressUpdateFailed,                  // Failed to send progress update
    SendProgressUpdateWithData(MediaID, f64, f64), // position, duration - captures data at message creation time
    WatchProgressFetched(MediaID, Option<f32>), // Media ID and resume position

    // No-op message for task chaining
    Noop,
}

impl std::fmt::Debug for MediaMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Progress tracking
            Self::ProgressUpdateSent(id, pos, dur) => {
                write!(
                    f,
                    "Message::ProgressUpdateSent({:?}, {}, {})",
                    id, pos, dur
                )
            }
            Self::ProgressUpdateFailed => {
                write!(f, "Message::ProgressUpdateFailed")
            }
            Self::SendProgressUpdateWithData(id, pos, dur) => {
                write!(
                    f,
                    "Message::SendProgressUpdateWithData({:?}, {}, {})",
                    id, pos, dur
                )
            }
            Self::WatchProgressFetched(id, pos) => {
                write!(f, "Message::WatchProgressFetched({:?}, {:?})", id, pos)
            }

            // Internal
            Self::Noop => write!(f, "Message::Noop"),
        }
    }
}

impl MediaMessage {
    pub fn name(&self) -> &'static str {
        match self {
            // Watch progress tracking
            Self::ProgressUpdateSent(_, _, _) => "Media::ProgressUpdateSent",
            Self::ProgressUpdateFailed => "Media::ProgressUpdateFailed",

            Self::SendProgressUpdateWithData(_, _, _) => {
                "Media::SendProgressUpdateWithData"
            }
            Self::WatchProgressFetched(_, _) => "Media::WatchProgressFetched",

            // Internal
            Self::Noop => "Media::Noop",
        }
    }
}
