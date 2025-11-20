//! Curated surface for UI/client crates.
//! Keep imports focused by pulling from this module rather than directly from
//! the entire crate.

pub use crate::api_scan::{ScanConfig, ScanMetrics};
pub use crate::api_types::player::*;
pub use crate::auth::device::{
    AuthenticatedDevice, DeviceRegistration, Platform, generate_trust_token,
};
pub use crate::query::prelude::*;
pub use crate::traits::prelude::*;
pub use crate::types::prelude::*;
pub use crate::watch_status::{
    InProgressItem, UpdateProgressRequest, UserWatchState, WatchProgress, WatchStatusFilter,
};

// Auth rewrite: re-export current auth/user surfaces, documenting where new
// device/auth abstractions will hook in once stabilized.
pub use crate::rbac::{Permission, Role, UserPermissions};
pub use crate::user::{
    AuthToken, GridSize, LoginRequest, PlaybackPreferences, PlaybackQuality, RegisterRequest,
    ResumeBehavior, SubtitlePreferences, ThemePreference, UiPreferences, User, UserPreferences,
};

pub use crate::types::media_events::{
    MediaEvent, ScanEventMetadata, ScanProgressEvent, ScanStageLatencySummary,
};
