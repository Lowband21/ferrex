//! Search service for executing global, server-backed queries

use ferrex_core::player_prelude::{
    LibraryID, MediaQueryBuilder, MediaWithStatus, SearchField,
};
use std::sync::Arc;

use crate::infra::api_types::{Media, MovieReference, SeriesReference};
use crate::infra::services::api::ApiService;

use super::metrics::SearchPerformanceMetrics;
use super::types::{SearchResult, SearchStrategy};
use chrono::Datelike;
use std::time::Instant;

const SERVER_SEARCH_LIMIT: usize = 50;

/// Service for executing searches
#[derive(Debug)]
pub struct SearchService {
    /// API service for server-backed searching (optional)
    api_service: Option<Arc<dyn ApiService>>,
}

impl SearchService {
    /// Create a new search service
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    pub fn new(
        api_service: Option<Arc<dyn ApiService>>,
    ) -> Self {
        Self { api_service }
    }

    /// Check if network is available (api_service is present)
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    pub fn has_network(&self) -> bool {
        self.api_service.is_some()
    }

    pub async fn search(
        &self,
        query: &str,
        fields: &[SearchField],
        strategy: SearchStrategy,
        library_id: Option<LibraryID>,
        fuzzy: bool,
    ) -> Result<Vec<SearchResult>, String> {
        match strategy {
            SearchStrategy::Client => {
                self.search_hybrid(query, fields, library_id, fuzzy).await
            }
            SearchStrategy::Server => {
                self.search_server(query, fields, library_id, fuzzy).await
            }
            SearchStrategy::Hybrid => {
                self.search_hybrid(query, fields, library_id, fuzzy).await
            }
        }
    }

    pub async fn search_with_metrics(
        &self,
        query: &str,
        fields: &[SearchField],
        strategy: SearchStrategy,
        library_id: Option<LibraryID>,
        fuzzy: bool,
    ) -> (Result<Vec<SearchResult>, String>, SearchPerformanceMetrics) {
        let start = Instant::now();
        let query_length = query.len();
        let field_count = fields.len();

        log::debug!(
            "Search starting - Strategy: {:?}, Query: '{}', Fields: {:?}",
            strategy,
            query,
            fields
        );

        let result = self
            .search(query, fields, strategy, library_id, fuzzy)
            .await;

        let execution_time = start.elapsed();
        let result_count = result.as_ref().map(|r| r.len()).unwrap_or(0);
        let success = result.is_ok();

        log::info!(
            "Search completed - Strategy: {:?}, Time: {}ms, Results: {}, Success: {}",
            strategy,
            execution_time.as_millis(),
            result_count,
            success
        );

        let metric = SearchPerformanceMetrics {
            strategy,
            query_length,
            field_count,
            execution_time,
            result_count,
            success,
            network_latency: None, // Will be populated for server searches
            timestamp: start,
        };

        (result, metric)
    }

