use crate::{LibraryID, LibraryType, MediaID};
use crate::{LibraryReference, types::media::*};
use chrono::{DateTime, Utc};
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
    #[serde(default = "default_start_scan")]
    pub start_scan: bool,
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

fn default_start_scan() -> bool {
    true
}

// ===== Scan Control Types =====

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[serde(rename_all = "snake_case")]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub enum ScanLifecycleStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Canceled,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct ScanSnapshotDto {
    #[rkyv(with = crate::rkyv_wrappers::UuidWrapper)]
    pub scan_id: uuid::Uuid,
    pub library_id: LibraryID,
    pub status: ScanLifecycleStatus,
    pub completed_items: u64,
    pub total_items: u64,
    pub retrying_items: u64,
    pub dead_lettered_items: u64,
    #[rkyv(with = crate::rkyv_wrappers::UuidWrapper)]
    pub correlation_id: uuid::Uuid,
    pub idempotency_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_path: Option<String>,
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[rkyv(with = crate::rkyv_wrappers::OptionDateTime)]
    pub terminal_at: Option<DateTime<Utc>>,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveScansResponse {
    pub scans: Vec<ScanSnapshotDto>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestProgressResponse {
    pub scan_id: uuid::Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<ScanProgressEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartScanRequest {
    #[serde(default)]
    pub correlation_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanCommandRequest {
    pub scan_id: uuid::Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanCommandAcceptedResponse {
    pub scan_id: uuid::Uuid,
    pub correlation_id: uuid::Uuid,
}

// ===== Media Event Types for SSE =====

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ScanStageLatencySummary {
    pub scan: u64,
    pub analyze: u64,
    pub index: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgressEvent {
    pub version: String,
    pub scan_id: Uuid,
    pub library_id: LibraryID,
    pub status: String,
    pub completed_items: u64,
    pub total_items: u64,
    pub sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_key: Option<String>,
    pub p95_stage_latencies_ms: ScanStageLatencySummary,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub emitted_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrying_items: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_lettered_items: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanEventMetadata {
    pub version: String,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub library_id: LibraryID,
}

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
        scan_id: Uuid,
        metadata: ScanEventMetadata,
    },
    ScanProgress {
        scan_id: Uuid,
        progress: ScanProgressEvent,
    },
    ScanCompleted {
        scan_id: Uuid,
        metadata: ScanEventMetadata,
    },
    ScanFailed {
        scan_id: Uuid,
        error: String,
        metadata: ScanEventMetadata,
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

// ===== Index-based Sorting/Filtering Types =====

use crate::{
    query::types::{MediaTypeFilter, SortBy, SortOrder},
    watch_status::WatchStatusFilter,
};

/// Compact response for index-based sorting/filtering
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct IndicesResponse {
    /// Version of the library content used to compute indices (for cache/mismatch detection)
    pub content_version: u32,
    /// Positions into the client's archived media slice for the target library
    pub indices: Vec<u32>,
}

/// Inclusive range for scalar filters
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ScalarRange<T> {
    pub min: T,
    pub max: T,
}

impl<T> ScalarRange<T> {
    pub fn new(min: T, max: T) -> Self {
        Self { min, max }
    }
}

/// Ratings are stored as tenths to avoid floating point hashing/serialization issues.
pub type RatingValue = u16;

/// Scaling factor used when converting between float ratings and stored values.
pub const RATING_SCALE_FACTOR: RatingValue = 10;

/// BigDecimal scale that represents the `RATING_SCALE_FACTOR` when materializing for SQL.
pub const RATING_DECIMAL_SCALE: u64 = 1;

#[inline]
pub fn rating_value_from_f32(value: f32) -> RatingValue {
    let clamped = value.clamp(0.0, 10.0);
    (clamped * RATING_SCALE_FACTOR as f32).round() as RatingValue
}

#[inline]
pub fn rating_value_to_f32(value: RatingValue) -> f32 {
    value as f32 / RATING_SCALE_FACTOR as f32
}

impl ScalarRange<f32> {
    /// Convert a floating-point range into a scaled rating range (tenths of a point).
    pub fn to_rating_value(self) -> ScalarRange<RatingValue> {
        ScalarRange::new(
            rating_value_from_f32(self.min),
            rating_value_from_f32(self.max),
        )
    }
}

impl ScalarRange<RatingValue> {
    /// Convert a scaled rating range back into floating-point representation.
    pub fn to_f32(self) -> ScalarRange<f32> {
        ScalarRange::new(rating_value_to_f32(self.min), rating_value_to_f32(self.max))
    }
}

/// Request payload for index-based filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterIndicesRequest {
    /// Optional media type filter; Phase 1 supports Movie only
    pub media_type: Option<MediaTypeFilter>,
    /// Filter by genre names
    #[serde(default)]
    pub genres: Vec<String>,
    /// Filter by inclusive year range (release year)
    pub year_range: Option<ScalarRange<u16>>,
    /// Filter by inclusive rating range in tenths of a point (0-100 => 0.0-10.0)
    pub rating_range: Option<ScalarRange<RatingValue>>,
    /// Filter by inclusive resolution range (vertical pixels)
    pub resolution_range: Option<ScalarRange<u16>>,
    /// Optional watch status filter derived from authenticated user
    pub watch_status: Option<WatchStatusFilter>,
    /// Optional simple search text (applied to title/overview)
    pub search: Option<String>,
    /// Optional sort field (snake_case per SortBy serde)
    pub sort: Option<SortBy>,
    /// Optional sort order ("asc"/"desc")
    pub order: Option<SortOrder>,
}
