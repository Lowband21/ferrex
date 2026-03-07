pub mod image_cache_cleanup_subscription;
pub mod image_events_subscription;
pub mod image_loading_subscription;
pub mod subscription;
pub mod subscriptions;

use ferrex_core::player_prelude::{ImageRequest, MediaID};
use ferrex_model::{Media, Series};

#[derive(Clone)]
pub enum MetadataMessage {
    // Metadata service
    InitializeService,

    MediaDetailsLoaded(Result<Vec<Media>, String>), // Full media details loaded
    SeriesSortingCompleted(Vec<Series>), // Series sorted in background

    // Force rescan
    ForceRescan,

    // Image loading
    UnifiedImageLoaded(ImageRequest, iced::widget::image::Handle, u64),
    UnifiedImageLoadFailed(ImageRequest, String),
    UnifiedImageCancelled(ImageRequest),
    ImageBlobReady(ImageRequest, String),

    NoOp,
}

impl MetadataMessage {
    pub fn name(&self) -> &'static str {
        match self {
            MetadataMessage::InitializeService => "Metadata::InitializeService",
            MetadataMessage::MediaDetailsLoaded(_) => {
                "Metadata::MediaDetailsLoaded"
            }
            MetadataMessage::UnifiedImageLoaded(_, _, _) => {
                "Metadata::UnifiedImageLoaded"
            }
            MetadataMessage::UnifiedImageLoadFailed(_, _) => {
                "Metadata::UnifiedImageLoadFailed"
            }
            MetadataMessage::UnifiedImageCancelled(_) => {
                "Metadata::UnifiedImageCancelled"
            }
            MetadataMessage::ImageBlobReady(_, _) => "Metadata::ImageBlobReady",
            MetadataMessage::SeriesSortingCompleted(_) => {
                "Metadata::SeriesSortingCompleted"
            }
            MetadataMessage::ForceRescan => "Metadata::ForceRescan",
            MetadataMessage::NoOp => "Metadata::NoOp",
        }
    }
}

impl std::fmt::Debug for MetadataMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitializeService => write!(f, "Metadata::InitializeService"),
            Self::MediaDetailsLoaded(result) => match result {
                Ok(refs) => write!(
                    f,
                    "Metadata::MediaDetailsLoaded(Ok: {} items)",
                    refs.len()
                ),
                Err(e) => write!(f, "Metadata::MediaDetailsLoaded(Err: {})", e),
            },
            Self::UnifiedImageLoaded(req, _, estimated_bytes) => write!(
                f,
                "Metadata::UnifiedImageLoaded({:?}, <handle>, {} bytes est.)",
                req, estimated_bytes
            ),
            Self::UnifiedImageLoadFailed(req, err) => f
                .debug_tuple("Metadata::UnifiedImageLoadFailed")
                .field(req)
                .field(err)
                .finish(),
            Self::UnifiedImageCancelled(req) => f
                .debug_tuple("Metadata::UnifiedImageCancelled")
                .field(req)
                .finish(),
            Self::ImageBlobReady(req, token) => f
                .debug_tuple("Metadata::ImageBlobReady")
                .field(req)
                .field(token)
                .finish(),
            Self::SeriesSortingCompleted(series) => write!(
                f,
                "Metadata::SeriesSortingCompleted({} series)",
                series.len()
            ),
            Self::ForceRescan => write!(f, "Metadata::ForceRescan"),
            Self::NoOp => write!(f, "Metadata::NoOp"),
        }
    }
}

/// Metadata domain events
#[derive(Clone, Debug)]
pub enum MetadataEvent {
    MetadataUpdated(MediaID),
    BatchMetadataReady(Vec<Media>),
    ImageReady(String, iced::widget::image::Handle),
}
