//! SortableEntity implementations for media reference types
//!
//! This module provides sorting implementations that extract sort keys
//! from media references based on available data.

use super::{
    DateAddedField, HasField, MovieFieldSet, OptionalDateKey, OptionalFloatKey, OptionalU32Key,
    OptionalU64Key, SeriesFieldSet, SortFieldMarker, SortKey, SortableEntity, StringKey,
};

use crate::{Media, MediaDetailsOption, MovieReference, SeriesReference, TmdbDetails};

/// Implementation of SortableEntity for MovieReference
impl SortableEntity for MovieReference {
    type AvailableFields = MovieFieldSet;

    fn extract_key<F: SortFieldMarker>(&self, _field: F) -> F::Key
    where
        Self::AvailableFields: HasField<F>,
    {
        // We need to match on the field's ID to determine which key to extract
        // This is a runtime dispatch based on the const ID
        //
        // NOTE: We use a type assertion approach to avoid unsafe transmute_copy
        // which was causing double-free issues. The compiler should optimize
        // this to be zero-cost.

        if F::ID == "title" {
            // Title is always available from MovieReference
            let key = StringKey::from_str(self.title.as_str());
            // We know F::Key = StringKey when F::ID = "title"
            // This will panic if the type doesn't match, which is what we want
            // since it indicates a programming error
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "date_added" {
            // Date added comes from the file creation time
            let key = OptionalDateKey::new(Some(self.file.created_at));
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "release_date" {
            // Release date requires TMDB details
            let date = match &self.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(movie_details)) => {
                    movie_details.release_date.as_ref().and_then(|date_str| {
                        // Parse the date string (expected format: YYYY-MM-DD)
                        use chrono::{NaiveDate, NaiveTime, Utc};
                        NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                            .ok()
                            .and_then(|date| {
                                let naive_datetime =
                                    date.and_time(NaiveTime::from_hms_opt(0, 0, 0)?);
                                naive_datetime.and_local_timezone(Utc).single()
                            })
                    })
                }
                _ => None,
            };
            let key = OptionalDateKey::new(date);
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "rating" {
            // Rating (vote_average) requires TMDB details
            let rating = match &self.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(movie_details)) => {
                    movie_details.vote_average
                }
                _ => None,
            };
            let key = OptionalFloatKey::new(rating);
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "popularity" {
            // Popularity requires TMDB details
            let popularity = match &self.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(movie_details)) => {
                    movie_details.popularity
                }
                _ => None,
            };
            let key = OptionalFloatKey::new(popularity);
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "runtime" {
            // Runtime requires TMDB details
            let runtime = match &self.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(movie_details)) => {
                    movie_details.runtime.map(|r| r as u32)
                }
                _ => None,
            };
            let key = OptionalU32Key::new(runtime);
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "file_size" {
            let key = OptionalU64Key::new(Some(self.file.size));
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "resolution" {
            let height = self
                .file
                .media_file_metadata
                .as_ref()
                .and_then(|meta| meta.height);
            let key = OptionalU32Key::new(height);
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "bitrate" {
            let bitrate = self
                .file
                .media_file_metadata
                .as_ref()
                .and_then(|meta| meta.bitrate);
            let key = OptionalU64Key::new(bitrate);
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "content_rating" {
            let key = StringKey::missing();
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "last_watched" {
            // Last watched requires user context and watch status data
            // For now, return missing data - this will be implemented with watch status integration
            let key = <OptionalDateKey as SortKey>::missing();
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "watch_progress" {
            // Watch progress requires user context and watch status data
            // For now, return missing data - this will be implemented with watch status integration
            let key = <OptionalFloatKey as SortKey>::missing();
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else {
            // This should never happen if the trait system is used correctly
            panic!("Unknown sort field ID: {}", F::ID)
        }
    }
}

/// Implementation of SortableEntity for SeriesReference
impl SortableEntity for SeriesReference {
    type AvailableFields = SeriesFieldSet;

    fn extract_key<F: SortFieldMarker>(&self, _field: F) -> F::Key
    where
        Self::AvailableFields: HasField<F>,
    {
        // We need to match on the field's ID to determine which key to extract
        // Using type assertion to avoid unsafe code

        if F::ID == "title" {
            // Title is always available from SeriesReference
            let key = StringKey::from_str(self.title.as_str());
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "date_added" {
            // Date added comes from the series folder creation time
            let key = OptionalDateKey::new(Some(self.created_at));
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "release_date" {
            // First air date requires TMDB details
            let date = match &self.details {
                MediaDetailsOption::Details(TmdbDetails::Series(series_details)) => {
                    series_details.first_air_date.as_ref().and_then(|date_str| {
                        // Parse the date string (expected format: YYYY-MM-DD)
                        use chrono::{NaiveDate, NaiveTime, Utc};
                        NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                            .ok()
                            .and_then(|date| {
                                let naive_datetime =
                                    date.and_time(NaiveTime::from_hms_opt(0, 0, 0)?);
                                naive_datetime.and_local_timezone(Utc).single()
                            })
                    })
                }
                _ => None,
            };
            let key = OptionalDateKey::new(date);
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "rating" {
            // Rating (vote_average) requires TMDB details
            let rating = match &self.details {
                MediaDetailsOption::Details(TmdbDetails::Series(series_details)) => {
                    series_details.vote_average
                }
                _ => None,
            };
            let key = OptionalFloatKey::new(rating);
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "popularity" {
            // Popularity requires TMDB details
            let popularity = match &self.details {
                MediaDetailsOption::Details(TmdbDetails::Series(series_details)) => {
                    series_details.popularity
                }
                _ => None,
            };
            let key = OptionalFloatKey::new(popularity);
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "last_watched" {
            // Last watched requires user context and watch status data
            // For now, return missing data - this will be implemented with watch status integration
            let key = <OptionalDateKey as SortKey>::missing();
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else if F::ID == "watch_progress" {
            // Watch progress requires user context and watch status data
            // For now, return missing data - this will be implemented with watch status integration
            let key = <OptionalFloatKey as SortKey>::missing();
            *Box::<dyn std::any::Any>::downcast(Box::new(key) as Box<dyn std::any::Any>).unwrap()
        } else {
            // This should never happen if the trait system is used correctly
            panic!("Unknown sort field ID: {}", F::ID)
        }
    }
}
// NOTE: Media deliberately does NOT implement SortableEntity
// because different media types (Movie, Series, Season, Episode) have
// different sortable fields. Use type-specific sorting for each media type.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::sorting::fields::{
        DateAddedField, PopularityField, RatingField, ReleaseDateField, RuntimeField, TitleField,
    };
    use crate::query::sorting::strategy::FieldSort;
    use crate::{EnhancedMovieDetails, LibraryID, MovieID, MovieTitle, MovieURL, UrlLike};
    use uuid::Uuid;

    fn create_test_movie_with_details() -> MovieReference {
        let mut details = EnhancedMovieDetails {
            id: 12345,
            title: "Test Movie".to_string(),
            overview: Some("A test movie".to_string()),
            release_date: Some("2023-01-15".to_string()),
            runtime: Some(120),
            vote_average: Some(7.5f32),
            vote_count: Some(1000),
            popularity: Some(85.3f32),
            content_rating: Some("PG-13".to_string()),
            ..Default::default()
        };
        details
            .spoken_languages
            .push(crate::types::details::SpokenLanguage {
                iso_639_1: Some("en".to_string()),
                name: "English".to_string(),
                english_name: Some("English".to_string()),
            });

        MovieReference {
            id: MovieID::new(),
            library_id: LibraryID::new(),
            tmdb_id: 12345,
            title: MovieTitle::new("Test Movie".to_string()).unwrap(),
            details: MediaDetailsOption::Details(TmdbDetails::Movie(details)),
            endpoint: MovieURL::from_string("/movies/test-movie-1".to_string()),
            file: crate::MediaFile {
                id: Uuid::new_v4(),
                path: std::path::PathBuf::from("/movies/test.mp4"),
                filename: "test.mp4".to_string(),
                size: 1000000,
                created_at: chrono::Utc::now(),
                media_file_metadata: None,
                library_id: LibraryID::new(),
            },
            theme_color: None,
        }
    }

    fn create_test_movie_without_details() -> MovieReference {
        let mut movie = create_test_movie_with_details();
        movie.details = MediaDetailsOption::Endpoint("/movies/test-movie-1".to_string());
        movie
    }

    #[test]
    fn test_extract_title_key() {
        let movie = create_test_movie_with_details();
        let key: StringKey = movie.extract_key(TitleField);
        assert!(!key.is_missing());
    }

    #[test]
    fn test_extract_date_added_key() {
        let movie = create_test_movie_with_details();
        let key: OptionalDateKey = movie.extract_key(DateAddedField);
        assert!(!key.is_missing());
    }

    #[test]
    fn test_extract_release_date_with_details() {
        let movie = create_test_movie_with_details();
        let key: OptionalDateKey = movie.extract_key(ReleaseDateField);
        assert!(!key.is_missing());
    }

    #[test]
    fn test_extract_release_date_without_details() {
        let movie = create_test_movie_without_details();
        let key: OptionalDateKey = movie.extract_key(ReleaseDateField);
        assert!(key.is_missing());
    }

    #[test]
    fn test_extract_rating_with_details() {
        let movie = create_test_movie_with_details();
        let key: OptionalFloatKey = movie.extract_key(RatingField);
        assert!(!key.is_missing());
    }

    #[test]
    fn test_extract_rating_without_details() {
        let movie = create_test_movie_without_details();
        let key: OptionalFloatKey = movie.extract_key(RatingField);
        assert!(key.is_missing());
    }

    #[test]
    fn test_extract_runtime_with_details() {
        let movie = create_test_movie_with_details();
        let key: OptionalU32Key = movie.extract_key(RuntimeField);
        assert!(!key.is_missing());
    }

    #[test]
    fn test_extract_popularity_with_details() {
        let movie = create_test_movie_with_details();
        let key: OptionalFloatKey = movie.extract_key(PopularityField);
        assert!(!key.is_missing());
    }

    #[test]
    fn test_missing_optional_values_stay_last_on_desc() {
        let mut with_details = create_test_movie_with_details();
        with_details.title = MovieTitle::new("Rated Movie".to_string()).unwrap();

        let mut without_details = create_test_movie_without_details();
        without_details.title = MovieTitle::new("Unrated Movie".to_string()).unwrap();

        let mut movies = vec![without_details.clone(), with_details.clone()];

        let sorter = FieldSort::<MovieReference, RatingField>::new(RatingField, true);
        sorter.sort(&mut movies);

        assert_eq!(movies[0].title, with_details.title);
        assert_eq!(movies[1].title, without_details.title);
    }
}
