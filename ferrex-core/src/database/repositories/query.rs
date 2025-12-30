use std::collections::HashSet;

use async_trait::async_trait;
use sqlx::Row;
use sqlx::{
    PgPool, Postgres, QueryBuilder,
    types::{BigDecimal, Uuid},
};
// Use Media enum from our domain prelude, not tmdb_api

use crate::domain::watch::{CompletedItem, ItemWatchStatus};
use crate::{
    api::types::{RATING_DECIMAL_SCALE, RatingValue},
    database::repositories::fuzzy_title_search::{
        TitleCandidate, rank_title_candidates, supports_title_only_search,
    },
    database::repository_ports::query::QueryRepository,
    error::{MediaError, Result},
    player_prelude::*,
    query::types::{MediaQuery, MediaWithStatus},
};

fn rating_bound(value: RatingValue) -> BigDecimal {
    BigDecimal::from(value).with_scale(RATING_DECIMAL_SCALE as i64)
}

#[derive(Clone, Debug)]
pub struct PostgresQueryRepository {
    pool: PgPool,
}

#[derive(Debug)]
struct InProgressRow {
    id: Uuid,
    file_id: Uuid,
    position: f32,
    duration: f32,
    last_watched: i64,
    media_kind: i32,
}

#[derive(Debug)]
struct CompletedRow {
    id: Uuid,
    last_watched: i64,
    media_kind: i32,
}

#[derive(Debug, sqlx::FromRow)]
struct TitleCandidateRow {
    id: Uuid,
    title: String,
}

