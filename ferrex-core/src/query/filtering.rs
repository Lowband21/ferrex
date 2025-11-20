//! Shared helpers for translating UI filter state into backend-ready queries.
//!
//! # Schema Reference
//!
//! These helpers assume the following Postgres layout:
//!
//! - `movie_references` (`id`, `library_id`, `file_id`, `tmdb_id`, `title`, `theme_color`, `created_at`, `updated_at`)
//!   paired with `media_files` for technical metadata such as bitrate and resolution.
//! - `movie_metadata` (`movie_id`, `release_date`, `vote_average`, `runtime`, `popularity`, `overview`, etc.)
//!   provides core numeric fields, while genre or cast filters are resolved via the
//!   normalized `movie_genres`/`movie_cast` join tables.
//! - `series_references`/`series_metadata` mirror the same structure for TV support with
//!   per-season data in `season_references` and related join tables for genres, credits,
//!   and popularity metrics.
//! - `user_watch_progress` (`user_id`, `media_uuid`, `media_type`, `position`, `duration`, `last_watched`, `updated_at`)
//!   holds in-progress playback, while `user_completed_media` stores completed entries. Both tables index
//!   `(user_id, media_uuid)` for fast joins when applying watch-status filters or sort keys such as last watched.
//!
//! The utilities in this module normalize UI inputs into consistent scalar ranges so both the
//! desktop player and the server can build SQL predicates using the same expectations.

use crate::{
    UiDecade, UiResolution, UiWatchStatus,
    api_types::{FilterIndicesRequest, ScalarRange},
    query::types::{MediaTypeFilter, SortBy, SortOrder},
    watch_status::WatchStatusFilter,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Parameters collected from UI or other client state to build a `FilterIndicesRequest`.
#[derive(Debug, Clone)]
pub struct FilterRequestParams<'a> {
    pub media_type: Option<MediaTypeFilter>,
    pub genres: &'a [String],
    pub decade: Option<UiDecade>,
    pub explicit_year_range: Option<ScalarRange<u16>>,
    pub rating: Option<ScalarRange<f32>>,
    pub resolution: UiResolution,
    pub watch_status: UiWatchStatus,
    pub search: Option<&'a str>,
    pub sort: SortBy,
    pub order: SortOrder,
}

impl<'a> FilterRequestParams<'a> {
    pub fn into_request(self) -> FilterIndicesRequest {
        let mut genres: Vec<String> = self.genres.iter().cloned().collect();
        // Deduplicate to keep SQL arrays small
        genres.sort_unstable();
        genres.dedup();

        let search = self.search.and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        FilterIndicesRequest {
            media_type: self.media_type,
            genres,
            year_range: self
                .explicit_year_range
                .or_else(|| self.decade.map(decade_to_year_range)),
            rating_range: self.rating,
            resolution_range: resolution_to_range(self.resolution),
            watch_status: watch_status_to_filter(self.watch_status),
            search,
            sort: Some(self.sort),
            order: Some(self.order),
        }
    }
}

/// Convenience builder when the caller prefers a functional style.
pub fn build_filter_indices_request(params: FilterRequestParams<'_>) -> FilterIndicesRequest {
    params.into_request()
}

/// Map a decade enum to an inclusive year range.
pub fn decade_to_year_range(decade: UiDecade) -> ScalarRange<u16> {
    let start = decade.start_year();
    ScalarRange::new(start, start.saturating_add(9))
}

/// Convert UI resolution buckets into inclusive pixel ranges.
pub fn resolution_to_range(resolution: UiResolution) -> Option<ScalarRange<u16>> {
    use UiResolution::*;
    match resolution {
        Any => None,
        SD => Some(ScalarRange::new(0, 576)),
        HD720 => Some(ScalarRange::new(577, 960)),
        FHD1080 => Some(ScalarRange::new(961, 1344)),
        QHD1440 => Some(ScalarRange::new(1345, 1792)),
        UHD4K => Some(ScalarRange::new(1793, 2800)),
        UHD8K => Some(ScalarRange::new(2801, u16::MAX)),
    }
}

/// Map UI watch status to backend filter variant.
pub fn watch_status_to_filter(status: UiWatchStatus) -> Option<WatchStatusFilter> {
    use UiWatchStatus::*;
    match status {
        Any => None,
        Unwatched => Some(WatchStatusFilter::Unwatched),
        InProgress => Some(WatchStatusFilter::InProgress),
        Completed => Some(WatchStatusFilter::Completed),
    }
}

/// Compute a stable hash for a filter specification
pub fn hash_filter_spec(spec: &FilterIndicesRequest) -> u64 {
    let mut hasher = DefaultHasher::new();
    spec.media_type.hash(&mut hasher);

    let mut genres = spec.genres.clone();
    genres.sort();
    genres.dedup();
    genres.hash(&mut hasher);

    spec.year_range.hash(&mut hasher);
    spec.rating_range.hash(&mut hasher);
    spec.resolution_range.hash(&mut hasher);
    spec.watch_status.hash(&mut hasher);

    match spec.search.as_ref() {
        Some(search) => search.trim().to_lowercase().hash(&mut hasher),
        None => ().hash(&mut hasher),
    }

    spec.sort.hash(&mut hasher);
    spec.order.hash(&mut hasher);

    hasher.finish()
}
