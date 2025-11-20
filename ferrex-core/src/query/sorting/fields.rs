//! Field marker types for compile-time safe sorting
//!
//! These zero-sized types represent different fields that can be used for sorting.
//! Each field marker implements the SortFieldMarker trait to specify its key type.

use super::keys::{OptionalDateKey, OptionalFloatKey, OptionalU32Key, OptionalU64Key, StringKey};
use super::traits::SortFieldMarker;
use uuid::Uuid;

// Zero-sized field markers

/// Sort by title (alphabetical)
#[derive(Copy, Clone, Debug)]
pub struct TitleField;

impl SortFieldMarker for TitleField {
    type Key = StringKey;
    const ID: &'static str = "title";
    const REQUIRES_FETCH: bool = false;
}

/// Sort by date added to library
#[derive(Copy, Clone, Debug)]
pub struct DateAddedField;

impl SortFieldMarker for DateAddedField {
    type Key = OptionalDateKey;
    const ID: &'static str = "date_added";
    const REQUIRES_FETCH: bool = false;
}

/// Sort by release date (movies/episodes)
#[derive(Copy, Clone, Debug)]
pub struct ReleaseDateField;

impl SortFieldMarker for ReleaseDateField {
    type Key = OptionalDateKey;
    const ID: &'static str = "release_date";
    const REQUIRES_FETCH: bool = true; // Requires TMDB data
}

/// Sort by user rating (TMDB rating)
#[derive(Copy, Clone, Debug)]
pub struct RatingField;

impl SortFieldMarker for RatingField {
    type Key = OptionalFloatKey;
    const ID: &'static str = "rating";
    const REQUIRES_FETCH: bool = true; // Requires TMDB data
}

/// Sort by popularity (TMDB popularity score)
#[derive(Copy, Clone, Debug)]
pub struct PopularityField;

impl SortFieldMarker for PopularityField {
    type Key = OptionalFloatKey;
    const ID: &'static str = "popularity";
    const REQUIRES_FETCH: bool = true; // Requires TMDB data
}

/// Sort by runtime/duration
#[derive(Copy, Clone, Debug)]
pub struct RuntimeField;

impl SortFieldMarker for RuntimeField {
    type Key = OptionalU32Key;
    const ID: &'static str = "runtime";
    const REQUIRES_FETCH: bool = true; // May require TMDB or file analysis
}

/// Sort by last watched date for a specific user
#[derive(Copy, Clone, Debug)]
pub struct LastWatchedField(pub Uuid); // User context

impl SortFieldMarker for LastWatchedField {
    type Key = OptionalDateKey;
    const ID: &'static str = "last_watched";
    const REQUIRES_FETCH: bool = true; // Requires watch status data
}

/// Sort by watch progress for a specific user
#[derive(Copy, Clone, Debug)]
pub struct WatchProgressField(pub Uuid); // User context

impl SortFieldMarker for WatchProgressField {
    type Key = OptionalFloatKey; // Percentage 0.0-1.0
    const ID: &'static str = "watch_progress";
    const REQUIRES_FETCH: bool = true; // Requires watch status data
}

/// Sort by bitrate (bits per second)
#[derive(Copy, Clone, Debug)]
pub struct BitrateField;

impl SortFieldMarker for BitrateField {
    type Key = OptionalU64Key;
    const ID: &'static str = "bitrate";
    const REQUIRES_FETCH: bool = false; // Available in media file metadata
}

/// Sort by file size (in bytes)
#[derive(Copy, Clone, Debug)]
pub struct FileSizeField;

impl SortFieldMarker for FileSizeField {
    type Key = OptionalU64Key;
    const ID: &'static str = "file_size";
    const REQUIRES_FETCH: bool = false; // Available in media file
}

/// Sort by content rating (e.g., PG, PG-13, R)
#[derive(Copy, Clone, Debug)]
pub struct ContentRatingField;

impl SortFieldMarker for ContentRatingField {
    type Key = StringKey;
    const ID: &'static str = "content_rating";
    const REQUIRES_FETCH: bool = true; // Requires TMDB data
}

/// Sort by resolution (height in pixels)
#[derive(Copy, Clone, Debug)]
pub struct ResolutionField;

impl SortFieldMarker for ResolutionField {
    type Key = OptionalU32Key;
    const ID: &'static str = "resolution";
    const REQUIRES_FETCH: bool = false; // Available in media file metadata
}
