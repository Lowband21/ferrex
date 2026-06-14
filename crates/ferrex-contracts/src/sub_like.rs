use super::{
    details_like::{SeasonDetailsLike, SeriesDetailsLike},
    media_ops::MediaOps,
};
use ferrex_model::media::{
    EpisodeReference, MovieReference, SeasonReference, Series,
};
use ferrex_model::{
    details::{
        EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeDetails,
        SeasonDetails,
    },
    files::MediaFile,
};

pub trait MovieLike: MediaOps {
    type Movie: MediaOps;
    type Details;

    fn title(&self) -> &str;

    fn file(self) -> MediaFile;

    fn details(&self) -> &Self::Details;

    fn release_year(&self) -> Option<&str>;
}

impl MovieLike for Box<MovieReference> {
    type Movie = Box<MovieReference>;
    type Details = EnhancedMovieDetails;

    fn title(&self) -> &str {
        self.title.as_ref()
    }

    fn file(self) -> MediaFile {
        self.file
    }

    fn release_year(&self) -> Option<&str> {
        self.details
            .release_date
            .as_ref()
            .map(|rd| rd.split('-').next())?
    }

    fn details(&self) -> &EnhancedMovieDetails {
        &self.details
    }
}

pub trait SeriesLike: MediaOps {
    type Series: MediaOps;
    type Details: SeriesDetailsLike;

    fn title(&self) -> &str;

    fn details(&self) -> &Self::Details;

    fn num_seasons(&self) -> u16 {
        self.details().num_seasons().unwrap_or_default()
    }
}

impl SeriesLike for Box<Series> {
    type Series = Box<Series>;
    type Details = EnhancedSeriesDetails;

    fn title(&self) -> &str {
        self.title.as_ref()
    }
    fn details(&self) -> &EnhancedSeriesDetails {
        &self.details
    }
}

pub trait SeasonLike: MediaOps {
    type Season: MediaOps;
    type Details: SeasonDetailsLike;

    fn details(&self) -> &Self::Details;

    fn num_episodes(&self) -> u16 {
        self.details().num_episodes()
    }
}

impl SeasonLike for Box<SeasonReference> {
    type Season = Box<SeasonReference>;
    type Details = SeasonDetails;

    fn details(&self) -> &SeasonDetails {
        &self.details
    }
}

pub trait EpisodeLike: MediaOps {
    type Episode: MediaOps;
    type Details;

    fn details(&self) -> &Self::Details;

    fn file(self) -> MediaFile;
}

impl EpisodeLike for Box<EpisodeReference> {
    type Episode = Box<EpisodeReference>;
    type Details = EpisodeDetails;

    fn file(self) -> MediaFile {
        self.file
    }

    fn details(&self) -> &EpisodeDetails {
        &self.details
    }
}

#[cfg(feature = "rkyv")]
mod archived {
    use super::*;
    use ferrex_model::details::{
        ArchivedEnhancedMovieDetails, ArchivedEnhancedSeriesDetails,
        ArchivedEpisodeDetails, ArchivedSeasonDetails,
    };
    use ferrex_model::media::{
        ArchivedEpisodeReference, ArchivedMovieReference,
        ArchivedSeasonReference, ArchivedSeries,
    };
    use rkyv::{
        boxed::ArchivedBox, deserialize, option::ArchivedOption, rancor::Error,
    };

    impl MovieLike for ArchivedBox<ArchivedMovieReference> {
        type Movie = ArchivedBox<ArchivedMovieReference>;
        type Details = ArchivedEnhancedMovieDetails;

        fn title(&self) -> &str {
            self.title.as_ref()
        }

        fn file(self) -> MediaFile {
            deserialize::<MediaFile, Error>(&self.file).unwrap()
        }

        fn release_year(&self) -> Option<&str> {
            match &self.get().details.release_date {
                ArchivedOption::Some(rd) => rd.split('-').next(),
                ArchivedOption::None => None,
            }
        }

        fn details(&self) -> &ArchivedEnhancedMovieDetails {
            &self.details
        }
    }

    impl SeriesLike for ArchivedBox<ArchivedSeries> {
        type Series = ArchivedBox<ArchivedSeries>;
        type Details = ArchivedEnhancedSeriesDetails;

        fn title(&self) -> &str {
            self.title.as_ref()
        }

        fn details(&self) -> &ArchivedEnhancedSeriesDetails {
            &self.details
        }
    }

    impl SeasonLike for ArchivedBox<ArchivedSeasonReference> {
        type Season = ArchivedBox<ArchivedSeasonReference>;
        type Details = ArchivedSeasonDetails;

        fn details(&self) -> &ArchivedSeasonDetails {
            &self.details
        }
    }

    impl EpisodeLike for ArchivedBox<ArchivedEpisodeReference> {
        type Episode = ArchivedBox<ArchivedEpisodeReference>;
        type Details = ArchivedEpisodeDetails;

        fn file(self) -> MediaFile {
            deserialize::<MediaFile, Error>(&self.file).unwrap()
        }

        fn details(&self) -> &ArchivedEpisodeDetails {
            &self.details
        }
    }
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
            match &self.details.release_date {
                ArchivedOption::Some(rd) => rd.split('-').next(),
                ArchivedOption::None => None,
            }
        }

        fn details(&self) -> &ArchivedEnhancedMovieDetails {
            &self.details
        }
    }

    impl SeriesLike for ArchivedSeries {
        type Series = ArchivedSeries;
        type Details = ArchivedEnhancedSeriesDetails;

        fn title(&self) -> &str {
            self.title.as_ref()
        }

        fn details(&self) -> &ArchivedEnhancedSeriesDetails {
            &self.details
        }
    }

    impl SeasonLike for ArchivedSeasonReference {
        type Season = ArchivedSeasonReference;
        type Details = ArchivedSeasonDetails;

        fn details(&self) -> &ArchivedSeasonDetails {
            &self.details
        }
    }

    impl EpisodeLike for ArchivedEpisodeReference {
        type Episode = ArchivedEpisodeReference;
        type Details = ArchivedEpisodeDetails;

        fn file(self) -> MediaFile {
            deserialize::<MediaFile, Error>(&self.file).unwrap()
        }

        fn details(&self) -> &ArchivedEpisodeDetails {
            &self.details
        }
    }
}
