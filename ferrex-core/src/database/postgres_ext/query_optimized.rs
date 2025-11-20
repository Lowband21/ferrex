use crate::{
    EpisodeID, EpisodeNumber, EpisodeURL, LibraryID, MediaDetailsOption, MediaError, MediaFile,
    MediaID, MediaIDLike, MovieID, MovieTitle, MovieURL, Result, SeasonID, SeasonNumber, SeasonURL,
    SeriesID, SeriesTitle, SeriesURL, TmdbDetails, UrlLike,
    database::{postgres::PostgresDatabase, traits::MediaDatabaseTrait},
    query::*,
    types::media::{Media, *},
    watch_status::{InProgressItem, WatchStatusFilter},
};
use sqlx::types::BigDecimal;
use sqlx::{Postgres, QueryBuilder, Row};
use std::str::FromStr;
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
        let mut results = match query.filters.media_type {
            Some(MediaTypeFilter::Movie) => self.query_movies_optimized(query).await?,
            Some(MediaTypeFilter::Series) => self.query_tv_shows_optimized(query).await?,
            Some(MediaTypeFilter::Season) | Some(MediaTypeFilter::Episode) => {
                // For Season/Episode filters, query TV shows and filter results
                self.query_tv_shows_optimized(query).await?
            }
            None => {
                // Query both movies and TV shows
                let mut combined = self.query_movies_optimized(query).await?;
                combined.extend(self.query_tv_shows_optimized(query).await?);
                combined
            }
        };

        // Apply cross-media-type sorting if needed
        if query.filters.media_type.is_none() {
            self.sort_combined_results(&mut results, &query.sort);
        }

        Ok(results)
    }

    async fn query_movies_optimized(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>> {
        let mut sql_builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT
                mr.id, mr.tmdb_id, mr.title, mr.theme_color,
                mf.id as file_id, mf.file_path, mf.filename, mf.file_size,
                mf.created_at as file_created_at, mf.technical_metadata, mf.library_id,
                mm.release_date, mm.vote_average, mm.runtime, mm.popularity,
                mm.genre_names, mm.release_year, mm.overview, mm.cast_names
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
            sql_builder.push(" AND mm.genre_names && ");
            sql_builder.push_bind(&query.filters.genres);
        }

        // Add year range filter
        if let Some((min_year, max_year)) = query.filters.year_range {
            sql_builder.push(" AND mm.release_year BETWEEN ");
            sql_builder.push_bind(min_year as i32);
            sql_builder.push(" AND ");
            sql_builder.push_bind(max_year as i32);
        }

        // Add rating range filter
        if let Some((min_rating, max_rating)) = query.filters.rating_range {
            sql_builder.push(" AND mm.vote_average BETWEEN ");
            sql_builder.push_bind(BigDecimal::from_str(&min_rating.to_string()).unwrap());
            sql_builder.push(" AND ");
            sql_builder.push_bind(BigDecimal::from_str(&max_rating.to_string()).unwrap());
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
                    sr.id, sr.tmdb_id, sr.title,
                    sm.first_air_date, sm.vote_average, sm.popularity,
                    sm.genre_names, sm.first_air_year, sm.overview, sm.cast_names
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
            sql_builder.push(" AND sm.genre_names && ");
            sql_builder.push_bind(&query.filters.genres);
        }

        // Add year range filter
        if let Some((min_year, max_year)) = query.filters.year_range {
            sql_builder.push(" AND sm.first_air_year BETWEEN ");
            sql_builder.push_bind(min_year as i32);
            sql_builder.push(" AND ");
            sql_builder.push_bind(max_year as i32);
        }

        // Add rating range filter
        if let Some((min_rating, max_rating)) = query.filters.rating_range {
            sql_builder.push(" AND sm.vote_average BETWEEN ");
            sql_builder.push_bind(BigDecimal::from_str(&min_rating.to_string()).unwrap());
            sql_builder.push(" AND ");
            sql_builder.push_bind(BigDecimal::from_str(&max_rating.to_string()).unwrap());
        }

        sql_builder.push(
            r#"
            )
            SELECT
                sd.*,
                sn.id as season_id, sn.season_number,
                ep.id as episode_id, ep.season_number as ep_season,
                ep.episode_number, ep.file_id,
                mf.file_path, mf.filename, mf.file_size,
                mf.created_at as file_created_at, mf.library_id
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
                position: position,
                duration: duration,
                last_watched: last_watched,
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
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        // Query media that doesn't have watch progress or completion records
        // This is more complex as it requires exclusion joins
        todo!("Implement unwatched media query")
    }

    async fn query_recently_watched_media(
        &self,
        user_id: Uuid,
        days: u32,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        // Query media watched within the specified number of days
        todo!("Implement recently watched media query")
    }

    fn add_search_clause(&self, sql_builder: &mut QueryBuilder<Postgres>, search: &SearchQuery) {
        // For fuzzy search, use trigram similarity
        if search.fuzzy {
            // Use trigram similarity for fuzzy search
            sql_builder.push(" AND (");

            if search.fields.is_empty() || search.fields.contains(&SearchField::All) {
                sql_builder.push("mr.title % ");
                sql_builder.push_bind(search.text.clone());
                sql_builder.push(" OR mm.overview % ");
                sql_builder.push_bind(search.text.clone());
            } else {
                let mut first = true;
                for field in &search.fields {
                    if !first {
                        sql_builder.push(" OR ");
                    }
                    first = false;

                    match field {
                        SearchField::Title => {
                            sql_builder.push("mr.title % ");
                            sql_builder.push_bind(search.text.clone());
                        }
                        SearchField::Overview => {
                            sql_builder.push("mm.overview % ");
                            sql_builder.push_bind(search.text.clone());
                        }
                        SearchField::Cast => {
                            sql_builder.push(
                                "EXISTS (SELECT 1 FROM unnest(mm.cast_names) AS name WHERE name % ",
                            );
                            sql_builder.push_bind(search.text.clone());
                            sql_builder.push(")");
                        }
                        _ => {}
                    }
                }
            }
            sql_builder.push(")");
        } else {
            // Exact search with LIKE
            if search.fields.is_empty() || search.fields.contains(&SearchField::All) {
                sql_builder.push(" AND (");
                sql_builder.push("mr.title ILIKE ");
                sql_builder.push_bind(format!("%{}%", search.text));
                sql_builder.push(" OR mm.overview ILIKE ");
                sql_builder.push_bind(format!("%{}%", search.text));
                sql_builder.push(" OR ARRAY[");
                sql_builder.push_bind(search.text.clone());
                sql_builder.push("] && mm.cast_names)");
            } else {
                sql_builder.push(" AND (");
                let mut first = true;

                for field in &search.fields {
                    if !first {
                        sql_builder.push(" OR ");
                    }
                    first = false;

                    match field {
                        SearchField::Title => {
                            sql_builder.push("mr.title ILIKE ");
                            sql_builder.push_bind(format!("%{}%", search.text));
                        }
                        SearchField::Overview => {
                            sql_builder.push("mm.overview ILIKE ");
                            sql_builder.push_bind(format!("%{}%", search.text));
                        }
                        SearchField::Cast => {
                            sql_builder.push("ARRAY[");
                            sql_builder.push_bind(search.text.clone());
                            sql_builder.push("] && mm.cast_names");
                        }
                        _ => {}
                    }
                }
                sql_builder.push(")");
            }
        }
    }

    fn add_movie_sort_clause(&self, sql_builder: &mut QueryBuilder<Postgres>, sort: &SortCriteria) {
        sql_builder.push(" ORDER BY ");

        let (field, null_position) = match sort.primary {
            SortBy::Title => ("LOWER(mr.title)", "LAST"),
            SortBy::DateAdded => ("mf.created_at", "LAST"),
            SortBy::ReleaseDate => ("mm.release_date", "LAST"),
            SortBy::Rating => ("mm.vote_average", "LAST"),
            SortBy::Runtime => ("mm.runtime", "LAST"),
            _ => ("mf.created_at", "LAST"), // Default to date added
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
            SortBy::DateAdded => ("file_created_at", "LAST"),
            SortBy::ReleaseDate => ("sd.first_air_date", "LAST"),
            SortBy::Rating => ("sd.vote_average", "LAST"),
            _ => ("file_created_at", "LAST"),
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

    fn sort_combined_results(&self, results: &mut Vec<MediaWithStatus>, sort: &SortCriteria) {
        use crate::query::{SortBy, SortOrder};

        results.sort_by(|a, b| {
            // Extract sort values based on the sort field
            let (a_value, b_value) = match sort.primary {
                SortBy::Title => {
                    let a_title = match &a.media {
                        Media::Movie(m) => m.title.as_str(),
                        Media::Series(s) => s.title.as_str(),
                        Media::Season(_) => "",  // Seasons don't have titles
                        Media::Episode(_) => "", // Episodes don't have titles
                    };
                    let b_title = match &b.media {
                        Media::Movie(m) => m.title.as_str(),
                        Media::Series(s) => s.title.as_str(),
                        Media::Season(_) => "",  // Seasons don't have titles
                        Media::Episode(_) => "", // Episodes don't have titles
                    };
                    (a_title, b_title)
                }
                SortBy::DateAdded => {
                    // For DateAdded, we need to get the file created_at
                    // Since we don't have direct access to file data here,
                    // we'll use the ID comparison as a proxy (newer IDs = newer files)
                    let a_id = match &a.media {
                        Media::Movie(m) => m.file.created_at,
                        Media::Series(s) => s.created_at,
                        Media::Season(s) => s.created_at,
                        Media::Episode(e) => e.file.created_at,
                    };
                    let b_id = match &b.media {
                        Media::Movie(m) => m.file.created_at,
                        Media::Series(s) => s.created_at,
                        Media::Season(s) => s.created_at,
                        Media::Episode(e) => e.file.created_at,
                    };
                    return match sort.order {
                        SortOrder::Ascending => a_id.cmp(&b_id),
                        SortOrder::Descending => b_id.cmp(&a_id),
                    };
                }
                SortBy::ReleaseDate => {
                    // Extract release dates from details if available
                    let a_date = extract_release_date(&a.media);
                    let b_date = extract_release_date(&b.media);

                    match (a_date, b_date) {
                        (Some(a), Some(b)) => {
                            return match sort.order {
                                SortOrder::Ascending => a.cmp(&b),
                                SortOrder::Descending => b.cmp(&a),
                            };
                        }
                        (Some(_), None) => return std::cmp::Ordering::Less,
                        (None, Some(_)) => return std::cmp::Ordering::Greater,
                        (None, None) => return std::cmp::Ordering::Equal,
                    }
                }
                SortBy::Rating => {
                    // Extract ratings from details if available
                    let a_rating = extract_rating(&a.media);
                    let b_rating = extract_rating(&b.media);

                    match (a_rating, b_rating) {
                        (Some(a), Some(b)) => {
                            let ordering = a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
                            return match sort.order {
                                SortOrder::Ascending => ordering,
                                SortOrder::Descending => ordering.reverse(),
                            };
                        }
                        (Some(_), None) => return std::cmp::Ordering::Less,
                        (None, Some(_)) => return std::cmp::Ordering::Greater,
                        (None, None) => return std::cmp::Ordering::Equal,
                    }
                }
                SortBy::Runtime => {
                    // Extract runtime from details if available
                    let a_runtime = extract_runtime(&a.media);
                    let b_runtime = extract_runtime(&b.media);

                    match (a_runtime, b_runtime) {
                        (Some(a), Some(b)) => {
                            return match sort.order {
                                SortOrder::Ascending => a.cmp(&b),
                                SortOrder::Descending => b.cmp(&a),
                            };
                        }
                        (Some(_), None) => return std::cmp::Ordering::Less,
                        (None, Some(_)) => return std::cmp::Ordering::Greater,
                        (None, None) => return std::cmp::Ordering::Equal,
                    }
                }
                SortBy::Popularity => {
                    let a_popularity = extract_popularity(&a.media);
                    let b_popularity = extract_popularity(&b.media);

                    match (a_popularity, b_popularity) {
                        (Some(a), Some(b)) => {
                            let ordering = a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
                            return match sort.order {
                                SortOrder::Ascending => ordering,
                                SortOrder::Descending => ordering.reverse(),
                            };
                        }
                        (Some(_), None) => return std::cmp::Ordering::Less,
                        (None, Some(_)) => return std::cmp::Ordering::Greater,
                        (None, None) => return std::cmp::Ordering::Equal,
                    }
                }
                SortBy::FileSize => {
                    let a_size = extract_file_size(&a.media);
                    let b_size = extract_file_size(&b.media);

                    match (a_size, b_size) {
                        (Some(a), Some(b)) => {
                            return match sort.order {
                                SortOrder::Ascending => a.cmp(&b),
                                SortOrder::Descending => b.cmp(&a),
                            };
                        }
                        (Some(_), None) => return std::cmp::Ordering::Less,
                        (None, Some(_)) => return std::cmp::Ordering::Greater,
                        (None, None) => return std::cmp::Ordering::Equal,
                    }
                }
                SortBy::Resolution => {
                    let a_res = extract_resolution(&a.media);
                    let b_res = extract_resolution(&b.media);

                    match (a_res, b_res) {
                        (Some(a), Some(b)) => {
                            return match sort.order {
                                SortOrder::Ascending => a.cmp(&b),
                                SortOrder::Descending => b.cmp(&a),
                            };
                        }
                        (Some(_), None) => return std::cmp::Ordering::Less,
                        (None, Some(_)) => return std::cmp::Ordering::Greater,
                        (None, None) => return std::cmp::Ordering::Equal,
                    }
                }
                SortBy::Bitrate => {
                    let a_bitrate = extract_bitrate(&a.media);
                    let b_bitrate = extract_bitrate(&b.media);

                    match (a_bitrate, b_bitrate) {
                        (Some(a), Some(b)) => {
                            return match sort.order {
                                SortOrder::Ascending => a.cmp(&b),
                                SortOrder::Descending => b.cmp(&a),
                            };
                        }
                        (Some(_), None) => return std::cmp::Ordering::Less,
                        (None, Some(_)) => return std::cmp::Ordering::Greater,
                        (None, None) => return std::cmp::Ordering::Equal,
                    }
                }
                SortBy::ContentRating => {
                    let a_rating = extract_content_rating(&a.media);
                    let b_rating = extract_content_rating(&b.media);

                    match (a_rating, b_rating) {
                        (Some(a), Some(b)) => {
                            return match sort.order {
                                SortOrder::Ascending => a.cmp(&b),
                                SortOrder::Descending => b.cmp(&a),
                            };
                        }
                        (Some(_), None) => return std::cmp::Ordering::Less,
                        (None, Some(_)) => return std::cmp::Ordering::Greater,
                        (None, None) => return std::cmp::Ordering::Equal,
                    }
                }
                SortBy::LastWatched | SortBy::WatchProgress => {
                    // These require user context and watch status
                    // For now, maintain original order
                    return std::cmp::Ordering::Equal;
                }
                _ => ("", ""),
            };

            // For string comparisons (Title)
            match sort.order {
                SortOrder::Ascending => a_value.cmp(&b_value),
                SortOrder::Descending => b_value.cmp(&a_value),
            }
        });

        // Helper functions to extract values from Media
        fn extract_release_date(media: &Media) -> Option<String> {
            match media {
                Media::Movie(m) => {
                    if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &m.details {
                        details.release_date.clone()
                    } else {
                        None
                    }
                }
                Media::Series(s) => {
                    if let MediaDetailsOption::Details(TmdbDetails::Series(details)) = &s.details {
                        details.first_air_date.clone()
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }

        fn extract_rating(media: &Media) -> Option<f32> {
            match media {
                Media::Movie(m) => {
                    if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &m.details {
                        details.vote_average
                    } else {
                        None
                    }
                }
                Media::Series(s) => {
                    if let MediaDetailsOption::Details(TmdbDetails::Series(details)) = &s.details {
                        details.vote_average
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }

        fn extract_runtime(media: &Media) -> Option<u32> {
            match media {
                Media::Movie(m) => {
                    if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &m.details {
                        details.runtime
                    } else {
                        None
                    }
                }
                // TV series don't have a single runtime, so we skip them
                _ => None,
            }
        }

        fn extract_popularity(media: &Media) -> Option<f32> {
            match media {
                Media::Movie(m) => {
                    if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &m.details {
                        details.popularity
                    } else {
                        None
                    }
                }
                Media::Series(s) => {
                    if let MediaDetailsOption::Details(TmdbDetails::Series(details)) = &s.details {
                        details.popularity
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }

        fn extract_file_size(media: &Media) -> Option<u64> {
            match media {
                Media::Movie(m) => Some(m.file.size),
                Media::Episode(e) => Some(e.file.size),
                _ => None,
            }
        }

        fn extract_resolution(media: &Media) -> Option<u32> {
            let metadata = match media {
                Media::Movie(m) => m.file.media_file_metadata.as_ref(),
                Media::Episode(e) => e.file.media_file_metadata.as_ref(),
                _ => None,
            };
            metadata.and_then(|meta| meta.height)
        }

        fn extract_bitrate(media: &Media) -> Option<u64> {
            let metadata = match media {
                Media::Movie(m) => m.file.media_file_metadata.as_ref(),
                Media::Episode(e) => e.file.media_file_metadata.as_ref(),
                _ => None,
            };
            metadata.and_then(|meta| meta.bitrate)
        }

        fn extract_content_rating(_media: &Media) -> Option<String> {
            // TODO: populate from TMDB content ratings when available
            None
        }
    }

    fn row_to_movie_reference(&self, row: &sqlx::postgres::PgRow) -> Result<MovieReference> {
        use sqlx::Row;

        let technical_metadata: Option<serde_json::Value> = row.try_get("technical_metadata").ok();
        let media_file_metadata = technical_metadata
            .map(|tm| serde_json::from_value(tm))
            .transpose()
            .map_err(|e| MediaError::Internal(format!("Failed to deserialize metadata: {}", e)))?;

        let library_id = LibraryID(row.get("library_id"));

        let media_file = MediaFile {
            id: row.get("file_id"),
            path: std::path::PathBuf::from(row.get::<String, _>("file_path")),
            filename: row.get("filename"),
            size: row.get::<i64, _>("file_size") as u64,
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
            let series_id: Uuid = row.get("id");
            let library_id = LibraryID(row.get("library_id"));

            // Create or get series reference
            if !series_map.contains_key(&series_id) {
                let series_ref = SeriesReference {
                    id: SeriesID(series_id),
                    library_id,
                    tmdb_id: row.get::<i64, _>("tmdb_id") as u64,
                    title: SeriesTitle::new(row.get("title"))?,
                    details: MediaDetailsOption::Endpoint(format!("/series/{}", series_id)),
                    endpoint: SeriesURL::from_string(format!("/series/{}", series_id)),
                    created_at: row
                        .try_get("created_at")
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    theme_color: row.try_get("theme_color").ok(),
                };

                series_map.insert(series_id, series_ref.clone());

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

                if !season_map.contains_key(&key) {
                    let season_ref = SeasonReference {
                        id: SeasonID(season_id),
                        series_id: SeriesID(series_id),
                        season_number: SeasonNumber::new(season_number as u8),
                        library_id,
                        tmdb_series_id: row.get::<i64, _>("tmdb_id") as u64,
                        details: MediaDetailsOption::Endpoint(format!(
                            "/series/{}/season/{}",
                            series_id, season_number
                        )),
                        endpoint: SeasonURL::from_string(format!(
                            "/series/{}/season/{}",
                            series_id, season_number
                        )),
                        created_at: row
                            .try_get("created_at")
                            .unwrap_or_else(|_| chrono::Utc::now()),
                        theme_color: None,
                    };

                    season_map.insert(key, season_ref.clone());

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
                    created_at: row.get("file_created_at"),
                    media_file_metadata: None,
                    library_id,
                };

                let episode_ref = EpisodeReference {
                    id: EpisodeID(episode_id),
                    library_id,
                    series_id: SeriesID(series_id),
                    season_id: SeasonID(row.get::<Uuid, _>("season_id")),
                    season_number: SeasonNumber::new(season_number as u8),
                    episode_number: EpisodeNumber::new(episode_number as u8),
                    tmdb_series_id: row.get::<i64, _>("tmdb_id") as u64,
                    details: MediaDetailsOption::Endpoint(format!(
                        "/series/{}/season/{}/episode/{}",
                        series_id, season_number, episode_number
                    )),
                    endpoint: EpisodeURL::from_string(format!("/stream/{}", file_id)),
                    file: media_file,
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
                position: row.position as f32,
                duration: row.duration as f32,
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
                position: row.position as f32,
                duration: row.duration as f32,
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
