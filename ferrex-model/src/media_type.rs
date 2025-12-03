use std::fmt::Formatter;

use std::fmt::Display;

/// Simple enum for media types
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum MediaType {
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

impl Display for MediaType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaType::Movie => write!(f, "Movie"),
            MediaType::Series => write!(f, "Series"),
            MediaType::Season => write!(f, "Season"),
            MediaType::Episode => write!(f, "Episode"),
            MediaType::Person => write!(f, "Person"),
        }
    }
}

impl From<i16> for MediaType {
    fn from(value: i16) -> Self {
        match value {
            0 => MediaType::Movie,
            1 => MediaType::Series,
            2 => MediaType::Season,
            3 => MediaType::Episode,
            4 => MediaType::Person,
            _ => panic!("Invalid media type"),
        }
    }
}
