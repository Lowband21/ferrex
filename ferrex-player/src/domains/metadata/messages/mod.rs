pub mod image_cache_cleanup_subscription;
pub mod image_loading_subscription;
pub mod subscription;
pub mod subscriptions;

use ferrex_core::player_prelude::{ImageRequest, MediaID};
use ferrex_model::{Media, SeriesReference};

#[derive(Clone)]
pub enum MetadataMessage {
    // Metadata service
    InitializeService,

    MediaDetailsLoaded(Result<Vec<Media>, String>), // Full media details loaded
    SeriesSortingCompleted(Vec<SeriesReference>), // Series sorted in background

    // Force rescan
    ForceRescan,

    // Image loading
    ImageLoaded(String, Result<Vec<u8>, String>), // cache_key, result
    UnifiedImageLoaded(ImageRequest, iced::widget::image::Handle),
    UnifiedImageLoadFailed(ImageRequest, String),
    UnifiedImageCancelled(ImageRequest),

    NoOp,
}

impl MetadataMessage {
    pub fn name(&self) -> &'static str {
        match self {
            MetadataMessage::InitializeService => "Metadata::InitializeService",
            MetadataMessage::MediaDetailsLoaded(_) => {
                "Metadata::MediaDetailsLoaded"
            }
            //Message::MediaDetailsFetched(_, _) => "Metadata::MediaDetailsFetched",
            //Message::MetadataUpdated(_) => "Metadata::MetadataUpdated",
            MetadataMessage::ImageLoaded(_, _) => "Metadata::ImageLoaded",
            MetadataMessage::UnifiedImageLoaded(_, _) => {
                "Metadata::UnifiedImageLoaded"
            }
            MetadataMessage::UnifiedImageLoadFailed(_, _) => {
                "Metadata::UnifiedImageLoadFailed"
            }
            MetadataMessage::UnifiedImageCancelled(_) => {
                "Metadata::UnifiedImageCancelled"
            }
            //Message::MediaOrganized(_, _) => "Metadata::MediaOrganized",
            MetadataMessage::SeriesSortingCompleted(_) => {
                "Metadata::SeriesSortingCompleted"
            }
            MetadataMessage::ForceRescan => "Metadata::ForceRescan",
            //Message::FetchBatchMetadata(_) => "Metadata::FetchBatchMetadata",
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
            Self::ImageLoaded(cache_key, result) => match result {
                Ok(data) => write!(
                    f,
                    "Metadata::ImageLoaded({}, Ok: {} bytes)",
                    cache_key,
                    data.len()
                ),
                Err(e) => write!(
                    f,
                    "Metadata::ImageLoaded({}, Err: {})",
                    cache_key, e
                ),
            },
            Self::UnifiedImageLoaded(req, _) => {
                write!(f, "Metadata::UnifiedImageLoaded({:?}, <handle>)", req)
            }
            Self::UnifiedImageLoadFailed(req, err) => f
                .debug_tuple("Metadata::UnifiedImageLoadFailed")
                .field(req)
                .field(err)
                .finish(),
            Self::UnifiedImageCancelled(req) => f
                .debug_tuple("Metadata::UnifiedImageCancelled")
                .field(req)
                .finish(),
            //Self::MediaOrganized(files, shows) => write!(
            //    f,
            //    "Metadata::MediaOrganized({} files, {} shows)",
            //    files.len(),
            //    shows.len()
            //),
            Self::SeriesSortingCompleted(series) => write!(
                f,
                "Metadata::SeriesSortingCompleted({} series)",
                series.len()
            ),
            Self::ForceRescan => write!(f, "Metadata::ForceRescan"),
            //Self::FetchBatchMetadata(libraries_data) => write!(
            //    f,
            //    "Metadata::FetchBatchMetadata({} libraries)",
            //    libraries_data.len()
            //),
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
