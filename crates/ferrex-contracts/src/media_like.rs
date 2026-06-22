//! Media enum access traits shared by owned and archived references.
//!
//! `MediaLike` lets player and repository code branch on media shape through a
//! trait instead of matching every owned/archived representation separately.

use super::{id::MediaIDLike, media_ops::MediaOps};
use ferrex_model::media::{
    EpisodeReference, Media, MovieReference, SeasonReference, Series,
};
use ferrex_model::media_id::MediaID;
use ferrex_model::media_type::VideoMediaType;

/// Trait that normalizes access to owned and archived media enum variants.
pub trait MediaLike {
    /// Movie reference type returned by movie accessors.
    type MovieRef: MediaOps;
    /// Series reference type returned by series accessors.
    type SeriesRef: MediaOps;
    /// Season reference type returned by season accessors.
    type SeasonRef: MediaOps;
    /// Episode reference type returned by episode accessors.
    type EpisodeRef: MediaOps;
    /// Media id type associated with this media representation.
    type MediaID: MediaIDLike;

    /// Try to extract an owned movie reference.
    fn to_movie(self) -> Option<Self::MovieRef>;

    /// Try to extract an owned series reference.
    fn to_series(self) -> Option<Self::SeriesRef>;

    /// Try to extract an owned season reference.
    fn to_season(self) -> Option<Self::SeasonRef>;

    /// Try to extract an owned episode reference.
    fn to_episode(self) -> Option<Self::EpisodeRef>;

    /// Try to borrow the movie reference.
    fn as_movie(&self) -> Option<&Self::MovieRef>;

    /// Try to borrow the series reference.
    fn as_series(&self) -> Option<&Self::SeriesRef>;

    /// Try to borrow the season reference.
    fn as_season(&self) -> Option<&Self::SeasonRef>;

    /// Try to borrow the episode reference.
    fn as_episode(&self) -> Option<&Self::EpisodeRef>;

    /*
    /// Get id of the media reference
    fn media_id(&self) -> Self::MediaID;*/

    /// Return the playable media category for the current variant.
    fn media_type(&self) -> VideoMediaType;
}

impl MediaLike for Media {
    type MovieRef = Box<MovieReference>;
    type SeriesRef = Box<Series>;
    type SeasonRef = Box<SeasonReference>;
    type EpisodeRef = Box<EpisodeReference>;

    type MediaID = MediaID;

    /// Try to extract owned movie reference
    fn to_movie(self) -> Option<Box<MovieReference>> {
        match self {
            Self::Movie(m) => Some(m),
            _ => None,
        }
    }

    /// Try to extract owned series reference
    fn to_series(self) -> Option<Box<Series>> {
        match self {
            Self::Series(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract owned season reference
    fn to_season(self) -> Option<Box<SeasonReference>> {
        match self {
            Self::Season(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract owned episode reference
    fn to_episode(self) -> Option<Box<EpisodeReference>> {
        match self {
            Self::Episode(e) => Some(e),
            _ => None,
        }
    }

    /// Try to extract movie reference
    fn as_movie(&self) -> Option<&Box<MovieReference>> {
        match self {
            Self::Movie(m) => Some(m),
            _ => None,
        }
    }

    /// Try to extract series reference
    fn as_series(&self) -> Option<&Box<Series>> {
        match self {
            Self::Series(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract season reference
    fn as_season(&self) -> Option<&Box<SeasonReference>> {
        match self {
            Self::Season(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract episode reference
    fn as_episode(&self) -> Option<&Box<EpisodeReference>> {
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
    fn media_type(&self) -> VideoMediaType {
        match self {
            Self::Movie(_) => VideoMediaType::Movie,
            Self::Series(_) => VideoMediaType::Series,
            Self::Season(_) => VideoMediaType::Season,
            Self::Episode(_) => VideoMediaType::Episode,
        }
    }
}

#[cfg(feature = "rkyv")]
mod archived {
    use super::*;
    use ferrex_model::media::{
        ArchivedEpisodeReference, ArchivedMedia, ArchivedMovieReference,
        ArchivedSeasonReference, ArchivedSeries,
    };
    use ferrex_model::media_id::ArchivedMediaID;
    use rkyv::boxed::ArchivedBox;

    impl MediaLike for ArchivedMedia {
        type MovieRef = ArchivedBox<ArchivedMovieReference>;
        type SeriesRef = ArchivedBox<ArchivedSeries>;
        type SeasonRef = ArchivedBox<ArchivedSeasonReference>;
        type EpisodeRef = ArchivedBox<ArchivedEpisodeReference>;
        type MediaID = ArchivedMediaID;

        fn to_movie(self) -> Option<ArchivedBox<ArchivedMovieReference>> {
            match self {
                ArchivedMedia::Movie(m) => Some(m),
                _ => None,
            }
        }

        fn to_series(self) -> Option<ArchivedBox<ArchivedSeries>> {
            match self {
                ArchivedMedia::Series(s) => Some(s),
                _ => None,
            }
        }

        fn to_season(self) -> Option<ArchivedBox<ArchivedSeasonReference>> {
            match self {
                ArchivedMedia::Season(s) => Some(s),
                _ => None,
            }
        }

        fn to_episode(self) -> Option<ArchivedBox<ArchivedEpisodeReference>> {
            match self {
                ArchivedMedia::Episode(e) => Some(e),
                _ => None,
            }
        }

        fn as_movie(&self) -> Option<&ArchivedBox<ArchivedMovieReference>> {
            match self {
                ArchivedMedia::Movie(m) => Some(m),
                _ => None,
            }
        }

        fn as_series(&self) -> Option<&ArchivedBox<ArchivedSeries>> {
            match self {
                ArchivedMedia::Series(s) => Some(s),
                _ => None,
            }
        }

        fn as_season(&self) -> Option<&ArchivedBox<ArchivedSeasonReference>> {
            match self {
                ArchivedMedia::Season(s) => Some(s),
                _ => None,
            }
        }

        fn as_episode(&self) -> Option<&ArchivedBox<ArchivedEpisodeReference>> {
            match self {
                ArchivedMedia::Episode(e) => Some(e),
                _ => None,
            }
        }

        fn media_type(&self) -> VideoMediaType {
            match self {
                ArchivedMedia::Movie(_) => VideoMediaType::Movie,
                ArchivedMedia::Series(_) => VideoMediaType::Series,
                ArchivedMedia::Season(_) => VideoMediaType::Season,
                ArchivedMedia::Episode(_) => VideoMediaType::Episode,
            }
        }
    }
}
