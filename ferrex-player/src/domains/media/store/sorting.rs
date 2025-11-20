//! Sorting functionality for MediaStore
//!
//! This module provides sorting capabilities for the MediaStore,
//! keeping the main store implementation focused on core functionality.

use std::collections::HashMap;
use uuid::Uuid;

use crate::infrastructure::api_types::{MediaId, MediaReference, MovieReference, SeriesReference};
use crate::domains::media::store::{MediaStore, MediaType};
use ferrex_core::query::sorting::{
    SortStrategy, FieldSort, TitleField, DateAddedField, ReleaseDateField, RatingField,
    RuntimeField, PopularityField
};
use ferrex_core::query::{SortCriteria, SortField, SortOrder as QuerySortOrder};
use crate::domains::ui::SortBy;

/// Trait that adds sorting capabilities to MediaStore
pub trait MediaStoreSorting {
    /// Sort movies in-place using the provided strategy
    fn sort_movies<S: SortStrategy<MovieReference>>(&mut self, strategy: S);
    
    /// Get sorted movies without mutation (clones)
    fn get_sorted_movies<S: SortStrategy<MovieReference>>(
        &self, 
        library_id: Option<Uuid>,
        strategy: S
    ) -> Vec<MovieReference>;
    
    /// Sort series in-place using the provided strategy
    fn sort_series<S: SortStrategy<SeriesReference>>(&mut self, strategy: S);
    
    /// Get sorted series without mutation (clones)
    fn get_sorted_series<S: SortStrategy<SeriesReference>>(
        &self,
        library_id: Option<Uuid>,
        strategy: S
    ) -> Vec<SeriesReference>;
    
    /// Create a sort strategy for the current SortBy setting from UI state
    fn create_sort_strategy_for_movies(
        sort_by: SortBy,
        ascending: bool
    ) -> Box<dyn SortStrategy<MovieReference>>;
    
    /// Create a sort strategy for the current SortBy setting from UI state  
    fn create_sort_strategy_for_series(
        sort_by: SortBy,
        ascending: bool
    ) -> Box<dyn SortStrategy<SeriesReference>>;
    
    /// Convert MediaQuery SortCriteria to a sort strategy for movies
    fn create_sort_strategy_from_query_for_movies(
        sort: &SortCriteria
    ) -> Box<dyn SortStrategy<MovieReference>>;
    
    /// Convert MediaQuery SortCriteria to a sort strategy for series
    fn create_sort_strategy_from_query_for_series(
        sort: &SortCriteria
    ) -> Box<dyn SortStrategy<SeriesReference>>;
    
    /// Convert UI SortBy to MediaQuery SortField
    fn ui_sort_to_query_field(sort_by: SortBy) -> SortField;
}

impl MediaStoreSorting for MediaStore {
    fn sort_movies<S: SortStrategy<MovieReference>>(&mut self, strategy: S) {
        // This is a legacy method that doesn't fit well with our new sorted indices approach
        // For now, we'll just update the sorted indices based on default sort
        // In the future, this method should be deprecated in favor of set_movie_sort()
        self.update_sorted_movie_ids();
    }

    fn get_sorted_movies<S: SortStrategy<MovieReference>>(
        &self, 
        library_id: Option<Uuid>,
        strategy: S
    ) -> Vec<MovieReference> {
        let mut movies: Vec<MovieReference> = self.get_movies(library_id)
            .into_iter()
            .cloned()
            .collect();
        
        strategy.sort(&mut movies);
        movies
    }

    fn sort_series<S: SortStrategy<SeriesReference>>(&mut self, strategy: S) {
        // This is a legacy method that doesn't fit well with our new sorted indices approach
        // For now, we'll just update the sorted indices based on default sort
        // In the future, this method should be deprecated in favor of set_series_sort()
        self.update_sorted_series_ids();
    }

    fn get_sorted_series<S: SortStrategy<SeriesReference>>(
        &self,
        library_id: Option<Uuid>,
        strategy: S
    ) -> Vec<SeriesReference> {
        let mut series: Vec<SeriesReference> = self.get_series(library_id)
            .into_iter()
            .cloned()
            .collect();
        
        strategy.sort(&mut series);
        series
    }

