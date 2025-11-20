//! Conversion helpers for archived persistence snapshots.
//!
//! These helpers live in `ferrex-core` so infrastructure and UI layers can
//! convert the rkyv-backed archived payloads produced by the server back into
//! the owned `ferrex-model` structs without repeating boilerplate or reaching
//! for `rkyv::deserialize` directly.

use ferrex_model::{
    library::{ArchivedLibrary, Library},
    media::{
        ArchivedEpisodeReference, ArchivedMedia, ArchivedMovieReference,
        ArchivedSeasonReference, ArchivedSeriesReference, EpisodeReference,
        Media, MovieReference, SeasonReference, SeriesReference,
    },
};
use rkyv::{deserialize, rancor::Error as RkyvError};

/// Trait implemented by archived snapshots that can be materialized into their
/// runtime `ferrex-model` counterparts.
pub trait ArchivedModel {
    /// The owned model type this archived value can materialize into.
    type Model;

    /// Try to materialize the archived value into the owned model type.
    fn try_to_model(&self) -> Result<Self::Model, RkyvError>;

    /// Materialize the archived value into the owned model type, panicking if
    /// deserialization fails. Prefer [`ArchivedModel::try_to_model`] when error
    /// propagation is desirable.
    fn to_model(&self) -> Self::Model {
        self.try_to_model().expect(
            "archived payload emitted by ferrex-server must deserialize",
        )
    }
}

impl ArchivedModel for ArchivedMedia {
    type Model = Media;

    fn try_to_model(&self) -> Result<Self::Model, RkyvError> {
        deserialize::<Media, RkyvError>(self)
    }
}

impl ArchivedModel for ArchivedMovieReference {
    type Model = MovieReference;

    fn try_to_model(&self) -> Result<Self::Model, RkyvError> {
        deserialize::<MovieReference, RkyvError>(self)
    }
}

impl ArchivedModel for ArchivedSeriesReference {
    type Model = SeriesReference;

    fn try_to_model(&self) -> Result<Self::Model, RkyvError> {
        deserialize::<SeriesReference, RkyvError>(self)
    }
}

impl ArchivedModel for ArchivedSeasonReference {
    type Model = SeasonReference;

    fn try_to_model(&self) -> Result<Self::Model, RkyvError> {
        deserialize::<SeasonReference, RkyvError>(self)
    }
}

impl ArchivedModel for ArchivedEpisodeReference {
    type Model = EpisodeReference;

    fn try_to_model(&self) -> Result<Self::Model, RkyvError> {
        deserialize::<EpisodeReference, RkyvError>(self)
    }
}

impl ArchivedModel for ArchivedLibrary {
    type Model = Library;

    fn try_to_model(&self) -> Result<Self::Model, RkyvError> {
        deserialize::<Library, RkyvError>(self)
    }
}
