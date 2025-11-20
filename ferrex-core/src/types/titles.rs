use crate::MediaError;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

/// Strongly typed movie title
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct MovieTitle(String);

impl MovieTitle {
    pub fn new(title: String) -> Result<Self, MediaError> {
        if title.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie title cannot be empty".to_string(),
            ));
        }
        Ok(MovieTitle(title))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MovieTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for ArchivedMovieTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for MovieTitle {
    fn from(s: String) -> Self {
        MovieTitle(s)
    }
}

impl From<&str> for MovieTitle {
    fn from(s: &str) -> Self {
        MovieTitle(s.to_string())
    }
}

impl AsRef<str> for MovieTitle {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ArchivedMovieTitle {
    fn as_ref(&self) -> &str {
        &self.0.as_str()
    }
}

impl std::hash::Hash for MovieTitle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialOrd for MovieTitle {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MovieTitle {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

/// Strongly typed series title
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct SeriesTitle(String);

impl SeriesTitle {
    pub fn new(title: String) -> Result<Self, MediaError> {
        if title.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Series title cannot be empty".to_string(),
            ));
        }
        Ok(SeriesTitle(title))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SeriesTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::fmt::Display for ArchivedSeriesTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SeriesTitle {
    fn from(s: String) -> Self {
        SeriesTitle(s)
    }
}

impl From<&str> for SeriesTitle {
    fn from(s: &str) -> Self {
        SeriesTitle(s.to_string())
    }
}

impl AsRef<str> for SeriesTitle {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl AsRef<str> for ArchivedSeriesTitle {
    fn as_ref(&self) -> &str {
        &self.0.as_str()
    }
}

impl std::hash::Hash for SeriesTitle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialOrd for SeriesTitle {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SeriesTitle {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

/// Strongly typed episode title
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct EpisodeTitle(String);

impl EpisodeTitle {
    pub fn new(title: String) -> Result<Self, MediaError> {
        if title.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Episode title cannot be empty".to_string(),
            ));
        }
        Ok(EpisodeTitle(title))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EpisodeTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::fmt::Display for ArchivedEpisodeTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for EpisodeTitle {
    fn from(s: String) -> Self {
        EpisodeTitle(s)
    }
}

impl From<&str> for EpisodeTitle {
    fn from(s: &str) -> Self {
        EpisodeTitle(s.to_string())
    }
}

impl AsRef<str> for EpisodeTitle {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl AsRef<str> for ArchivedEpisodeTitle {
    fn as_ref(&self) -> &str {
        &self.0.as_str()
    }
}

impl std::hash::Hash for EpisodeTitle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialOrd for EpisodeTitle {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EpisodeTitle {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}