impl PostgresQueryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn query_media_by_title_search(
        &self,
        query: &MediaQuery,
        search: &SearchQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        let search_text = search.text.trim();
        if search_text.is_empty() {
            return Ok(Vec::new());
        }

        let fetch_limit = query
            .pagination
            .offset
            .saturating_add(query.pagination.limit);

        if fetch_limit == 0 {
            return Ok(Vec::new());
        }

        let base_candidate_limit = compute_candidate_limit(fetch_limit);
        let query_len = search_text.chars().count();

        let want_movies = query.filters.media_type.is_none()
            || matches!(query.filters.media_type, Some(MediaTypeFilter::Movie));
        let want_series = query.filters.media_type.is_none()
            || matches!(
                query.filters.media_type,
                Some(MediaTypeFilter::Series)
            );
        let want_episodes = query.filters.media_type.is_none()
            || matches!(
                query.filters.media_type,
                Some(MediaTypeFilter::Episode)
            );

        let kind_count = usize::from(want_movies)
            + usize::from(want_series)
            + usize::from(want_episodes);
        let per_kind_limit = if kind_count == 0 {
            base_candidate_limit as usize
        } else {
            (base_candidate_limit as usize)
                .div_ceil(kind_count)
                .max(200)
        };
        let candidate_limit = per_kind_limit as i64;

        let mut candidates =
            Vec::with_capacity(per_kind_limit.saturating_mul(kind_count));

        if want_movies {
            candidates.extend(
                self.fetch_movie_title_candidates(
                    search_text,
                    query_len,
                    &query.filters.library_ids,
                    candidate_limit,
                )
                .await?,
            );
        }

        if want_series {
            candidates.extend(
                self.fetch_series_title_candidates(
                    search_text,
                    query_len,
                    &query.filters.library_ids,
                    candidate_limit,
                )
                .await?,
            );
        }

        let explicit_episode_filter =
            matches!(query.filters.media_type, Some(MediaTypeFilter::Episode));
        if want_episodes && (query_len > 2 || explicit_episode_filter) {
            candidates.extend(
                self.fetch_episode_title_candidates(
                    search_text,
                    query_len,
                    &query.filters.library_ids,
                    candidate_limit,
                )
                .await?,
            );
        }

        let ranked = rank_title_candidates(search_text, candidates);

        let start = query.pagination.offset.min(ranked.len());
        let end = (start + query.pagination.limit).min(ranked.len());

        let mut results = Vec::with_capacity(end.saturating_sub(start));

        for candidate in &ranked[start..end] {
            let watch_status = if let Some(user_id) = query.user_context {
                match candidate.media_id {
                    MediaID::Movie(movie_id) => {
                        self.get_movie_watch_status(user_id, &movie_id).await?
                    }
                    MediaID::Episode(episode_id) => {
                        self.get_episode_watch_status(user_id, &episode_id)
                            .await?
                    }
                    _ => None,
                }
            } else {
                None
            };

            results.push(MediaWithStatus {
                id: candidate.media_id,
                watch_status,
            });
        }

        Ok(results)
    }

    async fn fetch_movie_title_candidates(
        &self,
        search_text: &str,
        query_len: usize,
        library_ids: &[Uuid],
        candidate_limit: i64,
    ) -> Result<Vec<TitleCandidate>> {
        let escaped = escape_like_literal(search_text);

        let mut sql_builder = QueryBuilder::<Postgres>::new(
            "SELECT mr.id, mr.title FROM movie_references mr WHERE 1=1",
        );

        if !library_ids.is_empty() {
            sql_builder.push(" AND mr.library_id = ANY(");
            sql_builder.push_bind(library_ids);
            sql_builder.push(")");
        }

        if query_len <= 2 {
            let prefix_pattern = format!("{}%", escaped);
            sql_builder.push(" AND mr.title ILIKE ");
            sql_builder.push_bind(prefix_pattern);
            sql_builder.push(" ESCAPE E'\\\\'");
            sql_builder
                .push(" ORDER BY LOWER(mr.title) ASC, LENGTH(mr.title) ASC");
        } else {
            let similarity_threshold = similarity_threshold(query_len);
            let substring_pattern = format!("%{}%", escaped);
            sql_builder.push(" AND (");
            sql_builder.push("mr.title ILIKE ");
            sql_builder.push_bind(substring_pattern.clone());
            sql_builder.push(" ESCAPE E'\\\\'");
            sql_builder.push(" OR similarity(mr.title, ");
            sql_builder.push_bind(search_text);
            sql_builder.push(") > ");
            sql_builder.push_bind(similarity_threshold);
            sql_builder.push(")");

            let prefix_pattern = format!("{}%", escaped);
            sql_builder.push(" ORDER BY ");
            sql_builder.push("CASE ");
            sql_builder.push("WHEN LOWER(mr.title) = LOWER(");
            sql_builder.push_bind(search_text);
            sql_builder.push(") THEN 0 ");
            sql_builder.push("WHEN LOWER(mr.title) LIKE LOWER(");
            sql_builder.push_bind(prefix_pattern);
            sql_builder.push(") ESCAPE E'\\\\' THEN 1 ");
            sql_builder.push("WHEN mr.title ILIKE ");
            sql_builder.push_bind(substring_pattern);
            sql_builder.push(" ESCAPE E'\\\\' THEN 2 ");
            sql_builder.push("ELSE 3 END, ");
            sql_builder.push("similarity(mr.title, ");
            sql_builder.push_bind(search_text);
            sql_builder.push(") DESC, ");
            sql_builder.push("LENGTH(mr.title) ASC, LOWER(mr.title) ASC");
        }

        sql_builder.push(" LIMIT ");
        sql_builder.push_bind(candidate_limit);

        let rows = sql_builder
            .build_query_as::<TitleCandidateRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Database candidate query failed: {}",
                    e
                ))
            })?;

        Ok(rows
            .into_iter()
            .map(|row| TitleCandidate {
                media_id: MediaID::Movie(MovieID(row.id)),
                title: row.title,
            })
            .collect())
    }

    async fn fetch_series_title_candidates(
        &self,
        search_text: &str,
        query_len: usize,
        library_ids: &[Uuid],
        candidate_limit: i64,
    ) -> Result<Vec<TitleCandidate>> {
        let escaped = escape_like_literal(search_text);

        let mut sql_builder = QueryBuilder::<Postgres>::new(
            "SELECT s.id, s.title \
             FROM series s \
             INNER JOIN series_bundle_versioning sbv \
               ON sbv.series_id = s.id \
              AND sbv.library_id = s.library_id \
             WHERE sbv.finalized = true",
        );

        if !library_ids.is_empty() {
            sql_builder.push(" AND s.library_id = ANY(");
            sql_builder.push_bind(library_ids);
            sql_builder.push(")");
        }

        if query_len <= 2 {
            let prefix_pattern = format!("{}%", escaped);
            sql_builder.push(" AND s.title ILIKE ");
            sql_builder.push_bind(prefix_pattern);
            sql_builder.push(" ESCAPE E'\\\\'");
            sql_builder
                .push(" ORDER BY LOWER(s.title) ASC, LENGTH(s.title) ASC");
        } else {
            let similarity_threshold = similarity_threshold(query_len);
            let substring_pattern = format!("%{}%", escaped);
            sql_builder.push(" AND (");
            sql_builder.push("s.title ILIKE ");
            sql_builder.push_bind(substring_pattern.clone());
            sql_builder.push(" ESCAPE E'\\\\'");
            sql_builder.push(" OR similarity(s.title, ");
            sql_builder.push_bind(search_text);
            sql_builder.push(") > ");
            sql_builder.push_bind(similarity_threshold);
            sql_builder.push(")");

            let prefix_pattern = format!("{}%", escaped);
            sql_builder.push(" ORDER BY ");
            sql_builder.push("CASE ");
            sql_builder.push("WHEN LOWER(s.title) = LOWER(");
            sql_builder.push_bind(search_text);
            sql_builder.push(") THEN 0 ");
            sql_builder.push("WHEN LOWER(s.title) LIKE LOWER(");
            sql_builder.push_bind(prefix_pattern);
            sql_builder.push(") ESCAPE E'\\\\' THEN 1 ");
            sql_builder.push("WHEN s.title ILIKE ");
            sql_builder.push_bind(substring_pattern);
            sql_builder.push(" ESCAPE E'\\\\' THEN 2 ");
            sql_builder.push("ELSE 3 END, ");
            sql_builder.push("similarity(s.title, ");
            sql_builder.push_bind(search_text);
            sql_builder.push(") DESC, ");
            sql_builder.push("LENGTH(s.title) ASC, LOWER(s.title) ASC");
        }

        sql_builder.push(" LIMIT ");
        sql_builder.push_bind(candidate_limit);

        let rows = sql_builder
            .build_query_as::<TitleCandidateRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Database candidate query failed: {}",
                    e
                ))
            })?;

        Ok(rows
            .into_iter()
            .map(|row| TitleCandidate {
                media_id: MediaID::Series(SeriesID(row.id)),
                title: row.title,
            })
            .collect())
    }

    async fn fetch_episode_title_candidates(
        &self,
        search_text: &str,
        query_len: usize,
        library_ids: &[Uuid],
        candidate_limit: i64,
    ) -> Result<Vec<TitleCandidate>> {
        let escaped = escape_like_literal(search_text);

        let mut sql_builder = QueryBuilder::<Postgres>::new(
            "SELECT er.id, em.name AS title \
            FROM episode_references er \
            JOIN episode_metadata em ON em.episode_id = er.id \
            JOIN series s ON s.id = er.series_id \
            INNER JOIN series_bundle_versioning sbv \
              ON sbv.series_id = s.id \
             AND sbv.library_id = s.library_id \
            WHERE em.name IS NOT NULL \
              AND sbv.finalized = true",
        );

        if !library_ids.is_empty() {
            sql_builder.push(" AND s.library_id = ANY(");
            sql_builder.push_bind(library_ids);
            sql_builder.push(")");
        }

        if query_len <= 2 {
            let prefix_pattern = format!("{}%", escaped);
            sql_builder.push(" AND em.name ILIKE ");
            sql_builder.push_bind(prefix_pattern);
            sql_builder.push(" ESCAPE E'\\\\'");
            sql_builder
                .push(" ORDER BY LOWER(em.name) ASC, LENGTH(em.name) ASC");
        } else {
            let similarity_threshold = similarity_threshold(query_len);
            let substring_pattern = format!("%{}%", escaped);
            sql_builder.push(" AND (");
            sql_builder.push("em.name ILIKE ");
            sql_builder.push_bind(substring_pattern.clone());
            sql_builder.push(" ESCAPE E'\\\\'");
            sql_builder.push(" OR similarity(em.name, ");
            sql_builder.push_bind(search_text);
            sql_builder.push(") > ");
            sql_builder.push_bind(similarity_threshold);
            sql_builder.push(")");

            let prefix_pattern = format!("{}%", escaped);
            sql_builder.push(" ORDER BY ");
            sql_builder.push("CASE ");
            sql_builder.push("WHEN LOWER(em.name) = LOWER(");
            sql_builder.push_bind(search_text);
            sql_builder.push(") THEN 0 ");
            sql_builder.push("WHEN LOWER(em.name) LIKE LOWER(");
            sql_builder.push_bind(prefix_pattern);
            sql_builder.push(") ESCAPE E'\\\\' THEN 1 ");
            sql_builder.push("WHEN em.name ILIKE ");
            sql_builder.push_bind(substring_pattern);
            sql_builder.push(" ESCAPE E'\\\\' THEN 2 ");
            sql_builder.push("ELSE 3 END, ");
            sql_builder.push("similarity(em.name, ");
            sql_builder.push_bind(search_text);
            sql_builder.push(") DESC, ");
            sql_builder.push("LENGTH(em.name) ASC, LOWER(em.name) ASC");
        }

        sql_builder.push(" LIMIT ");
        sql_builder.push_bind(candidate_limit);

        let rows = sql_builder
            .build_query_as::<TitleCandidateRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Database candidate query failed: {}",
                    e
                ))
            })?;

        Ok(rows
            .into_iter()
            .map(|row| TitleCandidate {
                media_id: MediaID::Episode(EpisodeID(row.id)),
                title: row.title,
            })
            .collect())
    }
}

