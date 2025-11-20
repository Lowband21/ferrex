use crate::{
    LibraryID, Media, MediaDatabase, MediaDetailsOption, Result, TmdbDetails,
    query::{SortBy, SortOrder},
};
use std::{collections::HashMap, fmt, sync::Arc};
use uuid::Uuid;

/// Manages in-memory sorting for libraries
pub struct IndexManager {
    db: Arc<MediaDatabase>,
}

impl fmt::Debug for IndexManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IndexManager")
            .field("db", &self.db)
            .finish()
    }
}

impl IndexManager {
    pub fn new(db: Arc<MediaDatabase>) -> Self {
        Self { db }
    }

    /// Convenience: sort media IDs for a library (no persistence)
    pub async fn sort_media_ids_for_library(
        &self,
        library_id: LibraryID,
        library_type: crate::LibraryType,
        sort_field: SortBy,
        sort_order: SortOrder,
        user_id: Option<Uuid>,
    ) -> Result<Vec<Uuid>> {
        let media_items = self
            .db
            .backend()
            .get_library_media_references(library_id, library_type)
            .await?;
        let watch_data = if matches!(sort_field, SortBy::WatchProgress | SortBy::LastWatched) {
            let user_id = user_id.ok_or_else(|| {
                crate::MediaError::InvalidMedia(
                    "watch-based sorting requires an authenticated user".to_string(),
                )
            })?;
            Some(self.load_watch_data(user_id).await?)
        } else {
            None
        };

        self.sort_media(&media_items, sort_field, sort_order, watch_data.as_ref())
            .await
    }

    /// Compute a title-based position map for a library to translate UUIDs into indices
    /// Uses the database's natural ordering (ORDER BY title) to maintain consistency
    pub async fn compute_title_position_map(
        &self,
        library_id: LibraryID,
        library_type: crate::LibraryType,
    ) -> Result<HashMap<Uuid, u32>> {
        // Get media items directly from database - they're already ordered by title
        let media_items = self
            .db
            .backend()
            .get_library_media_references(library_id, library_type)
            .await?;

        let mut positions = HashMap::with_capacity(media_items.len());
        for (idx, media) in media_items.iter().enumerate() {
            let media_id = Self::get_media_id(media);
            positions.insert(media_id, idx as u32);
        }
        Ok(positions)
    }

    /// Sort media items based on the specified field
    async fn sort_media(
        &self,
        media_items: &[Media],
        sort_field: SortBy,
        sort_order: SortOrder,
        watch_data: Option<&WatchData>,
    ) -> Result<Vec<Uuid>> {
        let mut indexed_items: Vec<(usize, &Media)> = media_items.iter().enumerate().collect();

        indexed_items.sort_by(|a, b| {
            let cmp = match sort_field {
                SortBy::Title => {
                    let a_title = Self::get_title(a.1);
                    let b_title = Self::get_title(b.1);
                    a_title.to_lowercase().cmp(&b_title.to_lowercase())
                }
                SortBy::DateAdded => {
                    let a_date = Self::get_date_added(a.1);
                    let b_date = Self::get_date_added(b.1);
                    a_date.cmp(&b_date)
                }
                SortBy::CreatedAt => {
                    let a_date = Self::get_created_at(a.1);
                    let b_date = Self::get_created_at(b.1);
                    a_date.cmp(&b_date)
                }
                SortBy::FileSize => {
                    let a_size = Self::get_file_size(a.1);
                    let b_size = Self::get_file_size(b.1);
                    a_size.cmp(&b_size)
                }
                SortBy::ReleaseDate => {
                    let a_date = Self::get_release_date(a.1);
                    let b_date = Self::get_release_date(b.1);
                    Self::compare_optional(a_date, b_date)
                }
                SortBy::Rating => {
                    let a_rating = Self::get_rating(a.1);
                    let b_rating = Self::get_rating(b.1);
                    Self::compare_optional_partial(a_rating, b_rating)
                }
                SortBy::Runtime => {
                    let a_runtime = Self::get_runtime(a.1);
                    let b_runtime = Self::get_runtime(b.1);
                    Self::compare_optional(a_runtime, b_runtime)
                }
                SortBy::Popularity => {
                    let a_popularity = Self::get_popularity(a.1);
                    let b_popularity = Self::get_popularity(b.1);
                    Self::compare_optional_partial(a_popularity, b_popularity)
                }
                SortBy::Bitrate => {
                    let a_bitrate = Self::get_bitrate(a.1);
                    let b_bitrate = Self::get_bitrate(b.1);
                    Self::compare_optional(a_bitrate, b_bitrate)
                }
                SortBy::ContentRating => {
                    let a_rating = Self::get_content_rating(a.1);
                    let b_rating = Self::get_content_rating(b.1);
                    Self::compare_optional_str(a_rating.as_deref(), b_rating.as_deref())
                }
                SortBy::Resolution => {
                    let a_res = Self::get_resolution(a.1);
                    let b_res = Self::get_resolution(b.1);
                    Self::compare_optional(a_res, b_res)
                }
                SortBy::WatchProgress => {
                    if let Some(data) = watch_data {
                        let a_ratio = data.progress(&Self::get_media_id(a.1));
                        let b_ratio = data.progress(&Self::get_media_id(b.1));
                        Self::compare_optional_partial(a_ratio, b_ratio)
                    } else {
                        std::cmp::Ordering::Equal
                    }
                }
                SortBy::LastWatched => {
                    if let Some(data) = watch_data {
                        let a_last = data.last_watched(&Self::get_media_id(a.1));
                        let b_last = data.last_watched(&Self::get_media_id(b.1));
                        Self::compare_optional(a_last, b_last)
                    } else {
                        std::cmp::Ordering::Equal
                    }
                }
            };
            if sort_order == SortOrder::Descending {
                cmp.reverse()
            } else {
                cmp
            }
        });

        let sorted_ids = indexed_items
            .into_iter()
            .map(|(_, media)| Self::get_media_id(media))
            .collect();

        Ok(sorted_ids)
    }

