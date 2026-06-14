//! Player media repository and disk cache primitives.
//!
//! This crate contains the dependency-light media repository extracted from the
//! desktop player monolith. It is safe for player state/domain code to depend on
//! these types directly; UI/image-handle caches remain in `ferrex-player`.

pub mod accessor;
pub mod media_repo;
pub mod media_repo_disk_cache;
pub mod movie_batches;
pub mod series_bundles;
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

pub trait MaybeYoked {
    type InnerRef: MediaOps;

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

pub type LibraryYoke = Yoke<&'static ArchivedLibrary, Arc<AlignedVec>>;
pub type MediaYoke = Yoke<&'static ArchivedMedia, Arc<AlignedVec>>;

pub type MovieVecYoke =
    Yoke<&'static Vec<ArchivedMovieReference>, Arc<AlignedVec>>;

pub type ArcMovieYoke =
    Arc<Yoke<&'static ArchivedMovieReference, Arc<AlignedVec>>>;
pub type MovieYoke = Yoke<&'static ArchivedMovieReference, Arc<AlignedVec>>;

pub type ArcSeriesYoke = Arc<Yoke<&'static ArchivedSeries, Arc<AlignedVec>>>;
pub type SeriesYoke = Yoke<&'static ArchivedSeries, Arc<AlignedVec>>;

pub type SeasonYoke = Yoke<&'static ArchivedSeasonReference, Arc<AlignedVec>>;
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
