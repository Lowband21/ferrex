use std::{convert::Infallible, fmt, str::FromStr};

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Logical categories of images associated with media items.
///
/// Values mapping to the legacy database representation are stored in
/// lowercase snake_case (`poster`, `backdrop`, ...). Unknown categories are
/// preserved as-is to avoid data loss during migration; they should be mapped
/// to first-class variants before being produced by new code.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Archive, RkyvSerialize, RkyvDeserialize)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub enum MediaImageKind {
    Poster,
    Backdrop,
    Logo,
    Thumbnail,
    Cast,
    /// Any unrecognised value. We allow custom strings so migrations can
    /// round-trip existing data before tighter guarantees are introduced.
    Other(String),
}

impl MediaImageKind {
    /// Canonical lowercase representation used by the database schema.
    pub fn as_str(&self) -> &str {
        match self {
            MediaImageKind::Poster => "poster",
            MediaImageKind::Backdrop => "backdrop",
            MediaImageKind::Logo => "logo",
            MediaImageKind::Thumbnail => "thumbnail",
            MediaImageKind::Cast => "cast",
            MediaImageKind::Other(value) => value.as_str(),
        }
    }

    /// Returns a string owned by value. Unknown kinds preserve the original
    /// casing that was provided when constructing the enum.
    pub fn into_string(self) -> String {
        match self {
            MediaImageKind::Poster => "poster".to_owned(),
            MediaImageKind::Backdrop => "backdrop".to_owned(),
            MediaImageKind::Logo => "logo".to_owned(),
            MediaImageKind::Thumbnail => "thumbnail".to_owned(),
            MediaImageKind::Cast => "cast".to_owned(),
            MediaImageKind::Other(value) => value,
        }
    }

    /// Convenience for matches that should treat unknown kinds uniformly.
    pub fn is_other(&self) -> bool {
        matches!(self, MediaImageKind::Other(_))
    }

    /// Parses a value using ASCII-case-insensitive matching while preserving
    /// the original casing for unknown values.
    pub fn parse(value: &str) -> Self {
        if value.eq_ignore_ascii_case("poster") {
            MediaImageKind::Poster
        } else if value.eq_ignore_ascii_case("backdrop") {
            MediaImageKind::Backdrop
        } else if value.eq_ignore_ascii_case("logo") {
            MediaImageKind::Logo
        } else if value.eq_ignore_ascii_case("thumbnail") {
            MediaImageKind::Thumbnail
        } else if value.eq_ignore_ascii_case("cast") {
            MediaImageKind::Cast
        } else {
            MediaImageKind::Other(value.to_string())
        }
    }
}

impl FromStr for MediaImageKind {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(MediaImageKind::parse(s))
    }
}

impl fmt::Display for MediaImageKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for MediaImageKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MediaImageKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(MediaImageKind::parse(&value))
    }
}

impl From<MediaImageKind> for String {
    fn from(value: MediaImageKind) -> Self {
        value.into_string()
    }
}

impl From<&MediaImageKind> for String {
    fn from(value: &MediaImageKind) -> Self {
        value.as_str().to_string()
    }
}
