use async_trait::async_trait;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    api::types::{
        RATING_DECIMAL_SCALE, RatingValue, filters::FilterIndicesRequest,
    },
    database::repository_ports::indices::IndicesRepository,
    domain::watch::WatchStatusFilter,
    error::{MediaError, Result},
    query::types::{MediaTypeFilter, SortBy, SortOrder},
    types::ids::LibraryId,
};

#[derive(Clone, Debug)]
pub struct PostgresIndicesRepository {
    pool: PgPool,
}

#[derive(Debug)]
struct MovieIndexRow {
    idx: i32,
}

#[derive(Debug, Clone, Copy)]
enum MoviePositionSort {
    TitleAsc = 0,
    TitleDesc = 1,
    DateAddedAsc = 2,
    DateAddedDesc = 3,
    CreatedAtAsc = 4,
    CreatedAtDesc = 5,
    ReleaseDateAsc = 6,
    ReleaseDateDesc = 7,
    RatingAsc = 8,
    RatingDesc = 9,
    RuntimeAsc = 10,
    RuntimeDesc = 11,
    PopularityAsc = 12,
    PopularityDesc = 13,
    BitrateAsc = 14,
    BitrateDesc = 15,
    FileSizeAsc = 16,
    FileSizeDesc = 17,
    ContentRatingAsc = 18,
    ContentRatingDesc = 19,
    ResolutionAsc = 20,
    ResolutionDesc = 21,
}

impl MoviePositionSort {
    fn from_sort(sort: SortBy, order: SortOrder) -> Self {
        match (sort, order) {
            (SortBy::Title, SortOrder::Ascending) => Self::TitleAsc,
            (SortBy::Title, SortOrder::Descending) => Self::TitleDesc,
            (SortBy::DateAdded, SortOrder::Ascending) => Self::DateAddedAsc,
            (SortBy::DateAdded, SortOrder::Descending) => Self::DateAddedDesc,
            (SortBy::CreatedAt, SortOrder::Ascending) => Self::CreatedAtAsc,
            (SortBy::CreatedAt, SortOrder::Descending) => Self::CreatedAtDesc,
            (SortBy::ReleaseDate, SortOrder::Ascending) => Self::ReleaseDateAsc,
            (SortBy::ReleaseDate, SortOrder::Descending) => {
                Self::ReleaseDateDesc
            }
            (SortBy::Rating, SortOrder::Ascending) => Self::RatingAsc,
            (SortBy::Rating, SortOrder::Descending) => Self::RatingDesc,
            (SortBy::Runtime, SortOrder::Ascending) => Self::RuntimeAsc,
            (SortBy::Runtime, SortOrder::Descending) => Self::RuntimeDesc,
            (SortBy::Popularity, SortOrder::Ascending) => Self::PopularityAsc,
            (SortBy::Popularity, SortOrder::Descending) => Self::PopularityDesc,
            (SortBy::Bitrate, SortOrder::Ascending) => Self::BitrateAsc,
            (SortBy::Bitrate, SortOrder::Descending) => Self::BitrateDesc,
            (SortBy::FileSize, SortOrder::Ascending) => Self::FileSizeAsc,
            (SortBy::FileSize, SortOrder::Descending) => Self::FileSizeDesc,
            (SortBy::ContentRating, SortOrder::Ascending) => {
                Self::ContentRatingAsc
            }
            (SortBy::ContentRating, SortOrder::Descending) => {
                Self::ContentRatingDesc
            }
            (SortBy::Resolution, SortOrder::Ascending) => Self::ResolutionAsc,
            (SortBy::Resolution, SortOrder::Descending) => Self::ResolutionDesc,
            _ => Self::TitleAsc,
        }
    }

    fn as_i16(self) -> i16 {
        self as i16
    }
}

#[derive(Debug, Clone, Copy)]
enum FilteredMovieSort {
    Position(MoviePositionSort),
    WatchProgressAsc,
    WatchProgressDesc,
    LastWatchedAsc,
    LastWatchedDesc,
}

