//! Player media repository and disk cache primitives.
//!
//! This crate contains the dependency-light media repository extracted from the
//! desktop player monolith. It is safe for player state/domain code to depend on
//! these types directly; UI/image-handle caches remain in `ferrex-player`.

/// Typed read/write accessors for shared repository handles.
pub mod accessor;
/// In-memory media repository indexes and lookup helpers.
pub mod media_repo;
/// Disk cache integration for serialized repository snapshots.
pub mod media_repo_disk_cache;
/// Movie-batch cache/index structures.
pub mod movie_batches;
/// Series-bundle cache/index structures.
pub mod series_bundles;
/// LRU-style yoke cache for archived model payloads.
pub mod yoke_cache;

pub use accessor::*;
pub use ferrex_player_foundation::repository::{
    RepositoryError, RepositoryResult,
};
pub use media_repo::*;
pub use media_repo_disk_cache::*;
pub use movie_batches::*;
pub use series_bundles::*;
pub use yoke_cache::*;

use std::sync::Arc;

use ferrex_core::player_prelude::{
    ArchivedEpisodeReference, ArchivedLibrary, ArchivedMedia,
    ArchivedMovieReference, ArchivedSeasonReference, ArchivedSeries, Media,
    MediaOps, MovieReference, Series,
};
use rkyv::util::AlignedVec;
use yoke::Yoke;

/// Borrow the underlying media reference from either owned or yoke-backed values.
pub trait MaybeYoked {
    /// Reference type returned by the wrapper.
    type InnerRef: MediaOps;

    /// Borrow the underlying media reference.
    fn get(&self) -> &Self::InnerRef;
}

impl MaybeYoked for MediaYoke {
    type InnerRef = ArchivedMedia;

    fn get(&self) -> &Self::InnerRef {
        self.get()
    }
}

impl MaybeYoked for Media {
    type InnerRef = Media;

    fn get(&self) -> &Self::InnerRef {
        self
    }
}

/// Archived library snapshot tied to its backing byte buffer.
pub type LibraryYoke = Yoke<&'static ArchivedLibrary, Arc<AlignedVec>>;
/// Archived media snapshot tied to its backing byte buffer.
pub type MediaYoke = Yoke<&'static ArchivedMedia, Arc<AlignedVec>>;

/// Archived movie-reference vector tied to its backing byte buffer.
pub type MovieVecYoke =
    Yoke<&'static Vec<ArchivedMovieReference>, Arc<AlignedVec>>;

/// Shared archived movie reference.
pub type ArcMovieYoke =
    Arc<Yoke<&'static ArchivedMovieReference, Arc<AlignedVec>>>;
/// Archived movie reference tied to its backing byte buffer.
pub type MovieYoke = Yoke<&'static ArchivedMovieReference, Arc<AlignedVec>>;

/// Shared archived series reference.
pub type ArcSeriesYoke = Arc<Yoke<&'static ArchivedSeries, Arc<AlignedVec>>>;
/// Archived series reference tied to its backing byte buffer.
pub type SeriesYoke = Yoke<&'static ArchivedSeries, Arc<AlignedVec>>;

/// Archived season reference tied to its backing byte buffer.
pub type SeasonYoke = Yoke<&'static ArchivedSeasonReference, Arc<AlignedVec>>;
/// Archived episode reference tied to its backing byte buffer.
pub type EpisodeYoke = Yoke<&'static ArchivedEpisodeReference, Arc<AlignedVec>>;

impl MaybeYoked for MovieYoke {
    type InnerRef = ArchivedMovieReference;

    fn get(&self) -> &Self::InnerRef {
        self.get()
    }
}

impl MaybeYoked for ArcMovieYoke {
    type InnerRef = ArchivedMovieReference;

    fn get(&self) -> &Self::InnerRef {
        self.as_ref().get()
    }
}

impl MaybeYoked for MovieReference {
    type InnerRef = MovieReference;

    fn get(&self) -> &Self::InnerRef {
        self
    }
}

impl MaybeYoked for SeriesYoke {
    type InnerRef = ArchivedSeries;

    fn get(&self) -> &Self::InnerRef {
        self.get()
    }
}

impl MaybeYoked for ArcSeriesYoke {
    type InnerRef = ArchivedSeries;

    fn get(&self) -> &Self::InnerRef {
        self.as_ref().get()
    }
}

impl MaybeYoked for Series {
    type InnerRef = Series;

    fn get(&self) -> &Self::InnerRef {
        self
    }
}
