use crate::{Media, MediaDetailsOption, MediaOps, MovieLike, Playable, SeriesLike, TmdbDetails};

// ===== Media Sorting and Filtering Methods =====
//
// These methods are kept for backward compatibility but now use
// the trait system internally for cleaner implementation.

impl Media {
    /// Get rating (vote average) if available
    pub fn rating(&self) -> Option<f32> {
        match &self {
            Media::Movie(movie) => movie.rating(),
            Media::Series(tv_show) => tv_show.rating(),
            Media::Episode(episode) => episode.rating(),
            Media::Season(_) => None,
        }
    }

    /// Get genres if available
    pub fn genres(&self) -> Option<Vec<&str>> {
        match &self {
            Media::Movie(movie) => movie.genres(),
            Media::Series(tv_show) => tv_show.genres(),
            Media::Episode(episode) => episode.genres(),
            Media::Season(season) => season.genres(),
        }
    }

    /// Sort by rating
    pub fn sort_by_rating(items: &mut [Self], ascending: bool) {
        items.sort_by(|a, b| {
            let a_rating = a.rating();
            let b_rating = b.rating();
            match (a_rating, b_rating) {
                (Some(a), Some(b)) => {
                    if ascending {
                        a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
                    }
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
    }

    /// Filter by genre
    pub fn filter_by_genre<'a>(
        items: &'a [Self],
        genre: &str,
    ) -> impl Iterator<Item = &'a Self> + 'a + use<'a> {
        let genre = genre.to_string();
        items.iter().filter(move |item| {
            item.genres()
                .map(|genres| genres.iter().any(|g| g.eq_ignore_ascii_case(&genre)))
                .unwrap_or(false)
        })
    }

    /// Filter by rating range
    pub fn filter_by_rating_range<'a>(
        items: &'a [Self],
        min_rating: Option<f32>,
        max_rating: Option<f32>,
    ) -> impl Iterator<Item = &'a Self> {
        items.iter().filter(move |item| {
            if let Some(rating) = item.rating() {
                let above_min = min_rating.map(|min| rating >= min).unwrap_or(true);
                let below_max = max_rating.map(|max| rating <= max).unwrap_or(true);
                above_min && below_max
            } else {
                false
            }
        })
    }
}

/// Sort options for media references
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSortBy {
    Title,
    Year,
    Rating,
    DateAdded,
}

/// Filter options for media references
#[derive(Debug, Clone, Default)]
pub struct MediaFilters {
    pub genre: Option<String>,
    pub min_year: Option<u16>,
    pub max_year: Option<u16>,
    pub min_rating: Option<f32>,
    pub max_rating: Option<f32>,
}
