pub mod accessor;
pub mod repository;

pub use accessor::*;
pub use repository::*;

use std::sync::Arc;

use ferrex_core::{
    ArchivedEpisodeReference, ArchivedLibrary, ArchivedMedia, ArchivedMovieReference,
    ArchivedSeasonReference, ArchivedSeriesReference, Media, MediaOps, MovieReference,
    SeriesReference,
};
use rkyv::util::AlignedVec;
use yoke::Yoke;

pub trait MaybeYoked {
    type InnerRef: MediaOps;
    //type Deserialized;

    fn get<'a>(&'a self) -> &'a Self::InnerRef;
    //fn get_owned(&self) -> Self::Deserialized;
}

impl MaybeYoked for MediaYoke {
    type InnerRef = ArchivedMedia;

    fn get<'a>(&'a self) -> &'a Self::InnerRef {
        *self.get()
    }
}

impl MaybeYoked for Media {
    type InnerRef = Media;

    fn get<'a>(&'a self) -> &'a Self::InnerRef {
        self
    }
}

pub type LibraryYoke = Yoke<&'static ArchivedLibrary, Arc<AlignedVec>>;
//pub type MediaYoke = Yoke<&'static MediaMaybeArchived<'static>, Arc<AlignedVec>>;
pub type MediaYoke = Yoke<&'static ArchivedMedia, Arc<AlignedVec>>;

pub type MovieVecYoke = Yoke<&'static Vec<ArchivedMovieReference>, Arc<AlignedVec>>;

pub type ArcMovieYoke = Arc<Yoke<&'static ArchivedMovieReference, Arc<AlignedVec>>>;
pub type MovieYoke = Yoke<&'static ArchivedMovieReference, Arc<AlignedVec>>;

pub type ArcSeriesYoke = Arc<Yoke<&'static ArchivedSeriesReference, Arc<AlignedVec>>>;
pub type SeriesYoke = Yoke<&'static ArchivedSeriesReference, Arc<AlignedVec>>;

pub type SeasonYoke = Yoke<&'static ArchivedSeasonReference, Arc<AlignedVec>>;
pub type EpisodeYoke = Yoke<&'static ArchivedEpisodeReference, Arc<AlignedVec>>;

impl MaybeYoked for MovieYoke {
    type InnerRef = ArchivedMovieReference;

    fn get<'a>(&'a self) -> &'a Self::InnerRef {
        *self.get()
    }
}

impl MaybeYoked for ArcMovieYoke {
    //type Deserialized = MovieReference;
    type InnerRef = ArchivedMovieReference;

    fn get<'a>(&'a self) -> &'a Self::InnerRef {
        self.as_ref().get()
    }
}

impl MaybeYoked for MovieReference {
    type InnerRef = MovieReference;

    fn get<'a>(&'a self) -> &'a Self::InnerRef {
        self
    }
}

impl MaybeYoked for SeriesYoke {
    type InnerRef = ArchivedSeriesReference;

    fn get<'a>(&'a self) -> &'a Self::InnerRef {
        *self.get()
    }
}

impl MaybeYoked for ArcSeriesYoke {
    //type Deserialized = SeriesReference;
    type InnerRef = ArchivedSeriesReference;

    fn get<'a>(&'a self) -> &'a Self::InnerRef {
        self.as_ref().get()
    }
}

impl MaybeYoked for SeriesReference {
    type InnerRef = SeriesReference;

    fn get<'a>(&'a self) -> &'a Self::InnerRef {
        self
    }
}

/// Result type for repository operations
pub type RepositoryResult<T> = Result<T, RepositoryError>;

/// Repository-specific errors with proper context
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound { entity_type: String, id: String },

    #[error("Query failed: {0}")]
    QueryFailed(String),

    #[error("Deserialization failed: {0}")]
    DeserializationError(String),

    #[error("Update failed: {0}")]
    UpdateFailed(String),

    #[error("Delete failed: {0}")]
    DeleteFailed(String),

    #[error("Create failed: {0}")]
    CreateFailed(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Lock acquisition failed: {0}")]
    LockError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Media error: {0}")]
    MediaError(#[from] ferrex_core::error::MediaError),
}
