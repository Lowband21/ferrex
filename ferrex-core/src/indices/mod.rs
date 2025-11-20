use crate::{
    LibraryID, Media, MediaDatabase, MediaDetailsOption, Result, TmdbDetails,
    database::traits::MediaDatabaseTrait,
    query::{SortBy, SortOrder},
};
use std::sync::Arc;
use uuid::Uuid;

/// Manages in-memory sorting for libraries
pub struct IndexManager {
    db: Arc<MediaDatabase>,
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
    ) -> Result<Vec<Uuid>> {
        let media_items = self
            .db
            .backend()
            .get_library_media_references(library_id, library_type)
            .await?;
        self.sort_media(&media_items, sort_field, sort_order).await
    }

    /// Sort media items based on the specified field
    async fn sort_media(
        &self,
        media_items: &[Media],
        sort_field: SortBy,
        sort_order: SortOrder,
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
                _ => std::cmp::Ordering::Equal,
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
            Media::Movie(m) => m.file.created_at,
            Media::Series(s) => s.created_at,
            Media::Season(s) => s.created_at,
            Media::Episode(e) => e.file.created_at,
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
                MediaDetailsOption::Details(TmdbDetails::Movie(details)) => parse(&details.release_date),
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
            Media::Movie(m) => m.file.media_file_metadata.as_ref().and_then(|meta| meta.bitrate),
            Media::Episode(e) => e.file.media_file_metadata.as_ref().and_then(|meta| meta.bitrate),
            _ => None,
        }
    }

    fn get_resolution(media: &Media) -> Option<u32> {
        match media {
            Media::Movie(m) => m.file.media_file_metadata.as_ref().and_then(|meta| meta.height),
            Media::Episode(e) => e.file.media_file_metadata.as_ref().and_then(|meta| meta.height),
            _ => None,
        }
    }

    fn get_content_rating(media: &Media) -> Option<String> {
        match media {
            Media::Movie(_m) => None, // TODO: populate from metadata when available
            Media::Series(_s) => None,
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

    fn compare_optional_partial<T: PartialOrd>(
        a: Option<T>,
        b: Option<T>,
    ) -> std::cmp::Ordering {
        match (a, b) {
            (Some(a), Some(b)) => a
                .partial_cmp(&b)
                .unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }

    fn compare_optional_str(
        a: Option<&str>,
        b: Option<&str>,
    ) -> std::cmp::Ordering {
        match (a, b) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }
}
