use crate::MediaError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Strongly typed ID for movies with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
pub struct MovieID(pub Uuid);

impl MovieID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(MovieID(id.parse().expect("Failed to parse movie ID")))
    }
    pub fn new_u64(id: Uuid) -> Result<Self, MediaError> {
        Ok(MovieID(id))
    }

    pub fn new_uuid() -> Self {
        MovieID(Uuid::now_v7())
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_ref(&self) -> &Uuid {
        &self.0
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for MovieID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for series with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
pub struct SeriesID(pub Uuid);

impl SeriesID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(SeriesID(id.parse().expect("Failed to parse movie ID")))
    }
    pub fn new_u64(id: Uuid) -> Result<Self, MediaError> {
        Ok(SeriesID(id))
    }

    pub fn new_uuid() -> Self {
        SeriesID(Uuid::now_v7())
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_ref(&self) -> &Uuid {
        &self.0
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for SeriesID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for seasons with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
pub struct SeasonID(Uuid);

impl SeasonID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(SeasonID(id.parse().expect("Failed to parse movie ID")))
    }
    pub fn new_u64(id: Uuid) -> Result<Self, MediaError> {
        Ok(SeasonID(id))
    }

    pub fn new_uuid() -> Self {
        SeasonID(Uuid::now_v7())
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_ref(&self) -> &Uuid {
        &self.0
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for SeasonID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for episodes with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
pub struct EpisodeID(pub Uuid);

impl EpisodeID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(EpisodeID(id.parse().expect("Failed to parse movie ID")))
    }
    pub fn new_u64(id: Uuid) -> Result<Self, MediaError> {
        Ok(EpisodeID(id))
    }

    pub fn new_uuid() -> Self {
        EpisodeID(Uuid::now_v7())
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_ref(&self) -> &Uuid {
        &self.0
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for EpisodeID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for persons with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
pub struct PersonID(Uuid);

impl PersonID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(PersonID(id.parse().expect("Failed to parse movie ID")))
    }
    pub fn new_u64(id: Uuid) -> Result<Self, MediaError> {
        Ok(PersonID(id))
    }

    pub fn new_uuid() -> Self {
        PersonID(Uuid::now_v7())
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_ref(&self) -> &Uuid {
        &self.0
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for PersonID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
