use super::{id::MediaIDLike, media_ops::MediaOps};
use ferrex_model::media::{
    EpisodeReference, Media, MovieReference, SeasonReference, SeriesReference,
};
use ferrex_model::media_id::MediaID;
use ferrex_model::util_types::MediaType;

// A trait that allows us to treat archived and non-archived media references as the same type
pub trait MediaLike {
    type MovieRef: MediaOps;
    type SeriesRef: MediaOps;
    type SeasonRef: MediaOps;
    type EpisodeRef: MediaOps;
    type MediaID: MediaIDLike;

    /// Try to extract owned movie reference
    fn to_movie(self) -> Option<Self::MovieRef>;

    /// Try to extract owned series reference
    fn to_series(self) -> Option<Self::SeriesRef>;

    /// Try to extract owned season reference
    fn to_season(self) -> Option<Self::SeasonRef>;

    /// Try to extract owned episode reference
    fn to_episode(self) -> Option<Self::EpisodeRef>;

    /// Try to extract movie reference
    fn as_movie(&self) -> Option<&Self::MovieRef>;

    /// Try to extract series reference
    fn as_series(&self) -> Option<&Self::SeriesRef>;

    /// Try to extract season reference
    fn as_season(&self) -> Option<&Self::SeasonRef>;

    /// Try to extract episode reference
    fn as_episode(&self) -> Option<&Self::EpisodeRef>;

    /*
    /// Get id of the media reference
    fn media_id(&self) -> Self::MediaID;*/

    /// Helper to get media type
    fn media_type(&self) -> MediaType;
}

impl MediaLike for Media {
    type MovieRef = MovieReference;
    type SeriesRef = SeriesReference;
    type SeasonRef = SeasonReference;
    type EpisodeRef = EpisodeReference;

    type MediaID = MediaID;

    /// Try to extract owned movie reference
    fn to_movie(self) -> Option<MovieReference> {
        match self {
            Self::Movie(m) => Some(m),
            _ => None,
        }
    }

    /// Try to extract owned series reference
    fn to_series(self) -> Option<SeriesReference> {
        match self {
            Self::Series(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract owned season reference
    fn to_season(self) -> Option<SeasonReference> {
        match self {
            Self::Season(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract owned episode reference
    fn to_episode(self) -> Option<EpisodeReference> {
        match self {
            Self::Episode(e) => Some(e),
            _ => None,
        }
    }

    /// Try to extract movie reference
    fn as_movie(&self) -> Option<&MovieReference> {
        match self {
            Self::Movie(m) => Some(m),
            _ => None,
        }
    }

    /// Try to extract series reference
    fn as_series(&self) -> Option<&SeriesReference> {
        match self {
            Self::Series(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract season reference
    fn as_season(&self) -> Option<&SeasonReference> {
        match self {
            Self::Season(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract episode reference
    fn as_episode(&self) -> Option<&EpisodeReference> {
        match self {
            Self::Episode(e) => Some(e),
            _ => None,
        }
    }

    /*
    /// Get id of the media reference
    fn media_id(&self) -> MediaID {
        match self {
            Self::Movie(m) => MediaID::Movie(m.id),
            Self::Series(s) => MediaID::Series(s.id),
            Self::Season(s) => MediaID::Season(s.id),
            Self::Episode(e) => MediaID::Episode(e.id),
        }
    }*/

    /// Helper to get media type
    fn media_type(&self) -> MediaType {
        match self {
            Self::Movie(_) => MediaType::Movie,
            Self::Series(_) => MediaType::Series,
            Self::Season(_) => MediaType::Season,
            Self::Episode(_) => MediaType::Episode,
        }
    }
}

#[cfg(feature = "rkyv")]
mod archived {
    use super::*;
    use ferrex_model::media::{
        ArchivedEpisodeReference, ArchivedMedia, ArchivedMovieReference,
        ArchivedSeasonReference, ArchivedSeriesReference,
    };
    use ferrex_model::media_id::ArchivedMediaID;

    impl MediaLike for ArchivedMedia {
        type MovieRef = ArchivedMovieReference;
        type SeriesRef = ArchivedSeriesReference;
        type SeasonRef = ArchivedSeasonReference;
        type EpisodeRef = ArchivedEpisodeReference;
        type MediaID = ArchivedMediaID;

        fn to_movie(self) -> Option<ArchivedMovieReference> {
            match self {
                ArchivedMedia::Movie(m) => Some(m),
                _ => None,
            }
        }

        fn to_series(self) -> Option<ArchivedSeriesReference> {
            match self {
                ArchivedMedia::Series(s) => Some(s),
                _ => None,
            }
        }

        fn to_season(self) -> Option<ArchivedSeasonReference> {
            match self {
                ArchivedMedia::Season(s) => Some(s),
                _ => None,
            }
        }

        fn to_episode(self) -> Option<ArchivedEpisodeReference> {
            match self {
                ArchivedMedia::Episode(e) => Some(e),
                _ => None,
            }
        }

        fn as_movie(&self) -> Option<&ArchivedMovieReference> {
            match self {
                ArchivedMedia::Movie(m) => Some(m),
                _ => None,
            }
        }

        fn as_series(&self) -> Option<&ArchivedSeriesReference> {
            match self {
                ArchivedMedia::Series(s) => Some(s),
                _ => None,
            }
        }

        fn as_season(&self) -> Option<&ArchivedSeasonReference> {
            match self {
                ArchivedMedia::Season(s) => Some(s),
                _ => None,
            }
        }

        fn as_episode(&self) -> Option<&ArchivedEpisodeReference> {
            match self {
                ArchivedMedia::Episode(e) => Some(e),
                _ => None,
            }
        }

        fn media_type(&self) -> MediaType {
            match self {
                ArchivedMedia::Movie(_) => MediaType::Movie,
                ArchivedMedia::Series(_) => MediaType::Series,
                ArchivedMedia::Season(_) => MediaType::Season,
                ArchivedMedia::Episode(_) => MediaType::Episode,
            }
        }
    }
}
