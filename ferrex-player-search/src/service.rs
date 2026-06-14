//! Search service for executing global, server-backed queries

use std::sync::Arc;

use crate::error::SearchError;
use ferrex_player_api::services::api::ApiService;
use ferrex_player_library::repository::{Accessor, ReadOnly};

use super::metrics::SearchPerformanceMetrics;
use super::types::{SearchResponse, SearchStrategy};
use chrono::Datelike;
use ferrex_core::player_prelude::{
    ArchivedModel, MediaWithStatus, SearchField,
};
use ferrex_core::query::MediaQueryBuilder;
use ferrex_model::{LibraryId, Media, MediaID};
use log::warn;
use std::time::Instant;

const SERVER_SEARCH_LIMIT: usize = 50;

/// Service for executing searches
#[derive(Debug)]
pub struct SearchService {
    /// API service for server-backed searching (optional)
    api_service: Option<Arc<dyn ApiService>>,
    repo_accessor: Option<Arc<Accessor<ReadOnly>>>,
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
        repo_accessor: Option<Arc<Accessor<ReadOnly>>>,
    ) -> Self {
        Self {
            api_service,
            repo_accessor,
        }
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
        library_id: Option<LibraryId>,
        fuzzy: bool,
    ) -> anyhow::Result<Vec<SearchResponse>, SearchError> {
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
        library_id: Option<LibraryId>,
        fuzzy: bool,
    ) -> (
        anyhow::Result<Vec<SearchResponse>, SearchError>,
        SearchPerformanceMetrics,
    ) {
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

    async fn search_server(
        &self,
        query: &str,
        fields: &[SearchField],
        _library_id: Option<LibraryId>,
        _fuzzy: bool,
    ) -> anyhow::Result<Vec<SearchResponse>, SearchError> {
        let api_service = self.api_service.as_ref().ok_or_else(|| {
            SearchError::Server(
                "No API service available for server search".to_string(),
            )
        })?;

        // Build MediaQuery for server
        let mut query_builder = MediaQueryBuilder::new();

        // Global search only: ignore any library filter for now

        // Always use fuzzy search (which includes exact matches)
        // NOTE: the server-side fuzzy path is currently title-only; treat
        // `All` as "Title" until multi-field fuzzy search is implemented.
        let effective_fields =
            if fields.is_empty() || fields.contains(&SearchField::All) {
                vec![SearchField::Title]
            } else {
                fields.to_vec()
            };

        query_builder = query_builder.search_in(query, effective_fields);

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

        // Convert server results to SearchResponse
        self.convert_api_results_from_status(response, query)
            .map(|opt_vec| opt_vec.into_iter().flatten().collect())
    }

    async fn search_hybrid(
        &self,
        query: &str,
        fields: &[SearchField],
        library_id: Option<LibraryId>,
        fuzzy: bool,
    ) -> anyhow::Result<Vec<SearchResponse>, SearchError> {
        self.search_server(query, fields, library_id, fuzzy).await
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
    ) -> anyhow::Result<Vec<Option<SearchResponse>>, SearchError> {
        response
            .into_iter()
            .map(|item| self.convert_media_ref_to_result(item.id, query))
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
        id: MediaID,
        _query: &str,
    ) -> anyhow::Result<Option<SearchResponse>, SearchError> {
        let Some(ra) = &self.repo_accessor else {
            return Err(SearchError::Internal(format!(
                "Search for {:#?} failed: Repo accessor unavailable",
                id
            )));
        };

        let yoke_opt = ra.get_media_yoke(&id).ok();

        let media: Media = match yoke_opt {
            Some(yoke) => {
                yoke.get().try_to_model().map_err(SearchError::Rkyv)?
            }
            None => {
                warn!("Failed to find media in repo for media_id: {}", &id);
                return Ok(None);
            }
        };

        match media.clone() {
            Media::Movie(ref movie) => Ok(Some(SearchResponse {
                media_ref: media,
                title: movie.title.to_string(),
                subtitle: movie.details.release_date.as_ref().and_then(|d| {
                    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                        .ok()
                        .map(|date| format!("{} • Movie", date.year()))
                }),
                year: movie.details.release_date.as_ref().and_then(|d| {
                    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                        .ok()
                        .map(|date| date.year())
                }),
                poster_url: movie.details.poster_path.clone(),
                match_score: 1.0, // Server results assumed to be relevant
                match_field: SearchField::All,
                library_id: Some(movie.file.library_id),
            })),
            Media::Series(series) => Ok(Some(SearchResponse {
                media_ref: media,
                title: series.title.as_str().to_string(),
                subtitle: series.details.first_air_date.as_ref().and_then(
                    |d| {
                        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                            .ok()
                            .map(|date| format!("{} • Series", date.year()))
                    },
                ),
                year: series.details.first_air_date.as_ref().and_then(|d| {
                    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                        .ok()
                        .map(|date| date.year())
                }),
                poster_url: series.details.poster_path.clone(),
                match_score: 1.0,
                match_field: SearchField::All,
                library_id: Some(series.library_id),
            })),
            Media::Season(season) => Ok(Some(SearchResponse {
                media_ref: media,
                title: format!("Season {}", season.season_number.value()),
                subtitle: Some("Series • Season".to_string()),
                year: None,
                poster_url: (season.details.poster_path.clone()),
                match_score: 0.8,
                match_field: SearchField::All,
                library_id: Some(season.library_id),
            })),
            Media::Episode(episode) => Ok(Some(SearchResponse {
                media_ref: media,
                title: episode.details.name.clone(),
                subtitle: Some(format!(
                    "Episode {} • S{:02}E{:02}",
                    episode.episode_number.value(),
                    episode.season_number.value(),
                    episode.episode_number.value()
                )),
                year: None,
                poster_url: episode.details.still_path.clone(),
                match_score: 0.7,
                match_field: SearchField::All,
                library_id: Some(episode.file.library_id),
            })),
        }
    }
}
