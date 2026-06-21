use std::collections::HashSet;

use async_trait::async_trait;
use sqlx::{PgPool, types::Uuid};
// Use Media enum from our domain prelude, not tmdb_api

use crate::domain::watch::{CompletedItem, ItemWatchStatus, WatchResumePolicy};
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

const MOVIE_WATCH_KIND: i32 = 0;
const EPISODE_WATCH_KIND: i32 = 3;

fn rating_bound(value: RatingValue) -> f32 {
    value as f32 / 10f32.powi(RATING_DECIMAL_SCALE as i32)
}

fn is_completed_progress(position: f32, duration: f32) -> bool {
    WatchResumePolicy::from_env().is_completed_progress(position, duration)
}

fn media_id_from_watch_kind(kind: i32, id: Uuid) -> Result<MediaID> {
    match kind {
        MOVIE_WATCH_KIND => Ok(MediaID::Movie(MovieID(id))),
        EPISODE_WATCH_KIND => Ok(MediaID::Episode(EpisodeID(id))),
        other => Err(MediaError::Internal(format!(
            "Unexpected watch-status media kind: {other}"
        ))),
    }
}

fn watch_status_from_progress(
    media_id: MediaID,
    position: f32,
    duration: f32,
    last_watched: i64,
) -> ItemWatchStatus {
    if is_completed_progress(position, duration) {
        ItemWatchStatus::Completed(CompletedItem {
            media_id,
            last_watched,
        })
    } else {
        ItemWatchStatus::InProgress(InProgressItem {
            media_id: *media_id.as_uuid(),
            position,
            duration,
            last_watched,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PostgresQueryRepository {
    pool: PgPool,
}

#[derive(Debug, sqlx::FromRow)]
struct InProgressRow {
    id: Uuid,
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

#[derive(Debug, Clone)]
struct SearchSqlParams {
    apply: bool,
    fuzzy: bool,
    text: String,
    like_pattern: String,
    include_title: bool,
    include_overview: bool,
    include_cast: bool,
}

impl SearchSqlParams {
    fn from_query(search: Option<&SearchQuery>) -> Self {
        let Some(search) = search else {
            return Self::disabled();
        };

        let include_title = search.fields.is_empty()
            || search.fields.contains(&SearchField::All)
            || search.fields.contains(&SearchField::Title);
        let include_overview = search.fields.is_empty()
            || search.fields.contains(&SearchField::All)
            || search.fields.contains(&SearchField::Overview);
        let include_cast = search.fields.is_empty()
            || search.fields.contains(&SearchField::All)
            || search.fields.contains(&SearchField::Cast);
        let apply = include_title || include_overview || include_cast;

        Self {
            apply,
            fuzzy: search.fuzzy,
            text: search.text.clone(),
            like_pattern: format!("%{}%", search.text),
            include_title,
            include_overview,
            include_cast,
        }
    }

    fn disabled() -> Self {
        Self {
            apply: false,
            fuzzy: false,
            text: String::new(),
            like_pattern: String::new(),
            include_title: false,
            include_overview: false,
            include_cast: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum MediaSqlSortKey {
    Title = 0,
    DateAdded = 1,
    CreatedAt = 2,
    ReleaseDate = 3,
    Rating = 4,
    Runtime = 5,
}

impl MediaSqlSortKey {
    fn for_movie(sort: &SortCriteria) -> Self {
        match sort.primary {
            SortBy::Title => Self::Title,
            SortBy::DateAdded => Self::DateAdded,
            SortBy::CreatedAt => Self::CreatedAt,
            SortBy::ReleaseDate => Self::ReleaseDate,
            SortBy::Rating => Self::Rating,
            SortBy::Runtime => Self::Runtime,
            _ => Self::DateAdded,
        }
    }

    fn for_series(sort: &SortCriteria) -> Self {
        match sort.primary {
            SortBy::Title => Self::Title,
            SortBy::DateAdded => Self::DateAdded,
            SortBy::CreatedAt => Self::CreatedAt,
            SortBy::ReleaseDate => Self::ReleaseDate,
            SortBy::Rating => Self::Rating,
            _ => Self::DateAdded,
        }
    }

    fn as_i16(self) -> i16 {
        self as i16
    }
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

        let rows = if query_len <= 2 {
            let prefix_pattern = format!("{}%", escaped);
            sqlx::query_as!(
                TitleCandidateRow,
                r#"
                SELECT mr.id AS "id!", mr.title AS "title!"
                FROM movie_references mr
                WHERE (cardinality($1::uuid[]) = 0 OR mr.library_id = ANY($1))
                  AND mr.title ILIKE $2 ESCAPE E'\\'
                ORDER BY LOWER(mr.title) ASC, LENGTH(mr.title) ASC
                LIMIT $3
                "#,
                library_ids,
                prefix_pattern,
                candidate_limit
            )
            .fetch_all(&self.pool)
            .await
        } else {
            let similarity_threshold = similarity_threshold(query_len);
            let substring_pattern = format!("%{}%", escaped);
            let subsequence_pattern = build_subsequence_regex(search_text);
            let token_like_pattern = first_token_like_pattern(search_text);
            let prefix_pattern = format!("{}%", escaped);

            sqlx::query_as!(
                TitleCandidateRow,
                r#"
                SELECT mr.id AS "id!", mr.title AS "title!"
                FROM movie_references mr
                WHERE (cardinality($1::uuid[]) = 0 OR mr.library_id = ANY($1))
                  AND (
                    mr.title ILIKE $2 ESCAPE E'\\'
                    OR similarity(mr.title, $3) > $4::real
                    OR ($5::text IS NOT NULL AND LOWER(mr.title) ~ $5)
                    OR ($6::text IS NOT NULL AND LOWER(mr.title) LIKE $6)
                  )
                ORDER BY
                  CASE
                    WHEN LOWER(mr.title) = LOWER($3) THEN 0
                    WHEN LOWER(mr.title) LIKE LOWER($7) ESCAPE E'\\' THEN 1
                    WHEN mr.title ILIKE $2 ESCAPE E'\\' THEN 2
                    ELSE 3
                  END,
                  similarity(mr.title, $3) DESC,
                  LENGTH(mr.title) ASC,
                  LOWER(mr.title) ASC
                LIMIT $8
                "#,
                library_ids,
                substring_pattern,
                search_text,
                similarity_threshold,
                subsequence_pattern.as_deref(),
                token_like_pattern.as_deref(),
                prefix_pattern,
                candidate_limit
            )
            .fetch_all(&self.pool)
            .await
        }
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

        let rows = if query_len <= 2 {
            let prefix_pattern = format!("{}%", escaped);
            sqlx::query_as!(
                TitleCandidateRow,
                r#"
                SELECT s.id AS "id!", s.title AS "title!"
                FROM series s
                INNER JOIN series_bundle_versioning sbv
                  ON sbv.series_id = s.id
                 AND sbv.library_id = s.library_id
                WHERE sbv.finalized = true
                  AND (cardinality($1::uuid[]) = 0 OR s.library_id = ANY($1))
                  AND s.title ILIKE $2 ESCAPE E'\\'
                ORDER BY LOWER(s.title) ASC, LENGTH(s.title) ASC
                LIMIT $3
                "#,
                library_ids,
                prefix_pattern,
                candidate_limit
            )
            .fetch_all(&self.pool)
            .await
        } else {
            let similarity_threshold = similarity_threshold(query_len);
            let substring_pattern = format!("%{}%", escaped);
            let subsequence_pattern = build_subsequence_regex(search_text);
            let token_like_pattern = first_token_like_pattern(search_text);
            let prefix_pattern = format!("{}%", escaped);

            sqlx::query_as!(
                TitleCandidateRow,
                r#"
                SELECT s.id AS "id!", s.title AS "title!"
                FROM series s
                INNER JOIN series_bundle_versioning sbv
                  ON sbv.series_id = s.id
                 AND sbv.library_id = s.library_id
                WHERE sbv.finalized = true
                  AND (cardinality($1::uuid[]) = 0 OR s.library_id = ANY($1))
                  AND (
                    s.title ILIKE $2 ESCAPE E'\\'
                    OR similarity(s.title, $3) > $4::real
                    OR ($5::text IS NOT NULL AND LOWER(s.title) ~ $5)
                    OR ($6::text IS NOT NULL AND LOWER(s.title) LIKE $6)
                  )
                ORDER BY
                  CASE
                    WHEN LOWER(s.title) = LOWER($3) THEN 0
                    WHEN LOWER(s.title) LIKE LOWER($7) ESCAPE E'\\' THEN 1
                    WHEN s.title ILIKE $2 ESCAPE E'\\' THEN 2
                    ELSE 3
                  END,
                  similarity(s.title, $3) DESC,
                  LENGTH(s.title) ASC,
                  LOWER(s.title) ASC
                LIMIT $8
                "#,
                library_ids,
                substring_pattern,
                search_text,
                similarity_threshold,
                subsequence_pattern.as_deref(),
                token_like_pattern.as_deref(),
                prefix_pattern,
                candidate_limit
            )
            .fetch_all(&self.pool)
            .await
        }
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

        let rows = if query_len <= 2 {
            let prefix_pattern = format!("{}%", escaped);
            sqlx::query_as!(
                TitleCandidateRow,
                r#"
                SELECT er.id AS "id!", em.name AS "title!"
                FROM episode_references er
                JOIN episode_metadata em ON em.episode_id = er.id
                JOIN series s ON s.id = er.series_id
                INNER JOIN series_bundle_versioning sbv
                  ON sbv.series_id = s.id
                 AND sbv.library_id = s.library_id
                WHERE em.name IS NOT NULL
                  AND sbv.finalized = true
                  AND (cardinality($1::uuid[]) = 0 OR s.library_id = ANY($1))
                  AND em.name ILIKE $2 ESCAPE E'\\'
                ORDER BY LOWER(em.name) ASC, LENGTH(em.name) ASC
                LIMIT $3
                "#,
                library_ids,
                prefix_pattern,
                candidate_limit
            )
            .fetch_all(&self.pool)
            .await
        } else {
            let similarity_threshold = similarity_threshold(query_len);
            let substring_pattern = format!("%{}%", escaped);
            let subsequence_pattern = build_subsequence_regex(search_text);
            let token_like_pattern = first_token_like_pattern(search_text);
            let prefix_pattern = format!("{}%", escaped);

            sqlx::query_as!(
                TitleCandidateRow,
                r#"
                SELECT er.id AS "id!", em.name AS "title!"
                FROM episode_references er
                JOIN episode_metadata em ON em.episode_id = er.id
                JOIN series s ON s.id = er.series_id
                INNER JOIN series_bundle_versioning sbv
                  ON sbv.series_id = s.id
                 AND sbv.library_id = s.library_id
                WHERE em.name IS NOT NULL
                  AND sbv.finalized = true
                  AND (cardinality($1::uuid[]) = 0 OR s.library_id = ANY($1))
                  AND (
                    em.name ILIKE $2 ESCAPE E'\\'
                    OR similarity(em.name, $3) > $4::real
                    OR ($5::text IS NOT NULL AND LOWER(em.name) ~ $5)
                    OR ($6::text IS NOT NULL AND LOWER(em.name) LIKE $6)
                  )
                ORDER BY
                  CASE
                    WHEN LOWER(em.name) = LOWER($3) THEN 0
                    WHEN LOWER(em.name) LIKE LOWER($7) ESCAPE E'\\' THEN 1
                    WHEN em.name ILIKE $2 ESCAPE E'\\' THEN 2
                    ELSE 3
                  END,
                  similarity(em.name, $3) DESC,
                  LENGTH(em.name) ASC,
                  LOWER(em.name) ASC
                LIMIT $8
                "#,
                library_ids,
                substring_pattern,
                search_text,
                similarity_threshold,
                subsequence_pattern.as_deref(),
                token_like_pattern.as_deref(),
                prefix_pattern,
                candidate_limit
            )
            .fetch_all(&self.pool)
            .await
        }
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
        let year_min = query.filters.year_range.map(|range| range.min as i32);
        let year_max = query.filters.year_range.map(|range| range.max as i32);
        let rating_min = query
            .filters
            .rating_range
            .map(|range| rating_bound(range.min));
        let rating_max = query
            .filters
            .rating_range
            .map(|range| rating_bound(range.max));
        let search = SearchSqlParams::from_query(query.search.as_ref());
        let sort_key = MediaSqlSortKey::for_movie(&query.sort).as_i16();
        let sort_ascending = matches!(query.sort.order, SortOrder::Ascending);

        let rows = sqlx::query!(
            r#"
            SELECT mr.id AS "id!"
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            LEFT JOIN movie_metadata mm ON mr.id = mm.movie_id
            WHERE mf.is_available = TRUE
              AND (cardinality($1::uuid[]) = 0 OR mr.library_id = ANY($1))
              AND (cardinality($2::text[]) = 0 OR EXISTS (
                    SELECT 1 FROM movie_genres mg
                    WHERE mg.movie_id = mr.id AND mg.name = ANY($2)
              ))
              AND ($3::int4 IS NULL OR (
                    mm.release_date IS NOT NULL
                    AND EXTRACT(YEAR FROM mm.release_date)::int BETWEEN $3 AND $4
              ))
              AND ($5::real IS NULL OR mm.vote_average BETWEEN $5 AND $6)
              AND (
                    NOT $7::bool
                    OR (
                        $8::bool AND (
                            ($11::bool AND mr.title % $9)
                            OR ($12::bool AND mm.overview % $9)
                            OR ($13::bool AND EXISTS (
                                SELECT 1
                                FROM movie_cast search_mc
                                JOIN persons search_p ON search_p.id = search_mc.person_id
                                WHERE search_mc.movie_id = mr.id AND search_p.name % $9
                            ))
                        )
                    )
                    OR (
                        NOT $8::bool AND (
                            ($11::bool AND mr.title ILIKE $10)
                            OR ($12::bool AND mm.overview ILIKE $10)
                            OR ($13::bool AND EXISTS (
                                SELECT 1
                                FROM movie_cast search_mc
                                JOIN persons search_p ON search_p.id = search_mc.person_id
                                WHERE search_mc.movie_id = mr.id AND search_p.name ILIKE $10
                            ))
                        )
                    )
              )
            ORDER BY
              CASE WHEN $14::int2 = 0 AND $15::bool THEN LOWER(mr.title) END ASC NULLS LAST,
              CASE WHEN $14::int2 = 0 AND NOT $15::bool THEN LOWER(mr.title) END DESC NULLS LAST,
              CASE WHEN $14::int2 = 1 AND $15::bool THEN mf.discovered_at END ASC NULLS LAST,
              CASE WHEN $14::int2 = 1 AND NOT $15::bool THEN mf.discovered_at END DESC NULLS LAST,
              CASE WHEN $14::int2 = 2 AND $15::bool THEN mf.created_at END ASC NULLS LAST,
              CASE WHEN $14::int2 = 2 AND NOT $15::bool THEN mf.created_at END DESC NULLS LAST,
              CASE WHEN $14::int2 = 3 AND $15::bool THEN mm.release_date END ASC NULLS LAST,
              CASE WHEN $14::int2 = 3 AND NOT $15::bool THEN mm.release_date END DESC NULLS LAST,
              CASE WHEN $14::int2 = 4 AND $15::bool THEN mm.vote_average END ASC NULLS LAST,
              CASE WHEN $14::int2 = 4 AND NOT $15::bool THEN mm.vote_average END DESC NULLS LAST,
              CASE WHEN $14::int2 = 5 AND $15::bool THEN mm.runtime END ASC NULLS LAST,
              CASE WHEN $14::int2 = 5 AND NOT $15::bool THEN mm.runtime END DESC NULLS LAST,
              mr.id ASC
            LIMIT $16 OFFSET $17
            "#,
            &query.filters.library_ids,
            &query.filters.genres,
            year_min,
            year_max,
            rating_min,
            rating_max,
            search.apply,
            search.fuzzy,
            search.text,
            search.like_pattern,
            search.include_title,
            search.include_overview,
            search.include_cast,
            sort_key,
            sort_ascending,
            query.pagination.limit as i64,
            query.pagination.offset as i64
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let id = MovieID(row.id);
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
        let year_min = query.filters.year_range.map(|range| range.min as i32);
        let year_max = query.filters.year_range.map(|range| range.max as i32);
        let rating_min = query
            .filters
            .rating_range
            .map(|range| rating_bound(range.min));
        let rating_max = query
            .filters
            .rating_range
            .map(|range| rating_bound(range.max));
        let search = SearchSqlParams::from_query(query.search.as_ref());
        let sort_key = MediaSqlSortKey::for_series(&query.sort).as_i16();
        let sort_ascending = matches!(query.sort.order, SortOrder::Ascending);

        let rows = sqlx::query!(
            r#"
            WITH series_data AS (
                SELECT
                    sr.id,
                    sr.title,
                    sr.discovered_at,
                    sr.created_at,
                    sm.first_air_date,
                    sm.vote_average,
                    sm.overview
                FROM series sr
                LEFT JOIN series_metadata sm ON sr.id = sm.series_id
                WHERE EXISTS (
                    SELECT 1
                    FROM episode_references er_visible
                    JOIN media_files mf_visible ON mf_visible.id = er_visible.file_id
                    WHERE er_visible.series_id = sr.id
                      AND mf_visible.is_available = TRUE
                )
                  AND (cardinality($1::uuid[]) = 0 OR sr.library_id = ANY($1))
                  AND (cardinality($2::text[]) = 0 OR EXISTS (
                        SELECT 1 FROM series_genres sg
                        WHERE sg.series_id = sr.id AND sg.name = ANY($2)
                  ))
                  AND ($3::int4 IS NULL OR (
                        sm.first_air_date IS NOT NULL
                        AND EXTRACT(YEAR FROM sm.first_air_date)::int BETWEEN $3 AND $4
                  ))
                  AND ($5::real IS NULL OR sm.vote_average BETWEEN $5 AND $6)
                  AND (
                        NOT $7::bool
                        OR (
                            $8::bool AND (
                                ($11::bool AND sr.title % $9)
                                OR ($12::bool AND sm.overview % $9)
                                OR ($13::bool AND EXISTS (
                                    SELECT 1
                                    FROM series_cast search_sc
                                    JOIN persons search_p ON search_p.id = search_sc.person_id
                                    WHERE search_sc.series_id = sr.id AND search_p.name % $9
                                ))
                            )
                        )
                        OR (
                            NOT $8::bool AND (
                                ($11::bool AND sr.title ILIKE $10)
                                OR ($12::bool AND sm.overview ILIKE $10)
                                OR ($13::bool AND EXISTS (
                                    SELECT 1
                                    FROM series_cast search_sc
                                    JOIN persons search_p ON search_p.id = search_sc.person_id
                                    WHERE search_sc.series_id = sr.id AND search_p.name ILIKE $10
                                ))
                            )
                        )
                  )
            )
            SELECT
                sd.id AS "series_id!",
                sn.id AS "season_id?",
                ep.id AS "episode_id?"
            FROM series_data sd
            LEFT JOIN LATERAL (
                SELECT * FROM season_references
                WHERE series_id = sd.id
                ORDER BY season_number
            ) sn ON true
            LEFT JOIN LATERAL (
                SELECT er.*
                FROM episode_references er
                JOIN media_files mf_visible ON mf_visible.id = er.file_id
                WHERE er.series_id = sd.id
                  AND er.season_id = sn.id
                  AND mf_visible.is_available = TRUE
                ORDER BY er.season_number, er.episode_number
            ) ep ON true
            LEFT JOIN media_files mf ON ep.file_id = mf.id
            ORDER BY
              CASE WHEN $14::int2 = 0 AND $15::bool THEN LOWER(sd.title) END ASC NULLS LAST,
              CASE WHEN $14::int2 = 0 AND NOT $15::bool THEN LOWER(sd.title) END DESC NULLS LAST,
              CASE WHEN $14::int2 = 1 AND $15::bool THEN COALESCE(mf.discovered_at, sn.discovered_at, sd.discovered_at) END ASC NULLS LAST,
              CASE WHEN $14::int2 = 1 AND NOT $15::bool THEN COALESCE(mf.discovered_at, sn.discovered_at, sd.discovered_at) END DESC NULLS LAST,
              CASE WHEN $14::int2 = 2 AND $15::bool THEN COALESCE(mf.created_at, sn.created_at, sd.created_at) END ASC NULLS LAST,
              CASE WHEN $14::int2 = 2 AND NOT $15::bool THEN COALESCE(mf.created_at, sn.created_at, sd.created_at) END DESC NULLS LAST,
              CASE WHEN $14::int2 = 3 AND $15::bool THEN sd.first_air_date END ASC NULLS LAST,
              CASE WHEN $14::int2 = 3 AND NOT $15::bool THEN sd.first_air_date END DESC NULLS LAST,
              CASE WHEN $14::int2 = 4 AND $15::bool THEN sd.vote_average END ASC NULLS LAST,
              CASE WHEN $14::int2 = 4 AND NOT $15::bool THEN sd.vote_average END DESC NULLS LAST,
              sd.id,
              sn.season_number,
              ep.episode_number
            "#,
            &query.filters.library_ids,
            &query.filters.genres,
            year_min,
            year_max,
            rating_min,
            rating_max,
            search.apply,
            search.fuzzy,
            search.text,
            search.like_pattern,
            search.include_title,
            search.include_overview,
            search.include_cast,
            sort_key,
            sort_ascending
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let mut media_ids: HashSet<MediaID> = HashSet::new();
        let mut results = Vec::new();

        for row in rows {
            let id: MediaID = MediaID::Series(SeriesID(row.series_id));

            if !media_ids.contains(&id) {
                media_ids.insert(id);
                results.push(MediaWithStatus {
                    id,
                    watch_status: None,
                });
            }

            if let Some(season_id) = row.season_id {
                let season_id = MediaID::Season(SeasonID(season_id));

                if media_ids.contains(&id) {
                    media_ids.insert(season_id);
                    results.push(MediaWithStatus {
                        id: season_id,
                        watch_status: None,
                    });
                }
            }

            if let Some(episode_id) = row.episode_id {
                let episode_media_id = MediaID::Episode(EpisodeID(episode_id));
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
                  AND (duration > 0) AND (position / duration) < $4
            )
            SELECT
                inprog_rows.id AS "id!",
                inprog_rows.position AS "position!",
                inprog_rows.duration AS "duration!",
                inprog_rows.last_watched AS "last_watched!",
                inprog_rows.media_kind AS "media_kind!"
            FROM (
                SELECT
                    mr.id AS id,
                    inprog.position::real AS position,
                    inprog.duration::real AS duration,
                    inprog.last_watched::bigint AS last_watched,
                    0::int4 AS media_kind
                FROM inprog
                JOIN movie_references mr ON inprog.media_uuid = mr.id AND inprog.media_type = 0

                UNION ALL

                SELECT
                    er.id AS id,
                    inprog.position::real AS position,
                    inprog.duration::real AS duration,
                    inprog.last_watched::bigint AS last_watched,
                    3::int4 AS media_kind
                FROM inprog
                JOIN episode_references er ON inprog.media_uuid = er.id AND inprog.media_type = 3
            ) AS inprog_rows
            ORDER BY inprog_rows.last_watched DESC
            LIMIT $2 OFFSET $3
            "#,
            user_id,
            query.pagination.limit as i64,
            query.pagination.offset as i64,
            WatchResumePolicy::from_env().completion_threshold
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let id = media_id_from_watch_kind(row.media_kind, row.id)?;

            results.push(MediaWithStatus {
                id,
                watch_status: Some(watch_status_from_progress(
                    id,
                    row.position,
                    row.duration,
                    row.last_watched,
                )),
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
            let id = media_id_from_watch_kind(row.media_kind, row.id)?;
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
            return Ok(Some(watch_status_from_progress(
                MediaID::Movie(*movie_id),
                row.position,
                row.duration,
                row.last_watched,
            )));
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
            return Ok(Some(watch_status_from_progress(
                MediaID::Episode(*episode_id),
                row.position,
                row.duration,
                row.last_watched,
            )));
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

fn build_subsequence_regex(query: &str) -> Option<String> {
    let clean: String = query.chars().filter(|c| c.is_alphanumeric()).collect();
    if clean.len() < 2 || clean.len() > 24 {
        return None;
    }
    let mut pattern = String::with_capacity(clean.len() * 3);
    for (i, ch) in clean.chars().enumerate() {
        if i > 0 {
            pattern.push_str(".*");
        }
        if ch.is_ascii_alphabetic() {
            pattern.push(ch.to_ascii_lowercase());
        } else {
            pattern.push(ch);
        }
    }
    Some(pattern)
}

fn first_token_like_pattern(query: &str) -> Option<String> {
    let mut tokens = query.split_whitespace().filter(|t| !t.is_empty());
    let first = tokens.next()?;
    tokens.next()?;
    let clean: String = first.chars().filter(|c| c.is_alphanumeric()).collect();
    if clean.len() < 2 {
        return None;
    }
    Some(format!("%{}%", clean.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_below_threshold_stays_in_progress_on_logical_item() {
        let media_uuid = Uuid::new_v4();
        let media_id = MediaID::Episode(EpisodeID(media_uuid));

        let status = watch_status_from_progress(media_id, 94.0, 100.0, 123);

        match status {
            ItemWatchStatus::InProgress(item) => {
                assert_eq!(item.media_id, media_uuid);
                assert_eq!(item.position, 94.0);
                assert_eq!(item.duration, 100.0);
                assert_eq!(item.last_watched, 123);
            }
            other => panic!("expected in-progress status, got {other:?}"),
        }
    }

    #[test]
    fn progress_at_threshold_is_completed() {
        let media_uuid = Uuid::new_v4();
        let media_id = MediaID::Movie(MovieID(media_uuid));

        let status = watch_status_from_progress(media_id, 95.0, 100.0, 456);

        match status {
            ItemWatchStatus::Completed(item) => {
                assert_eq!(item.media_id, media_id);
                assert_eq!(item.last_watched, 456);
            }
            other => panic!("expected completed status, got {other:?}"),
        }
    }

    #[test]
    fn watch_kind_mapping_uses_episode_ids_for_episode_rows() {
        let media_uuid = Uuid::new_v4();

        let media_id = media_id_from_watch_kind(EPISODE_WATCH_KIND, media_uuid)
            .expect("episode kind should map");

        assert_eq!(media_id, MediaID::Episode(EpisodeID(media_uuid)));
    }
}
