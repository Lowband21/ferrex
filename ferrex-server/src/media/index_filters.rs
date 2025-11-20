use std::str::FromStr;

use chrono::{Duration, Utc};
use ferrex_core::{
    FilterIndicesRequest,
    api_types::ScalarRange,
    query::types::{MediaTypeFilter, SortBy, SortOrder},
    watch_status::WatchStatusFilter,
};
use num_traits::FromPrimitive;
use sqlx::{Postgres, QueryBuilder, types::BigDecimal};
use uuid::Uuid;

#[derive(Debug)]
pub enum FilterQueryError {
    MissingUserContext(&'static str),
    UnsupportedMediaType(MediaTypeFilter),
    InvalidNumeric(&'static str),
}

impl std::fmt::Display for FilterQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterQueryError::MissingUserContext(reason) => {
                write!(f, "user context required for {reason}")
            }
            FilterQueryError::UnsupportedMediaType(media_type) => {
                write!(
                    f,
                    "unsupported media type {:?} for filtered indices",
                    media_type
                )
            }
            FilterQueryError::InvalidNumeric(field) => {
                write!(f, "invalid numeric value for {field}")
            }
        }
    }
}

impl std::error::Error for FilterQueryError {}

pub struct FilteredMovieIndexBuilder<'a> {
    spec: &'a FilterIndicesRequest,
    qb: QueryBuilder<'a, Postgres>,
    sort: SortBy,
    order: SortOrder,
    user_id: Option<Uuid>,
    needs_watch_progress: bool,
    needs_watch_completed: bool,
}