    fn get_title(media: &Media) -> &str {
        match media {
            Media::Movie(m) => m.title.as_str(),
            Media::Series(s) => s.title.as_str(),
            Media::Season(_) => "",
            Media::Episode(_) => "",
        }
    }

    fn get_date_added(media: &Media) -> chrono::DateTime<chrono::Utc> {
        match media {
            Media::Movie(m) => m.file.discovered_at,
            Media::Series(s) => s.discovered_at,
            Media::Season(s) => s.discovered_at,
            Media::Episode(e) => e.discovered_at,
        }
    }

    fn get_created_at(media: &Media) -> chrono::DateTime<chrono::Utc> {
        match media {
            Media::Movie(m) => m.file.created_at,
            Media::Series(s) => s.created_at,
            Media::Season(s) => s.created_at,
            Media::Episode(e) => e.created_at,
        }
    }

    fn get_file_size(media: &Media) -> u64 {
        match media {
            Media::Movie(m) => m.file.size,
            Media::Episode(e) => e.file.size,
            _ => 0,
        }
    }

    fn get_release_date(media: &Media) -> Option<chrono::NaiveDate> {
        fn parse(date: &Option<String>) -> Option<chrono::NaiveDate> {
            date.as_deref()
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        }

        match media {
            Media::Movie(m) => match &m.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(details)) => {
                    parse(&details.release_date)
                }
                _ => None,
            },
            Media::Series(s) => match &s.details {
                MediaDetailsOption::Details(TmdbDetails::Series(details)) => {
                    parse(&details.first_air_date)
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn get_rating(media: &Media) -> Option<f32> {
        match media {
            Media::Movie(m) => match &m.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(details)) => details.vote_average,
                _ => None,
            },
            Media::Series(s) => match &s.details {
                MediaDetailsOption::Details(TmdbDetails::Series(details)) => details.vote_average,
                _ => None,
            },
            _ => None,
        }
    }

