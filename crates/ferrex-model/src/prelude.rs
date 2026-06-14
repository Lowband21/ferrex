//! Player/UI focused snapshot of the types surface.
//! Prefer importing from this module instead of individual tree nodes when
//! working in ferrex-player or other presentation layers.

#[cfg(feature = "rkyv")]
pub use super::details::ArchivedCastMember;
pub use super::details::{
    EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeDetails, GenreInfo,
    LibraryReference, NetworkInfo, ProductionCompany, ProductionCountry,
    SeasonDetails, SpokenLanguage, TmdbDetails,
};
pub use super::files::{MediaFile, MediaFileMetadata, ParsedMediaInfo};
pub use super::filter_types::{UiDecade, UiGenre, UiResolution, UiWatchStatus};
pub use super::ids::{
    EpisodeID, LibraryId, MovieBatchId, MovieID, MovieReferenceBatchSize,
    SeasonID, SeriesID,
};
pub use super::image::{
    BackdropSize, EpisodeSize, ImageSize, PosterSize, ProfileSize,
};
pub use super::image::{ImageRequest, Priority};
#[cfg(feature = "rkyv")]
pub use super::library::{
    ArchivedLibrary, ArchivedLibraryExt, ArchivedLibraryType,
};
pub use super::library::{Library, LibraryLike, LibraryType};
#[cfg(feature = "rkyv")]
pub use super::media::{
    ArchivedEpisodeReference, ArchivedMedia, ArchivedMovieReference,
    ArchivedSeasonReference, ArchivedSeries,
};
pub use super::media::{
    EpisodeReference, Media, MovieReference, SeasonReference, Series,
};
#[cfg(feature = "rkyv")]
pub use super::media_id::ArchivedMediaID;
pub use super::media_id::MediaID;
pub use super::media_type::{ImageMediaType, VideoMediaType};
pub use super::transcoding::{
    TranscodingJobResponse, TranscodingProgressDetails, TranscodingStatus,
};
pub use super::watch::{
    EpisodeKey, EpisodeStatus, NextEpisode, NextReason, SeasonKey,
    SeasonWatchStatus, SeriesWatchStatus,
};