    fn create_sort_strategy_for_movies(
        sort_by: SortBy,
        ascending: bool
    ) -> Box<dyn SortStrategy<MovieReference>> {
        match sort_by {
            SortBy::Title => Box::new(FieldSort::new(TitleField, !ascending)),
            SortBy::DateAdded => Box::new(FieldSort::new(DateAddedField, !ascending)),
            SortBy::Year => Box::new(FieldSort::new(ReleaseDateField, !ascending)),
            SortBy::Rating => Box::new(FieldSort::new(RatingField, !ascending)),
            SortBy::Runtime => Box::new(FieldSort::new(RuntimeField, !ascending)),
            SortBy::Popularity => Box::new(FieldSort::new(PopularityField, !ascending)),
            // These fields need custom implementation
            SortBy::FileSize | SortBy::Resolution | SortBy::LastWatched | SortBy::Genre => {
                // Fall back to title sort for now
                Box::new(FieldSort::new(TitleField, !ascending))
            }
        }
    }

    fn create_sort_strategy_for_series(
        sort_by: SortBy,
        ascending: bool
    ) -> Box<dyn SortStrategy<SeriesReference>> {
        match sort_by {
            SortBy::Title => Box::new(FieldSort::new(TitleField, !ascending)),
            SortBy::DateAdded => Box::new(FieldSort::new(DateAddedField, !ascending)),
            SortBy::Year => Box::new(FieldSort::new(ReleaseDateField, !ascending)),
            SortBy::Rating => Box::new(FieldSort::new(RatingField, !ascending)),
            SortBy::Popularity => Box::new(FieldSort::new(PopularityField, !ascending)),
            // These fields need custom implementation or don't apply to series
            SortBy::Runtime | SortBy::FileSize | SortBy::Resolution 
            | SortBy::LastWatched | SortBy::Genre => {
                // Fall back to title sort for now
                Box::new(FieldSort::new(TitleField, !ascending))
            }
        }
    }

    fn create_sort_strategy_from_query_for_movies(
        sort: &SortCriteria
    ) -> Box<dyn SortStrategy<MovieReference>> {
        let reverse = matches!(sort.order, QuerySortOrder::Descending);
        
        match sort.primary {
            SortField::Title => Box::new(FieldSort::new(TitleField, reverse)),
            SortField::DateAdded => Box::new(FieldSort::new(DateAddedField, reverse)),
            SortField::ReleaseDate => Box::new(FieldSort::new(ReleaseDateField, reverse)),
            SortField::Rating => Box::new(FieldSort::new(RatingField, reverse)),
            // Fields that require user context or aren't supported yet
            SortField::LastWatched | SortField::WatchProgress | SortField::Runtime => {
                // Default to DateAdded for unsupported fields
                Box::new(FieldSort::new(DateAddedField, reverse))
            }
        }
    }

    fn create_sort_strategy_from_query_for_series(
        sort: &SortCriteria
    ) -> Box<dyn SortStrategy<SeriesReference>> {
        let reverse = matches!(sort.order, QuerySortOrder::Descending);
        
        match sort.primary {
            SortField::Title => Box::new(FieldSort::new(TitleField, reverse)),
            SortField::DateAdded => Box::new(FieldSort::new(DateAddedField, reverse)),
            SortField::ReleaseDate => Box::new(FieldSort::new(ReleaseDateField, reverse)),
            SortField::Rating => Box::new(FieldSort::new(RatingField, reverse)),
            // Fields that require user context or aren't supported yet
            SortField::LastWatched | SortField::WatchProgress | SortField::Runtime => {
                // Default to DateAdded for unsupported fields
                Box::new(FieldSort::new(DateAddedField, reverse))
            }
        }
    }

    fn ui_sort_to_query_field(sort_by: SortBy) -> SortField {
        match sort_by {
            SortBy::Title => SortField::Title,
            SortBy::DateAdded => SortField::DateAdded,
            SortBy::Year => SortField::ReleaseDate,
            SortBy::Rating => SortField::Rating,
            SortBy::Runtime => SortField::Runtime,
            // Map other fields to reasonable defaults
            SortBy::Popularity | SortBy::FileSize | SortBy::Resolution 
            | SortBy::LastWatched | SortBy::Genre => SortField::DateAdded,
        }
    }
}