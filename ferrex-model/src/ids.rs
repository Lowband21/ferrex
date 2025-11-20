use crate::error::ModelError as MediaError;
use uuid::Uuid;

/// Strongly typed ID for libraries with validation
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(
    feature = "rkyv",
    rkyv(derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Copy))
)]
pub struct LibraryID(pub Uuid);

impl Default for LibraryID {
    fn default() -> Self {
        Self::new()
    }
}

impl LibraryID {
    pub fn new() -> Self {
        LibraryID(Uuid::now_v7())
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn to_uuid(&self) -> Uuid {
        self.0
    }
}

impl AsRef<Uuid> for LibraryID {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

#[cfg(feature = "rkyv")]
impl ArchivedLibraryID {
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for LibraryID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for movies with validation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(
    feature = "rkyv",
    rkyv(derive(Debug, Clone, PartialEq, Eq, Hash, Copy))
)]
pub struct MovieID(pub Uuid);

impl Default for MovieID {
    fn default() -> Self {
        Self::new()
    }
}

impl MovieID {
    pub fn new() -> Self {
        MovieID(Uuid::now_v7())
    }

    pub fn new_u64(id: Uuid) -> Result<Self, MediaError> {
        Ok(MovieID(id))
    }

    pub fn new_uuid() -> Self {
        MovieID(Uuid::now_v7())
    }

    pub fn from_string(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(MovieID(id.parse().expect("Failed to parse movie ID")))
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn to_uuid(&self) -> Uuid {
        self.0
    }
}

impl AsRef<Uuid> for MovieID {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::fmt::Display for MovieID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(feature = "rkyv")]
impl ArchivedMovieID {
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn to_uuid(&self) -> Uuid {
        self.0
    }
}

/// Strongly typed ID for series with validation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(
    feature = "rkyv",
    rkyv(derive(Debug, Clone, PartialEq, Eq, Hash, Copy))
)]
pub struct SeriesID(pub Uuid);

impl Default for SeriesID {
    fn default() -> Self {
        Self::new()
    }
}

impl SeriesID {
    pub fn new() -> Self {
        SeriesID(Uuid::now_v7())
    }

    pub fn new_u64(id: Uuid) -> Result<Self, MediaError> {
        Ok(SeriesID(id))
    }

    pub fn new_uuid() -> Self {
        SeriesID(Uuid::now_v7())
    }

    pub fn from_string(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(SeriesID(id.parse().expect("Failed to parse movie ID")))
    }

    pub fn from(id: Uuid) -> Self {
        SeriesID(id)
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn to_uuid(&self) -> Uuid {
        self.0
    }
}

impl AsRef<Uuid> for SeriesID {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::fmt::Display for SeriesID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(feature = "rkyv")]
impl ArchivedSeriesID {
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn to_uuid(&self) -> Uuid {
        self.0
    }
}

/// Strongly typed ID for seasons with validation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(
    feature = "rkyv",
    rkyv(derive(Debug, Clone, PartialEq, Eq, Hash, Copy))
)]
pub struct SeasonID(pub Uuid);

impl Default for SeasonID {
    fn default() -> Self {
        Self::new()
    }
}

impl SeasonID {
    pub fn new() -> Self {
        SeasonID(Uuid::now_v7())
    }

    pub fn new_u64(id: Uuid) -> Result<Self, MediaError> {
        Ok(SeasonID(id))
    }

    pub fn new_uuid() -> Self {
        SeasonID(Uuid::now_v7())
    }

    pub fn from(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(SeasonID(id.parse().expect("Failed to parse movie ID")))
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn to_uuid(&self) -> Uuid {
        self.0
    }
}

impl AsRef<Uuid> for SeasonID {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::fmt::Display for SeasonID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(feature = "rkyv")]
impl ArchivedSeasonID {
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn to_uuid(&self) -> Uuid {
        self.0
    }
}

/// Strongly typed ID for episodes with validation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(
    feature = "rkyv",
    rkyv(derive(Debug, Clone, PartialEq, Eq, Hash, Copy))
)]
pub struct EpisodeID(pub Uuid);

impl Default for EpisodeID {
    fn default() -> Self {
        Self::new()
    }
}

impl EpisodeID {
    pub fn new() -> Self {
        EpisodeID(Uuid::now_v7())
    }

    pub fn new_u64(id: Uuid) -> Result<Self, MediaError> {
        Ok(EpisodeID(id))
    }

    pub fn new_uuid() -> Self {
        EpisodeID(Uuid::now_v7())
    }

    pub fn from(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(EpisodeID(id.parse().expect("Failed to parse movie ID")))
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn to_uuid(&self) -> Uuid {
        self.0
    }
}

impl AsRef<Uuid> for EpisodeID {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::fmt::Display for EpisodeID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(feature = "rkyv")]
impl ArchivedEpisodeID {
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn to_uuid(&self) -> Uuid {
        self.0
    }
}

/// Strongly typed ID for persons with validation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(
    feature = "rkyv",
    rkyv(derive(Debug, Clone, PartialEq, Eq, Hash, Copy))
)]
pub struct PersonID(pub Uuid);

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
}

impl AsRef<Uuid> for PersonID {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::fmt::Display for PersonID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
