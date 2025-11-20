//! Player/UI focused snapshot of the types surface.
//! Prefer importing from this module instead of individual tree nodes when
//! working in ferrex-player or other presentation layers.

pub use super::details::{
    ArchivedCastMember, EnhancedMovieDetails, EnhancedSeriesDetails,
    EpisodeDetails, GenreInfo, LibraryReference, MediaDetailsOption,
    NetworkInfo, ProductionCompany, ProductionCountry, SeasonDetails,
    SpokenLanguage, TmdbDetails,
};
pub use super::files::{MediaFile, MediaFileMetadata, ParsedMediaInfo};
pub use super::filter_types::{UiDecade, UiGenre, UiResolution, UiWatchStatus};
pub use super::ids::{EpisodeID, LibraryID, MovieID, SeasonID, SeriesID};
pub use super::image_request::{
    BackdropKind, BackdropSize, EpisodeStillSize, ImageRequest, PosterKind,
    PosterSize, Priority, ProfileSize,
};
pub use super::library::{
    ArchivedLibrary, ArchivedLibraryExt, ArchivedLibraryType, Library,
    LibraryLike, LibraryType,
};
pub use super::media::{
    ArchivedEpisodeReference, ArchivedMedia, ArchivedMovieReference,
    ArchivedSeasonReference, ArchivedSeriesReference, EpisodeReference, Media,
    MovieReference, SeasonReference, SeriesReference,
};
pub use super::media_id::{ArchivedMediaID, MediaID};
pub use super::transcoding::{
    TranscodingJobResponse, TranscodingProgressDetails, TranscodingStatus,
};
pub use super::util_types::{ImageSize, ImageType, MediaType};
