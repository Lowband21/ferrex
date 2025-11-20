//! Playback/watch bounded context facade.

#[cfg(feature = "database")]
pub mod ports {
    pub use crate::database::ports::sync_sessions::SyncSessionsRepository;
    pub use crate::database::ports::watch_metrics::{ProgressEntry, WatchMetricsReadPort};
    pub use crate::database::ports::watch_status::WatchStatusRepository;
}

pub mod watch_status {
    pub use crate::watch_status::*;
}

pub mod sync_session {
    pub use crate::sync_session::*;
}