    /*
    /// Client-side search using MediaStore
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn search_client(
        &self,
        query: &str,
        fields: &[SearchField],
        library_id: Option<LibraryID>,
        fuzzy: bool,
    ) -> Result<Vec<SearchResult>, String> {
        //let store = self
        //    .media_store
        //    .read()
        //    .map_err(|e| format!("Failed to acquire media store lock: {}", e))?;

        let mut results = Vec::new();
        let query_lower = query.to_lowercase();

        // Search movies
        let movies = store.get_all_movies();
        for movie in movies {
            if let Some(score) = self.match_movie(&movie, &query_lower, fields, fuzzy) {
                // Check library filter
                if let Some(lib_id) = library_id {
                    if movie.file.library_id != lib_id {
                        continue;
                    }
                }

                results.push(SearchResult {
                    media_ref: Media::Movie(movie.clone()),
                    title: movie.title.as_str().to_string(),
                    subtitle: movie
                        .details
                        .get_release_year()
                        .map(|year| format!("{} • Movie", year)),
                    year: match &movie.details {
                        MediaDetailsOption::Details(
                            TmdbDetails::Movie(details),
                        ) => details.release_date.as_ref().and_then(|d| {
                            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                                .ok()
                                .map(|date| date.year())
                        }),
                        _ => None,
                    },
                    poster_url: match &movie.details {
                        MediaDetailsOption::Details(
                            TmdbDetails::Movie(details),
                        ) => details.poster_path.clone(),
                        _ => None,
                    },
                    match_score: score,
                    match_field: SearchField::Title, // TODO: Track which field matched
                    library_id: Some(movie.file.library_id),
                });
            }
        }

        // Search series
        let series_list = store.get_all_series();
        for series in series_list {
            if let Some(score) = self.match_series(&series, &query_lower, fields, fuzzy) {
                // Check library filter
                if let Some(lib_id) = library_id {
                    if series.library_id != lib_id {
                        continue;
                    }
                }

                results.push(SearchResult {
                    media_ref: Media::Series(series.clone()),
                    title: series.title.as_str().to_string(),
                    subtitle: match &series.details {
                        MediaDetailsOption::Details(
                            TmdbDetails::Series(details),
                        ) => details.first_air_date.as_ref().and_then(|d| {
                            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                                .ok()
                                .map(|date| format!("{} • Series", date.year()))
                        }),
                        _ => Some("Series".to_string()),
                    },
                    year: match &series.details {
                        MediaDetailsOption::Details(
                            TmdbDetails::Series(details),
                        ) => details.first_air_date.as_ref().and_then(|d| {
                            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                                .ok()
                                .map(|date| date.year())
                        }),
                        _ => None,
                    },
                    poster_url: match &series.details {
                        MediaDetailsOption::Details(
                            TmdbDetails::Series(details),
                        ) => details.poster_path.clone(),
                        _ => None,
                    },
                    match_score: score,
                    match_field: SearchField::Title,
                    library_id: Some(series.library_id),
                });
            }
        }

        // TODO: Search episodes within series

        // Sort by relevance score
        results.sort_by(|a, b| b.match_score.partial_cmp(&a.match_score).unwrap());

        Ok(results)
    } */
    /*
    /// Client-side search using MediaStore
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn search_client(
        &self,
        query: &str,
        fields: &[SearchField],
        library_id: Option<LibraryID>,
        fuzzy: bool,
    ) -> Result<Vec<SearchResult>, String> {
        //let store = self
        //    .media_store
        //    .read()
        //    .map_err(|e| format!("Failed to acquire media store lock: {}", e))?;

        let mut results = Vec::new();
        let query_lower = query.to_lowercase();

        // Search movies
        let movies = store.get_all_movies();
        for movie in movies {
            if let Some(score) = self.match_movie(&movie, &query_lower, fields, fuzzy) {
                // Check library filter
                if let Some(lib_id) = library_id {
                    if movie.file.library_id != lib_id {
                        continue;
                    }
                }

                results.push(SearchResult {
                    media_ref: Media::Movie(movie.clone()),
                    title: movie.title.as_str().to_string(),
                    subtitle: movie
                        .details
                        .get_release_year()
                        .map(|year| format!("{} • Movie", year)),
                    year: match &movie.details {
                        ferrex_core::MediaDetailsOption::Details(
                            ferrex_core::TmdbDetails::Movie(details),
                        ) => details.release_date.as_ref().and_then(|d| {
                            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                                .ok()
                                .map(|date| date.year())
                        }),
                        _ => None,
                    },
                    poster_url: match &movie.details {
                        ferrex_core::MediaDetailsOption::Details(
                            ferrex_core::TmdbDetails::Movie(details),
                        ) => details.poster_path.clone(),
                        _ => None,
                    },
                    match_score: score,
                    match_field: SearchField::Title, // TODO: Track which field matched
                    library_id: Some(movie.file.library_id),
                });
            }
        }

        // Search series
        let series_list = store.get_all_series();
        for series in series_list {
            if let Some(score) = self.match_series(&series, &query_lower, fields, fuzzy) {
                // Check library filter
                if let Some(lib_id) = library_id {
                    if series.library_id != lib_id {
                        continue;
                    }
                }

                results.push(SearchResult {
                    media_ref: Media::Series(series.clone()),
                    title: series.title.as_str().to_string(),
                    subtitle: match &series.details {
                        ferrex_core::MediaDetailsOption::Details(
                            ferrex_core::TmdbDetails::Series(details),
                        ) => details.first_air_date.as_ref().and_then(|d| {
                            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                                .ok()
                                .map(|date| format!("{} • Series", date.year()))
                        }),
                        _ => Some("Series".to_string()),
                    },
                    year: match &series.details {
                        ferrex_core::MediaDetailsOption::Details(
                            ferrex_core::TmdbDetails::Series(details),
                        ) => details.first_air_date.as_ref().and_then(|d| {
                            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                                .ok()
                                .map(|date| date.year())
                        }),
                        _ => None,
                    },
                    poster_url: match &series.details {
                        ferrex_core::MediaDetailsOption::Details(
                            ferrex_core::TmdbDetails::Series(details),
                        ) => details.poster_path.clone(),
                        _ => None,
                    },
                    match_score: score,
                    match_field: SearchField::Title,
                    library_id: Some(series.library_id),
                });
            }
        }

        // TODO: Search episodes within series

        // Sort by relevance score
        results.sort_by(|a, b| b.match_score.partial_cmp(&a.match_score).unwrap());

        Ok(results)
    } */

