use async_trait::async_trait;
use chrono::{Duration, Utc};
use sqlx::types::BigDecimal;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    api_types::{RATING_DECIMAL_SCALE, RatingValue, filters::FilterIndicesRequest},
    database::ports::indices::IndicesRepository,
    error::{MediaError, Result},
    query::types::{MediaTypeFilter, SortBy, SortOrder},
    types::ids::LibraryID,
    watch_status::WatchStatusFilter,
};

#[derive(Clone, Debug)]
pub struct PostgresIndicesRepository {
    pool: PgPool,
}

impl PostgresIndicesRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl IndicesRepository for PostgresIndicesRepository {
    async fn rebuild_movie_sort_positions(&self, library_id: LibraryID) -> Result<()> {
        sqlx::query("SELECT rebuild_movie_sort_positions($1)")
            .bind(library_id.as_uuid())
            .execute(self.pool())
            .await
            .map_err(|err| {
                MediaError::Internal(format!(
                    "Failed to rebuild movie_sort_positions for library {}: {}",
                    library_id, err
                ))
            })?;
        Ok(())
    }

    async fn fetch_sorted_movie_indices(
        &self,
        library_id: LibraryID,
        sort: SortBy,
        order: SortOrder,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<Vec<u32>> {
        let (order_column, direction) = map_sort_to_column(sort, order);

        let mut qb = QueryBuilder::new(
            "SELECT (msp.title_pos - 1)::INT4 AS idx FROM movie_sort_positions msp WHERE msp.library_id = ",
        );
        qb.push_bind(library_id.as_uuid());
        qb.push(" ORDER BY ");
        qb.push(order_column);
        qb.push(" ");
        qb.push(direction);

        if let Some(offset) = offset {
            qb.push(" OFFSET ");
            qb.push_bind(offset as i64);
        }
        if let Some(limit) = limit {
            qb.push(" LIMIT ");
            qb.push_bind(limit as i64);
        }

        let rows = qb.build().fetch_all(self.pool()).await.map_err(|err| {
            MediaError::Internal(format!(
                "Failed to fetch sorted indices for library {}: {}",
                library_id, err
            ))
        })?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let idx: i32 = row.get("idx");
                (idx >= 0).then(|| idx as u32)
            })
            .collect())
    }

    async fn fetch_filtered_movie_indices(
        &self,
        library_id: LibraryID,
        spec: &FilterIndicesRequest,
        user_id: Option<Uuid>,
    ) -> Result<Vec<u32>> {
        let library_uuid = library_id.as_uuid();
        let builder = FilteredMovieIndexBuilder::new(library_uuid, spec, user_id)
            .map_err(|err| MediaError::InvalidMedia(err.to_string()))?;

        let mut qb = builder
            .build()
            .map_err(|err| MediaError::InvalidMedia(err.to_string()))?;

        let rows = qb.build().fetch_all(self.pool()).await.map_err(|err| {
            MediaError::Internal(format!(
                "Failed to fetch filtered indices for library {}: {}",
                library_id, err
            ))
        })?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let idx: i32 = row.get("idx");
                (idx >= 0).then(|| idx as u32)
            })
            .collect())
    }
}

struct FilteredMovieIndexBuilder<'a> {
    spec: &'a FilterIndicesRequest,
    qb: QueryBuilder<'a, Postgres>,
    sort: SortBy,
    order: SortOrder,
    needs_watch_progress: bool,
    needs_watch_completed: bool,
}

impl<'a> fmt::Debug for FilteredMovieIndexBuilder<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FilteredMovieIndexBuilder")
            .field("sort", &self.sort)
            .field("order", &self.order)
            .field("needs_watch_progress", &self.needs_watch_progress)
            .field("needs_watch_completed", &self.needs_watch_completed)
            .field("filters", &self.spec)
            .field("query_builder", &"<sqlx::QueryBuilder<Postgres>>")
            .finish()
    }
}

