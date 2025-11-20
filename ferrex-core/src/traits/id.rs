/// Trait that allows us to treat archived MediaIDs the same as MediaIDs.
//pub trait MediaIDVariant {
//    fn as_str(&self) -> String;
//    fn as_uuid(&self) -> Uuid;
//    fn sub_eq(&self, other: &impl MediaIDVariant) -> bool;
//}
use uuid::Uuid;

use crate::{
    ArchivedEpisodeID, ArchivedMediaID, ArchivedMovieID, ArchivedSeasonID, ArchivedSeriesID,
    EpisodeID, MediaID, MediaType, MovieID, SeasonID, SeriesID,
};

pub trait MediaIDLike {
    type MediaId: MediaIDLike;

    fn as_ref(&self) -> &Self;
    fn to_media_id(self) -> Self::MediaId;

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str;
    fn to_string_buf<'a>(&self, buffer: &'a mut [u8; 45]) -> String {
        String::from(self.as_str(buffer))
    }

    fn as_uuid(&self) -> &Uuid;
    fn to_uuid(self) -> Uuid;

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool;

    fn media_type(&self) -> MediaType;
}

impl MediaIDLike for MediaID {
    type MediaId = MediaID;

    fn as_ref(&self) -> &Self {
        &self
    }

    fn to_media_id(self) -> Self::MediaId {
        self
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        match &self {
            MediaID::Movie(movie_id) => movie_id.as_str(buffer),
            MediaID::Series(series_id) => series_id.as_str(buffer),
            MediaID::Season(season_id) => season_id.as_str(buffer),
            MediaID::Episode(episode_id) => episode_id.as_str(buffer),
        }
    }

    fn as_uuid(&self) -> &Uuid {
        match &self {
            MediaID::Movie(movie_id) => movie_id.as_uuid(),
            MediaID::Series(series_id) => series_id.as_uuid(),
            MediaID::Season(season_id) => season_id.as_uuid(),
            MediaID::Episode(episode_id) => episode_id.as_uuid(),
        }
    }

    fn to_uuid(self) -> Uuid {
        match self {
            MediaID::Movie(movie_id) => movie_id.to_uuid(),
            MediaID::Series(series_id) => series_id.to_uuid(),
            MediaID::Season(season_id) => season_id.to_uuid(),
            MediaID::Episode(episode_id) => episode_id.to_uuid(),
        }
    }

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool {
        self.as_uuid() == other.as_uuid()
    }

    fn media_type(&self) -> MediaType {
        match &self {
            MediaID::Movie(_) => MediaType::Movie,
            MediaID::Series(_) => MediaType::Series,
            MediaID::Season(_) => MediaType::Season,
            MediaID::Episode(_) => MediaType::Episode,
        }
    }
}

impl MediaIDLike for ArchivedMediaID {
    type MediaId = ArchivedMediaID;

    fn as_ref(&self) -> &Self {
        &self
    }

    fn to_media_id(self) -> Self::MediaId {
        self
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        match &self {
            ArchivedMediaID::Movie(movie_id) => movie_id.as_str(buffer),
            ArchivedMediaID::Series(series_id) => series_id.as_str(buffer),
            ArchivedMediaID::Season(season_id) => season_id.as_str(buffer),
            ArchivedMediaID::Episode(episode_id) => episode_id.as_str(buffer),
        }
    }

    fn as_uuid(&self) -> &Uuid {
        match &self {
            ArchivedMediaID::Movie(movie_id) => movie_id.as_uuid(),
            ArchivedMediaID::Series(series_id) => series_id.as_uuid(),
            ArchivedMediaID::Season(season_id) => season_id.as_uuid(),
            ArchivedMediaID::Episode(episode_id) => episode_id.as_uuid(),
        }
    }

    fn to_uuid(self) -> Uuid {
        match self {
            ArchivedMediaID::Movie(movie_id) => movie_id.to_uuid(),
            ArchivedMediaID::Series(series_id) => series_id.to_uuid(),
            ArchivedMediaID::Season(season_id) => season_id.to_uuid(),
            ArchivedMediaID::Episode(episode_id) => episode_id.to_uuid(),
        }
    }

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool {
        self.as_uuid() == other.as_uuid()
    }

    fn media_type(&self) -> MediaType {
        match &self {
            ArchivedMediaID::Movie(_) => MediaType::Movie,
            ArchivedMediaID::Series(_) => MediaType::Series,
            ArchivedMediaID::Season(_) => MediaType::Season,
            ArchivedMediaID::Episode(_) => MediaType::Episode,
        }
    }
}

impl MediaIDLike for MovieID {
    type MediaId = MediaID;

    fn as_ref(&self) -> &Self {
        &self
    }

