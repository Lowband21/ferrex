use crate::api_types::{MovieID, SeriesID};
use crate::media::*;
use crate::LibraryType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ===== Library API Types =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryMediaResponse {
    pub library: LibraryReference,
    pub media: Vec<MediaReference>, // Lightweight - MediaDetailsOption will be Endpoint variant
}

// Per-library media cache to store media references for each library
#[derive(Debug, Clone)]
pub enum LibraryMediaCache {
    Movies {
        references: Vec<MovieReference>,
    },
    TvShows {
        series_references: HashMap<Uuid, SeriesReference>,
        series_references_sorted: Vec<SeriesReference>,
        series_indices_sorted: Vec<String>,
        season_references: HashMap<Uuid, Vec<SeasonReference>>,
        episode_references: HashMap<Uuid, Vec<EpisodeReference>>,
    },
}

impl LibraryMediaCache {
    pub fn is_empty(&self) -> bool {
        match self {
            LibraryMediaCache::Movies { references } => references.is_empty(),
            LibraryMediaCache::TvShows {
                series_references, ..
            } => series_references.is_empty(),
        }
    }

    pub fn new(library_type: LibraryType) -> Self {
        match library_type {
            LibraryType::Movies => LibraryMediaCache::Movies {
                references: Vec::new(),
            },
            LibraryType::TvShows => LibraryMediaCache::TvShows {
                series_references: HashMap::new(),
                series_references_sorted: Vec::new(),
                series_indices_sorted: Vec::new(),
                season_references: HashMap::new(),
                episode_references: HashMap::new(),
            },
        }
    }
}

// ===== Media Fetch Types =====

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Copy)]
pub enum MediaId {
    Movie(MovieID),
    Series(SeriesID),
    Season(SeasonID),
    Episode(EpisodeID),
    Person(PersonID),
}

impl MediaId {
    pub fn as_str(&self) -> String {
        match &self {
            MediaId::Movie(movie_id) => movie_id.as_str(),
            MediaId::Series(series_id) => series_id.as_str(),
            MediaId::Season(season_id) => season_id.as_str(),
            MediaId::Episode(episode_id) => episode_id.as_str(),
            MediaId::Person(person_id) => person_id.as_str(),
        }
    }

    pub fn as_ref(&self) -> &Uuid {
        match &self {
            MediaId::Movie(movie_id) => movie_id.as_ref(),
            MediaId::Series(series_id) => series_id.as_ref(),
            MediaId::Season(season_id) => season_id.as_ref(),
            MediaId::Episode(episode_id) => episode_id.as_ref(),
            MediaId::Person(person_id) => person_id.as_ref(),
        }
    }

    pub fn as_uuid(&self) -> Uuid {
        match &self {
            MediaId::Movie(movie_id) => movie_id.as_uuid(),
            MediaId::Series(series_id) => series_id.as_uuid(),
            MediaId::Season(season_id) => season_id.as_uuid(),
            MediaId::Episode(episode_id) => episode_id.as_uuid(),
            MediaId::Person(person_id) => person_id.as_uuid(),
        }
    }

    pub fn sub_eq(&self, other: &MediaId) -> bool {
        match (self, other) {
            (MediaId::Movie(a), MediaId::Movie(b)) => a == b,
            (MediaId::Series(a), MediaId::Series(b)) => a == b,
            (MediaId::Season(a), MediaId::Season(b)) => a == b,
            (MediaId::Episode(a), MediaId::Episode(b)) => a == b,
            (MediaId::Person(a), MediaId::Person(b)) => a == b,
            _ => false,
        }
    }

    pub fn eq_movie(&self, other: &MovieID) -> bool {
        match (self, other) {
            (MediaId::Movie(MovieID(a)), MovieID(b)) => a == b,
            _ => false,
        }
    }
    pub fn eq_series(&self, other: &SeriesID) -> bool {
        match (self, other) {
            (MediaId::Series(SeriesID(a)), SeriesID(b)) => a == b,
            _ => false,
        }
    }
    pub fn eq_episode(&self, other: &EpisodeID) -> bool {
        match (self, other) {
            (MediaId::Episode(EpisodeID(a)), EpisodeID(b)) => a == b,
            _ => false,
        }
    }
}

