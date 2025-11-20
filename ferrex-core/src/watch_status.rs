//! Media watch status and progress tracking
//!
//! This module provides functionality for tracking user viewing progress across
//! all media types (movies, episodes). It maintains watch history, completion
//! status, and enables "continue watching" features.
//!
//! ## Key Concepts
//!
//! - **In Progress**: Media items currently being watched (0% < progress < 95%)
//! - **Completed**: Media items watched to completion (progress >= 95%)
//! - **Watch State**: Combined view of in-progress and completed items
//!
//! ## Progress Tracking
//!
//! Progress is tracked as position/duration and automatically moves items
//! between in-progress and completed states based on viewing percentage.
//!
//! ## Example
//!
//! ```no_run
//! use ferrex_core::player_prelude::{
//!     MediaID, MediaIDLike, MovieID, UpdateProgressRequest, UserWatchState,
//! };
//!
//! let mut watch_state = UserWatchState::new();
//!
//! // Update progress for a movie
//! let movie = MediaID::Movie(MovieID::new());
//! let request = UpdateProgressRequest {
//!     media_id: movie.to_uuid(),
//!     media_type: movie.media_type(),
//!     position: 1800.0,  // 30 minutes
//!     duration: 7200.0,  // 2 hours
//! };
//!
//! watch_state.update_progress(request.media_id, request.position, request.duration);
//! ```

use crate::types::util_types::MediaType;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
};
use uuid::Uuid;

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
    /// Media to update progress for
    pub media_id: Uuid,
    /// Type of media (movie, series, season, episode)
    pub media_type: MediaType,
    /// Current playback position in seconds
    pub position: f32,
    /// Total media duration in seconds
    pub duration: f32,
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

    /// Check if this item is considered completed (>95%)
    pub fn is_completed(&self) -> bool {
        self.0 > 0.95
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
