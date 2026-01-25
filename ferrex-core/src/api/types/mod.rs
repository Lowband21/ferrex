//! DTOs exchanged across service boundaries.
//! Modules expose intentional surfaces so downstream crates can depend on
//! specialized namespaces instead of the entire API layer.

pub mod admin;
pub mod demo;
pub mod filters;
pub mod library;
pub mod media;
pub mod media_repo_sync;
pub mod responses;
pub mod scan;
pub mod setup;
pub mod users_admin;

pub use admin::{
    MediaRootBreadcrumb, MediaRootBrowseRequest, MediaRootBrowseResponse,
    MediaRootEntry, MediaRootEntryKind,
};
pub use demo::{DemoLibraryStatus, DemoResetRequest, DemoStatus};
pub use filters::{
    FilterIndicesRequest, IndicesResponse, LibraryFilters,
    RATING_DECIMAL_SCALE, RATING_SCALE_FACTOR, RatingValue, ScalarRange,
    rating_value_from_f32, rating_value_to_f32,
};
pub use library::{
    BatchMediaRequest, BatchMediaResponse, CreateLibraryRequest,
    FetchMediaRequest, LibraryMediaCache, LibraryMediaResponse,
    ManualMatchRequest, MovieReferenceBatchBlob,
    MovieReferenceBatchBundleResponse, MovieReferenceBatchResponse,
    SeriesBundleBlob, SeriesBundleBundleResponse, SeriesBundleResponse,
    UpdateLibraryRequest,
};
pub use media::{
    ImageData, ImageManifestItem, ImageManifestRequest, ImageManifestResponse,
    ImageManifestResult, ImageManifestStatus,
};
pub use media_repo_sync::{
    MovieBatchFetchRequest, MovieBatchSyncRequest, MovieBatchSyncResponse,
    MovieBatchVersionManifestEntry, SeriesBundleFetchRequest,
    SeriesBundleSyncRequest, SeriesBundleSyncResponse,
    SeriesBundleVersionManifestEntry,
};
pub use responses::{ApiResponse, MediaStats, MetadataRequest};
pub use scan::{
    ActiveScansResponse, LatestProgressResponse, ScanCommandAcceptedResponse,
    ScanCommandRequest, ScanLifecycleStatus, ScanSnapshotDto, StartScanRequest,
};
pub use users_admin::{AdminUserInfo, CreateUserRequest, UpdateUserRequest};

/// Curated exports relied on by the UI/player crates.
pub mod player {
    pub use super::admin::{
        MediaRootBreadcrumb, MediaRootBrowseRequest, MediaRootBrowseResponse,
        MediaRootEntry, MediaRootEntryKind,
    };
    pub use super::demo::{DemoLibraryStatus, DemoResetRequest, DemoStatus};
    pub use super::library::{
        BatchMediaRequest, BatchMediaResponse, CreateLibraryRequest,
        FetchMediaRequest, LibraryMediaCache, LibraryMediaResponse,
        ManualMatchRequest, MovieReferenceBatchBlob,
        MovieReferenceBatchBundleResponse, MovieReferenceBatchResponse,
        SeriesBundleBlob, SeriesBundleBundleResponse, SeriesBundleResponse,
        UpdateLibraryRequest,
    };
    pub use super::media::{
        ImageData, ImageManifestItem, ImageManifestRequest,
        ImageManifestResponse, ImageManifestResult, ImageManifestStatus,
    };
    pub use super::media_repo_sync::{
        MovieBatchFetchRequest, MovieBatchSyncRequest, MovieBatchSyncResponse,
        MovieBatchVersionManifestEntry, SeriesBundleFetchRequest,
        SeriesBundleSyncRequest, SeriesBundleSyncResponse,
        SeriesBundleVersionManifestEntry,
    };
    pub use super::responses::ApiResponse;
    pub use super::scan::{
        ActiveScansResponse, LatestProgressResponse,
        ScanCommandAcceptedResponse, ScanCommandRequest, ScanLifecycleStatus,
        ScanSnapshotDto, StartScanRequest, events::*,
    };
    pub use super::setup::{
        ConfirmClaimRequest, ConfirmClaimResponse, StartClaimRequest,
        StartClaimResponse,
    };
    pub use super::users_admin::{
        AdminUserInfo, CreateUserRequest, UpdateUserRequest,
    };
    pub use super::{
        FilterIndicesRequest, IndicesResponse, MetadataRequest, RatingValue,
        ScalarRange, rating_value_from_f32, rating_value_to_f32,
    };
}
