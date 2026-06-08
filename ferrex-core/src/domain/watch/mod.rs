//! Watch-state domain logic for tracking per-user media progress.
//!
//! Consolidates the previous `watch_status` root module under the domain layer
//! so downstream crates can import via `crate::domain::watch::*` while legacy
//! paths continue to work through compatibility shims.

// Re-export identity types from model for convenience
pub use crate::types::watch::{
    EpisodeKey, EpisodeStatus, NextEpisode, NextReason, SeasonKey,
    SeasonWatchStatus, SeriesWatchStatus,
};
use ferrex_model::{MediaID, VideoMediaType};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
};
use uuid::Uuid;

pub const FERREX_WATCH_COMPLETION_THRESHOLD_ENV: &str =
    "FERREX_WATCH_COMPLETION_THRESHOLD";
pub const FERREX_WATCH_RESUME_MIN_POSITION_SECONDS_ENV: &str =
    "FERREX_WATCH_RESUME_MIN_POSITION_SECONDS";
pub const FERREX_WATCH_RESUME_MIN_PROGRESS_RATIO_ENV: &str =
    "FERREX_WATCH_RESUME_MIN_PROGRESS_RATIO";
pub const FERREX_WATCH_RESUME_MIN_REMAINING_SECONDS_ENV: &str =
    "FERREX_WATCH_RESUME_MIN_REMAINING_SECONDS";

/// Default completion threshold used by shared watch-state helpers.
pub const DEFAULT_COMPLETION_THRESHOLD: f32 = 0.95;
/// Minimum playback position before a partially watched item is resumable.
pub const DEFAULT_RESUME_MIN_POSITION_SECONDS: f32 = 30.0;
/// Minimum playback ratio before a partially watched item is resumable.
pub const DEFAULT_RESUME_MIN_PROGRESS_RATIO: f32 = 0.02;
/// Minimum remaining runtime required to show a resume action.
pub const DEFAULT_RESUME_MIN_REMAINING_SECONDS: f32 = 60.0;

/// Server-configurable watch-state thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WatchResumePolicy {
    pub completion_threshold: f32,
    pub resume_min_position_seconds: f32,
    pub resume_min_progress_ratio: f32,
    pub resume_min_remaining_seconds: f32,
}

impl WatchResumePolicy {
    pub const fn default_values() -> Self {
        Self {
            completion_threshold: DEFAULT_COMPLETION_THRESHOLD,
            resume_min_position_seconds: DEFAULT_RESUME_MIN_POSITION_SECONDS,
            resume_min_progress_ratio: DEFAULT_RESUME_MIN_PROGRESS_RATIO,
            resume_min_remaining_seconds: DEFAULT_RESUME_MIN_REMAINING_SECONDS,
        }
    }

    pub fn from_env() -> Self {
        let defaults = Self::default_values();
        Self {
            completion_threshold: parse_watch_threshold_env(
                FERREX_WATCH_COMPLETION_THRESHOLD_ENV,
                defaults.completion_threshold,
            )
            .clamp(0.0, 1.0),
            resume_min_position_seconds: parse_watch_threshold_env(
                FERREX_WATCH_RESUME_MIN_POSITION_SECONDS_ENV,
                defaults.resume_min_position_seconds,
            ),
            resume_min_progress_ratio: parse_watch_threshold_env(
                FERREX_WATCH_RESUME_MIN_PROGRESS_RATIO_ENV,
                defaults.resume_min_progress_ratio,
            )
            .clamp(0.0, 1.0),
            resume_min_remaining_seconds: parse_watch_threshold_env(
                FERREX_WATCH_RESUME_MIN_REMAINING_SECONDS_ENV,
                defaults.resume_min_remaining_seconds,
            ),
        }
    }

    pub fn is_completed_progress(&self, position: f32, duration: f32) -> bool {
        duration > 0.0 && position / duration >= self.completion_threshold
    }