impl FilteredMovieSort {
    fn from_sort(sort: SortBy, order: SortOrder) -> Self {
        match (sort, order) {
            (SortBy::WatchProgress, SortOrder::Ascending) => {
                Self::WatchProgressAsc
            }
            (SortBy::WatchProgress, SortOrder::Descending) => {
                Self::WatchProgressDesc
            }
            (SortBy::LastWatched, SortOrder::Ascending) => Self::LastWatchedAsc,
            (SortBy::LastWatched, SortOrder::Descending) => {
                Self::LastWatchedDesc
            }
            (sort, order) => {
                Self::Position(MoviePositionSort::from_sort(sort, order))
            }
        }
    }

    fn as_i16(self) -> i16 {
        match self {
            Self::Position(sort) => sort.as_i16(),
            Self::WatchProgressAsc => 22,
            Self::WatchProgressDesc => 23,
            Self::LastWatchedAsc => 24,
            Self::LastWatchedDesc => 25,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum WatchFilterKey {
    None = 0,
    InProgress = 1,
    Completed = 2,
    Unwatched = 3,
    RecentlyWatched = 4,
}

impl WatchFilterKey {
    fn as_i16(self) -> i16 {
        self as i16
    }
}

#[derive(Debug)]
struct FilteredMovieIndexParams<'a> {
    library_id: Uuid,
    genres: &'a [String],
    year_min: Option<i32>,
    year_max: Option<i32>,
    rating_min: Option<f32>,
    rating_max: Option<f32>,
    resolution_min: Option<i32>,
    resolution_max: Option<i32>,
    search_like: Option<String>,
    user_id: Option<Uuid>,
    watch_filter: WatchFilterKey,
    recent_epoch: Option<i64>,
    sort_key: i16,
}

impl<'a> FilteredMovieIndexParams<'a> {
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
        let filtered_sort = FilteredMovieSort::from_sort(sort, order);

        let needs_user =
            matches!(sort, SortBy::WatchProgress | SortBy::LastWatched)
                || spec.watch_status.is_some();
        if needs_user && user_id.is_none() {
            return Err(FilterQueryError::MissingUserContext(
                "watch-status filters and sorts",
            ));
        }

        let (watch_filter, recent_epoch) = match spec.watch_status.as_ref() {
            Some(WatchStatusFilter::InProgress) => {
                (WatchFilterKey::InProgress, None)
            }
            Some(WatchStatusFilter::Completed) => {
                (WatchFilterKey::Completed, None)
            }
            Some(WatchStatusFilter::Unwatched) => {
                (WatchFilterKey::Unwatched, None)
            }
            Some(WatchStatusFilter::RecentlyWatched { days }) => {
                let days = (*days).max(1) as i64;
                let threshold = Utc::now() - Duration::days(days);
                (WatchFilterKey::RecentlyWatched, Some(threshold.timestamp()))
            }
            None => (WatchFilterKey::None, None),
        };

        Ok(Self {
            library_id,
            genres: &spec.genres,
            year_min: spec.year_range.map(|range| range.min as i32),
            year_max: spec.year_range.map(|range| range.max as i32),
            rating_min: spec.rating_range.map(|range| rating_bound(range.min)),
            rating_max: spec.rating_range.map(|range| rating_bound(range.max)),
            resolution_min: spec.resolution_range.map(|range| range.min as i32),
            resolution_max: spec.resolution_range.map(|range| range.max as i32),
            search_like: spec
                .search
                .as_ref()
                .map(|search| format!("%{search}%")),
            user_id,
            watch_filter,
            recent_epoch,
            sort_key: filtered_sort.as_i16(),
        })
    }
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
    async fn rebuild_movie_sort_positions(
        &self,
        library_id: LibraryId,
    ) -> Result<()> {
        sqlx::query!(
            "SELECT rebuild_movie_sort_positions($1)",
            library_id.as_uuid()
        )
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
        library_id: LibraryId,
        sort: SortBy,
        order: SortOrder,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<Vec<u32>> {
        let library_uuid = library_id.as_uuid();
        let offset = offset.map(|value| value as i64);
        let limit = limit.map(|value| value as i64);
        let sort_key = MoviePositionSort::from_sort(sort, order).as_i16();

        let rows = sqlx::query_as!(
            MovieIndexRow,
            r#"
            WITH available_movie_positions AS (
                SELECT msp.*,
                       (ROW_NUMBER() OVER (
                           PARTITION BY msp.library_id
                           ORDER BY msp.title_pos, msp.movie_id
                       ) - 1)::INT4 AS compact_title_idx
                  FROM movie_sort_positions msp
                  JOIN movie_references mr
                    ON mr.id = msp.movie_id
                   AND mr.library_id = msp.library_id
                  JOIN media_files mf
                    ON mf.id = mr.file_id
                   AND mf.library_id = mr.library_id
                 WHERE mf.is_available = TRUE
                   AND msp.library_id = $1
            )
            SELECT msp.compact_title_idx AS "idx!"
              FROM available_movie_positions msp
              LEFT JOIN movie_metadata mm
                ON mm.movie_id = msp.movie_id
               AND mm.library_id = msp.library_id
             ORDER BY CASE WHEN COALESCE(mm.poster_path, '') = '' THEN 1 ELSE 0 END,
               CASE WHEN $4::int2 = 0 THEN msp.title_pos END ASC,
               CASE WHEN $4::int2 = 1 THEN msp.title_pos_desc END ASC,
               CASE WHEN $4::int2 = 2 THEN msp.date_added_pos END ASC,
               CASE WHEN $4::int2 = 3 THEN msp.date_added_pos_desc END ASC,
               CASE WHEN $4::int2 = 4 THEN msp.created_at_pos END ASC,
               CASE WHEN $4::int2 = 5 THEN msp.created_at_pos_desc END ASC,
               CASE WHEN $4::int2 = 6 THEN msp.release_date_pos END ASC,
               CASE WHEN $4::int2 = 7 THEN msp.release_date_pos_desc END ASC,
               CASE WHEN $4::int2 = 8 THEN msp.rating_pos END ASC,
               CASE WHEN $4::int2 = 9 THEN msp.rating_pos_desc END ASC,
               CASE WHEN $4::int2 = 10 THEN msp.runtime_pos END ASC,
               CASE WHEN $4::int2 = 11 THEN msp.runtime_pos_desc END ASC,
               CASE WHEN $4::int2 = 12 THEN msp.popularity_pos END ASC,
               CASE WHEN $4::int2 = 13 THEN msp.popularity_pos_desc END ASC,
               CASE WHEN $4::int2 = 14 THEN msp.bitrate_pos END ASC,
               CASE WHEN $4::int2 = 15 THEN msp.bitrate_pos_desc END ASC,
               CASE WHEN $4::int2 = 16 THEN msp.file_size_pos END ASC,
               CASE WHEN $4::int2 = 17 THEN msp.file_size_pos_desc END ASC,
               CASE WHEN $4::int2 = 18 THEN msp.content_rating_pos END ASC,
               CASE WHEN $4::int2 = 19 THEN msp.content_rating_pos_desc END ASC,
               CASE WHEN $4::int2 = 20 THEN msp.resolution_pos END ASC,
               CASE WHEN $4::int2 = 21 THEN msp.resolution_pos_desc END ASC
             OFFSET $2::bigint
             LIMIT $3::bigint
            "#,
            library_uuid,
            offset,
            limit,
            sort_key
        )
        .fetch_all(self.pool())
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "Failed to fetch sorted indices for library {}: {}",
                library_id, err
            ))
        })?;

        Ok(rows_to_indices(rows))
    }

    async fn fetch_filtered_movie_indices(
        &self,
        library_id: LibraryId,
        spec: &FilterIndicesRequest,
        user_id: Option<Uuid>,
    ) -> Result<Vec<u32>> {
        let params =
            FilteredMovieIndexParams::new(library_id.to_uuid(), spec, user_id)
                .map_err(|err| MediaError::InvalidMedia(err.to_string()))?;

        let rows = sqlx::query_as!(
            MovieIndexRow,
            r#"
            WITH available_movie_positions AS (
                SELECT msp.movie_id,
                       msp.library_id,
                       (ROW_NUMBER() OVER (
                           PARTITION BY msp.library_id
                           ORDER BY msp.title_pos, msp.movie_id
                       ) - 1)::INT4 AS compact_title_idx
                  FROM movie_sort_positions msp
                  JOIN movie_references base_mr
                    ON base_mr.id = msp.movie_id
                   AND base_mr.library_id = msp.library_id
                  JOIN media_files base_mf
                    ON base_mf.id = base_mr.file_id
                   AND base_mf.library_id = base_mr.library_id
                 WHERE base_mf.is_available = TRUE
                   AND msp.library_id = $1
            )
            SELECT amp.compact_title_idx AS "idx!"
              FROM movie_references mr
              JOIN media_files mf
                ON mr.file_id = mf.id
               AND mf.library_id = mr.library_id
              LEFT JOIN movie_metadata mm
                ON mr.id = mm.movie_id
               AND mm.library_id = mr.library_id
              JOIN movie_sort_positions msp
                ON msp.movie_id = mr.id
               AND msp.library_id = mr.library_id
              JOIN available_movie_positions amp
                ON amp.movie_id = mr.id
               AND amp.library_id = mr.library_id
              LEFT JOIN user_watch_progress uwp
                ON uwp.media_uuid = mr.id
               AND uwp.media_type = 0
               AND uwp.user_id = $10
              LEFT JOIN user_completed_media ucm
                ON ucm.media_uuid = mr.id
               AND ucm.media_type = 0
               AND ucm.user_id = $10
             WHERE mf.is_available = TRUE
               AND mr.library_id = $1
               AND msp.library_id = $1
               AND (cardinality($2::text[]) = 0 OR EXISTS (
                    SELECT 1
                      FROM movie_genres mg
                     WHERE mg.movie_id = mr.id
                       AND mg.name = ANY($2)
               ))
               AND ($3::int4 IS NULL OR (
                    mm.release_date IS NOT NULL
                    AND EXTRACT(YEAR FROM mm.release_date)::INT BETWEEN $3 AND $4
               ))
               AND ($5::real IS NULL OR mm.vote_average BETWEEN $5 AND $6)
               AND ($7::int4 IS NULL OR ((mf.technical_metadata->>'height')::INTEGER) BETWEEN $7 AND $8)
               AND ($9::text IS NULL OR (mr.title ILIKE $9 OR mm.overview ILIKE $9))
               AND (
                    $11::int2 = 0
                    OR ($11::int2 = 1 AND uwp.media_uuid IS NOT NULL)
                    OR ($11::int2 = 2 AND ucm.media_uuid IS NOT NULL)
                    OR ($11::int2 = 3 AND uwp.media_uuid IS NULL AND ucm.media_uuid IS NULL)
                    OR ($11::int2 = 4 AND GREATEST(COALESCE(uwp.last_watched, 0), COALESCE(ucm.completed_at, 0)) >= $12)
               )
             ORDER BY CASE WHEN COALESCE(mm.poster_path, '') = '' THEN 1 ELSE 0 END,
               CASE WHEN $13::int2 = 0 THEN msp.title_pos END ASC,
               CASE WHEN $13::int2 = 1 THEN msp.title_pos_desc END ASC,
               CASE WHEN $13::int2 = 2 THEN msp.date_added_pos END ASC,
               CASE WHEN $13::int2 = 3 THEN msp.date_added_pos_desc END ASC,
               CASE WHEN $13::int2 = 4 THEN msp.created_at_pos END ASC,
               CASE WHEN $13::int2 = 5 THEN msp.created_at_pos_desc END ASC,
               CASE WHEN $13::int2 = 6 THEN msp.release_date_pos END ASC,
               CASE WHEN $13::int2 = 7 THEN msp.release_date_pos_desc END ASC,
               CASE WHEN $13::int2 = 8 THEN msp.rating_pos END ASC,
               CASE WHEN $13::int2 = 9 THEN msp.rating_pos_desc END ASC,
               CASE WHEN $13::int2 = 10 THEN msp.runtime_pos END ASC,
               CASE WHEN $13::int2 = 11 THEN msp.runtime_pos_desc END ASC,
               CASE WHEN $13::int2 = 12 THEN msp.popularity_pos END ASC,
               CASE WHEN $13::int2 = 13 THEN msp.popularity_pos_desc END ASC,
               CASE WHEN $13::int2 = 14 THEN msp.bitrate_pos END ASC,
               CASE WHEN $13::int2 = 15 THEN msp.bitrate_pos_desc END ASC,
               CASE WHEN $13::int2 = 16 THEN msp.file_size_pos END ASC,
               CASE WHEN $13::int2 = 17 THEN msp.file_size_pos_desc END ASC,
               CASE WHEN $13::int2 = 18 THEN msp.content_rating_pos END ASC,
               CASE WHEN $13::int2 = 19 THEN msp.content_rating_pos_desc END ASC,
               CASE WHEN $13::int2 = 20 THEN msp.resolution_pos END ASC,
               CASE WHEN $13::int2 = 21 THEN msp.resolution_pos_desc END ASC,
               CASE WHEN $13::int2 = 22 THEN CASE WHEN uwp.duration > 0 THEN (uwp.position::FLOAT8 / NULLIF(uwp.duration::FLOAT8, 0)) ELSE NULL END END ASC NULLS LAST,
               CASE WHEN $13::int2 = 23 THEN CASE WHEN uwp.duration > 0 THEN (uwp.position::FLOAT8 / NULLIF(uwp.duration::FLOAT8, 0)) ELSE NULL END END DESC NULLS LAST,
               CASE WHEN $13::int2 = 24 THEN GREATEST(COALESCE(uwp.last_watched, 0), COALESCE(ucm.completed_at, 0)) END ASC NULLS LAST,
               CASE WHEN $13::int2 = 25 THEN GREATEST(COALESCE(uwp.last_watched, 0), COALESCE(ucm.completed_at, 0)) END DESC NULLS LAST,
               msp.title_pos ASC
            "#,
            params.library_id,
            params.genres,
            params.year_min,
            params.year_max,
            params.rating_min,
            params.rating_max,
            params.resolution_min,
            params.resolution_max,
            params.search_like.as_deref(),
            params.user_id,
            params.watch_filter.as_i16(),
            params.recent_epoch,
            params.sort_key
        )
        .fetch_all(self.pool())
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "Failed to fetch filtered indices for library {}: {}",
                library_id, err
            ))
        })?;

        Ok(rows_to_indices(rows))
    }
}

#[derive(Debug, Error)]
enum FilterQueryError {
    #[error("user context required for {0}")]
    MissingUserContext(&'static str),
    #[error("unsupported media type {0:?} for filtered indices")]
    UnsupportedMediaType(MediaTypeFilter),
}

fn rows_to_indices(rows: Vec<MovieIndexRow>) -> Vec<u32> {
    rows.into_iter()
        .filter_map(|row| (row.idx >= 0).then_some(row.idx as u32))
        .collect()
}

fn rating_bound(value: RatingValue) -> f32 {
    value as f32 / 10f32.powi(RATING_DECIMAL_SCALE as i32)
}
