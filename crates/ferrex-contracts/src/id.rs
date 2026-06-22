//! Identifier traits for movie, series, season, and episode ids.
//!
//! The trait abstracts over owned model ids and archived rkyv ids while keeping a
//! small stack-allocated formatting buffer available for allocation-sensitive UI
//! and cache paths.

use uuid::Uuid;

use ferrex_model::ids::{EpisodeID, MovieID, SeasonID, SeriesID};
use ferrex_model::media_id::MediaID;
use ferrex_model::media_type::VideoMediaType;

const UUID_STR_LEN: usize = 36;

fn uuid_to_str(uuid: Uuid, buffer: &mut [u8; 45]) -> &str {
    let encoded: &mut str =
        uuid.hyphenated().encode_lower(&mut buffer[..UUID_STR_LEN]);
    encoded
}

/// Common interface for UUID-backed media identifiers.
pub trait MediaIDLike {
    /// Concrete media id type produced by [`MediaIDLike::to_media_id`].
    type MediaId: MediaIDLike;

    /// Borrow the id as its concrete type.
    fn as_ref(&self) -> &Self;
    /// Convert or clone the id into its concrete media-id representation.
    fn to_media_id(self) -> Self::MediaId;

    /// Format the UUID into the caller-provided buffer and return it as a string slice.
    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str;
    /// Format the UUID into the caller-provided buffer and copy it into an owned string.
    fn to_string_buf(&self, buffer: &mut [u8; 45]) -> String {
        String::from(self.as_str(buffer))
    }

    /// Borrow the underlying UUID.
    fn as_uuid(&self) -> &Uuid;
    /// Convert the id into the underlying UUID.
    fn to_uuid(self) -> Uuid;

    /// Compare this id with another media-id wrapper by UUID.
    fn sub_eq(&self, other: &impl MediaIDLike) -> bool;

    /// The playable media category for this id.
    ///
    /// Note: this intentionally returns `VideoMediaType` (Movie/Series/Season/Episode),
    /// not `ImageMediaType` (which includes `Person` for image ownership).
    fn media_type(&self) -> VideoMediaType;
}

impl MediaIDLike for MediaID {
    type MediaId = MediaID;

    fn as_ref(&self) -> &Self {
        self
    }

    fn to_media_id(self) -> Self::MediaId {
        self
    }

    fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
        match &self {
            MediaID::Movie(movie_id) => uuid_to_str(movie_id.to_uuid(), buffer),
            MediaID::Series(series_id) => {
                uuid_to_str(series_id.to_uuid(), buffer)
            }
            MediaID::Season(season_id) => {
                uuid_to_str(season_id.to_uuid(), buffer)
            }
            MediaID::Episode(episode_id) => {
                uuid_to_str(episode_id.to_uuid(), buffer)
            }
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

    fn media_type(&self) -> VideoMediaType {
        match &self {
            MediaID::Movie(_) => VideoMediaType::Movie,
            MediaID::Series(_) => VideoMediaType::Series,
            MediaID::Season(_) => VideoMediaType::Season,
            MediaID::Episode(_) => VideoMediaType::Episode,
        }
    }
}

#[cfg(feature = "rkyv")]
mod archived {
    use super::*;
    use ferrex_model::ids::{
        ArchivedEpisodeID, ArchivedMovieID, ArchivedSeasonID, ArchivedSeriesID,
    };
    use ferrex_model::media_id::ArchivedMediaID;

    impl MediaIDLike for ArchivedMediaID {
        type MediaId = ArchivedMediaID;

        fn as_ref(&self) -> &Self {
            self
        }

        fn to_media_id(self) -> Self::MediaId {
            self
        }

        fn as_str<'a>(&self, buffer: &'a mut [u8; 45]) -> &'a str {
            match &self {
                ArchivedMediaID::Movie(movie_id) => {
                    uuid_to_str(movie_id.to_uuid(), buffer)
                }
                ArchivedMediaID::Series(series_id) => {
                    uuid_to_str(series_id.to_uuid(), buffer)
                }
                ArchivedMediaID::Season(season_id) => {
                    uuid_to_str(season_id.to_uuid(), buffer)
                }
                ArchivedMediaID::Episode(episode_id) => {
                    uuid_to_str(episode_id.to_uuid(), buffer)
                }
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

        fn media_type(&self) -> VideoMediaType {
            match &self {
                ArchivedMediaID::Movie(_) => VideoMediaType::Movie,
                ArchivedMediaID::Series(_) => VideoMediaType::Series,
                ArchivedMediaID::Season(_) => VideoMediaType::Season,
                ArchivedMediaID::Episode(_) => VideoMediaType::Episode,
            }
        }
    }

    impl MediaIDLike for ArchivedMovieID {
        type MediaId = ArchivedMediaID;

        fn as_ref(&self) -> &Self {
            self
        }

        fn to_media_id(self) -> Self::MediaId {
            ArchivedMediaID::Movie(self)
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

        fn media_type(&self) -> VideoMediaType {
            VideoMediaType::Movie
        }
    }

    impl MediaIDLike for ArchivedSeriesID {
        type MediaId = ArchivedMediaID;

        fn as_ref(&self) -> &Self {
            self
        }

        fn to_media_id(self) -> Self::MediaId {
            ArchivedMediaID::Series(self)
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

        fn media_type(&self) -> VideoMediaType {
            VideoMediaType::Series
        }
    }

    impl MediaIDLike for ArchivedSeasonID {
        type MediaId = ArchivedMediaID;

        fn as_ref(&self) -> &Self {
            self
        }

        fn to_media_id(self) -> Self::MediaId {
            ArchivedMediaID::Season(self)
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

        fn media_type(&self) -> VideoMediaType {
            VideoMediaType::Season
        }
    }

    impl MediaIDLike for ArchivedEpisodeID {
        type MediaId = ArchivedMediaID;

        fn as_ref(&self) -> &Self {
            self
        }

        fn to_media_id(self) -> Self::MediaId {
            ArchivedMediaID::Episode(self)
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

        fn media_type(&self) -> VideoMediaType {
            VideoMediaType::Episode
        }
    }
}

impl MediaIDLike for MovieID {
    type MediaId = MediaID;

    fn as_ref(&self) -> &Self {
        self
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

    fn media_type(&self) -> VideoMediaType {
        VideoMediaType::Movie
    }
}

impl MediaIDLike for SeriesID {
    type MediaId = MediaID;

    fn as_ref(&self) -> &Self {
        self
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

    fn media_type(&self) -> VideoMediaType {
        VideoMediaType::Series
    }
}

impl MediaIDLike for SeasonID {
    type MediaId = MediaID;

    fn as_ref(&self) -> &Self {
        self
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

    fn media_type(&self) -> VideoMediaType {
        VideoMediaType::Season
    }
}

impl MediaIDLike for EpisodeID {
    type MediaId = MediaID;

    fn as_ref(&self) -> &Self {
        self
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

    fn media_type(&self) -> VideoMediaType {
        VideoMediaType::Episode
    }
}