    pub fn is_resume_eligible(&self, position: f32, duration: f32) -> bool {
        if duration <= 0.0 || position < self.resume_min_position_seconds {
            return false;
        }

        if self.is_completed_progress(position, duration) {
            return false;
        }

        let progress = position / duration;
        let remaining = (duration - position).max(0.0);

        progress >= self.resume_min_progress_ratio
            && remaining >= self.resume_min_remaining_seconds
    }
}

impl Default for WatchResumePolicy {
    fn default() -> Self {
        Self::default_values()
    }
}

fn parse_watch_threshold_env(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(default)
}

/// User's complete watch state across all media
///
/// Maintains two collections:
/// - `in_progress`: Active items being watched (limited to ~50 items)
/// - `completed`: Set of completed media for efficient lookup
///
/// The system automatically moves items between states based on
/// viewing progress (95% threshold for completion).
#[derive(Debug, Clone)]
pub struct UserWatchState {
    /// List of actively watching items (typically 10-50 items)
    ///
    /// Ordered by last_watched timestamp (most recent first)
    pub in_progress: HashMap<Uuid, InProgressItem>,

    /// Set of completed media IDs for efficient "watched" badge display
    ///
    /// Uses HashSet for O(1) lookup performance
    pub completed: HashSet<Uuid>,
}

// Custom serialization to handle HashMap with MediaID keys
impl Serialize for UserWatchState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;

        // Convert HashMap<MediaID, InProgressItem> to Vec<&InProgressItem> for serialization
        let in_progress_vec: Vec<&InProgressItem> =
            self.in_progress.values().collect();

        let mut state = serializer.serialize_struct("UserWatchState", 2)?;
        state.serialize_field("in_progress", &in_progress_vec)?;
        state.serialize_field("completed", &self.completed)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for UserWatchState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Debug, Deserialize)]
        struct UserWatchStateHelper {
            in_progress: Vec<InProgressItem>,
            completed: Vec<Uuid>,
        }

        let helper = UserWatchStateHelper::deserialize(deserializer)?;

        let mut in_progress_map = HashMap::new();
        for item in helper.in_progress {
            in_progress_map.insert(item.media_id, item);
        }

        Ok(UserWatchState {
            in_progress: in_progress_map,
            completed: helper.completed.into_iter().collect(),
        })
    }
}

impl UserWatchState {
    pub fn get_watch_progress(&self, media_id: &Uuid) -> Option<WatchProgress> {
        if self.completed.contains(media_id) {
            Some(WatchProgress::new(1.0))
        } else if let Some(item) = self.get_by_media_id(media_id) {
            Some(item.to_watch_progress())
        } else {
            Some(WatchProgress::new(0.0))
        }
    }

    pub fn get_by_media_id(&self, media_id: &Uuid) -> Option<&InProgressItem> {
        self.in_progress.get(media_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub enum ItemWatchStatus {
    InProgress(InProgressItem),
    Completed(CompletedItem),
}

impl ItemWatchStatus {
    pub fn to_watch_progress(self) -> WatchProgress {
        match self {
            ItemWatchStatus::InProgress(in_progress_item) => {
                in_progress_item.to_watch_progress()
            }
            ItemWatchStatus::Completed(completed_item) => {
                completed_item.to_watch_progress()
            }
        }
    }
}

/// Item currently being watched
///
/// Represents a single media item with viewing progress.
/// Automatically removed when progress reaches 95%.
///
/// # Example
///
/// ```json
/// {
///   "media_id": "movie:550e8400-e29b-41d4-a716-446655440000",
///   "position": 3600.0,
///   "duration": 7200.0,
///   "last_watched": 1704067200
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InProgressItem {
    /// The media being watched
    pub media_id: Uuid,
    /// Current playback position in seconds
    pub position: f32,
    /// Total duration in seconds
    pub duration: f32,
    /// Unix timestamp of last update
    pub last_watched: i64,
}

impl Eq for InProgressItem {}

impl PartialEq for InProgressItem {
    fn eq(&self, other: &Self) -> bool {
        self.media_id == other.media_id
    }
}

impl Hash for InProgressItem {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.media_id.hash(state);
    }
}

impl InProgressItem {
    pub fn to_watch_progress(&self) -> WatchProgress {
        WatchProgress::from(self)
    }
}

/// Hint describing the primary action for a continue-watching card.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContinueWatchingActionHint {
    Resume,
    NextEpisode,
}

/// Playback target clients should invoke for a continue-watching card.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ContinueWatchingActionTarget {
    pub media_id: Uuid,
    pub media_type: VideoMediaType,
}

