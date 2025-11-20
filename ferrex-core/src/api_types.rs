use crate::{LibraryID, LibraryType, MediaID};
use crate::{LibraryReference, types::media::*};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ===== Library API Types =====

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct LibraryMediaResponse {
    pub library: LibraryReference,
    pub media: Vec<Media>, // Lightweight - MediaDetailsOption will be Endpoint variant
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
            LibraryType::Series => LibraryMediaCache::TvShows {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchMediaRequest {
    pub library_id: LibraryID,
    pub media_id: MediaID,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMediaRequest {
    pub library_id: LibraryID,
    pub media_ids: Vec<MediaID>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMediaResponse {
    pub items: Vec<Media>,
    pub errors: Vec<(MediaID, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualMatchRequest {
    pub library_id: LibraryID,
    pub media_id: MediaID,
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
        id: MediaID,
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

// ===== Image Data Types =====

/// Wrapper for image binary data to enable rkyv serialization
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ImageData {
    /// The actual image bytes (JPEG/PNG/WebP)
    pub bytes: Vec<u8>,
    /// Content type of the image
    pub content_type: String,
    /// Optional width hint
    pub width: Option<u32>,
    /// Optional height hint
    pub height: Option<u32>,
}