#[async_trait]
impl QueryRepository for PostgresQueryRepository {
    /// Execute a media query using optimized SQL queries with proper indexing
    async fn query_media(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        // Handle watch status filter separately if provided
        if let Some(watch_filter) = &query.filters.watch_status {
            return self.query_media_by_watch_status(query, watch_filter).await;
        }

        // Title-only fuzzy search: use Postgres for candidate retrieval, then
        // apply skim/fzf-like scoring to produce relevance-ordered results.
        if let Some(search) = &query.search
            && supports_title_only_search(search)
            && matches!(
                query.filters.media_type,
                None | Some(MediaTypeFilter::Movie)
                    | Some(MediaTypeFilter::Series)
                    | Some(MediaTypeFilter::Episode)
            )
        {
            return self.query_media_by_title_search(query, search).await;
        }

        // Check if we can use presorted indices for single library queries
        if query.filters.library_ids.len() == 1 && query.search.is_none() {
            // TODO: Potentially use precomputed indices here in the future
        }

        // Build the main SQL query
        let results = match query.filters.media_type {
            Some(MediaTypeFilter::Movie) => self.query_movies(query).await?,
            Some(MediaTypeFilter::Series) => self.query_tv_shows(query).await?,
            Some(MediaTypeFilter::Season) | Some(MediaTypeFilter::Episode) => {
                // For Season/Episode filters, query TV shows and filter results
                self.query_tv_shows(query).await?
            }
            None => {
                if query.search.is_some() {
                    self.query_multi_type_search(query).await?
                } else {
                    // Default to movie listings when no media type is provided
                    self.query_movies(query).await?
                }
            }
        };

        Ok(results)
    }

