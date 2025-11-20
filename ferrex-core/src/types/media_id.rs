use crate::{
    MediaIDLike,
    types::{
        ids::{
            ArchivedEpisodeID, ArchivedMovieID, ArchivedSeasonID, ArchivedSeriesID, EpisodeID,
            MovieID, SeasonID, SeriesID,
        },
        media::{ArchivedMedia, ArchivedMovieReference},
    },
};
use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize, deserialize, rancor::Error,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Hash,
    Copy,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub enum MediaID {
    Movie(MovieID),
    Series(SeriesID),
    Season(SeasonID),
    Episode(EpisodeID),
}

impl MediaID {
    pub fn as_ref(&self) -> &Uuid {
        match &self {
            MediaID::Movie(movie_id) => movie_id.as_uuid(),
            MediaID::Series(series_id) => series_id.as_uuid(),
            MediaID::Season(season_id) => season_id.as_uuid(),
            MediaID::Episode(episode_id) => episode_id.as_uuid(),
        }
    }

    pub fn eq_movie(&self, other: &MovieID) -> bool {
        match (self, other) {
            (MediaID::Movie(MovieID(a)), MovieID(b)) => a == b,
            _ => false,
        }
    }
    pub fn eq_series(&self, other: &SeriesID) -> bool {
        match (self, other) {
            (MediaID::Series(SeriesID(a)), SeriesID(b)) => a == b,
            _ => false,
        }
    }
    pub fn eq_episode(&self, other: &EpisodeID) -> bool {
        match (self, other) {
            (MediaID::Episode(EpisodeID(a)), EpisodeID(b)) => a == b,
            _ => false,
        }
    }
}
impl ArchivedMediaID {
    pub fn eq_movie(&self, other: &ArchivedMovieID) -> bool {
        match (self, other) {
            (ArchivedMediaID::Movie(ArchivedMovieID(a)), ArchivedMovieID(b)) => a == b,
            _ => false,
        }
    }
    pub fn eq_series(&self, other: &ArchivedSeriesID) -> bool {
        match (self, other) {
            (ArchivedMediaID::Series(ArchivedSeriesID(a)), ArchivedSeriesID(b)) => a == b,
            _ => false,
        }
    }
    pub fn eq_episode(&self, other: &ArchivedEpisodeID) -> bool {
        match (self, other) {
            (ArchivedMediaID::Episode(ArchivedEpisodeID(a)), ArchivedEpisodeID(b)) => a == b,
            _ => false,
        }
    }
}

impl std::fmt::Display for MediaID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buff = Uuid::encode_buffer();
        match self {
            MediaID::Movie(id) => write!(f, "Movie({})", id.as_str(&mut buff)),
            MediaID::Series(id) => write!(f, "Series({})", id.as_str(&mut buff)),
            MediaID::Season(id) => write!(f, "Season({})", id.as_str(&mut buff)),
            MediaID::Episode(id) => write!(f, "Episode({})", id.as_str(&mut buff)),
        }
    }
}

impl From<ArchivedMedia> for ArchivedMovieReference {
    fn from(med_ref: ArchivedMedia) -> Self {
        match med_ref {
            ArchivedMedia::Movie(data) => data,
            _ => panic!("Cannot convert non-movie reference to movie reference"),
        }
    }
}

impl From<ArchivedMovieID> for ArchivedMediaID {
    fn from(id: ArchivedMovieID) -> Self {
        ArchivedMediaID::Movie(id)
    }
}

impl From<ArchivedSeriesID> for ArchivedMediaID {
    fn from(id: ArchivedSeriesID) -> Self {
        ArchivedMediaID::Series(id)
    }
}

impl From<ArchivedSeasonID> for ArchivedMediaID {
    fn from(id: ArchivedSeasonID) -> Self {
        ArchivedMediaID::Season(id)
    }
}

impl From<ArchivedEpisodeID> for ArchivedMediaID {
    fn from(id: ArchivedEpisodeID) -> Self {
        ArchivedMediaID::Episode(id)
    }
}

impl From<ArchivedMovieID> for MediaID {
    fn from(id: ArchivedMovieID) -> Self {
        let id = deserialize::<MovieID, Error>(&id).unwrap();
        MediaID::Movie(id)
    }
}

impl From<ArchivedSeriesID> for MediaID {
    fn from(id: ArchivedSeriesID) -> Self {
        let id = deserialize::<SeriesID, Error>(&id).unwrap();
        MediaID::Series(id)
    }
}

impl From<ArchivedSeasonID> for MediaID {
    fn from(id: ArchivedSeasonID) -> Self {
        let id = deserialize::<SeasonID, Error>(&id).unwrap();
        MediaID::Season(id)
    }
}

impl From<ArchivedEpisodeID> for MediaID {
    fn from(id: ArchivedEpisodeID) -> Self {
        let id = deserialize::<EpisodeID, Error>(&id).unwrap();
        MediaID::Episode(id)
    }
}

impl From<MovieID> for MediaID {
    fn from(id: MovieID) -> Self {
        MediaID::Movie(id)
    }
}

impl From<SeriesID> for MediaID {
    fn from(id: SeriesID) -> Self {
        MediaID::Series(id)
    }
}

impl From<SeasonID> for MediaID {
    fn from(id: SeasonID) -> Self {
        MediaID::Season(id)
    }
}

impl From<EpisodeID> for MediaID {
    fn from(id: EpisodeID) -> Self {
        MediaID::Episode(id)
    }
}
