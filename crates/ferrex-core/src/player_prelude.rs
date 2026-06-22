//! Curated surface for UI/client crates.
//!
//! Player crates import this module instead of reaching across the full core,
//! model, query, and contract trees. The surface intentionally includes API DTOs,
//! media/library/watch-state models, query helpers, auth/user preferences, and
//! trait contracts that are stable enough for client-facing code.

pub use crate::api::types::player::*;
pub use crate::api::types::{
    ActiveScansResponse, LatestProgressResponse, ScanCommandAcceptedResponse,
    ScanCommandRequest, ScanLifecycleStatus, ScanRunMode, ScanSnapshotDto,
    ScanStartDisposition, StartScanRequest,
};
pub use crate::api::{ScanConfig, ScanMetrics};
pub use crate::domain::theater_plate::*;
pub use crate::domain::users::auth::device::{
    AuthenticatedDevice, DeviceRegistration, Platform,
};
pub use crate::domain::watch::{
    InProgressItem, UpdateProgressRequest, UserWatchState, WatchProgress,
    WatchStatusFilter,
};
#[cfg(feature = "rkyv")]
pub use crate::infra::archive::ArchivedModel;
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

pub use ferrex_model::{
    MediaEvent, ScanEventMetadata, ScanPathReasonCategory,
    ScanPathReasonDetail, ScanProgressEvent, ScanStageLatencySummary,
};
