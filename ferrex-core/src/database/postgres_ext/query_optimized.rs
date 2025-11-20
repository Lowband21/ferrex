use crate::{
    EpisodeID, EpisodeNumber, EpisodeURL, LibraryID, MediaDetailsOption, MediaError, MediaFile,
    MediaIDLike, MovieID, MovieTitle, MovieURL, Result, SeasonID, SeasonNumber, SeasonURL,
    SeriesID, SeriesTitle, SeriesURL, UrlLike,
    api_types::{RATING_DECIMAL_SCALE, RatingValue},
    database::{postgres::PostgresDatabase, traits::MediaDatabaseTrait},
    query::*,
    types::media::{Media, *},
    watch_status::{InProgressItem, WatchStatusFilter},
};
use sqlx::types::BigDecimal;
use sqlx::{Postgres, QueryBuilder, Row};
use uuid::Uuid;

impl PostgresDatabase {
    /// Execute a media query using optimized SQL queries with proper indexing
    pub async fn query_media_optimized(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>> {
        // Handle watch status filter separately if provided
        if let Some(watch_filter) = &query.filters.watch_status {
            return self.query_media_by_watch_status(query, watch_filter).await;
        }

        // Check if we can use presorted indices for single library queries
        if query.filters.library_ids.len() == 1 && query.search.is_none() {
            // TODO: Potentially use precomputed indices here in the future
        }

        // Build the main SQL query
        let results = match query.filters.media_type {
            Some(MediaTypeFilter::Movie) => self.query_movies_optimized(query).await?,
            Some(MediaTypeFilter::Series) => self.query_tv_shows_optimized(query).await?,
            Some(MediaTypeFilter::Season) | Some(MediaTypeFilter::Episode) => {
                // For Season/Episode filters, query TV shows and filter results
                self.query_tv_shows_optimized(query).await?
            }
            None => {
                if query.search.is_some() {
                    self.query_multi_type_search(query).await?
                } else {
                    // Default to movie listings when no media type is provided
                    self.query_movies_optimized(query).await?
                }
            }
        };

        Ok(results)
    }

    async fn query_movies_optimized(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>> {
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
        let rows = sql_builder
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        // Convert rows to MediaWithStatus
        let mut results = Vec::new();
        for row in rows {
            let movie_ref = self.row_to_movie_reference(&row)?;

            // Get watch status if user context provided
            let (watch_status, is_completed) = if let Some(user_id) = query.user_context {
                self.get_movie_watch_status(user_id, &movie_ref.id).await?
            } else {
                (None, false)
            };

            results.push(MediaWithStatus {
                media: Media::Movie(movie_ref),
                watch_status,
                is_completed,
            });
        }

        Ok(results)
    }

    async fn query_tv_shows_optimized(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>> {
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
                FROM series_references sr
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

        let rows = sql_builder
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

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

    async fn query_multi_type_search(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>> {
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

        let movies = self.query_movies_optimized(&movie_query).await?;
        let series = self.query_tv_shows_optimized(&series_query).await?;

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
            MediaError::InvalidMedia("User context required for watch status filter".to_string())
        })?;

        match watch_filter {
            WatchStatusFilter::InProgress => self.query_in_progress_media(user_id, query).await,
            WatchStatusFilter::Completed => self.query_completed_media(user_id, query).await,
            WatchStatusFilter::Unwatched => self.query_unwatched_media(user_id, query).await,
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
        let mut sql_builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT
                uwp.media_uuid, uwp.image_type, uwp.position, uwp.duration, uwp.last_watched
            FROM user_watch_progress uwp
            WHERE uwp.user_id = $1
                AND uwp.position > 0
                AND (uwp.position / uwp.duration) < 0.95
            "#,
        );

        sql_builder.push_bind(user_id);

        // Add sorting for watch progress
        sql_builder.push(" ORDER BY uwp.last_watched DESC");

        // Add pagination
        sql_builder.push(" LIMIT ");
        sql_builder.push_bind(query.pagination.limit as i64);
        sql_builder.push(" OFFSET ");
        sql_builder.push_bind(query.pagination.offset as i64);

        let rows = sql_builder
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut results = Vec::new();
        for row in rows {
            let image_type: i16 = row.get("image_type");
            let media_uuid: Uuid = row.get("media_uuid");
            let position: f32 = row.get("position");
            let duration: f32 = row.get("duration");
            let last_watched: i64 = row.get("last_watched");

            let media_ref = match image_type {
                0 => {
                    let movie = self.get_movie_reference(&MovieID(media_uuid)).await?;
                    Media::Movie(movie)
                }
                3 => {
                    let episode = self.get_episode_reference(&EpisodeID(media_uuid)).await?;
                    Media::Episode(episode)
                }
                _ => continue,
            };

            let watch_status = InProgressItem {
                media_id: media_uuid,
                position,
                duration,
                last_watched,
            };

            results.push(MediaWithStatus {
                media: media_ref,
                watch_status: Some(watch_status),
                is_completed: false,
            });
        }

        Ok(results)
    }

    async fn query_completed_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        let mut sql_builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT
                ucm.media_uuid, ucm.image_type, ucm.completed_at
            FROM user_completed_media ucm
            WHERE ucm.user_id = $1
            "#,
        );

        sql_builder.push_bind(user_id);

        // Add sorting for completed media
        sql_builder.push(" ORDER BY ucm.completed_at DESC");

        // Add pagination
        sql_builder.push(" LIMIT ");
        sql_builder.push_bind(query.pagination.limit as i64);
        sql_builder.push(" OFFSET ");
        sql_builder.push_bind(query.pagination.offset as i64);

        let rows = sql_builder
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut results = Vec::new();
        for row in rows {
            let image_type: i16 = row.get("image_type");
            let media_uuid: Uuid = row.get("media_uuid");

            let media_ref = match image_type {
                0 => {
                    let movie = self.get_movie_reference(&MovieID(media_uuid)).await?;
                    Media::Movie(movie)
                }
                3 => {
                    let episode = self.get_episode_reference(&EpisodeID(media_uuid)).await?;
                    Media::Episode(episode)
                }
                _ => continue,
            };

            results.push(MediaWithStatus {
                media: media_ref,
                watch_status: None,
                is_completed: true,
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

    fn add_search_clause(&self, sql_builder: &mut QueryBuilder<Postgres>, search: &SearchQuery) {
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
                    "EXISTS (SELECT 1 FROM movie_cast search_mc JOIN persons search_p ON search_p.tmdb_id = search_mc.person_tmdb_id WHERE search_mc.movie_id = mr.id AND search_p.name % "
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
                    "EXISTS (SELECT 1 FROM movie_cast search_mc JOIN persons search_p ON search_p.tmdb_id = search_mc.person_tmdb_id WHERE search_mc.movie_id = mr.id AND search_p.name ILIKE "
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

    fn add_movie_sort_clause(&self, sql_builder: &mut QueryBuilder<Postgres>, sort: &SortCriteria) {
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
            SortBy::Title => ("LOWER(series_title)", "LAST"),
            SortBy::DateAdded => (
                "COALESCE(file_discovered_at, season_discovered_at, series_discovered_at)",
                "LAST",
            ),
            SortBy::CreatedAt => (
                "COALESCE(file_created_at, season_created_at, series_created_at)",
                "LAST",
            ),
            SortBy::ReleaseDate => ("series_first_air_date", "LAST"),
            SortBy::Rating => ("series_vote_average", "LAST"),
            _ => (
                "COALESCE(file_discovered_at, season_discovered_at, series_discovered_at)",
                "LAST",
            ),
        };

        sql_builder.push(field);

        match sort.order {
            SortOrder::Ascending => sql_builder.push(" ASC NULLS "),
            SortOrder::Descending => sql_builder.push(" DESC NULLS "),
        };
        sql_builder.push(null_position);

        // Secondary sort for hierarchy
        sql_builder.push(", sd.id, season_number, episode_number");
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
                    "EXISTS (SELECT 1 FROM series_cast search_sc JOIN persons search_p ON search_p.tmdb_id = search_sc.person_tmdb_id WHERE search_sc.series_id = sr.id AND search_p.name % "
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
                    "EXISTS (SELECT 1 FROM series_cast search_sc JOIN persons search_p ON search_p.tmdb_id = search_sc.person_tmdb_id WHERE search_sc.series_id = sr.id AND search_p.name ILIKE "
                );
                sql_builder.push_bind(like_pattern);
                sql_builder.push(")");
            }
        }

        sql_builder.push(")");
    }

    fn row_to_movie_reference(&self, row: &sqlx::postgres::PgRow) -> Result<MovieReference> {
        use sqlx::Row;

        let technical_metadata: Option<serde_json::Value> = row.try_get("technical_metadata").ok();
        let media_file_metadata = technical_metadata
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| MediaError::Internal(format!("Failed to deserialize metadata: {}", e)))?;

        let library_id = LibraryID(row.get("library_id"));

        let media_file = MediaFile {
            id: row.get("file_id"),
            path: std::path::PathBuf::from(row.get::<String, _>("file_path")),
            filename: row.get("filename"),
            size: row.get::<i64, _>("file_size") as u64,
            discovered_at: row.get("file_discovered_at"),
            created_at: row.get("file_created_at"),
            media_file_metadata,
            library_id,
        };

        Ok(MovieReference {
            id: MovieID(row.get::<Uuid, _>("id")),
            library_id,
            tmdb_id: row.get::<i64, _>("tmdb_id") as u64,
            title: MovieTitle::new(row.get("title"))?,
            details: MediaDetailsOption::Endpoint(format!("/movie/{}", row.get::<Uuid, _>("id"))),
            endpoint: MovieURL::from_string(format!("/stream/{}", row.get::<Uuid, _>("file_id"))),
            file: media_file,
            theme_color: row.try_get("theme_color").ok(),
        })
    }

    async fn build_tv_hierarchy_from_rows(
        &self,
        rows: Vec<sqlx::postgres::PgRow>,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        use sqlx::Row;
        use std::collections::HashMap;

        let mut series_map: HashMap<Uuid, SeriesReference> = HashMap::new();
        let mut season_map: HashMap<(Uuid, i16), SeasonReference> = HashMap::new();
        let mut results = Vec::new();

        for row in rows {
            let series_id: Uuid = row.get("series_id");
            let library_id = LibraryID(row.get("series_library_id"));

            // Create or get series reference
            if let std::collections::hash_map::Entry::Vacant(e) = series_map.entry(series_id) {
                let series_ref = SeriesReference {
                    id: SeriesID(series_id),
                    library_id,
                    tmdb_id: row.get::<i64, _>("series_tmdb_id") as u64,
                    title: SeriesTitle::new(row.get("series_title"))?,
                    details: MediaDetailsOption::Endpoint(format!("/series/{}", series_id)),
                    endpoint: SeriesURL::from_string(format!("/series/{}", series_id)),
                    discovered_at: row
                        .try_get("series_discovered_at")
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    created_at: row
                        .try_get("series_created_at")
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    theme_color: row
                        .try_get::<Option<String>, _>("series_theme_color")
                        .unwrap_or(None),
                };

                e.insert(series_ref.clone());

                // Add series to results
                results.push(MediaWithStatus {
                    media: Media::Series(series_ref),
                    watch_status: None,
                    is_completed: false,
                });
            }

            // Process season if present
            if let Ok(season_id) = row.try_get::<Uuid, _>("season_id") {
                let season_number: i16 = row.get("season_number");
                let key = (series_id, season_number);

                if let std::collections::hash_map::Entry::Vacant(e) = season_map.entry(key) {
                    let season_ref = SeasonReference {
                        id: SeasonID(season_id),
                        series_id: SeriesID(series_id),
                        season_number: SeasonNumber::new(season_number as u8),
                        library_id,
                        tmdb_series_id: row.get::<i64, _>("series_tmdb_id") as u64,
                        details: MediaDetailsOption::Endpoint(format!(
                            "/series/{}/season/{}",
                            series_id, season_number
                        )),
                        endpoint: SeasonURL::from_string(format!(
                            "/series/{}/season/{}",
                            series_id, season_number
                        )),
                        discovered_at: row
                            .try_get("season_discovered_at")
                            .or_else(|_| row.try_get("series_discovered_at"))
                            .unwrap_or_else(|_| chrono::Utc::now()),
                        created_at: row
                            .try_get("series_created_at")
                            .unwrap_or_else(|_| chrono::Utc::now()),
                        theme_color: None,
                    };

                    e.insert(season_ref.clone());

                    // Add season to results
                    results.push(MediaWithStatus {
                        media: Media::Season(season_ref),
                        watch_status: None,
                        is_completed: false,
                    });
                }
            }

            // Process episode if present
            if let Ok(episode_id) = row.try_get::<Uuid, _>("episode_id") {
                let season_number: i16 = row.get("ep_season");
                let episode_number: i16 = row.get("episode_number");
                let file_id: Uuid = row.get("file_id");

                let media_file = MediaFile {
                    id: file_id,
                    path: std::path::PathBuf::from(row.get::<String, _>("file_path")),
                    filename: row.get("filename"),
                    size: row.get::<i64, _>("file_size") as u64,
                    discovered_at: row.get("file_discovered_at"),
                    created_at: row.get("file_created_at"),
                    media_file_metadata: None,
                    library_id: row
                        .try_get::<Uuid, _>("file_library_id")
                        .map(LibraryID)
                        .unwrap_or(library_id),
                };

                let episode_ref = EpisodeReference {
                    id: EpisodeID(episode_id),
                    library_id,
                    series_id: SeriesID(series_id),
                    season_id: SeasonID(row.get::<Uuid, _>("season_id")),
                    season_number: SeasonNumber::new(season_number as u8),
                    episode_number: EpisodeNumber::new(episode_number as u8),
                    tmdb_series_id: row.get::<i64, _>("series_tmdb_id") as u64,
                    details: MediaDetailsOption::Endpoint(format!(
                        "/series/{}/season/{}/episode/{}",
                        series_id, season_number, episode_number
                    )),
                    endpoint: EpisodeURL::from_string(format!("/stream/{}", file_id)),
                    file: media_file,
                    discovered_at: row.get("file_discovered_at"),
                    created_at: row.get("file_created_at"),
                };

                // Get watch status if user context provided
                let (watch_status, is_completed) = if let Some(user_id) = query.user_context {
                    self.get_episode_watch_status(user_id, &episode_ref.id)
                        .await?
                } else {
                    (None, false)
                };

                results.push(MediaWithStatus {
                    media: Media::Episode(episode_ref),
                    watch_status,
                    is_completed,
                });
            }
        }

        Ok(results)
    }

    async fn get_movie_watch_status(
        &self,
        user_id: Uuid,
        movie_id: &MovieID,
    ) -> Result<(Option<InProgressItem>, bool)> {
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
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get watch status: {}", e)))?;

        if let Some(row) = progress {
            let watch_status = InProgressItem {
                media_id: movie_id.to_uuid(),
                position: row.position,
                duration: row.duration,
                last_watched: row.last_watched,
            };

            // Check if completed (>95% watched)
            let is_completed = (row.position as f64 / row.duration as f64) >= 0.95;
            return Ok((Some(watch_status), is_completed));
        }

        // Check completed media
        let completed = sqlx::query!(
            r#"
            SELECT 1 as exists
            FROM user_completed_media
            WHERE user_id = $1
                AND media_uuid = $2
            "#,
            user_id,
            movie_id.to_uuid()
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to check completion: {}", e)))?;

        Ok((None, completed.is_some()))
    }

    async fn get_episode_watch_status(
        &self,
        user_id: Uuid,
        episode_id: &EpisodeID,
    ) -> Result<(Option<InProgressItem>, bool)> {
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
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get watch status: {}", e)))?;

        if let Some(row) = progress {
            let watch_status = InProgressItem {
                media_id: episode_id.to_uuid(),
                position: row.position,
                duration: row.duration,
                last_watched: row.last_watched,
            };

            let is_completed = (row.position as f64 / row.duration as f64) >= 0.95;
            return Ok((Some(watch_status), is_completed));
        }

        let completed = sqlx::query!(
            r#"
            SELECT 1 as exists
            FROM user_completed_media
            WHERE user_id = $1
                AND media_uuid = $2
            "#,
            user_id,
            episode_id.to_uuid()
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to check completion: {}", e)))?;

        Ok((None, completed.is_some()))
    }
}

fn rating_bound(value: RatingValue) -> BigDecimal {
    BigDecimal::from(value).with_scale(RATING_DECIMAL_SCALE as i64)
}
