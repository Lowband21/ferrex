//! Query functionality for MediaStore
//!
//! This module provides MediaQuery execution capabilities for the MediaStore,
//! supporting filtering, sorting, and pagination.

use uuid::Uuid;

use crate::domains::media::store::sorting::MediaStoreSorting;
use crate::domains::media::store::MediaStore;
use crate::infrastructure::api_types::{MovieReference, SeriesReference};
use ferrex_core::query::MediaQuery;

/// Trait that adds MediaQuery execution capabilities to MediaStore
pub trait MediaStoreQuerying {
    /// Execute a MediaQuery to get filtered and sorted movies
    fn query_movies(&self, query: &MediaQuery, library_id: Option<Uuid>) -> Vec<MovieReference>;

    /// Execute a MediaQuery to get filtered and sorted series
    fn query_series(&self, query: &MediaQuery, library_id: Option<Uuid>) -> Vec<SeriesReference>;
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl MediaStoreQuerying for MediaStore {
    fn query_movies(&self, query: &MediaQuery, library_id: Option<Uuid>) -> Vec<MovieReference> {
        // Start with filtered movies
        let mut movies: Vec<MovieReference> =
            self.get_movies(library_id).into_iter().cloned().collect();

        // Apply filters (TODO: implement filtering in phase 3-6)
        // For now, just apply sorting

        // Convert MediaQuery sort to our strategy
        let strategy = Self::create_sort_strategy_from_query_for_movies(&query.sort);
        strategy.sort(&mut movies);

        // Apply pagination
        let start = query.pagination.offset;
        let limit = query.pagination.limit;

        movies.into_iter().skip(start).take(limit).collect()
    }

    fn query_series(&self, query: &MediaQuery, library_id: Option<Uuid>) -> Vec<SeriesReference> {
        // Start with filtered series
        let mut series: Vec<SeriesReference> =
            self.get_series(library_id).into_iter().cloned().collect();

        // Apply filters (TODO: implement filtering in phase 3-6)
        // For now, just apply sorting

        // Convert MediaQuery sort to our strategy
        let strategy = Self::create_sort_strategy_from_query_for_series(&query.sort);
        strategy.sort(&mut series);

        // Apply pagination
        let start = query.pagination.offset;
        let limit = query.pagination.limit;

        series.into_iter().skip(start).take(limit).collect()
    }
}