/// Display/action-ready continue-watching row returned by JSON APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinueWatchingItem {
    /// Backwards-compatible primary playback target for existing JSON clients.
    pub media_id: Uuid,
    /// Media type of the card itself.
    pub media_type: VideoMediaType,
    /// Logical movie/series item represented by the card.
    pub card_media_id: Uuid,
    /// Explicit action target for deterministic client recovery/resume flows.
    pub action_target: ContinueWatchingActionTarget,
    /// Hint describing the primary card action.
    pub action_hint: ContinueWatchingActionHint,
    /// Current playback position in seconds for the action target.
    pub position: f32,
    /// Total runtime in seconds for the action target.
    pub duration: f32,
    /// Unix timestamp of the last meaningful watch activity.
    pub last_watched: i64,
    /// Denormalized display title for the card.
    pub title: String,
    /// Optional subtitle / episode label for rendering resume intent.
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Optional poster image iid for the card.
    #[serde(default)]
    pub poster_iid: Option<Uuid>,
}

/// Watched item
///
/// Represents a single completed media item.
/// # Example
///
/// ```json
/// {
///   "media_id": "movie:550e8400-e29b-41d4-a716-446655440000",
///   "last_watched": 1704067200
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedItem {
    /// The media being watched
    pub media_id: MediaID,
    /// Unix timestamp of last update
    pub last_watched: i64,
}

impl Eq for CompletedItem {}

impl PartialEq for CompletedItem {
    fn eq(&self, other: &Self) -> bool {
        self.media_id == other.media_id
    }
}

impl Hash for CompletedItem {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.media_id.hash(state);
    }
}

impl CompletedItem {
    pub fn to_watch_progress(&self) -> WatchProgress {
        WatchProgress::from(self)
    }
}

/// Filter for watch status queries
///
/// Used to filter media by watch status in query operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum WatchStatusFilter {
    /// Media never watched by the user
    Unwatched,
    /// Media currently being watched (0% < progress < 95%)
    InProgress,
    /// Media watched to completion (progress >= 95%)
    Completed,
    /// Media watched within the specified number of days
    RecentlyWatched {
        /// Number of days to look back
        days: u32,
    },
}

/// Progress update request
///
/// Sent by clients to update viewing progress. Progress updates
/// are typically sent every 10-30 seconds during playback.
///
/// # Validation
///
/// - `position` must be >= 0
/// - `duration` must be > 0
/// - `position` should not exceed `duration`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProgressRequest {
    /// Media to update progress for (required for movies; for episodes this is the EpisodeReference id)
    pub media_id: Uuid,
    /// Type of media (movie, series, season, episode)
    pub media_type: VideoMediaType,
    /// Current playback position in seconds
    pub position: f32,
    /// Total media duration in seconds
    pub duration: f32,
    /// Optional identity locator for episodes to update identity-based state
    #[serde(default)]
    pub episode: Option<EpisodeKey>,
    /// Optional hint of the specific media UUID used for playback (useful for identity rows)
    #[serde(default)]
    pub last_media_uuid: Option<Uuid>,
}

/// Watch progress percentage
#[derive(Debug, Clone, Copy)]
pub struct WatchProgress(f32);

impl WatchProgress {
    /// Create a new watch progress, clamping between 0.0 and 1.0
    pub fn new(progress: f32) -> Self {
        WatchProgress(progress.clamp(0.0, 1.0))
    }

    /// Get the progress as a percentage (0.0 to 1.0)
    pub fn as_percentage(&self) -> f32 {
        self.0
    }

    /// Check if this item is considered completed (>=95%).
    pub fn is_completed(&self) -> bool {
        self.0 >= DEFAULT_COMPLETION_THRESHOLD
    }

    /// Check if this item has been started
    pub fn is_started(&self) -> bool {
        self.0 > 0.0
    }
}