    async fn search_server(
        &self,
        query: &str,
        fields: &[SearchField],
        library_id: Option<LibraryID>,
        _fuzzy: bool,
    ) -> Result<Vec<SearchResult>, String> {
        let api_service = self.api_service.as_ref().ok_or_else(|| {
            "No API service available for server search".to_string()
        })?;

        // Build MediaQuery for server
        let mut query_builder = MediaQueryBuilder::new();

        // Global search only: ignore any library filter for now

        // Always use fuzzy search (which includes exact matches)
        // This avoids API issues with exact search mode
        if fields.is_empty() || fields.contains(&SearchField::All) {
            query_builder = query_builder.search(query);
        } else {
            query_builder = query_builder.search_in(query, fields.to_vec());
        }

        let media_query = query_builder.limit(SERVER_SEARCH_LIMIT).build();

        // Log the query being sent for debugging
        log::debug!(
            "Sending search query to server: text='{}', fuzzy={}, fields={:?}",
            query,
            media_query
                .search
                .as_ref()
                .map(|s| s.fuzzy)
                .unwrap_or(false),
            media_query
                .search
                .as_ref()
                .map(|s| &s.fields)
                .unwrap_or(&vec![])
        );

        // Execute server query via API endpoint
        let response = match api_service.query_media(media_query.clone()).await
        {
            Ok(response) => response,
            Err(e) => {
                log::warn!(
                    "Server search failed for query '{}', with error {:?}",
                    query,
                    e
                );
                vec![]
            }
        };

        // Convert server results to SearchResult
        let results = self.convert_api_results_from_status(response, query);

        Ok(results)
    }

    async fn search_hybrid(
        &self,
        query: &str,
        fields: &[SearchField],
        library_id: Option<LibraryID>,
        fuzzy: bool,
    ) -> Result<Vec<SearchResult>, String> {
        // Start with client search
        //let mut results = self.search_client(query, fields, library_id, fuzzy)?;

        // If we have few results and server is available, augment with server search
        //if results.len() < 5 && self.api_service.is_some() {

        let server_results =
            self.search_server(query, fields, library_id, fuzzy).await?;

        let mut results = vec![];

        // Merge results, avoiding duplicates
        for server_result in server_results {
            //let is_duplicate =
            //    results
            //        .iter()
            //        .any(|r| match (&r.media_ref, &server_result.media_ref) {
            //            (Media::Movie(m1), Media::Movie(m2)) => m1.id == m2.id,
            //            (Media::Series(s1), Media::Series(s2)) => s1.id == s2.id,
            //            _ => false,
            //        });

            //if !is_duplicate {
            results.push(server_result);
            //}
        }

        // Re-sort by relevance
        //results.sort_by(|a, b| b.match_score.partial_cmp(&a.match_score).unwrap());
        //}

        Ok(results)
    }

