use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

use crate::{
    query::types::{MediaTypeFilter, SortBy, SortOrder},
    watch_status::WatchStatusFilter,
};

/// Legacy library filter payload (to be replaced by GraphQL)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryFilters {
    pub media_type: Option<String>,
    pub show_name: Option<String>,
    pub season: Option<u32>,
    pub order_by: Option<String>,
    pub limit: Option<u64>,
    pub library_id: Option<String>,
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

/// Compact response for index-based sorting/filtering
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[rkyv(derive(Debug, PartialEq, Eq))]
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
