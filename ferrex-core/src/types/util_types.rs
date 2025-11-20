use std::fmt::{Display, Formatter};

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

/// Simple enum for media types
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub enum MediaType {
    /// Movie media type
    Movie = 0,
    /// Series media type
    Series = 1,
    /// Season media type
    Season = 2,
    /// Episode media type
    Episode = 3,
}

impl Display for MediaType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaType::Movie => write!(f, "Movie"),
            MediaType::Series => write!(f, "Series"),
            MediaType::Season => write!(f, "Season"),
            MediaType::Episode => write!(f, "Episode"),
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
            _ => panic!("Invalid media type"),
        }
    }
}

/// Image type used for categorization of images
#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
    Hash,
    Eq,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub enum ImageType {
    Movie = 0,
    Series = 1,
    Season = 2,
    Episode = 3,
    Person = 4,
}

impl Display for ImageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageType::Movie => write!(f, "Movie"),
            ImageType::Series => write!(f, "Series"),
            ImageType::Season => write!(f, "Season"),
            ImageType::Episode => write!(f, "Episode"),
            ImageType::Person => write!(f, "Person"),
        }
    }
}

/// Image size variants
#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
    Hash,
    Eq,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub enum ImageSize {
    Thumbnail, // Small size for grids
    Poster,    // Standard poster size
    Backdrop,  // Wide backdrop/banner
    Full,      // Hero poster (w500 equivalent)
    Profile,   // Person profile image (2:3 aspect ratio)
}

impl Display for ImageSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageSize::Thumbnail => write!(f, "Thumbnail"),
            ImageSize::Poster => write!(f, "Poster"),
            ImageSize::Backdrop => write!(f, "Backdrop"),
            ImageSize::Full => write!(f, "Full"),
            ImageSize::Profile => write!(f, "Profile"),
        }
    }
}

impl ImageSize {
    pub fn dimensions(&self) -> (f32, f32) {
        match self {
            ImageSize::Thumbnail => (150.0, 225.0),
            ImageSize::Poster => (200.0, 300.0),
            ImageSize::Backdrop => (1920.0, 1080.0), // Full HD backdrop
            ImageSize::Full => (0.0, 0.0),           // Dynamic
            ImageSize::Profile => (120.0, 180.0),    // 2:3 aspect ratio for cast
        }
    }

    pub fn suffix(&self) -> &str {
        match self {
            ImageSize::Thumbnail => "_thumb",
            ImageSize::Poster => "_poster",
            ImageSize::Backdrop => "_backdrop",
            ImageSize::Full => "",
            ImageSize::Profile => "_profile",
        }
    }
}
