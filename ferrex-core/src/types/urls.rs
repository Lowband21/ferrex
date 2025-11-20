use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use url::Url;

pub trait UrlLike {
    fn new(url: Url) -> Self;
    fn from_string(s: String) -> Self;
    fn as_str(&self) -> &str;
    fn to_string(self) -> String;
}

/// Movie endpoint URL with validation
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct MovieURL(String);

impl UrlLike for MovieURL {
    fn new(url: Url) -> Self {
        MovieURL(url.to_string())
    }

    fn from_string(s: String) -> Self {
        MovieURL(s)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
    fn to_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for MovieURL {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for ArchivedMovieURL {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl From<String> for MovieURL {
    fn from(s: String) -> Self {
        MovieURL(s)
    }
}

impl AsRef<str> for MovieURL {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::hash::Hash for MovieURL {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Series endpoint URL with validation
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct SeriesURL(String);

impl SeriesURL {
    pub fn new(url: Url) -> Self {
        SeriesURL(url.to_string())
    }

    pub fn from_string(s: String) -> Self {
        SeriesURL(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SeriesURL {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::fmt::Display for ArchivedSeriesURL {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl From<String> for SeriesURL {
    fn from(s: String) -> Self {
        SeriesURL(s)
    }
}

impl AsRef<str> for SeriesURL {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::hash::Hash for SeriesURL {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Season endpoint URL with validation
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct SeasonURL(String);

impl SeasonURL {
    pub fn new(url: Url) -> Self {
        SeasonURL(url.to_string())
    }

    pub fn from_string(s: String) -> Self {
        SeasonURL(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SeasonURL {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::fmt::Display for ArchivedSeasonURL {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl From<String> for SeasonURL {
    fn from(s: String) -> Self {
        SeasonURL(s)
    }
}

impl AsRef<str> for SeasonURL {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::hash::Hash for SeasonURL {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Episode endpoint URL with validation
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct EpisodeURL(String);

impl EpisodeURL {
    pub fn new(url: Url) -> Self {
        EpisodeURL(url.to_string())
    }

    pub fn from_string(s: String) -> Self {
        EpisodeURL(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EpisodeURL {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::fmt::Display for ArchivedEpisodeURL {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl From<String> for EpisodeURL {
    fn from(s: String) -> Self {
        EpisodeURL(s)
    }
}

impl AsRef<str> for EpisodeURL {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::hash::Hash for EpisodeURL {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
