//! Core data model definitions shared across Ferrex crates.
#![allow(missing_docs)]

#[cfg(feature = "chrono")]
pub use ::chrono;

#[cfg(not(feature = "chrono"))]
pub mod chrono_stub;
#[cfg(not(feature = "chrono"))]
pub use chrono_stub as chrono;

pub mod details;
pub mod error;
pub mod events;
pub mod files;
pub mod filter_types;
pub mod ids;
pub mod image;
pub mod library;
pub mod media;
pub mod media_events;
pub mod media_id;
pub mod media_type;
pub mod numbers;
pub mod prelude;
#[cfg(feature = "rkyv")]
pub mod rkyv_wrappers;
pub mod scan;
pub mod subject_key;
pub mod titles;
pub mod transcoding;
pub mod urls;
pub mod watch;

// Intentionally curated re-exports for downstream consumers.
#[cfg(feature = "rkyv")]
pub use details::ArchivedCastMember;
pub use details::{
    EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeDetails, GenreInfo,
    LibraryReference, NetworkInfo, ProductionCompany, ProductionCountry,
    SeasonDetails, SpokenLanguage, TmdbDetails,
};
pub use error::{ModelError, Result as ModelResult};
pub use files::{MediaFile, MediaFileMetadata, ParsedMediaInfo};
pub use filter_types::{UiDecade, UiGenre, UiResolution, UiWatchStatus};
pub use ids::{
    EpisodeID, LibraryId, MovieBatchId, MovieID, MovieReferenceBatchSize,
    SeasonID, SeriesID,
};
pub use image::{
    BackdropSize, EpisodeSize, ImageSize, PosterSize, ProfileSize,
};
pub use image::{ImageRequest, Priority};
#[cfg(feature = "rkyv")]
pub use library::{ArchivedLibrary, ArchivedLibraryExt, ArchivedLibraryType};
pub use library::{Library, LibraryLike, LibraryLikeMut, LibraryType};
#[cfg(feature = "rkyv")]
pub use media::{
    ArchivedEpisodeReference, ArchivedMedia, ArchivedMovieReference,
    ArchivedSeasonReference, ArchivedSeries,
};
pub use media::{
    EpisodeReference, Media, MovieReference, SeasonReference, Series,
};
pub use media_events::{
    MediaEvent, ScanEventMetadata, ScanProgressEvent, ScanStageLatencySummary,
};
#[cfg(feature = "rkyv")]
pub use media_id::ArchivedMediaID;
pub use media_id::MediaID;
pub use media_type::ImageMediaType;
pub use media_type::VideoMediaType;
pub use subject_key::{NormalizedPathKey, OpaqueSubjectKey, SubjectKey};
pub use transcoding::{
    TranscodingJobResponse, TranscodingProgressDetails, TranscodingStatus,
};
pub use watch::{
    EpisodeKey, EpisodeStatus, NextEpisode, NextReason, SeasonKey,
    SeasonWatchStatus, SeriesWatchStatus,
};