impl From<&InProgressItem> for WatchProgress {
    fn from(item: &InProgressItem) -> Self {
        WatchProgress::new(item.position / item.duration)
    }
}

impl From<&CompletedItem> for WatchProgress {
    fn from(_: &CompletedItem) -> Self {
        WatchProgress::new(1.0)
    }
}

impl UserWatchState {
    /// Create a new empty watch state
    pub fn new() -> Self {
        Self {
            in_progress: HashMap::new(),
            completed: HashSet::new(),
        }
    }

    /// Update progress for a media item
    pub fn update_progress(
        &mut self,
        media_id: Uuid,
        position: f32,
        duration: f32,
    ) -> InProgressItem {
        let progress = WatchProgress::new(position / duration);
        let progress_item = InProgressItem {
            media_id,
            position,
            duration,
            last_watched: chrono::Utc::now().timestamp(),
        };

        if progress.is_completed() {
            // Move to completed
            self.in_progress.retain(|k, _| k != &media_id);
            self.completed.insert(media_id);
        } else if progress.is_started() {
            // Update or insert in progress
            if let Some(item) = self.in_progress.get_mut(&media_id) {
                item.position = position;
                item.last_watched = chrono::Utc::now().timestamp();
            } else {
                self.in_progress.insert(media_id, progress_item.clone()); // TODO: Clone
            }
        }
        progress_item
    }

    /// Check if a media item is completed
    pub fn is_completed(&self, media_id: &Uuid) -> bool {
        self.completed.contains(media_id)
    }

    /// Get progress for a specific media item
    pub fn get_progress(&self, media_id: &Uuid) -> Option<WatchProgress> {
        self.in_progress
            .get(media_id)
            .map(|item| WatchProgress::new(item.position / item.duration))
    }

    /// Get continue watching items (sorted by last watched)
    pub fn get_continue_watching(
        self,
        _limit: usize,
    ) -> HashMap<Uuid, InProgressItem> {
        self.in_progress
        //let mut items: Vec<InProgressItem> = self.in_progress.values().cloned().collect();
        //items.sort_by(|a, b| b.last_watched.cmp(&a.last_watched));
        //items.truncate(limit);
        //items
    }

    /// Clear watch progress for a specific item
    pub fn clear_progress(&mut self, media_id: &Uuid) {
        self.in_progress.remove(media_id);
        self.completed.remove(media_id);
    }
}

impl Default for UserWatchState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_resume_policy_has_exact_server_defaults() {
        assert_eq!(
            WatchResumePolicy::default(),
            WatchResumePolicy {
                completion_threshold: 0.95,
                resume_min_position_seconds: 30.0,
                resume_min_progress_ratio: 0.02,
                resume_min_remaining_seconds: 60.0,
            }
        );
    }

    #[test]
    fn watch_progress_below_threshold_is_not_completed() {
        assert!(!WatchProgress::new(0.949).is_completed());
    }

    #[test]
    fn watch_progress_marks_exact_threshold_as_completed() {
        assert!(
            WatchProgress::new(DEFAULT_COMPLETION_THRESHOLD).is_completed()
        );
    }

    #[test]
    fn watch_progress_negative_sentinel_is_not_completed() {
        assert!(!WatchProgress::new(-1.0).is_completed());
    }

    #[test]
    fn resume_policy_uses_exact_default_eligibility_boundaries() {
        let policy = WatchResumePolicy::default();

        assert!(policy.is_resume_eligible(30.0, 1_500.0));
        assert!(!policy.is_resume_eligible(29.9, 1_500.0));
        assert!(!policy.is_resume_eligible(30.0, 1_600.0));
        assert!(!policy.is_resume_eligible(1_440.0, 1_500.0));
    }

    #[test]
    fn update_watch_progress_moves_exact_threshold_item_to_completed() {
        let media_id = Uuid::new_v4();
        let mut state = UserWatchState::new();

        state.update_progress(media_id, 95.0, 100.0);

        assert!(state.completed.contains(&media_id));
        assert!(!state.in_progress.contains_key(&media_id));
    }
}