    async fn query_movies(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        let mut sql_builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mf.id AS file_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata,
                mf.library_id,
                mm.release_date,
                mm.vote_average,
                mm.runtime,
                mm.popularity,
                mm.overview
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            LEFT JOIN movie_metadata mm ON mr.id = mm.movie_id
            WHERE 1=1
            "#,
        );

        // Add library filter
        if !query.filters.library_ids.is_empty() {
            sql_builder.push(" AND mr.library_id = ANY(");
            sql_builder.push_bind(&query.filters.library_ids);
            sql_builder.push(")");
        }

        // Add genre filter
        if !query.filters.genres.is_empty() {
            sql_builder.push(
                " AND EXISTS (SELECT 1 FROM movie_genres mg WHERE mg.movie_id = mr.id AND mg.name = ANY("
            );
            sql_builder.push_bind(&query.filters.genres);
            sql_builder.push("))");
        }

        // Add year range filter
        if let Some(range) = &query.filters.year_range {
            sql_builder.push(
                " AND mm.release_date IS NOT NULL AND EXTRACT(YEAR FROM mm.release_date)::INT BETWEEN "
            );
            sql_builder.push_bind(range.min as i32);
            sql_builder.push(" AND ");
            sql_builder.push_bind(range.max as i32);
        }

        // Add rating range filter
        if let Some(range) = &query.filters.rating_range {
            sql_builder.push(" AND mm.vote_average BETWEEN ");
            sql_builder.push_bind(rating_bound(range.min));
            sql_builder.push(" AND ");
            sql_builder.push_bind(rating_bound(range.max));
        }

        // Add search query if present
        if let Some(search) = &query.search {
            self.add_search_clause(&mut sql_builder, search);
        }

        // Add sorting
        self.add_movie_sort_clause(&mut sql_builder, &query.sort);

        // Add pagination
        sql_builder.push(" LIMIT ");
        sql_builder.push_bind(query.pagination.limit as i64);
        sql_builder.push(" OFFSET ");
        sql_builder.push_bind(query.pagination.offset as i64);

