use crate::infrastructure::MediaID;
use ferrex_core::MediaFile;


#[derive(Clone)]
pub enum Message {
    // Watch progress tracking
    ProgressUpdateSent(MediaID, f64, f64), // Position that was successfully sent to server
    ProgressUpdateFailed,                  // Failed to send progress update
    SendProgressUpdateWithData(MediaID, f64, f64), // position, duration - captures data at message creation time

    Noop,
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Progress tracking
            Self::ProgressUpdateSent(id, pos, dur) => {
                write!(f, "Message::ProgressUpdateSent({:?}, {}, {})", id, pos, dur)
            }
            Self::ProgressUpdateFailed => write!(f, "Message::ProgressUpdateFailed"),
            Self::SendProgressUpdateWithData(id, pos, dur) => {
                write!(
                    f,
                    "Message::SendProgressUpdateWithData({:?}, {}, {})",
                    id, pos, dur
                )
            }


            // Internal
            Self::Noop => write!(f, "Message::Noop"),
        }
    }
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            // Watch progress tracking
            Self::ProgressUpdateSent(_, _, _) => "Media::ProgressUpdateSent",
            Self::ProgressUpdateFailed => "Media::ProgressUpdateFailed",

            Self::SendProgressUpdateWithData(_, _, _) => "Media::SendProgressUpdateWithData",


            // Internal
            Self::Noop => "Media::Noop",
        }
    }
}
