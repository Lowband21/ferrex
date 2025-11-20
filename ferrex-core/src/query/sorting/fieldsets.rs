//! Field set definitions for different media types
//!
//! These types define which sort fields are available for each media type,
//! providing compile-time verification of field validity.

use super::fields::*;
use super::traits::{HasField, SortFieldSet};

/// Field set for movies with full metadata
pub struct MovieFieldSet;

impl SortFieldSet for MovieFieldSet {}

// Movies support all standard fields
impl HasField<TitleField> for MovieFieldSet {}
impl HasField<DateAddedField> for MovieFieldSet {}
impl HasField<ReleaseDateField> for MovieFieldSet {}
impl HasField<RatingField> for MovieFieldSet {}
impl HasField<PopularityField> for MovieFieldSet {}
impl HasField<RuntimeField> for MovieFieldSet {}
impl HasField<LastWatchedField> for MovieFieldSet {}
impl HasField<WatchProgressField> for MovieFieldSet {}

/// Field set for TV series with full metadata
pub struct SeriesFieldSet;

impl SortFieldSet for SeriesFieldSet {}

// Series support most fields except runtime (which is per-episode)
impl HasField<TitleField> for SeriesFieldSet {}
impl HasField<DateAddedField> for SeriesFieldSet {}
impl HasField<ReleaseDateField> for SeriesFieldSet {} // First air date
impl HasField<RatingField> for SeriesFieldSet {}
impl HasField<PopularityField> for SeriesFieldSet {}
impl HasField<LastWatchedField> for SeriesFieldSet {}
impl HasField<WatchProgressField> for SeriesFieldSet {} // Overall series progress

/// Field set for episodes
pub struct EpisodeFieldSet;

impl SortFieldSet for EpisodeFieldSet {}

// Episodes support all fields
impl HasField<TitleField> for EpisodeFieldSet {}
impl HasField<DateAddedField> for EpisodeFieldSet {}
impl HasField<ReleaseDateField> for EpisodeFieldSet {} // Air date
impl HasField<RatingField> for EpisodeFieldSet {}
impl HasField<RuntimeField> for EpisodeFieldSet {}
impl HasField<LastWatchedField> for EpisodeFieldSet {}
impl HasField<WatchProgressField> for EpisodeFieldSet {}
// Note: Episodes don't have popularity field typically

/// Field set for seasons
pub struct SeasonFieldSet;

impl SortFieldSet for SeasonFieldSet {}

// Seasons have limited fields
impl HasField<TitleField> for SeasonFieldSet {} // "Season 1", etc.
impl HasField<DateAddedField> for SeasonFieldSet {}
impl HasField<ReleaseDateField> for SeasonFieldSet {} // First episode air date
impl HasField<LastWatchedField> for SeasonFieldSet {}
impl HasField<WatchProgressField> for SeasonFieldSet {} // Season completion

/// Basic field set for media without TMDB metadata
///
/// This is used when metadata hasn't been fetched yet or isn't available.
/// Only supports fields that can be derived from file information.
pub struct BasicMediaFieldSet;

impl SortFieldSet for BasicMediaFieldSet {}

// Only fields available from file system
impl HasField<TitleField> for BasicMediaFieldSet {} // From filename
impl HasField<DateAddedField> for BasicMediaFieldSet {} // File creation/scan time
                                                        // Note: Does NOT implement HasField for TMDB-dependent fields like Rating, Popularity, etc.

/// Field set for mixed media collections
///
/// Used when sorting heterogeneous collections (movies + series + episodes).
/// Only supports fields common to all media types.
pub struct MixedMediaFieldSet;

impl SortFieldSet for MixedMediaFieldSet {}

// Only universally available fields
impl HasField<TitleField> for MixedMediaFieldSet {}
impl HasField<DateAddedField> for MixedMediaFieldSet {}
impl HasField<LastWatchedField> for MixedMediaFieldSet {}
impl HasField<WatchProgressField> for MixedMediaFieldSet {}
