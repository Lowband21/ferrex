use crate::MediaError;
use serde::{Deserialize, Serialize};

/// Strongly typed ID for movies with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MovieID(String);

impl MovieID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(MovieID(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MovieID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for series with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SeriesID(String);

impl SeriesID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Series ID cannot be empty".to_string(),
            ));
        }
        Ok(SeriesID(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SeriesID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for seasons with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SeasonID(String);

impl SeasonID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Season ID cannot be empty".to_string(),
            ));
        }
        Ok(SeasonID(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SeasonID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for episodes with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EpisodeID(String);

impl EpisodeID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Episode ID cannot be empty".to_string(),
            ));
        }
        Ok(EpisodeID(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EpisodeID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for persons with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PersonID(String);

impl PersonID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Person ID cannot be empty".to_string(),
            ));
        }
        Ok(PersonID(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PersonID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