impl std::fmt::Display for MediaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaId::Movie(id) => write!(f, "Movie({})", id.as_str()),
            MediaId::Series(id) => write!(f, "Series({})", id.as_str()),
            MediaId::Season(id) => write!(f, "Season({})", id.as_str()),
            MediaId::Episode(id) => write!(f, "Episode({})", id.as_str()),
            MediaId::Person(id) => write!(f, "Person({})", id.as_str()),
        }
    }
}

impl From<MovieID> for MediaId {
    fn from(id: MovieID) -> Self {
        MediaId::Movie(id)
    }
}

impl From<SeriesID> for MediaId {
    fn from(id: SeriesID) -> Self {
        MediaId::Series(id)
    }
}

impl From<SeasonID> for MediaId {
    fn from(id: SeasonID) -> Self {
        MediaId::Season(id)
    }
}

impl From<EpisodeID> for MediaId {
    fn from(id: EpisodeID) -> Self {
        MediaId::Episode(id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchMediaRequest {
    pub library_id: Uuid,
    pub media_id: MediaId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMediaRequest {
    pub library_id: Uuid,
    pub media_ids: Vec<MediaId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMediaResponse {
    pub items: Vec<MediaReference>,
    pub errors: Vec<(MediaId, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualMatchRequest {
    pub library_id: Uuid,
    pub media_id: MediaId,
    pub tmdb_id: u64,
}

// ===== Library Management Types =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLibraryRequest {
    pub name: String,
    pub library_type: LibraryType,
    pub paths: Vec<String>,
    #[serde(default = "default_scan_interval")]
    pub scan_interval_minutes: u32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLibraryRequest {
    pub name: Option<String>,
    pub paths: Option<Vec<String>>,
    pub scan_interval_minutes: Option<u32>,
    pub enabled: Option<bool>,
}

fn default_scan_interval() -> u32 {
    60
}

fn default_enabled() -> bool {
    true
}

// ===== Scan Types =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRequest {
    pub path: Option<String>,
    pub paths: Option<Vec<String>>,
    pub library_id: Option<Uuid>,
    pub library_type: Option<LibraryType>,
    pub max_depth: Option<usize>,
    pub follow_links: bool,
    pub extract_metadata: bool,
    pub force_rescan: bool,
    pub use_streaming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scan_id: String,
    pub status: ScanStatus,
    pub path: String,
    pub library_name: Option<String>,
    pub library_id: Option<String>,
    pub total_files: usize,
    pub scanned_files: usize,
    pub stored_files: usize,
    pub metadata_fetched: usize,
    pub skipped_samples: usize,
    pub errors: Vec<String>,
    pub current_file: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub estimated_time_remaining: Option<std::time::Duration>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanStatus {
    Pending,
    Scanning,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

// ===== Media Event Types for SSE =====

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaEvent {
    // New reference-based events
    MovieAdded {
        movie: MovieReference,
    },
    SeriesAdded {
        series: SeriesReference,
    },
    SeasonAdded {
        season: SeasonReference,
    },
    EpisodeAdded {
        episode: EpisodeReference,
    },

    // Update events
    MovieUpdated {
        movie: MovieReference,
    },
    SeriesUpdated {
        series: SeriesReference,
    },
    SeasonUpdated {
        season: SeasonReference,
    },
    EpisodeUpdated {
        episode: EpisodeReference,
    },

    // Delete events
    MediaDeleted {
        id: MediaId,
    },

    // Scan events
    ScanStarted {
        scan_id: String,
    },
    ScanProgress {
        scan_id: String,
        progress: ScanProgress,
    },
    ScanCompleted {
        scan_id: String,
    },
    ScanFailed {
        scan_id: String,
        error: String,
    },
}

// ===== Filter Types =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryFilters {
    pub media_type: Option<String>,
    pub show_name: Option<String>,
    pub season: Option<u32>,
    pub order_by: Option<String>,
    pub limit: Option<u64>,
    pub library_id: Option<String>,
}

// ===== Response Types =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            status: "success".to_string(),
            data: Some(data),
            error: None,
            message: None,
        }
    }

    pub fn error(error: String) -> Self {
        Self {
            status: "error".to_string(),
            data: None,
            error: Some(error),
            message: None,
        }
    }

    pub fn with_message(mut self, message: String) -> Self {
        self.message = Some(message);
        self
    }
}

// ===== Metadata Types =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataRequest {
    pub path: String,
}

// ===== Media Stats =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaStats {
    pub total_files: u64,
    pub total_size: u64,
    pub by_type: HashMap<String, u64>,
}
