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

    /*
    /// Get file creation date for date_added sorting
    pub fn date_added(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        // Use the Playable trait to access file information
        self.as_playable()
            .map(|playable| playable.file().created_at)
    } */

    /*
    /// Sort a collection of media references by title
    pub fn sort_by_title(items: &mut [Self], ascending: bool) {
        items.sort_by(|a, b| {
            let a_title = a.title();
            let b_title = b.title();
            if ascending {
                a_title.cmp(b_title)
            } else {
                b_title.cmp(a_title)
            }
        });
    }

    /// Sort by year
    pub fn sort_by_year(items: &mut [Self], ascending: bool) {
        items.sort_by(|a, b| {
            let a_year = a.year();
            let b_year = b.year();
            match (a_year, b_year) {
                (Some(a), Some(b)) => {
                    if ascending {
                        a.cmp(&b)
                    } else {
                        b.cmp(&a)
                    }
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
    } */

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

    /*
    /// Sort by date added
    pub fn sort_by_date_added(items: &mut [Self], ascending: bool) {
        items.sort_by(|a, b| {
            let a_date = a.date_added();
            let b_date = b.date_added();
            match (a_date, b_date) {
                (Some(a), Some(b)) => {
                    if ascending {
                        a.cmp(&b)
                    } else {
                        b.cmp(&a)
                    }
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
    } */

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

    /*
    /// Text search across title and overview
    pub fn search<'a>(items: &'a [Self], query: &str) -> impl Iterator<Item = &'a Self> {
        let query_lower = query.to_lowercase();
        items.iter().filter(move |item| {
            // Search in title
            if item.title().to_lowercase().contains(&query_lower) {
                return true;
            }

            // Search in overview if available
            match item {
                Self::Movie(m) => match &m.details {
                    MediaDetailsOption::Details(TmdbDetails::Movie(details)) => details
                        .overview
                        .as_ref()
                        .map(|o| o.to_lowercase().contains(&query_lower))
                        .unwrap_or(false),
                    _ => false,
                },
                Self::Series(s) => match &s.details {
                    MediaDetailsOption::Details(TmdbDetails::Series(details)) => details
                        .overview
                        .as_ref()
                        .map(|o| o.to_lowercase().contains(&query_lower))
                        .unwrap_or(false),
                    _ => false,
                },
                Self::Episode(e) => match &e.details {
                    MediaDetailsOption::Details(TmdbDetails::Episode(details)) => details
                        .overview
                        .as_ref()
                        .map(|o| o.to_lowercase().contains(&query_lower))
                        .unwrap_or(false),
                    _ => false,
                },
                Self::Season(s) => match &s.details {
                    MediaDetailsOption::Details(TmdbDetails::Season(details)) => details
                        .overview
                        .as_ref()
                        .map(|o| o.to_lowercase().contains(&query_lower))
                        .unwrap_or(false),
                    _ => false,
                },
            }
        })
    }*/
}

/*
/// Extension trait for collections of Media
pub trait MediaExt {
    //fn apply_sort(&mut self, sort_by: MediaSortBy, ascending: bool);
    fn apply_filters(&self, filters: &MediaFilters) -> Vec<&Media>;
    //fn search(&self, query: &str) -> Vec<&Media>;
}

impl MediaExt for [Media] {
    /*
    fn apply_sort(&mut self, sort_by: MediaSortBy, ascending: bool) {
        match sort_by {
            MediaSortBy::Title => Media::sort_by_title(self, ascending),
            MediaSortBy::Year => Media::sort_by_year(self, ascending),
            MediaSortBy::Rating => Media::sort_by_rating(self, ascending),
            MediaSortBy::DateAdded => Media::sort_by_date_added(self, ascending),
        }
    } */

    fn apply_filters(&self, filters: &MediaFilters) -> Vec<&Media> {
        let mut results: Vec<&Media> = self.iter().collect();

        // Apply genre filter
        if let Some(genre) = &filters.genre {
            results = results
                .into_iter()
                .filter(|item| {
                    item.genres()
                        .map(|genres| genres.iter().any(|g| g.eq_ignore_ascii_case(genre)))
                        .unwrap_or(false)
                })
                .collect();
        }

        // Apply year range filter
        if filters.min_year.is_some() || filters.max_year.is_some() {
            results = results
                .into_iter()
                .filter(|item| {
                    if let Some(year) = item.year() {
                        let above_min = filters.min_year.map(|min| year >= min).unwrap_or(true);
                        let below_max = filters.max_year.map(|max| year <= max).unwrap_or(true);
                        above_min && below_max
                    } else {
                        false
                    }
                })
                .collect();
        }

        // Apply rating range filter
        if filters.min_rating.is_some() || filters.max_rating.is_some() {
            results = results
                .into_iter()
                .filter(|item| {
                    if let Some(rating) = item.rating() {
                        let above_min = filters.min_rating.map(|min| rating >= min).unwrap_or(true);
                        let below_max = filters.max_rating.map(|max| rating <= max).unwrap_or(true);
                        above_min && below_max
                    } else {
                        false
                    }
                })
                .collect();
        }

        results
    }

    /*fn search(&self, query: &str) -> Vec<&Media> {
        Media::search(self, query).collect()
    }*/
}*/

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
