// Curated surface from ferrex-core for player-facing code
pub use ferrex_contracts::prelude::{EpisodeLike, SeasonLike};
pub use ferrex_core::player_prelude::{
    AdminUserInfo, ApiResponse, BatchMediaRequest, BatchMediaResponse,
    ConfirmClaimRequest, ConfirmClaimResponse, CreateLibraryRequest,
    CreateUserRequest, DemoLibraryStatus, DemoResetRequest, DemoStatus,
    EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeID, EpisodeReference,
    FetchMediaRequest, ImageData, ImageRequest, ImageSize, Library, LibraryId,
    LibraryMediaCache, LibraryMediaResponse, LibraryReference, LibraryType,
    Media, MediaFile, MediaFileMetadata, MediaID, MovieID, MovieReference,
    ParsedMediaInfo, Priority,
};

// Poster presence and ordering are enforced centrally in core/server.
