//! Shared data models for the Ferrex media platform.
//!
//! `ferrex-model` is the dependency-light home for identifiers, media/library
//! DTOs, TMDB detail snapshots, image metadata, watch-state aggregates,
//! transcoding responses, scan progress events, and validation helpers that must
//! be shared by the server, desktop player, FlatBuffers conversion layer, and
//! future clients.
//!
//! Crate-root re-exports are intentionally curated for downstream consumers that
//! need the stable model surface without depending on the internal module tree.
//! The `prelude` module provides a player/UI-oriented import surface.

// Many public DTO fields intentionally mirror API/serialization contracts. The
// crate now documents its high-level modules and curated exports, while detailed
// field-level docs continue to land incrementally during 0.1.x API stabilization.
#![allow(missing_docs)]

#[cfg(feature = "chrono")]
pub use ::chrono;

/// Minimal chrono-compatible fallback used when the `chrono` feature is disabled.
#[cfg(not(feature = "chrono"))]
pub mod chrono_stub;
#[cfg(not(feature = "chrono"))]
pub use chrono_stub as chrono;

/// Metadata detail records sourced from TMDB and stored with media references.
pub mod details;
/// Model validation and construction errors.
pub mod error;
/// Generic event wrappers shared by runtime publishers.
pub mod events;
/// Media file paths, parsed file metadata, and file-classification DTOs.
pub mod files;
/// UI filter value objects for genre, decade, resolution, and watch state.
pub mod filter_types;
/// UUID-backed strongly typed identifiers.
pub mod ids;
/// Image sizing, request, metadata, and fetch DTOs.
pub mod image;
/// Image-ready event DTOs emitted by server processing.
pub mod image_events;
/// Library configuration and library-like traits.
pub mod library;
/// Movie, series, season, and episode reference DTOs.
pub mod media;
/// Media and scan-progress event DTOs.
pub mod media_events;
/// Sum type over movie, series, season, and episode identifiers.
pub mod media_id;
/// Media category enums used by APIs and image ownership.
pub mod media_type;
/// Bounded season and episode number value objects.
pub mod numbers;
/// Player/UI focused re-export surface.
pub mod prelude;
/// Rate-limit policy DTOs shared by configuration and server state.
pub mod rate_limit;
/// rkyv wrappers for external dependency types.
#[cfg(feature = "rkyv")]
pub mod rkyv_wrappers;
/// Scan configuration and scan-progress DTOs.
pub mod scan;
/// Stable subject-key value objects for cache and watch-state indexing.
pub mod subject_key;
/// Strongly typed title value objects.
pub mod titles;
/// Transcoding job status and progress DTOs.
pub mod transcoding;
/// URL value objects for media endpoints.
pub mod urls;
/// Aggregated watch-state keys and status DTOs.
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
    BackdropSize, EpisodeSize, ImageRequest, ImageSize, PosterSize, Priority,
    ProfileSize,
};
pub use image_events::ImageReadyEvent;
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
// Keep scan progress reason DTOs on the crate root so client-facing
// prelude modules can import the public model surface without reaching into
// media event internals.
pub use media_events::{
    MediaEvent, ScanEventMetadata, ScanPathReasonCategory,
    ScanPathReasonDetail, ScanProgressEvent, ScanStageLatencySummary,
};
#[cfg(feature = "rkyv")]
pub use media_id::ArchivedMediaID;
pub use media_id::MediaID;
pub use media_type::ImageMediaType;
pub use media_type::VideoMediaType;
pub use rate_limit::{
    EndpointLimits, RateLimitAlgorithm, RateLimitKey, RateLimitRule,
    TrustedSources,
};
pub use subject_key::{NormalizedPathKey, OpaqueSubjectKey, SubjectKey};
pub use transcoding::{
    ParseTranscodeQualityProfileError, StartTranscodeRequest,
    TranscodeJobState, TranscodeJobStatusResponse, TranscodeQualityProfile,
    TranscodingJobResponse, TranscodingProgressDetails, TranscodingStatus,
};
pub use watch::{
    EpisodeKey, EpisodeStatus, NextEpisode, NextReason, SeasonKey,
    SeasonWatchStatus, SeriesWatchStatus,
};
