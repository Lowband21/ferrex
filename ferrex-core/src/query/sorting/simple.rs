use crate::{
    query::types::{SortBy, SortOrder},
    types::{
        details::{MediaDetailsOption, TmdbDetails},
        media::Media,
    },
};
use chrono::{DateTime, NaiveDate, Utc};
use std::cmp::Ordering;

fn has_poster(media: &Media) -> bool {
    match media {
        Media::Movie(m) => m
            .details
            .as_movie()
            .and_then(|d| d.poster_path.as_deref())
            .map(|p| !p.is_empty())
            .unwrap_or(false),
        Media::Series(s) => s
            .details
            .as_series()
            .and_then(|d| d.poster_path.as_deref())
            .map(|p| !p.is_empty())
            .unwrap_or(false),
        Media::Season(season) => season
            .details
            .as_season()
            .and_then(|d| d.poster_path.as_deref())
            .map(|p| !p.is_empty())
            .unwrap_or(false),
        Media::Episode(e) => e
            .details
            .as_episode()
            .and_then(|d| d.still_path.as_deref())
            .map(|p| !p.is_empty())
            .unwrap_or(false),
    }
}

/// Compare two media items using the provided sort field and order.
/// Returns `None` when the comparison cannot be evaluated (e.g. user-context fields).
pub fn compare_media(
    a: &Media,
    b: &Media,
    sort_by: SortBy,
    sort_order: SortOrder,
) -> Option<Ordering> {
    // Always push items without posters to the end, regardless of the active sort.
    let a_has = has_poster(a);
    let b_has = has_poster(b);
    if a_has != b_has {
        return Some(if a_has {
            Ordering::Less
        } else {
            Ordering::Greater
        });
    }

    let ord = match sort_by {
        SortBy::Title => {
            let a_title = get_title(a).to_lowercase();
            let b_title = get_title(b).to_lowercase();
            Some(a_title.cmp(&b_title))
        }
        SortBy::DateAdded => Some(get_date_added(a).cmp(&get_date_added(b))),
        SortBy::CreatedAt => Some(get_created_at(a).cmp(&get_created_at(b))),
        SortBy::ReleaseDate => {
            Some(compare_optional(get_release_date(a), get_release_date(b)))
        }
        SortBy::Rating => {
            Some(compare_optional_partial(get_rating(a), get_rating(b)))
        }
        SortBy::Runtime => {
            Some(compare_optional(get_runtime(a), get_runtime(b)))
        }
        SortBy::Popularity => Some(compare_optional_partial(
            get_popularity(a),
            get_popularity(b),
        )),
        SortBy::Bitrate => {
            Some(compare_optional(get_bitrate(a), get_bitrate(b)))
        }
        SortBy::FileSize => {
            Some(compare_optional(get_file_size(a), get_file_size(b)))
        }
        SortBy::ContentRating => Some(compare_optional_str(
            get_content_rating(a).as_deref(),
            get_content_rating(b).as_deref(),
        )),
        SortBy::Resolution => {
            Some(compare_optional(get_resolution(a), get_resolution(b)))
        }
        SortBy::LastWatched | SortBy::WatchProgress => None,
    }?;

    Some(if sort_order == SortOrder::Descending {
        ord.reverse()
    } else {
        ord
    })
}

/// Sort media slice in-place using provided field/order.
/// Items that cannot be compared by the requested field are left in their relative order.
pub fn sort_media_slice(
    items: &mut [Media],
    sort_by: SortBy,
    sort_order: SortOrder,
) {
    items.sort_by(|a, b| {
        compare_media(a, b, sort_by, sort_order).unwrap_or_else(|| {
            compare_media(a, b, SortBy::Title, SortOrder::Ascending)
                .unwrap_or(Ordering::Equal)
        })
    });
}

fn get_title(media: &Media) -> &str {
    match media {
        Media::Movie(m) => m.title.as_str(),
        Media::Series(s) => s.title.as_str(),
        Media::Season(_) => "",
        Media::Episode(_) => "",
    }
}

fn get_date_added(media: &Media) -> DateTime<Utc> {
    match media {
        Media::Movie(m) => m.file.discovered_at,
        Media::Series(s) => s.discovered_at,
        Media::Season(s) => s.discovered_at,
        Media::Episode(e) => e.discovered_at,
    }
}

fn get_created_at(media: &Media) -> DateTime<Utc> {
    match media {
        Media::Movie(m) => m.file.created_at,
        Media::Series(s) => s.created_at,
        Media::Season(s) => s.created_at,
        Media::Episode(e) => e.created_at,
    }
}

fn get_file_size(media: &Media) -> Option<u64> {
    match media {
        Media::Movie(m) => Some(m.file.size),
        Media::Episode(e) => Some(e.file.size),
        _ => None,
    }
}

fn get_release_date(media: &Media) -> Option<NaiveDate> {
    fn parse(date: &Option<String>) -> Option<NaiveDate> {
        date.as_deref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    }

    match media {
        Media::Movie(m) => match &m.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(d) => parse(&d.release_date),
                _ => None,
            },
            _ => None,
        },
        Media::Series(s) => match &s.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Series(d) => parse(&d.first_air_date),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

fn get_rating(media: &Media) -> Option<f32> {
    match media {
        Media::Movie(m) => match &m.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(d) => d.vote_average,
                _ => None,
            },
            _ => None,
        },
        Media::Series(s) => match &s.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Series(d) => d.vote_average,
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

fn get_runtime(media: &Media) -> Option<u32> {
    match media {
        Media::Movie(m) => match &m.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(d) => d.runtime,
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

fn get_popularity(media: &Media) -> Option<f32> {
    match media {
        Media::Movie(m) => match &m.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(d) => d.popularity,
                _ => None,
            },
            _ => None,
        },
        Media::Series(s) => match &s.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Series(d) => d.popularity,
                _ => None,
            },
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
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(d) => d
                    .content_rating
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
                _ => None,
            },
            _ => None,
        },
        Media::Series(s) => match &s.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Series(d) => d
                    .content_rating
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

fn compare_optional<T: Ord>(a: Option<T>, b: Option<T>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_partial<T: PartialOrd>(
    a: Option<T>,
    b: Option<T>,
) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_str(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