impl<'a> FilteredMovieIndexBuilder<'a> {
    pub fn new(
        library_id: Uuid,
        spec: &'a FilterIndicesRequest,
        user_id: Option<Uuid>,
    ) -> Result<Self, FilterQueryError> {
        if let Some(media_type) = spec.media_type {
            if media_type != MediaTypeFilter::Movie {
                return Err(FilterQueryError::UnsupportedMediaType(media_type));
            }
        }

        let sort = spec.sort.unwrap_or(SortBy::Title);
        let order = spec.order.unwrap_or(SortOrder::Ascending);

        let mut qb = QueryBuilder::new(
            "SELECT mr.id \
             FROM movie_references mr \
             JOIN media_files mf ON mr.file_id = mf.id \
             LEFT JOIN movie_metadata mm ON mr.id = mm.movie_id",
        );

        let mut needs_watch_progress = matches!(sort, SortBy::WatchProgress | SortBy::LastWatched);
        let mut needs_watch_completed = matches!(sort, SortBy::LastWatched);

        if let Some(watch_status) = &spec.watch_status {
            match watch_status {
                WatchStatusFilter::InProgress => needs_watch_progress = true,
                WatchStatusFilter::Completed => needs_watch_completed = true,
                WatchStatusFilter::Unwatched => {
                    needs_watch_progress = true;
                    needs_watch_completed = true;
                }
                WatchStatusFilter::RecentlyWatched { .. } => {
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

        Ok(Self {
            spec,
            qb,
            sort,
            order,
            user_id,
            needs_watch_progress,
            needs_watch_completed,
        })
    }

    pub fn build(mut self) -> Result<QueryBuilder<'a, Postgres>, FilterQueryError> {
        self.apply_filters()?;
        self.apply_sort()?;
        Ok(self.qb)
    }

    fn apply_filters(&mut self) -> Result<(), FilterQueryError> {
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

    fn push_year_filter(&mut self, range: ScalarRange<u16>) {
        self.qb.push(
            " AND mm.release_date IS NOT NULL AND EXTRACT(YEAR FROM mm.release_date)::INT BETWEEN ",
        );
        self.qb.push_bind(range.min as i32);
        self.qb.push(" AND ");
        self.qb.push_bind(range.max as i32);
    }

    fn push_rating_filter(&mut self, range: ScalarRange<f32>) -> Result<(), FilterQueryError> {
        self.qb.push(" AND mm.vote_average BETWEEN ");
        let min = BigDecimal::from_f32(range.min)
            .or_else(|| BigDecimal::from_f64(f64::from(range.min)))
            .ok_or(FilterQueryError::InvalidNumeric("rating_min"))?;
        self.qb.push_bind(min);
        self.qb.push(" AND ");
        let max = BigDecimal::from_f32(range.max)
            .or_else(|| BigDecimal::from_f64(f64::from(range.max)))
            .ok_or(FilterQueryError::InvalidNumeric("rating_max"))?;
        self.qb.push_bind(max);
        Ok(())
    }

    fn push_resolution_filter(&mut self, range: ScalarRange<u16>) {
        self.qb
            .push(" AND ((mf.technical_metadata->>'height')::INTEGER) BETWEEN ");
        self.qb.push_bind(range.min as i32);
        self.qb.push(" AND ");
        self.qb.push_bind(range.max as i32);
    }

    fn apply_watch_status_filter(
        &mut self,
        filter: &WatchStatusFilter,
    ) -> Result<(), FilterQueryError> {
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

    fn apply_sort(&mut self) -> Result<(), FilterQueryError> {
        let order_suffix = match self.order {
            SortOrder::Ascending => " ASC NULLS LAST",
            SortOrder::Descending => " DESC NULLS LAST",
        };

        let primary_expr = match self.sort {
            SortBy::Title => "LOWER(mr.title)",
            SortBy::DateAdded => "mf.created_at",
            SortBy::ReleaseDate => "mm.release_date",
            SortBy::Rating => "mm.vote_average",
            SortBy::Runtime => "mm.runtime",
            SortBy::Popularity => "mm.popularity",
            SortBy::Bitrate => "((mf.technical_metadata->>'bitrate')::BIGINT)",
            SortBy::FileSize => "mf.file_size",
            SortBy::ContentRating => "mm.primary_certification",
            SortBy::Resolution => "((mf.technical_metadata->>'height')::INTEGER)",
            SortBy::WatchProgress => {
                if !self.needs_watch_progress {
                    return Err(FilterQueryError::MissingUserContext("watch progress sort"));
                }
                "CASE WHEN uwp.duration > 0 THEN uwp.position / uwp.duration ELSE NULL END"
            }
            SortBy::LastWatched => {
                if !(self.needs_watch_progress || self.needs_watch_completed) {
                    return Err(FilterQueryError::MissingUserContext("last watched sort"));
                }
                "GREATEST(COALESCE(uwp.last_watched, 0), COALESCE(ucm.completed_at, 0))"
            }
        };

        self.qb.push(" ORDER BY ");
        self.qb.push(primary_expr);
        self.qb.push(order_suffix);
        self.qb.push(", LOWER(mr.title) ASC, mr.id ASC");

        Ok(())
    }
}

pub fn build_filtered_movie_query(
    library_id: Uuid,
    spec: &FilterIndicesRequest,
    user_id: Option<Uuid>,
) -> Result<QueryBuilder<Postgres>, FilterQueryError> {
    FilteredMovieIndexBuilder::new(library_id, spec, user_id)?.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_core::{
        api_types::ScalarRange,
        query::types::{MediaTypeFilter, SortBy, SortOrder},
    };

    #[test]
    fn resolution_filter_included_in_sql() {
        let spec = FilterIndicesRequest {
            media_type: Some(MediaTypeFilter::Movie),
            genres: vec![],
            year_range: None,
            rating_range: None,
            resolution_range: Some(ScalarRange::new(720, 1080)),
            watch_status: None,
            search: None,
            sort: Some(SortBy::Title),
            order: Some(SortOrder::Ascending),
        };

        let qb = build_filtered_movie_query(Uuid::new_v4(), &spec, Some(Uuid::new_v4())).unwrap();
        let sql = qb.sql();
        assert!(sql.contains("technical_metadata->>'height'"));
        assert!(sql.contains("BETWEEN"));
    }

    #[test]
    fn watch_progress_sort_includes_progress_join() {
        let spec = FilterIndicesRequest {
            media_type: Some(MediaTypeFilter::Movie),
            genres: vec![],
            year_range: None,
            rating_range: None,
            resolution_range: None,
            watch_status: None,
            search: None,
            sort: Some(SortBy::WatchProgress),
            order: Some(SortOrder::Descending),
        };

        let qb = build_filtered_movie_query(Uuid::new_v4(), &spec, Some(Uuid::new_v4())).unwrap();
        let sql = qb.sql();
        assert!(sql.contains("user_watch_progress"));
    }

    #[test]
    fn completed_filter_adds_ucm_join() {
        let spec = FilterIndicesRequest {
            media_type: Some(MediaTypeFilter::Movie),
            genres: vec![],
            year_range: None,
            rating_range: None,
            resolution_range: None,
            watch_status: Some(WatchStatusFilter::Completed),
            search: None,
            sort: Some(SortBy::Title),
            order: Some(SortOrder::Ascending),
        };

        let qb = build_filtered_movie_query(Uuid::new_v4(), &spec, Some(Uuid::new_v4())).unwrap();
        let sql = qb.sql();
        assert!(sql.contains("user_completed_media"));
        assert!(sql.contains("ucm.media_uuid IS NOT NULL"));
    }
}
