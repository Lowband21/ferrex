use super::{id::MediaIDLike, media_ops::MediaOps};
use crate::types::media::{
    ArchivedEpisodeReference, ArchivedMedia, ArchivedMovieReference,
    ArchivedSeasonReference, ArchivedSeriesReference, EpisodeReference, Media,
    MovieReference, SeasonReference, SeriesReference,
};
use crate::types::media_id::{ArchivedMediaID, MediaID};
use crate::types::util_types::MediaType;

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

// ===== Media Helper Methods =====
impl MediaLike for ArchivedMedia {
    type MovieRef = ArchivedMovieReference;
    type SeriesRef = ArchivedSeriesReference;
    type SeasonRef = ArchivedSeasonReference;
    type EpisodeRef = ArchivedEpisodeReference;
    type MediaID = ArchivedMediaID;

    /// Try to extract owned movie reference
    fn to_movie(self) -> Option<ArchivedMovieReference> {
        match self {
            Self::Movie(m) => Some(m),
            _ => None,
        }
    }

    /// Try to extract owned series reference
    fn to_series(self) -> Option<ArchivedSeriesReference> {
        match self {
            Self::Series(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract owned season reference
    fn to_season(self) -> Option<ArchivedSeasonReference> {
        match self {
            Self::Season(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract owned episode reference
    fn to_episode(self) -> Option<ArchivedEpisodeReference> {
        match self {
            Self::Episode(e) => Some(e),
            _ => None,
        }
    }

    /// Try to extract movie reference
    fn as_movie(&self) -> Option<&ArchivedMovieReference> {
        match self {
            Self::Movie(m) => Some(m),
            _ => None,
        }
    }

    /// Try to extract series reference
    fn as_series(&self) -> Option<&ArchivedSeriesReference> {
        match self {
            Self::Series(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract season reference
    fn as_season(&self) -> Option<&ArchivedSeasonReference> {
        match self {
            Self::Season(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract episode reference
    fn as_episode(&self) -> Option<&ArchivedEpisodeReference> {
        match self {
            Self::Episode(e) => Some(e),
            _ => None,
        }
    }

    /*
    /// Get id of the media reference
    fn media_id(&self) -> ArchivedMediaID {
        match self {
            Self::Movie(m) => ArchivedMediaID::Movie(m.id),
            Self::Series(s) => ArchivedMediaID::Series(s.id),
            Self::Season(s) => ArchivedMediaID::Season(s.id),
            Self::Episode(e) => ArchivedMediaID::Episode(e.id),
        }
    } */

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

impl ArchivedMedia {
    pub fn archived_media_id(&self) -> ArchivedMediaID {
        match self {
            Self::Movie(m) => ArchivedMediaID::Movie(m.id),
            Self::Series(s) => ArchivedMediaID::Series(s.id),
            Self::Season(s) => ArchivedMediaID::Season(s.id),
            Self::Episode(e) => ArchivedMediaID::Episode(e.id),
        }
    }
}
