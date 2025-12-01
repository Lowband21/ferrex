//! Curated surface for UI/client crates.
//! Keep imports focused by pulling from this module rather than directly from
//! the entire crate.

pub use crate::api::types::player::*;
pub use crate::api::{ScanConfig, ScanMetrics};
pub use crate::domain::users::auth::device::{
    AuthenticatedDevice, DeviceRegistration, Platform,
};
pub use crate::domain::watch::{
    InProgressItem, UpdateProgressRequest, UserWatchState, WatchProgress,
    WatchStatusFilter,
};
#[cfg(feature = "rkyv")]
pub use crate::infrastructure::archive::ArchivedModel;
pub use crate::query::prelude::*;
pub use crate::traits::prelude::*;
pub use crate::types::prelude::*;
pub use crate::types::watch::{
    EpisodeKey, EpisodeStatus, NextEpisode, NextReason, SeasonKey,
    SeasonWatchStatus, SeriesWatchStatus,
};

// Auth rewrite: re-export current auth/user surfaces, documenting where new
// device/auth abstractions will hook in once stabilized.
pub use crate::domain::users::rbac::{Permission, Role, UserPermissions};
pub use crate::domain::users::user::{
    AuthToken, LoginRequest, PlaybackPreferences, PlaybackQuality,
    RegisterRequest, ResumeBehavior, SubtitlePreferences, ThemePreference,
    UiPreferences, User, UserPreferences, UserScale,
};

pub use crate::types::media_events::{
    MediaEvent, ScanEventMetadata, ScanProgressEvent, ScanStageLatencySummary,
};
