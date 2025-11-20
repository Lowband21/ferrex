pub mod accessor;
pub mod repository;

pub use accessor::*;
pub use repository::*;

use std::sync::Arc;

use ferrex_core::{
    ArchivedLibrary, ArchivedMedia, ArchivedMovieReference, ArchivedSeasonReference,
    ArchivedSeriesReference, Media, MediaOps, MovieReference, SeriesReference,
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