impl<'a> FilteredMovieIndexBuilder<'a> {
    fn new(
        library_id: Uuid,
        spec: &'a FilterIndicesRequest,
        user_id: Option<Uuid>,
    ) -> std::result::Result<Self, FilterQueryError> {
        if let Some(media_type) = spec.media_type
            && media_type != MediaTypeFilter::Movie
        {
            return Err(FilterQueryError::UnsupportedMediaType(media_type));
        }

        let sort = spec.sort.unwrap_or(SortBy::Title);
        let order = spec.order.unwrap_or(SortOrder::Ascending);

        let mut qb = QueryBuilder::new(
            "SELECT (msp.title_pos - 1)::INT4 AS idx \
             FROM movie_references mr \
             JOIN media_files mf ON mr.file_id = mf.id \
             LEFT JOIN movie_metadata mm ON mr.id = mm.movie_id \
             JOIN movie_sort_positions msp ON msp.movie_id = mr.id",
        );

        let mut needs_watch_progress = matches!(sort, SortBy::WatchProgress | SortBy::LastWatched);
        let mut needs_watch_completed = matches!(sort, SortBy::LastWatched);

        if let Some(watch_status) = &spec.watch_status {
            match watch_status {
                WatchStatusFilter::InProgress => needs_watch_progress = true,
                WatchStatusFilter::Completed => needs_watch_completed = true,
                WatchStatusFilter::Unwatched | WatchStatusFilter::RecentlyWatched { .. } => {
                    needs_watch_progress = true;
                    needs_watch_completed = true;
                }
            }
        }

        if (needs_watch_progress || needs_watch_completed) && user_id.is_none() {
            return Err(FilterQueryError::MissingUserContext(
                "watch-status filters and sorts",
            ));
        }

        if needs_watch_progress {
            qb.push(
                " LEFT JOIN user_watch_progress uwp \
                  ON uwp.media_uuid = mr.id \
                 AND uwp.media_type = 0",
            );
            if let Some(uid) = user_id {
                qb.push(" AND uwp.user_id = ");
                qb.push_bind(uid);
            }
        }

        if needs_watch_completed {
            qb.push(
                " LEFT JOIN user_completed_media ucm \
                  ON ucm.media_uuid = mr.id \
                 AND ucm.media_type = 0",
            );
            if let Some(uid) = user_id {
                qb.push(" AND ucm.user_id = ");
                qb.push_bind(uid);
            }
        }

        qb.push(" WHERE mr.library_id = ");
        qb.push_bind(library_id);
        qb.push(" AND msp.library_id = ");
        qb.push_bind(library_id);

        Ok(Self {
            spec,
            qb,
            sort,
            order,
            needs_watch_progress,
            needs_watch_completed,
        })
    }