    fn get_runtime(media: &Media) -> Option<u32> {
        match media {
            Media::Movie(m) => match &m.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(details)) => details.runtime,
                _ => None,
            },
            _ => None,
        }
    }

    fn get_popularity(media: &Media) -> Option<f32> {
        match media {
            Media::Movie(m) => match &m.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(details)) => details.popularity,
                _ => None,
            },
            Media::Series(s) => match &s.details {
                MediaDetailsOption::Details(TmdbDetails::Series(details)) => details.popularity,
                _ => None,
            },
            _ => None,
        }
    }

    fn get_bitrate(media: &Media) -> Option<u64> {
        match media {
            Media::Movie(m) => m
                .file
                .media_file_metadata
                .as_ref()
                .and_then(|meta| meta.bitrate),
            Media::Episode(e) => e
                .file
                .media_file_metadata
                .as_ref()
                .and_then(|meta| meta.bitrate),
            _ => None,
        }
    }

    fn get_resolution(media: &Media) -> Option<u32> {
        match media {
            Media::Movie(m) => m
                .file
                .media_file_metadata
                .as_ref()
                .and_then(|meta| meta.height),
            Media::Episode(e) => e
                .file
                .media_file_metadata
                .as_ref()
                .and_then(|meta| meta.height),
            _ => None,
        }
    }

    fn get_content_rating(media: &Media) -> Option<String> {
        match media {
            Media::Movie(m) => match &m.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(details)) => details
                    .content_rating
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
                _ => None,
            },
            Media::Series(s) => match &s.details {
                MediaDetailsOption::Details(TmdbDetails::Series(details)) => details
                    .content_rating
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
                _ => None,
            },
            _ => None,
        }
    }

    fn get_media_id(media: &Media) -> Uuid {
        use crate::MediaIDLike;
        match media {
            Media::Movie(m) => m.id.to_uuid(),
            Media::Series(s) => s.id.to_uuid(),
            Media::Season(s) => s.id.to_uuid(),
            Media::Episode(e) => e.id.to_uuid(),
        }
    }

    fn compare_optional<T: Ord>(a: Option<T>, b: Option<T>) -> std::cmp::Ordering {
        match (a, b) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }

    fn compare_optional_partial<T: PartialOrd>(a: Option<T>, b: Option<T>) -> std::cmp::Ordering {
        match (a, b) {
            (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }

    fn compare_optional_str(a: Option<&str>, b: Option<&str>) -> std::cmp::Ordering {
        match (a, b) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }
}

struct WatchData {
    progress: HashMap<Uuid, ProgressEntry>,
    completed: HashMap<Uuid, i64>,
}

impl WatchData {
    fn progress(&self, media_id: &Uuid) -> Option<f32> {
        if let Some(entry) = self.progress.get(media_id) {
            Some(entry.ratio)
        } else if self.completed.contains_key(media_id) {
            Some(1.0)
        } else {
            None
        }
    }

    fn last_watched(&self, media_id: &Uuid) -> Option<i64> {
        if let Some(entry) = self.progress.get(media_id) {
            Some(entry.last_watched)
        } else {
            self.completed.get(media_id).copied()
        }
    }
}

struct ProgressEntry {
    ratio: f32,
    last_watched: i64,
}

impl IndexManager {
    async fn load_watch_data(&self, user_id: Uuid) -> Result<WatchData> {
        use crate::database::postgres::PostgresDatabase;

        let backend = self.db.backend();
        let pg = backend
            .as_any()
            .downcast_ref::<PostgresDatabase>()
            .ok_or_else(|| {
                crate::MediaError::InvalidMedia(
                    "watch-based sorting is only supported on Postgres backend".to_string(),
                )
            })?;

        use sqlx::Row;

        let progress_rows = sqlx::query(
            r#"
            SELECT media_uuid, position, duration, last_watched
            FROM user_watch_progress
            WHERE user_id = $1
            ORDER BY last_watched DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(pg.pool())
        .await
        .map_err(|e| {
            crate::MediaError::Internal(format!(
                "Failed to load watch progress for user {}: {}",
                user_id, e
            ))
        })?;

        let mut progress = HashMap::new();
        for row in progress_rows {
            let duration: f32 = match row.try_get("duration") {
                Ok(value) => value,
                Err(_) => continue,
            };
            if duration <= 0.0 {
                continue;
            }
            let position: f32 = match row.try_get("position") {
                Ok(value) => value,
                Err(_) => continue,
            };
            let media_uuid: Uuid = match row.try_get("media_uuid") {
                Ok(value) => value,
                Err(_) => continue,
            };
            let last_watched: i64 = row.try_get("last_watched").unwrap_or(0);
            let ratio = (position / duration).clamp(0.0, 1.0);
            progress.insert(
                media_uuid,
                ProgressEntry {
                    ratio,
                    last_watched,
                },
            );
        }

        let completed_rows = sqlx::query(
            r#"
            SELECT media_uuid, completed_at
            FROM user_completed_media
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_all(pg.pool())
        .await
        .map_err(|e| {
            crate::MediaError::Internal(format!(
                "Failed to load completed media for user {}: {}",
                user_id, e
            ))
        })?;

        let mut completed = HashMap::new();
        for row in completed_rows {
            let media_uuid: Uuid = match row.try_get("media_uuid") {
                Ok(value) => value,
                Err(_) => continue,
            };
            let completed_at: i64 = row.try_get("completed_at").unwrap_or(0);
            completed.insert(media_uuid, completed_at);
        }

        Ok(WatchData {
            progress,
            completed,
        })
    }
}
