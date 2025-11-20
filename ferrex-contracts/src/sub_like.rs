use super::{
    details_like::{MediaDetails, SeasonDetailsLike, SeriesDetailsLike},
    media_ops::MediaOps,
};
use ferrex_model::details::{
    EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeDetails,
    MediaDetailsOption, SeasonDetails, TmdbDetails,
};
use ferrex_model::files::MediaFile;
use ferrex_model::media::{
    EpisodeReference, MovieReference, SeasonReference, SeriesReference,
};

pub trait MovieLike: MediaOps {
    type Movie: MediaOps;
    type Details;

    fn title(&self) -> &str;

    fn file(self) -> MediaFile;

    fn details(&self) -> Option<&Self::Details>;

    fn release_year(&self) -> Option<&str>;
}

impl MovieLike for MovieReference {
    type Movie = MovieReference;
    type Details = EnhancedMovieDetails;

    fn title(&self) -> &str {
        self.title.as_ref()
    }

    fn file(self) -> MediaFile {
        self.file
    }

    fn release_year(&self) -> Option<&str> {
        if let Some(details) = self.details() {
            if let Option::Some(release_date) = &details.release_date {
                release_date.split('-').next()
            } else {
                None
            }
        } else {
            None
        }
    }

    fn details(&self) -> Option<&EnhancedMovieDetails> {
        if let MediaDetailsOption::Details(details) = &self.details {
            match details.as_ref() {
                TmdbDetails::Movie(enhanced_movie_details) => {
                    Some(enhanced_movie_details)
                }
                _ => None,
            }
        } else {
            None
        }
    }
}

pub trait SeriesLike: MediaOps {
    type Series: MediaOps;
    type Details: SeriesDetailsLike;

    fn title(&self) -> &str;

    fn details(&self) -> Option<&Self::Details>;

    fn num_seasons(&self) -> u32 {
        let details_opt = self.details();
        if let Some(details) = details_opt {
            details.num_seasons().unwrap_or_default()
        } else {
            0
        }
    }
}

impl SeriesLike for SeriesReference {
    type Series = SeriesReference;
    type Details = EnhancedSeriesDetails;

    fn title(&self) -> &str {
        self.title.as_ref()
    }
    fn details(&self) -> Option<&EnhancedSeriesDetails> {
        if let MediaDetailsOption::Details(details) = &self.details {
            details.as_ref().to_series_details()
        } else {
            None
        }
    }
}

pub trait SeasonLike: MediaOps {
    type Season: MediaOps;
    type Details: SeasonDetailsLike;

    fn details(&self) -> Option<&Self::Details>;

    fn num_episodes(&self) -> u32 {
        let details_opt = self.details();
        if let Some(details) = details_opt {
            details.num_episodes()
        } else {
            0
        }
    }
}

impl SeasonLike for SeasonReference {
    type Season = SeasonReference;
    type Details = SeasonDetails;

    fn details(&self) -> Option<&SeasonDetails> {
        if let MediaDetailsOption::Details(details) = &self.details {
            match details.as_ref() {
                TmdbDetails::Season(season_details) => Some(season_details),
                _ => None,
            }
        } else {
            None
        }
    }
}

pub trait EpisodeLike: MediaOps {
    type Episode: MediaOps;
    type Details;

    fn details(&self) -> Option<&Self::Details>;

    fn file(self) -> MediaFile;
}

impl EpisodeLike for EpisodeReference {
    type Episode = EpisodeReference;
    type Details = EpisodeDetails;

    fn file(self) -> MediaFile {
        self.file
    }

    fn details(&self) -> Option<&EpisodeDetails> {
        if let MediaDetailsOption::Details(details) = &self.details {
            match details.as_ref() {
                TmdbDetails::Episode(episode_details) => Some(episode_details),
                _ => None,
            }
        } else {
            None
        }
    }
}

#[cfg(feature = "rkyv")]
mod archived {
    use super::*;
    use ferrex_model::details::{
        ArchivedEnhancedMovieDetails, ArchivedEnhancedSeriesDetails,
        ArchivedEpisodeDetails, ArchivedMediaDetailsOption,
        ArchivedSeasonDetails, ArchivedTmdbDetails,
    };
    use ferrex_model::media::{
        ArchivedEpisodeReference, ArchivedMovieReference,
        ArchivedSeasonReference, ArchivedSeriesReference,
    };
    use rkyv::{deserialize, option::ArchivedOption, rancor::Error};

    impl MovieLike for ArchivedMovieReference {
        type Movie = ArchivedMovieReference;
        type Details = ArchivedEnhancedMovieDetails;

        fn title(&self) -> &str {
            self.title.as_ref()
        }

        fn file(self) -> MediaFile {
            deserialize::<MediaFile, Error>(&self.file).unwrap()
        }

        fn release_year(&self) -> Option<&str> {
            if let Some(details) = self.details() {
                if let ArchivedOption::Some(release_date) =
                    &details.release_date
                {
                    release_date.split('-').next()
                } else {
                    None
                }
            } else {
                None
            }
        }

        fn details(&self) -> Option<&ArchivedEnhancedMovieDetails> {
            if let ArchivedMediaDetailsOption::Details(details) = &self.details
            {
                match details.as_ref() {
                    ArchivedTmdbDetails::Movie(enhanced_movie_details) => {
                        Some(enhanced_movie_details)
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
    }

    impl SeriesLike for ArchivedSeriesReference {
        type Series = ArchivedSeriesReference;
        type Details = ArchivedEnhancedSeriesDetails;

        fn title(&self) -> &str {
            self.title.as_ref()
        }

        fn details(&self) -> Option<&ArchivedEnhancedSeriesDetails> {
            if let ArchivedMediaDetailsOption::Details(details) = &self.details
            {
                match details.as_ref() {
                    ArchivedTmdbDetails::Series(series_details) => {
                        Some(series_details)
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
    }

    impl SeasonLike for ArchivedSeasonReference {
        type Season = ArchivedSeasonReference;
        type Details = ArchivedSeasonDetails;

        fn details(&self) -> Option<&ArchivedSeasonDetails> {
            if let ArchivedMediaDetailsOption::Details(details) = &self.details
            {
                match details.as_ref() {
                    ArchivedTmdbDetails::Season(season_details) => {
                        Some(season_details)
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
    }

    impl EpisodeLike for ArchivedEpisodeReference {
        type Episode = ArchivedEpisodeReference;
        type Details = ArchivedEpisodeDetails;

        fn file(self) -> MediaFile {
            deserialize::<MediaFile, Error>(&self.file).unwrap()
        }

        fn details(&self) -> Option<&ArchivedEpisodeDetails> {
            if let ArchivedMediaDetailsOption::Details(details) = &self.details
            {
                match details.as_ref() {
                    ArchivedTmdbDetails::Episode(episode_details) => {
                        Some(episode_details)
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
    }
}