        // Execute query
        let rows =
            sql_builder
                .build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "Database query failed: {}",
                        e
                    ))
                })?;

        // Convert rows to MediaWithStatus
        let mut results = Vec::new();
        for row in rows {
            let id = MovieID(row.get::<Uuid, _>("id"));

            // Get watch status if user context provided
            let watch_status = if let Some(user_id) = query.user_context {
                self.get_movie_watch_status(user_id, &id).await?
            } else {
                None
            };

            results.push(MediaWithStatus {
                id: MediaID::Movie(id),
                watch_status,
            });
        }

        Ok(results)
    }

    async fn query_tv_shows(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        // For TV shows, we need to handle the hierarchy differently
        // We'll use a LATERAL JOIN to efficiently fetch series with their episodes
        let mut sql_builder = QueryBuilder::<Postgres>::new(
            r#"
            WITH series_data AS (
                SELECT
                    sr.id,
                    sr.library_id,
                    sr.tmdb_id,
                    sr.title,
                    sr.theme_color,
                    sr.discovered_at,
                    sr.created_at,
                    sm.first_air_date,
                    sm.vote_average,
                    sm.popularity,
                    sm.overview
                FROM series sr
                LEFT JOIN series_metadata sm ON sr.id = sm.series_id
                WHERE 1=1
            "#,
        );

        // Add library filter
        if !query.filters.library_ids.is_empty() {
            sql_builder.push(" AND sr.library_id = ANY(");
            sql_builder.push_bind(&query.filters.library_ids);
            sql_builder.push(")");
        }

        // Add genre filter
        if !query.filters.genres.is_empty() {
            sql_builder.push(
                " AND EXISTS (SELECT 1 FROM series_genres sg WHERE sg.series_id = sr.id AND sg.name = ANY("
            );
            sql_builder.push_bind(&query.filters.genres);
            sql_builder.push("))");
        }

        // Add year range filter
        if let Some(range) = &query.filters.year_range {
            sql_builder.push(
                " AND sm.first_air_date IS NOT NULL AND EXTRACT(YEAR FROM sm.first_air_date)::INT BETWEEN "
            );
            sql_builder.push_bind(range.min as i32);
            sql_builder.push(" AND ");
            sql_builder.push_bind(range.max as i32);
        }

        // Add rating range filter
        if let Some(range) = &query.filters.rating_range {
            sql_builder.push(" AND sm.vote_average BETWEEN ");
            sql_builder.push_bind(rating_bound(range.min));
            sql_builder.push(" AND ");
            sql_builder.push_bind(rating_bound(range.max));
        }

        if let Some(search) = &query.search {
            self.add_series_search_clause(&mut sql_builder, search);
        }

        sql_builder.push(
            r#"
            )
            SELECT
                sd.id AS series_id,
                sd.library_id AS series_library_id,
                sd.tmdb_id AS series_tmdb_id,
                sd.title AS series_title,
                sd.theme_color AS series_theme_color,
                sd.discovered_at AS series_discovered_at,
                sd.created_at AS series_created_at,
                sd.first_air_date AS series_first_air_date,
                sd.vote_average AS series_vote_average,
                sd.popularity AS series_popularity,
                sd.overview AS series_overview,
                sn.id AS season_id,
                sn.season_number,
                sn.discovered_at AS season_discovered_at,
                sn.created_at AS season_created_at,
                ep.id AS episode_id,
                ep.season_number AS ep_season,
                ep.episode_number,
                ep.file_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.library_id AS file_library_id
            FROM series_data sd
            LEFT JOIN LATERAL (
                SELECT * FROM season_references
                WHERE series_id = sd.id
                ORDER BY season_number
            ) sn ON true
            LEFT JOIN LATERAL (
                SELECT * FROM episode_references
                WHERE series_id = sd.id AND season_id = sn.id
                ORDER BY season_number, episode_number
            ) ep ON true
            LEFT JOIN media_files mf ON ep.file_id = mf.id
            "#,
        );

        // Add sorting for series
        self.add_series_sort_clause(&mut sql_builder, &query.sort);

        // Note: Pagination for hierarchical data is complex
        // We'll apply it after building the hierarchy

        let rows =
            sql_builder
                .build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "Database query failed: {}",
                        e
                    ))
                })?;

        // Build hierarchical structure from flat rows
        let results = self.build_tv_hierarchy_from_rows(rows, query).await?;

        // Apply pagination to the final results
        let start = query.pagination.offset;
        if start >= results.len() {
            return Ok(Vec::new());
        }
        let end = (start + query.pagination.limit).min(results.len());

        Ok(results[start..end].to_vec())
    }

    async fn query_multi_type_search(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        let fetch_limit = query
            .pagination
            .offset
            .saturating_add(query.pagination.limit);

        if fetch_limit == 0 {
            return Ok(Vec::new());
        }

        let mut base_query = query.clone();
        base_query.pagination.offset = 0;
        base_query.pagination.limit = fetch_limit;

        let mut movie_query = base_query.clone();
        movie_query.filters.media_type = Some(MediaTypeFilter::Movie);

        let mut series_query = base_query;
        series_query.filters.media_type = Some(MediaTypeFilter::Series);

        let movies = self.query_movies(&movie_query).await?;
        let series = self.query_tv_shows(&series_query).await?;

        let mut movie_iter = movies.into_iter();
        let mut series_iter = series.into_iter();
        let mut combined = Vec::with_capacity(fetch_limit);

        loop {
            let mut added = false;

            if let Some(movie) = movie_iter.next() {
                combined.push(movie);
                added = true;
            }

            if combined.len() >= fetch_limit {
                break;
            }

            if let Some(series_item) = series_iter.next() {
                combined.push(series_item);
                added = true;
            }

            if combined.len() >= fetch_limit {
                break;
            }

            if !added {
                break;
            }
        }

        if combined.len() < fetch_limit {
            combined.extend(movie_iter);
            if combined.len() < fetch_limit {
                combined.extend(series_iter);
            }
        }

        if combined.len() > fetch_limit {
            combined.truncate(fetch_limit);
        }

        let skip = query.pagination.offset.min(combined.len());
        if skip > 0 {
            let _ = combined.drain(0..skip);
        }

        if combined.len() > query.pagination.limit {
            combined.truncate(query.pagination.limit);
        }

        Ok(combined)
    }

    async fn query_media_by_watch_status(
        &self,
        query: &MediaQuery,
        watch_filter: &WatchStatusFilter,
    ) -> Result<Vec<MediaWithStatus>> {
        let user_id = query.user_context.ok_or_else(|| {
            MediaError::InvalidMedia(
                "User context required for watch status filter".to_string(),
            )
        })?;

        match watch_filter {
            WatchStatusFilter::InProgress => {
                self.query_in_progress_media(user_id, query).await
            }
            WatchStatusFilter::Completed => {
                self.query_completed_media(user_id, query).await
            }
            WatchStatusFilter::Unwatched => {
                self.query_unwatched_media(user_id, query).await
            }
            WatchStatusFilter::RecentlyWatched { days } => {
                self.query_recently_watched_media(user_id, *days, query)
                    .await
            }
        }
    }

    async fn query_in_progress_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        let rows = sqlx::query_as!(
            InProgressRow,
            r#"
            WITH inprog AS (
                SELECT media_uuid, media_type, position, duration, last_watched
                FROM user_watch_progress
                WHERE user_id = $1
                  AND position > 0
                  AND (duration > 0) AND (position / duration) < 0.95
            )
            SELECT * FROM (
                SELECT
                    mr.id AS "id!",
                    mf.id AS "file_id!",
                    inprog.position::real AS "position!",
                    inprog.duration::real AS "duration!",
                    inprog.last_watched::bigint AS "last_watched!",
                    0::int4                  AS "media_kind!"
                FROM inprog
                JOIN movie_references mr ON inprog.media_uuid = mr.id AND inprog.media_type = 0
                JOIN media_files mf ON mr.file_id = mf.id

                UNION ALL

                SELECT
                    er.id              AS "id!",
                    mf.id              AS "file_id!",
                    inprog.position::real AS "position!",
                    inprog.duration::real AS "duration!",
                    inprog.last_watched::bigint AS "last_watched!",
                    3::int4                  AS "media_kind!"
                FROM inprog
                JOIN episode_references er ON inprog.media_uuid = er.id AND inprog.media_type = 3
                JOIN media_files mf ON er.file_id = mf.id
            ) AS inprog_rows
            ORDER BY inprog_rows."last_watched!" DESC
            LIMIT $2 OFFSET $3
            "#,
            user_id,
            query.pagination.limit as i64,
            query.pagination.offset as i64
        )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let kind = row.media_kind;
            let id = if kind == 0 {
                MediaID::Movie(MovieID(row.id))
            } else {
                MediaID::Series(SeriesID(row.id))
            };

            let watch_status = InProgressItem {
                media_id: row.file_id,
                position: row.position,
                duration: row.duration,
                last_watched: row.last_watched,
            };

            results.push(MediaWithStatus {
                id,
                watch_status: Some(ItemWatchStatus::InProgress(watch_status)),
            });
        }

        Ok(results)
    }

    async fn query_completed_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        let rows = sqlx::query_as!(
            CompletedRow,
            r#"
            WITH completed AS (
                SELECT media_uuid, media_type, completed_at
                FROM user_completed_media
                WHERE user_id = $1
            )
            SELECT * FROM (
                SELECT
                    mr.id AS "id!",
                    completed.completed_at::bigint AS "last_watched!",
                    0::int4                  AS "media_kind!"
                FROM completed
                JOIN movie_references mr ON completed.media_uuid = mr.id AND completed.media_type = 0

                UNION ALL

                SELECT
                    er.id              AS "id!",
                    completed.completed_at::bigint AS "last_watched!",
                    3::int4                  AS "media_kind!"
                FROM completed
                JOIN episode_references er ON completed.media_uuid = er.id AND completed.media_type = 3
            ) AS completed_rows
            ORDER BY completed_rows."last_watched!" DESC
            LIMIT $2 OFFSET $3
            "#,
            user_id,
            query.pagination.limit as i64,
            query.pagination.offset as i64
        )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let kind = row.media_kind;
            let id = if kind == 0 {
                MediaID::Movie(MovieID(row.id))
            } else {
                MediaID::Episode(EpisodeID(row.id))
            };
            let completed_item = CompletedItem {
                media_id: id,
                last_watched: row.last_watched,
            };

            results.push(MediaWithStatus {
                id,
                watch_status: Some(ItemWatchStatus::Completed(completed_item)),
            });
        }

        Ok(results)
    }

    async fn query_unwatched_media(
        &self,
        _user_id: Uuid,
        _query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        // Query media that doesn't have watch progress or completion records
        // This is more complex as it requires exclusion joins
        todo!("Implement unwatched media query")
    }

    async fn query_recently_watched_media(
        &self,
        _user_id: Uuid,
        _recent_days: u32,
        _query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        // Query media watched within the specified number of days
        todo!("Implement recently watched media query")
    }

    fn add_search_clause(
        &self,
        sql_builder: &mut QueryBuilder<Postgres>,
        search: &SearchQuery,
    ) {
        let include_title = search.fields.is_empty()
            || search.fields.contains(&SearchField::All)
            || search.fields.contains(&SearchField::Title);
        let include_overview = search.fields.is_empty()
            || search.fields.contains(&SearchField::All)
            || search.fields.contains(&SearchField::Overview);
        let include_cast = search.fields.is_empty()
            || search.fields.contains(&SearchField::All)
            || search.fields.contains(&SearchField::Cast);

        // If no supported fields are requested, avoid altering the query
        if !include_title && !include_overview && !include_cast {
            return;
        }

        sql_builder.push(" AND (");
        let mut has_clause = false;

        if search.fuzzy {
            if include_title {
                if has_clause {
                    sql_builder.push(" OR ");
                }
                has_clause = true;
                sql_builder.push("mr.title % ");
                sql_builder.push_bind(search.text.clone());
            }

            if include_overview {
                if has_clause {
                    sql_builder.push(" OR ");
                }
                has_clause = true;
                sql_builder.push("mm.overview % ");
                sql_builder.push_bind(search.text.clone());
            }

            if include_cast {
                if has_clause {
                    sql_builder.push(" OR ");
                }
                has_clause = true;
                sql_builder.push(
                    "EXISTS (SELECT 1 FROM movie_cast search_mc JOIN persons search_p ON search_p.id = search_mc.person_id WHERE search_mc.movie_id = mr.id AND search_p.name % "
                );
                sql_builder.push_bind(search.text.clone());
                sql_builder.push(")");
            }
        } else {
            let like_pattern = format!("%{}%", search.text);

            if include_title {
                if has_clause {
                    sql_builder.push(" OR ");
                }
                has_clause = true;
                sql_builder.push("mr.title ILIKE ");
                sql_builder.push_bind(like_pattern.clone());
            }

            if include_overview {
                if has_clause {
                    sql_builder.push(" OR ");
                }
                has_clause = true;
                sql_builder.push("mm.overview ILIKE ");
                sql_builder.push_bind(like_pattern.clone());
            }

            if include_cast {
                if has_clause {
                    sql_builder.push(" OR ");
                }
                has_clause = true;
                sql_builder.push(
                    "EXISTS (SELECT 1 FROM movie_cast search_mc JOIN persons search_p ON search_p.id = search_mc.person_id WHERE search_mc.movie_id = mr.id AND search_p.name ILIKE "
                );
                sql_builder.push_bind(like_pattern);
                sql_builder.push(")");
            }
        }

        if !has_clause {
            sql_builder.push("FALSE");
        }

        sql_builder.push(")");
    }

    fn add_movie_sort_clause(
        &self,
        sql_builder: &mut QueryBuilder<Postgres>,
        sort: &SortCriteria,
    ) {
        sql_builder.push(" ORDER BY ");

        let (field, null_position) = match sort.primary {
            SortBy::Title => ("LOWER(mr.title)", "LAST"),
            SortBy::DateAdded => ("mf.discovered_at", "LAST"),
            SortBy::CreatedAt => ("mf.created_at", "LAST"),
            SortBy::ReleaseDate => ("mm.release_date", "LAST"),
            SortBy::Rating => ("mm.vote_average", "LAST"),
            SortBy::Runtime => ("mm.runtime", "LAST"),
            _ => ("mf.discovered_at", "LAST"), // Default to date added
        };

        sql_builder.push(field);

        match sort.order {
            SortOrder::Ascending => sql_builder.push(" ASC NULLS "),
            SortOrder::Descending => sql_builder.push(" DESC NULLS "),
        };
        sql_builder.push(null_position);
    }

    fn add_series_sort_clause(
        &self,
        sql_builder: &mut QueryBuilder<Postgres>,
        sort: &SortCriteria,
    ) {
        sql_builder.push(" ORDER BY ");

        let (field, null_position) = match sort.primary {
            SortBy::Title => ("LOWER(sd.title)", "LAST"),
            SortBy::DateAdded => (
                "COALESCE(mf.discovered_at, sn.discovered_at, sd.discovered_at)",
                "LAST",
            ),
            SortBy::CreatedAt => (
                "COALESCE(mf.created_at, sn.created_at, sd.created_at)",
                "LAST",
            ),
            SortBy::ReleaseDate => ("sd.first_air_date", "LAST"),
            SortBy::Rating => ("sd.vote_average", "LAST"),
            _ => (
                "COALESCE(mf.discovered_at, sn.discovered_at, sd.discovered_at)",
                "LAST",
            ),
        };

        sql_builder.push(field);

        match sort.order {
            SortOrder::Ascending => sql_builder.push(" ASC NULLS "),
            SortOrder::Descending => sql_builder.push(" DESC NULLS "),
        };
        sql_builder.push(null_position);

        sql_builder.push(", sd.id, sn.season_number, ep.episode_number");
    }

    fn add_series_search_clause(
        &self,
        sql_builder: &mut QueryBuilder<Postgres>,
        search: &SearchQuery,
    ) {
        let include_title = search.fields.is_empty()
            || search.fields.contains(&SearchField::All)
            || search.fields.contains(&SearchField::Title);
        let include_overview = search.fields.is_empty()
            || search.fields.contains(&SearchField::All)
            || search.fields.contains(&SearchField::Overview);
        let include_cast = search.fields.is_empty()
            || search.fields.contains(&SearchField::All)
            || search.fields.contains(&SearchField::Cast);

        if !include_title && !include_overview && !include_cast {
            return;
        }

        sql_builder.push(" AND (");
        let mut has_clause = false;

        if search.fuzzy {
            if include_title {
                sql_builder.push("sr.title % ");
                sql_builder.push_bind(search.text.clone());
                has_clause = true;
            }

            if include_overview {
                if has_clause {
                    sql_builder.push(" OR ");
                }
                sql_builder.push("sm.overview % ");
                sql_builder.push_bind(search.text.clone());
                has_clause = true;
            }

            if include_cast {
                if has_clause {
                    sql_builder.push(" OR ");
                }
                sql_builder.push(
                    "EXISTS (SELECT 1 FROM series_cast search_sc JOIN persons search_p ON search_p.id = search_sc.person_id WHERE search_sc.series_id = sr.id AND search_p.name % "
                );
                sql_builder.push_bind(search.text.clone());
                sql_builder.push(")");
            }
        } else {
            let like_pattern = format!("%{}%", search.text);

            if include_title {
                sql_builder.push("sr.title ILIKE ");
                sql_builder.push_bind(like_pattern.clone());
                has_clause = true;
            }

            if include_overview {
                if has_clause {
                    sql_builder.push(" OR ");
                }
                sql_builder.push("sm.overview ILIKE ");
                sql_builder.push_bind(like_pattern.clone());
                has_clause = true;
            }

            if include_cast {
                if has_clause {
                    sql_builder.push(" OR ");
                }
                sql_builder.push(
                    "EXISTS (SELECT 1 FROM series_cast search_sc JOIN persons search_p ON search_p.id = search_sc.person_id WHERE search_sc.series_id = sr.id AND search_p.name ILIKE "
                );
                sql_builder.push_bind(like_pattern);
                sql_builder.push(")");
            }
        }

        sql_builder.push(")");
    }

    async fn build_tv_hierarchy_from_rows(
        &self,
        rows: Vec<sqlx::postgres::PgRow>,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        use sqlx::Row;

        let mut media_ids: HashSet<MediaID> = HashSet::new();

        let mut results = Vec::new();

        for row in rows {
            let id: MediaID = MediaID::Series(SeriesID(row.get("series_id")));

            // Create or get series reference
            if !media_ids.contains(&id) {
                media_ids.insert(id);

                // Add series to results
                results.push(MediaWithStatus {
                    id,
                    watch_status: None,
                });
            }

            // Process season if present
            if let Ok(season_id) = row.try_get::<Uuid, _>("season_id") {
                let season_id = MediaID::Season(SeasonID(season_id));

                if media_ids.contains(&id) {
                    media_ids.insert(season_id);

                    // Add season to results
                    results.push(MediaWithStatus {
                        id: season_id,
                        watch_status: None,
                    });
                }
            }

            // Process episode if present
            if let Ok(episode_id) = row.try_get::<Uuid, _>("episode_id") {
                let episode_media_id = MediaID::Episode(EpisodeID(episode_id));

                // Get watch status if user context provided
                let watch_status = if let Some(user_id) = query.user_context {
                    self.get_episode_watch_status(
                        user_id,
                        &EpisodeID(episode_id),
                    )
                    .await?
                } else {
                    None
                };

                results.push(MediaWithStatus {
                    id: episode_media_id,
                    watch_status,
                });
            }
        }

        Ok(results)
    }

    async fn get_movie_watch_status(
        &self,
        user_id: Uuid,
        movie_id: &MovieID,
    ) -> Result<Option<ItemWatchStatus>> {
        // Check watch progress
        let progress = sqlx::query!(
            r#"
            SELECT position, duration, last_watched
            FROM user_watch_progress
            WHERE user_id = $1
                AND media_uuid = $2
            "#,
            user_id,
            movie_id.to_uuid()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get watch status: {}", e))
        })?;

        if let Some(row) = progress {
            // Check if completed (>95% watched)
            let is_completed =
                (row.position as f64 / row.duration as f64) >= 0.95;
            if is_completed {
                let watch_status = InProgressItem {
                    media_id: movie_id.to_uuid(),
                    position: row.position,
                    duration: row.duration,
                    last_watched: row.last_watched,
                };
                return Ok(Some(ItemWatchStatus::InProgress(watch_status)));
            } else {
                let watch_status = CompletedItem {
                    media_id: MediaID::Movie(*movie_id),
                    last_watched: row.last_watched,
                };
                return Ok(Some(ItemWatchStatus::Completed(watch_status)));
            }
        }

        // Check completed media
        let completed_opt = sqlx::query!(
            r#"
            SELECT completed_at
            FROM user_completed_media
            WHERE user_id = $1
                AND media_uuid = $2
            "#,
            user_id,
            movie_id.to_uuid()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to check completion: {}", e))
        })?;

        if let Some(completed) = completed_opt {
            Ok(Some(ItemWatchStatus::Completed(CompletedItem {
                media_id: MediaID::Movie(*movie_id),
                last_watched: completed.completed_at,
            })))
        } else {
            Ok(None)
        }
    }

    async fn get_episode_watch_status(
        &self,
        user_id: Uuid,
        episode_id: &EpisodeID,
    ) -> Result<Option<ItemWatchStatus>> {
        // Similar to get_movie_watch_status but for episodes
        let progress = sqlx::query!(
            r#"
            SELECT position, duration, last_watched
            FROM user_watch_progress
            WHERE user_id = $1
                AND media_uuid = $2
            "#,
            user_id,
            episode_id.to_uuid()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get watch status: {}", e))
        })?;

        if let Some(row) = progress {
            let is_completed =
                (row.position as f64 / row.duration as f64) >= 0.95;

            if is_completed {
                let watch_status = InProgressItem {
                    media_id: episode_id.to_uuid(),
                    position: row.position,
                    duration: row.duration,
                    last_watched: row.last_watched,
                };
                return Ok(Some(ItemWatchStatus::InProgress(watch_status)));
            } else {
                let watch_status = CompletedItem {
                    media_id: MediaID::Episode(*episode_id),
                    last_watched: row.last_watched,
                };
                return Ok(Some(ItemWatchStatus::Completed(watch_status)));
            }
        }

        let completed_opt = sqlx::query!(
            r#"
            SELECT completed_at
            FROM user_completed_media
            WHERE user_id = $1
                AND media_uuid = $2
            "#,
            user_id,
            episode_id.to_uuid()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to check completion: {}", e))
        })?;

        if let Some(completed) = completed_opt {
            Ok(Some(ItemWatchStatus::Completed(CompletedItem {
                media_id: MediaID::Episode(*episode_id),
                last_watched: completed.completed_at,
            })))
        } else {
            Ok(None)
        }
    }
}

fn compute_candidate_limit(fetch_limit: usize) -> i64 {
    // Keep this bounded: candidates are scored in Rust to provide fzf/skim-like ordering,
    // while Postgres is used to keep the candidate set reasonable via indexes.
    let scaled = fetch_limit.saturating_mul(40);
    let clamped = scaled.clamp(200, 5_000);
    clamped as i64
}

fn escape_like_literal(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("\\%"),
            '_' => out.push_str("\\_"),
            other => out.push(other),
        }
    }
    out
}

fn similarity_threshold(query_len: usize) -> f32 {
    match query_len {
        0..=4 => 0.05,
        5..=8 => 0.10,
        _ => 0.15,
    }
}
