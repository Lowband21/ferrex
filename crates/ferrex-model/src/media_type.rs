use std::fmt::Formatter;

use std::fmt::Display;

use crate::MediaID;

/// Simple enum for media types
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "media_type", rename_all = "lowercase")
)]
pub enum ImageMediaType {
    /// Movie media type
    Movie = 0,
    /// Series media type
    Series = 1,
    /// Season media type
    Season = 2,
    /// Episode media type
    Episode = 3,
    /// Person media type
    Person = 4,
}

impl Display for ImageMediaType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageMediaType::Movie => write!(f, "Movie"),
            ImageMediaType::Series => write!(f, "Series"),
            ImageMediaType::Season => write!(f, "Season"),
            ImageMediaType::Episode => write!(f, "Episode"),
            ImageMediaType::Person => write!(f, "Person"),
        }
    }
}

impl From<u16> for ImageMediaType {
    fn from(value: u16) -> Self {
        match value {
            0 => ImageMediaType::Movie,
            1 => ImageMediaType::Series,
            2 => ImageMediaType::Season,
            3 => ImageMediaType::Episode,
            4 => ImageMediaType::Person,
            _ => panic!("Invalid media type"),
        }
    }
}

impl ImageMediaType {
    pub const fn as_u16(self) -> u16 {
        match self {
            ImageMediaType::Movie => 0,
            ImageMediaType::Series => 1,
            ImageMediaType::Season => 2,
            ImageMediaType::Episode => 3,
            ImageMediaType::Person => 4,
        }
    }
}

/// Media types supported by the card system
#[derive(Debug, Clone, Copy, PartialEq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "media_type", rename_all = "lowercase")
)]
pub enum VideoMediaType {
    #[default]
    Movie = 0,
    Series = 1,
    Season = 2,
    Episode = 3,
}

impl VideoMediaType {
    /// Get the default fallback icon/emoji for this media type
    pub fn default_icon(&self) -> &'static str {
        match self {
            VideoMediaType::Movie => "🎬",
            VideoMediaType::Series => "📺",
            VideoMediaType::Season => "📺",
            VideoMediaType::Episode => "🎞️",
        }
    }
}

impl Display for VideoMediaType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoMediaType::Movie => write!(f, "Movie"),
            VideoMediaType::Series => write!(f, "Series"),
            VideoMediaType::Season => write!(f, "Season"),
            VideoMediaType::Episode => write!(f, "Episode"),
        }
    }
}

impl From<u16> for VideoMediaType {
    fn from(value: u16) -> Self {
        match value {
            0 => VideoMediaType::Movie,
            1 => VideoMediaType::Series,
            2 => VideoMediaType::Season,
            3 => VideoMediaType::Episode,
            _ => panic!("Invalid VideoMediaType"),
        }
    }
}

impl VideoMediaType {
    pub const fn as_u16(self) -> u16 {
        match self {
            VideoMediaType::Movie => 0,
            VideoMediaType::Series => 1,
            VideoMediaType::Season => 2,
            VideoMediaType::Episode => 3,
        }
    }
}
impl From<MediaID> for VideoMediaType {
    fn from(value: MediaID) -> Self {
        match value {
            MediaID::Movie(_) => VideoMediaType::Movie,
            MediaID::Series(_) => VideoMediaType::Series,
            MediaID::Season(_) => VideoMediaType::Season,
            MediaID::Episode(_) => VideoMediaType::Episode,
        }
    }
}
