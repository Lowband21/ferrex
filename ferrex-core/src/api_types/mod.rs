//! DTOs exchanged across service boundaries.
//! Modules expose intentional surfaces so downstream crates can depend on
//! specialized namespaces instead of the entire API layer.

pub mod demo;
pub mod filters;
pub mod library;
pub mod media;
pub mod responses;
pub mod scan;
pub mod setup;

pub use demo::{DemoLibraryStatus, DemoResetRequest, DemoStatus};
pub use filters::{
    FilterIndicesRequest, IndicesResponse, LibraryFilters, RATING_DECIMAL_SCALE,
    RATING_SCALE_FACTOR, RatingValue, ScalarRange, rating_value_from_f32, rating_value_to_f32,
};
pub use library::{
    BatchMediaRequest, BatchMediaResponse, CreateLibraryRequest, FetchMediaRequest,
    LibraryMediaCache, LibraryMediaResponse, ManualMatchRequest, UpdateLibraryRequest,
};
pub use media::ImageData;
pub use responses::{ApiResponse, MediaStats, MetadataRequest};
pub use scan::{
    ActiveScansResponse, LatestProgressResponse, ScanCommandAcceptedResponse, ScanCommandRequest,
    ScanLifecycleStatus, ScanSnapshotDto, StartScanRequest,
};

/// Curated exports relied on by the UI/player crates.
pub mod player {
    pub use super::demo::{DemoLibraryStatus, DemoResetRequest, DemoStatus};
    pub use super::library::{
        BatchMediaRequest, BatchMediaResponse, CreateLibraryRequest, FetchMediaRequest,
        LibraryMediaCache, LibraryMediaResponse, ManualMatchRequest, UpdateLibraryRequest,
    };
    pub use super::media::ImageData;
    pub use super::responses::ApiResponse;
    pub use super::scan::{
        ActiveScansResponse, LatestProgressResponse, ScanCommandAcceptedResponse,
        ScanCommandRequest, ScanLifecycleStatus, ScanSnapshotDto, StartScanRequest, events::*,
    };
    pub use super::setup::{
        ConfirmClaimRequest, ConfirmClaimResponse, StartClaimRequest, StartClaimResponse,
    };
    pub use super::{
        FilterIndicesRequest, IndicesResponse, MetadataRequest, RatingValue, ScalarRange,
        rating_value_from_f32, rating_value_to_f32,
    };
}