    fn to_media_id(self) -> Self::MediaId {
        MediaID::Movie(self)
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        self.as_uuid().hyphenated().encode_lower(buffer)
    }

    fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    fn to_uuid(self) -> Uuid {
        self.0
    }

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool {
        self.as_uuid() == other.as_uuid()
    }

    fn media_type(&self) -> MediaType {
        MediaType::Movie
    }
}

impl MediaIDLike for ArchivedMovieID {
    type MediaId = ArchivedMediaID;

    fn as_ref(&self) -> &Self {
        &self
    }

    fn to_media_id(self) -> Self::MediaId {
        ArchivedMediaID::Movie(self)
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        self.to_uuid().hyphenated().encode_lower(buffer)
    }

    fn as_uuid(&self) -> &Uuid {
        Uuid::from_bytes_ref(&self.0)
    }

    fn to_uuid(self) -> Uuid {
        Uuid::from_bytes(self.0)
    }

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool {
        self.as_uuid() == other.as_uuid()
    }

    fn media_type(&self) -> MediaType {
        MediaType::Movie
    }
}

impl MediaIDLike for SeriesID {
    type MediaId = MediaID;

    fn as_ref(&self) -> &Self {
        &self
    }

    fn to_media_id(self) -> Self::MediaId {
        MediaID::Series(self)
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        self.to_uuid().hyphenated().encode_lower(buffer)
    }

    fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    fn to_uuid(self) -> Uuid {
        self.0
    }

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool {
        self.as_uuid() == other.as_uuid()
    }

    fn media_type(&self) -> MediaType {
        MediaType::Series
    }
}

impl MediaIDLike for ArchivedSeriesID {
    type MediaId = ArchivedMediaID;

    fn as_ref(&self) -> &Self {
        &self
    }

    fn to_media_id(self) -> Self::MediaId {
        ArchivedMediaID::Series(self)
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        self.to_uuid().hyphenated().encode_lower(buffer)
    }

    fn as_uuid(&self) -> &Uuid {
        Uuid::from_bytes_ref(&self.0)
    }

    fn to_uuid(self) -> Uuid {
        Uuid::from_bytes(self.0)
    }

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool {
        self.as_uuid() == other.as_uuid()
    }

    fn media_type(&self) -> MediaType {
        MediaType::Series
    }
}

impl MediaIDLike for SeasonID {
    type MediaId = MediaID;

    fn as_ref(&self) -> &Self {
        &self
    }

    fn to_media_id(self) -> Self::MediaId {
        MediaID::Season(self)
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        self.to_uuid().hyphenated().encode_lower(buffer)
    }

    fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    fn to_uuid(self) -> Uuid {
        self.0
    }

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool {
        self.as_uuid() == other.as_uuid()
    }

    fn media_type(&self) -> MediaType {
        MediaType::Season
    }
}

impl MediaIDLike for ArchivedSeasonID {
    type MediaId = ArchivedMediaID;

    fn as_ref(&self) -> &Self {
        &self
    }

    fn to_media_id(self) -> Self::MediaId {
        ArchivedMediaID::Season(self)
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        self.to_uuid().hyphenated().encode_lower(buffer)
    }

    fn as_uuid(&self) -> &Uuid {
        Uuid::from_bytes_ref(&self.0)
    }

    fn to_uuid(self) -> Uuid {
        Uuid::from_bytes(self.0)
    }

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool {
        self.as_uuid() == other.as_uuid()
    }

    fn media_type(&self) -> MediaType {
        MediaType::Season
    }
}

impl MediaIDLike for EpisodeID {
    type MediaId = MediaID;

    fn as_ref(&self) -> &Self {
        &self
    }

    fn to_media_id(self) -> Self::MediaId {
        MediaID::Episode(self)
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        self.to_uuid().hyphenated().encode_lower(buffer)
    }

    fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    fn to_uuid(self) -> Uuid {
        self.0
    }

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool {
        self.as_uuid() == other.as_uuid()
    }

    fn media_type(&self) -> MediaType {
        MediaType::Episode
    }
}

impl MediaIDLike for ArchivedEpisodeID {
    type MediaId = ArchivedMediaID;

    fn as_ref(&self) -> &Self {
        &self
    }

    fn to_media_id(self) -> Self::MediaId {
        ArchivedMediaID::Episode(self)
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        self.to_uuid().hyphenated().encode_lower(buffer)
    }

    fn as_uuid(&self) -> &Uuid {
        Uuid::from_bytes_ref(&self.0)
    }

    fn to_uuid(self) -> Uuid {
        Uuid::from_bytes(self.0)
    }

    fn sub_eq(&self, other: &impl MediaIDLike) -> bool {
        self.as_uuid() == other.as_uuid()
    }

    fn media_type(&self) -> MediaType {
        MediaType::Episode
    }
}
