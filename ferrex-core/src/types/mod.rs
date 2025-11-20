// Media module - organizes all media-related types and traits

pub mod details;
pub mod events;
pub mod files;
pub mod filter_types;
pub mod ids;
pub mod image;
pub mod image_request;
pub mod library;
pub mod media;
pub mod media_events;
pub mod media_id;
pub mod numbers;
pub mod prelude;
pub mod scan;
pub mod titles;
pub mod transcoding;
pub mod urls;
pub mod util_types;

// Intentionally curated re-exports for downstream consumers.
pub use details::{
    ArchivedCastMember, EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeDetails, GenreInfo,
    LibraryReference, MediaDetailsOption, NetworkInfo, ProductionCompany, ProductionCountry,
    SeasonDetails, SpokenLanguage, TmdbDetails,
};
pub use files::{MediaFile, MediaFileMetadata, ParsedMediaInfo};
pub use filter_types::{UiDecade, UiGenre, UiResolution, UiWatchStatus};
pub use ids::{EpisodeID, LibraryID, MovieID, SeasonID, SeriesID};
pub use image_request::{
    BackdropKind, BackdropSize, EpisodeStillSize, ImageRequest, PosterKind, PosterSize, Priority,
    ProfileSize,
};
pub use library::{
    ArchivedLibrary, ArchivedLibraryExt, ArchivedLibraryType, Library, LibraryLike, LibraryLikeMut,
    LibraryType,
};
pub use media::{
    ArchivedEpisodeReference, ArchivedMedia, ArchivedMovieReference, ArchivedSeasonReference,
    ArchivedSeriesReference, EpisodeReference, Media, MovieReference, SeasonReference,
    SeriesReference,
};
pub use media_events::{MediaEvent, ScanEventMetadata, ScanProgressEvent, ScanStageLatencySummary};
pub use media_id::{ArchivedMediaID, MediaID};
pub use transcoding::{TranscodingJobResponse, TranscodingProgressDetails, TranscodingStatus};
pub use util_types::{ImageSize, ImageType, MediaType};
