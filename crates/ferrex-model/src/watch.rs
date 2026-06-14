use uuid::Uuid;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Episode identity independent of files
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EpisodeKey {
    pub tmdb_series_id: u64,
    pub season_number: u16,
    pub episode_number: u16,
}

/// Season identity independent of files
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SeasonKey {
    pub tmdb_series_id: u64,
    pub season_number: u16,
}

/// Reason chosen for the next episode pick
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum NextReason {
    ResumeInProgress,
    FirstUnwatched,
}

/// Description of the next episode to play
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NextEpisode {
    pub key: EpisodeKey,
    /// If available, a specific playable episode reference (media UUID)
    pub playable_media_id: Option<Uuid>,
    pub reason: NextReason,
}

/// Per-episode watch status
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "state", rename_all = "snake_case"))]
pub enum EpisodeStatus {
    Unwatched,
    InProgress { progress: f32 },
    Completed,
}

impl EpisodeStatus {
    pub fn is_completed(&self) -> bool {
        matches!(self, EpisodeStatus::Completed)
    }
}

/// Aggregated season watch status
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SeasonWatchStatus {
    pub key: SeasonKey,
    pub total: u32,
    pub watched: u32,
    pub in_progress: u32,
    pub is_completed: bool,
    /// Map of episode number -> status
    pub episodes: std::collections::HashMap<u16, EpisodeStatus>,
}

/// Aggregated series watch status with next-episode hint
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SeriesWatchStatus {
    pub tmdb_series_id: u64,
    pub total_episodes: u32,
    pub watched: u32,
    pub in_progress: u32,
    pub seasons: std::collections::HashMap<u16, SeasonWatchStatus>,
    pub next_episode: Option<NextEpisode>,
}