    /// Match a movie against search query
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn match_movie(
        &self,
        movie: &MovieReference,
        query: &str,
        fields: &[SearchField],
        fuzzy: bool,
    ) -> Option<f32> {
        let check_all = fields.is_empty() || fields.contains(&SearchField::All);
        let mut best_score = 0.0f32;

        // Check title
        if (check_all || fields.contains(&SearchField::Title))
            && let Some(score) =
                self.calculate_match_score(movie.title.as_str(), query, fuzzy)
        {
            best_score = best_score.max(score);
        }

        let details = movie.details.as_movie();

        // Check overview
        if (check_all || fields.contains(&SearchField::Overview))
            && let Some(details) = details
            && let Some(overview) = details.overview.as_ref()
            && let Some(score) =
                self.calculate_match_score(overview, query, fuzzy)
        {
            best_score = best_score.max(score * 0.8); // Lower weight for overview matches
        }

        // Check genres
        if (check_all || fields.contains(&SearchField::Genre))
            && let Some(details) = details
        {
            for genre in &details.genres {
                if let Some(score) =
                    self.calculate_match_score(&genre.name, query, fuzzy)
                {
                    best_score = best_score.max(score * 0.9);
                }
            }
        }

        if best_score > 0.0 {
            Some(best_score)
        } else {
            None
        }
    }

    /// Match a series against search query
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn match_series(
        &self,
        series: &SeriesReference,
        query: &str,
        fields: &[SearchField],
        fuzzy: bool,
    ) -> Option<f32> {
        let check_all = fields.is_empty() || fields.contains(&SearchField::All);
        let mut best_score = 0.0f32;

        // Check title
        if (check_all || fields.contains(&SearchField::Title))
            && let Some(score) =
                self.calculate_match_score(series.title.as_str(), query, fuzzy)
        {
            best_score = best_score.max(score);
        }

        let details = series.details.as_series();

        // Check overview
        if (check_all || fields.contains(&SearchField::Overview))
            && let Some(details) = details
            && let Some(overview) = details.overview.as_ref()
            && let Some(score) =
                self.calculate_match_score(overview, query, fuzzy)
        {
            best_score = best_score.max(score * 0.8);
        }

        // Check genres
        if (check_all || fields.contains(&SearchField::Genre))
            && let Some(details) = details
        {
            for genre in &details.genres {
                if let Some(score) =
                    self.calculate_match_score(&genre.name, query, fuzzy)
                {
                    best_score = best_score.max(score * 0.9);
                }
            }
        }

        if best_score > 0.0 {
            Some(best_score)
        } else {
            None
        }
    }

    /// Calculate match score between text and query
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn calculate_match_score(
        &self,
        text: &str,
        query: &str,
        _fuzzy: bool,
    ) -> Option<f32> {
        let text_lower = text.to_lowercase();
        let query_lower = query.to_lowercase();

        // Always use fuzzy matching that prioritizes exact matches

        // Perfect match gets highest score
        if text_lower == query_lower {
            return Some(1.0);
        }

        // Exact substring match gets high score
        if text_lower.contains(&query_lower) {
            let position = text_lower.find(&query_lower).unwrap() as f32;
            let text_len = text_lower.len() as f32;
            let query_len = query_lower.len() as f32;

            // Score based on position (earlier = better) and coverage (more coverage = better)
            let position_score = 1.0 - (position / text_len);
            let coverage_score = query_len / text_len;

            // If it starts with the query, give extra boost
            if position == 0.0 {
                return Some(0.95 + (coverage_score * 0.05));
            }

            return Some(
                (position_score * 0.6 + coverage_score * 0.4).max(0.7),
            );
        }

        // Check for partial word matches (fuzzy matching)
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        if query_words.is_empty() {
            return None;
        }

        let text_words: Vec<String> = text_lower
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let mut matched_words = 0;
        let mut exact_word_matches = 0;

        for query_word in &query_words {
            for text_word in &text_words {
                if text_word == query_word {
                    exact_word_matches += 1;
                    matched_words += 1;
                    break;
                } else if text_word.contains(query_word) {
                    matched_words += 1;
                    break;
                }
            }
        }

        if matched_words > 0 {
            let base_score = matched_words as f32 / query_words.len() as f32;
            let exact_bonus =
                exact_word_matches as f32 / query_words.len() as f32 * 0.2;
            Some((base_score * 0.6 + exact_bonus).min(0.69)) // Cap fuzzy matches below exact substring matches
        } else {
            None
        }
    }

