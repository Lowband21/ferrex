use crate::error::ModelError as MediaError;
use std::num::NonZeroU32;
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
pub struct LibraryId(pub Uuid);

impl Default for LibraryId {
    fn default() -> Self {
        Self::new()
    }
}

impl LibraryId {
    pub fn new() -> Self {
        LibraryId(Uuid::now_v7())
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

impl AsRef<Uuid> for LibraryId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

#[cfg(feature = "rkyv")]
impl ArchivedLibraryId {
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for LibraryId {
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

/// Per-library monotonically increasing batch id for movie references.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(
    feature = "rkyv",
    rkyv(derive(Debug, Clone, PartialEq, Eq, Hash, Copy, PartialOrd, Ord))
)]
pub struct MovieBatchId(pub u32);

impl MovieBatchId {
    pub fn new(value: u32) -> Result<Self, MediaError> {
        if value == 0 {
            return Err(MediaError::InvalidMedia(
                "Movie batch id cannot be 0".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }

    pub fn as_i64(&self) -> i64 {
        self.0 as i64
    }
}

impl std::fmt::Display for MovieBatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Fixed per-library batch size for movie reference ingestion.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(
    feature = "rkyv",
    rkyv(derive(Debug, Clone, PartialEq, Eq, Hash, Copy, PartialOrd, Ord))
)]
pub struct MovieReferenceBatchSize(pub NonZeroU32);

impl Default for MovieReferenceBatchSize {
    fn default() -> Self {
        Self(NonZeroU32::new(100).unwrap())
    }
}

impl MovieReferenceBatchSize {
    pub fn new(value: u32) -> Result<Self, MediaError> {
        let nz = NonZeroU32::new(value).ok_or_else(|| {
            MediaError::InvalidMedia(
                "Movie reference batch size must be > 0".to_string(),
            )
        })?;
        Ok(Self(nz))
    }

    pub fn get(&self) -> u32 {
        self.0.get()
    }
}

impl std::fmt::Display for MovieReferenceBatchSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.get())
    }
}

impl Default for MovieID {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for MovieID {
    fn from(id: Uuid) -> Self {
        MovieID(id)
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

impl From<Uuid> for SeriesID {
    fn from(id: Uuid) -> Self {
        SeriesID(id)
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

impl From<Uuid> for SeasonID {
    fn from(id: Uuid) -> Self {
        SeasonID(id)
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

impl From<Uuid> for EpisodeID {
    fn from(id: Uuid) -> Self {
        EpisodeID(id)
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

impl From<Uuid> for PersonID {
    fn from(id: Uuid) -> Self {
        PersonID(id)
    }
}

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