    fn build(mut self) -> std::result::Result<QueryBuilder<'a, Postgres>, FilterQueryError> {
        self.apply_filters()?;
        self.apply_sort()?;
        Ok(self.qb)
    }

    fn apply_filters(&mut self) -> std::result::Result<(), FilterQueryError> {
        if !self.spec.genres.is_empty() {
            self.qb.push(
                " AND EXISTS (SELECT 1 FROM movie_genres mg WHERE mg.movie_id = mr.id AND mg.name = ANY("
            );
            self.qb.push_bind(&self.spec.genres);
            self.qb.push("))");
        }

        if let Some(range) = self.spec.year_range {
            self.push_year_filter(range);
        }

        if let Some(range) = self.spec.rating_range {
            self.push_rating_filter(range)?;
        }

        if let Some(range) = self.spec.resolution_range {
            self.push_resolution_filter(range);
        }

        if let Some(search) = self.spec.search.as_ref() {
            let like = format!("%{}%", search);
            self.qb.push(" AND (mr.title ILIKE ");
            self.qb.push_bind(like.clone());
            self.qb.push(" OR mm.overview ILIKE ");
            self.qb.push_bind(like);
            self.qb.push(")");
        }

        if let Some(status) = self.spec.watch_status.as_ref() {
            self.apply_watch_status_filter(status)?;
        }

        Ok(())
    }

    fn push_year_filter(&mut self, range: crate::api_types::ScalarRange<u16>) {
        self.qb.push(
            " AND mm.release_date IS NOT NULL AND EXTRACT(YEAR FROM mm.release_date)::INT BETWEEN ",
        );
        self.qb.push_bind(range.min as i32);
        self.qb.push(" AND ");
        self.qb.push_bind(range.max as i32);
    }

    fn push_rating_filter(
        &mut self,
        range: crate::api_types::ScalarRange<RatingValue>,
    ) -> std::result::Result<(), FilterQueryError> {
        self.qb.push(" AND mm.vote_average BETWEEN ");
        self.qb.push_bind(rating_bound(range.min));
        self.qb.push(" AND ");
        self.qb.push_bind(rating_bound(range.max));
        Ok(())
    }

    fn push_resolution_filter(&mut self, range: crate::api_types::ScalarRange<u16>) {
        self.qb
            .push(" AND ((mf.technical_metadata->>'height')::INTEGER) BETWEEN ");
        self.qb.push_bind(range.min as i32);
        self.qb.push(" AND ");
        self.qb.push_bind(range.max as i32);
    }

    fn apply_watch_status_filter(
        &mut self,
        filter: &WatchStatusFilter,
    ) -> std::result::Result<(), FilterQueryError> {
        match filter {
            WatchStatusFilter::InProgress => {
                if !self.needs_watch_progress {
                    return Err(FilterQueryError::MissingUserContext("in-progress filter"));
                }
                self.qb.push(" AND uwp.media_uuid IS NOT NULL");
            }
            WatchStatusFilter::Completed => {
                if !self.needs_watch_completed {
                    return Err(FilterQueryError::MissingUserContext("completed filter"));
                }
                self.qb.push(" AND ucm.media_uuid IS NOT NULL");
            }
            WatchStatusFilter::Unwatched => {
                if !(self.needs_watch_completed && self.needs_watch_progress) {
                    return Err(FilterQueryError::MissingUserContext("unwatched filter"));
                }
                self.qb
                    .push(" AND uwp.media_uuid IS NULL AND ucm.media_uuid IS NULL");
            }
            WatchStatusFilter::RecentlyWatched { days } => {
                if !(self.needs_watch_completed && self.needs_watch_progress) {
                    return Err(FilterQueryError::MissingUserContext(
                        "recently watched filter",
                    ));
                }
                let days = (*days).max(1) as i64;
                let threshold = Utc::now() - Duration::days(days);
                let epoch = threshold.timestamp();
                self.qb.push(
                    " AND GREATEST(COALESCE(uwp.last_watched, 0), COALESCE(ucm.completed_at, 0)) >= ",
                );
                self.qb.push_bind(epoch);
            }
        }

        Ok(())
    }

    fn apply_sort(&mut self) -> std::result::Result<(), FilterQueryError> {
        match self.sort {
            SortBy::WatchProgress => {
                if !self.needs_watch_progress {
                    return Err(FilterQueryError::MissingUserContext("watch progress sort"));
                }
                let order_suffix = match self.order {
                    SortOrder::Ascending => " ASC NULLS LAST",
                    SortOrder::Descending => " DESC NULLS LAST",
                };
                self.qb.push(" ORDER BY CASE WHEN uwp.duration > 0 THEN (uwp.position::FLOAT8 / NULLIF(uwp.duration::FLOAT8, 0)) ELSE NULL END");
                self.qb.push(order_suffix);
                self.qb.push(", msp.title_pos ASC");
            }
            SortBy::LastWatched => {
                if !(self.needs_watch_progress || self.needs_watch_completed) {
                    return Err(FilterQueryError::MissingUserContext("last watched sort"));
                }
                let order_suffix = match self.order {
                    SortOrder::Ascending => " ASC NULLS LAST",
                    SortOrder::Descending => " DESC NULLS LAST",
                };
                self.qb.push(" ORDER BY GREATEST(COALESCE(uwp.last_watched, 0), COALESCE(ucm.completed_at, 0))");
                self.qb.push(order_suffix);
                self.qb.push(", msp.title_pos ASC");
            }
            other => {
                let (order_col, direction) = map_sort_to_column(other, self.order);
                self.qb.push(" ORDER BY ");
                self.qb.push(order_col);
                self.qb.push(" ");
                self.qb.push(direction);
            }
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
enum FilterQueryError {
    #[error("user context required for {0}")]
    MissingUserContext(&'static str),
    #[error("unsupported media type {0:?} for filtered indices")]
    UnsupportedMediaType(MediaTypeFilter),
}

fn map_sort_to_column(sort: SortBy, order: SortOrder) -> (&'static str, &'static str) {
    match (sort, order) {
        (SortBy::Title, SortOrder::Ascending) => ("msp.title_pos", "ASC"),
        (SortBy::Title, SortOrder::Descending) => ("msp.title_pos_desc", "ASC"),
        (SortBy::DateAdded, SortOrder::Ascending) => ("msp.date_added_pos", "ASC"),
        (SortBy::DateAdded, SortOrder::Descending) => ("msp.date_added_pos_desc", "ASC"),
        (SortBy::CreatedAt, SortOrder::Ascending) => ("msp.created_at_pos", "ASC"),
        (SortBy::CreatedAt, SortOrder::Descending) => ("msp.created_at_pos_desc", "ASC"),
        (SortBy::ReleaseDate, SortOrder::Ascending) => ("msp.release_date_pos", "ASC"),
        (SortBy::ReleaseDate, SortOrder::Descending) => ("msp.release_date_pos_desc", "ASC"),
        (SortBy::Rating, SortOrder::Ascending) => ("msp.rating_pos", "ASC"),
        (SortBy::Rating, SortOrder::Descending) => ("msp.rating_pos_desc", "ASC"),
        (SortBy::Runtime, SortOrder::Ascending) => ("msp.runtime_pos", "ASC"),
        (SortBy::Runtime, SortOrder::Descending) => ("msp.runtime_pos_desc", "ASC"),
        (SortBy::Popularity, SortOrder::Ascending) => ("msp.popularity_pos", "ASC"),
        (SortBy::Popularity, SortOrder::Descending) => ("msp.popularity_pos_desc", "ASC"),
        (SortBy::Bitrate, SortOrder::Ascending) => ("msp.bitrate_pos", "ASC"),
        (SortBy::Bitrate, SortOrder::Descending) => ("msp.bitrate_pos_desc", "ASC"),
        (SortBy::FileSize, SortOrder::Ascending) => ("msp.file_size_pos", "ASC"),
        (SortBy::FileSize, SortOrder::Descending) => ("msp.file_size_pos_desc", "ASC"),
        (SortBy::ContentRating, SortOrder::Ascending) => ("msp.content_rating_pos", "ASC"),
        (SortBy::ContentRating, SortOrder::Descending) => ("msp.content_rating_pos_desc", "ASC"),
        (SortBy::Resolution, SortOrder::Ascending) => ("msp.resolution_pos", "ASC"),
        (SortBy::Resolution, SortOrder::Descending) => ("msp.resolution_pos_desc", "ASC"),
        _ => ("msp.title_pos", "ASC"),
    }
}

fn rating_bound(value: RatingValue) -> BigDecimal {
    BigDecimal::from(value).with_scale(RATING_DECIMAL_SCALE as i64)
}