    /// Convert API response with status to SearchResult format
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn convert_api_results_from_status(
        &self,
        response: Vec<MediaWithStatus>,
        query: &str,
    ) -> Vec<SearchResult> {
        response
            .into_iter()
            .map(|item| self.convert_media_ref_to_result(item.media, query))
            .collect()
    }

    fn convert_api_results(
        &self,
        response: Vec<Media>,
        query: &str,
    ) -> Vec<SearchResult> {
        response
            .into_iter()
            .map(|media_ref| self.convert_media_ref_to_result(media_ref, query))
            .collect()
    }

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn convert_media_ref_to_result(
        &self,
        media_ref: Media,
        _query: &str,
    ) -> SearchResult {
        match &media_ref {
            Media::Movie(movie) => SearchResult {
                title: movie.title.as_str().to_string(),
                subtitle: movie
                    .details
                    .as_movie()
                    .and_then(|details| {
                        details.release_date.as_ref().and_then(|d| {
                            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                                .ok()
                                .map(|date| format!("{} • Movie", date.year()))
                        })
                    })
                    .or(Some("Movie".to_string())),
                year: movie.details.as_movie().and_then(|details| {
                    details.release_date.as_ref().and_then(|d| {
                        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                            .ok()
                            .map(|date| date.year())
                    })
                }),
                poster_url: match &movie.details {
                    ferrex_core::MediaDetailsOption::Details(
                        ferrex_core::TmdbDetails::Movie(details),
                    ) => details.poster_path.clone(),
                    _ => None,
                },
                match_score: 1.0, // Server results assumed to be relevant
                match_field: SearchField::All,
                library_id: Some(movie.file.library_id),
                media_ref,
            },
            Media::Series(series) => SearchResult {
                title: series.title.as_str().to_string(),
                subtitle: series
                    .details
                    .as_series()
                    .and_then(|details| {
                        details.first_air_date.as_ref().and_then(|d| {
                            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                                .ok()
                                .map(|date| format!("{} • Series", date.year()))
                        })
                    })
                    .or(Some("Series".to_string())),
                year: series.details.as_series().and_then(|details| {
                    details.first_air_date.as_ref().and_then(|d| {
                        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                            .ok()
                            .map(|date| date.year())
                    })
                }),
                poster_url: series
                    .details
                    .as_series()
                    .and_then(|details| details.poster_path.clone()),
                match_score: 1.0,
                match_field: SearchField::All,
                library_id: Some(series.library_id),
                media_ref,
            },
            Media::Season(season) => SearchResult {
                title: format!("Season {}", season.season_number.value()),
                subtitle: Some("Series • Season".to_string()),
                year: None,
                poster_url: (season.details)
                    .as_season()
                    .and_then(|details| details.poster_path.clone()),
                match_score: 0.8,
                match_field: SearchField::All,
                library_id: Some(season.library_id),
                media_ref,
            },
            Media::Episode(episode) => SearchResult {
                title: episode
                    .details
                    .as_episode()
                    .map(|details| details.name.clone())
                    .unwrap_or_else(|| {
                        format!("Episode {}", episode.episode_number.value())
                    }),
                subtitle: Some(format!(
                    "Episode {} • S{:02}E{:02}",
                    episode.episode_number.value(),
                    episode.season_number.value(),
                    episode.episode_number.value()
                )),
                year: None,
                poster_url: episode
                    .details
                    .as_episode()
                    .and_then(|details| details.still_path.clone()),
                match_score: 0.7,
                match_field: SearchField::All,
                library_id: Some(episode.file.library_id),
                media_ref,
            },
        }
    }
}
