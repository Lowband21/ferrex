pub mod image_cache_cleanup_subscription;
pub mod image_loading_subscription;
pub mod subscription;
pub mod subscriptions;

use crate::infrastructure::api_types::{Media, SeriesReference};
use ferrex_core::player_prelude::{EpisodeID, ImageRequest, MediaID, SeasonID, SeriesID};

#[derive(Clone)]
pub enum Message {
    // Metadata service
    InitializeService,

    RefreshShowMetadata(SeriesID), // Refresh metadata for all episodes in a show
    RefreshSeasonMetadata(SeasonID, u32), // Refresh metadata for all episodes in a season
    RefreshEpisodeMetadata(EpisodeID), // Refresh metadata for a single episode
    ShowMetadataRefreshed(String), // show_name
    ShowMetadataRefreshFailed(String, String), // show_name, error

    // Batch operations
    BatchMetadataComplete,
    CheckDetailsFetcherQueue, // Check if background fetcher has completed items
    MediaDetailsLoaded(Result<Vec<Media>, String>), // Full media details loaded
    SeriesSortingCompleted(Vec<SeriesReference>), // Series sorted in background

    // Force rescan
    ForceRescan,

    // Image loading
    ImageLoaded(String, Result<Vec<u8>, String>), // cache_key, result
    UnifiedImageLoaded(ImageRequest, iced::widget::image::Handle),
    UnifiedImageLoadFailed(ImageRequest, String),

    NoOp,
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            Message::InitializeService => "Metadata::InitializeService",
            /*
            Message::FetchMetadata(_) => "Metadata::FetchMetadata",
            Message::MetadataFetched(_, _) => "Metadata::MetadataFetched",
            Message::TvShowLoaded(_, _) => "Metadata::TvShowLoaded",
            Message::SeasonLoaded(_, _, _) => "Metadata::SeasonLoaded",
             */
            Message::RefreshShowMetadata(_) => "Metadata::RefreshShowMetadata",
            Message::RefreshSeasonMetadata(_, _) => "Metadata::RefreshSeasonMetadata",
            Message::RefreshEpisodeMetadata(_) => "Metadata::RefreshEpisodeMetadata",
            Message::ShowMetadataRefreshed(_) => "Metadata::ShowMetadataRefreshed",
            Message::ShowMetadataRefreshFailed(_, _) => "Metadata::ShowMetadataRefreshFailed",
            Message::BatchMetadataComplete => "Metadata::BatchMetadataComplete",
            //Message::MediaDetailsUpdated(_) => "Metadata::MediaDetailsUpdated",
            //Message::MediaDetailsBatch(_) => "Metadata::MediaDetailsBatch",
            Message::CheckDetailsFetcherQueue => "Metadata::CheckDetailsFetcherQueue",
            Message::MediaDetailsLoaded(_) => "Metadata::MediaDetailsLoaded",
            //Message::MediaDetailsFetched(_, _) => "Metadata::MediaDetailsFetched",
            //Message::MetadataUpdated(_) => "Metadata::MetadataUpdated",
            Message::ImageLoaded(_, _) => "Metadata::ImageLoaded",
            Message::UnifiedImageLoaded(_, _) => "Metadata::UnifiedImageLoaded",
            Message::UnifiedImageLoadFailed(_, _) => "Metadata::UnifiedImageLoadFailed",
            //Message::MediaOrganized(_, _) => "Metadata::MediaOrganized",
            Message::SeriesSortingCompleted(_) => "Metadata::SeriesSortingCompleted",
            Message::ForceRescan => "Metadata::ForceRescan",
            //Message::FetchBatchMetadata(_) => "Metadata::FetchBatchMetadata",
            Message::NoOp => "Metadata::NoOp",
        }
    }
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitializeService => write!(f, "Metadata::InitializeService"),
            Self::RefreshShowMetadata(id) => f
                .debug_tuple("Metadata::RefreshShowMetadata")
                .field(id)
                .finish(),
            Self::RefreshSeasonMetadata(id, num) => f
                .debug_tuple("Metadata::RefreshSeasonMetadata")
                .field(id)
                .field(num)
                .finish(),
            Self::RefreshEpisodeMetadata(id) => f
                .debug_tuple("Metadata::RefreshEpisodeMetadata")
                .field(id)
                .finish(),
            Self::ShowMetadataRefreshed(name) => f
                .debug_tuple("Metadata::ShowMetadataRefreshed")
                .field(name)
                .finish(),
            Self::ShowMetadataRefreshFailed(name, err) => f
                .debug_tuple("Metadata::ShowMetadataRefreshFailed")
                .field(name)
                .field(err)
                .finish(),
            Self::BatchMetadataComplete => write!(f, "Metadata::BatchMetadataComplete"),
            //Self::MediaDetailsUpdated(media) => {
            //    // Show summary instead of full details
            //    match media {
            //        Media::Movie(m) => write!(
            //            f,
            //            "Metadata::MediaDetailsUpdated(Movie: {})",
            //            m.title.as_str()
            //        ),
            //        Media::Series(s) => write!(
            //            f,
            //            "Metadata::MediaDetailsUpdated(Series: {})",
            //            s.title.as_str()
            //        ),
            //        Media::Season(s) => write!(
            //            f,
            //            "Metadata::MediaDetailsUpdated(Season: {})",
            //            s.id.as_str()
            //        ),
            //        Media::Episode(e) => write!(
            //            f,
            //            "Metadata::MediaDetailsUpdated(Episode: S{:02}E{:02})",
            //            e.season_number.value(),
            //            e.episode_number.value()
            //        ),
            //    }
            //}
            //Self::MediaDetailsBatch(batch) => {
            //    write!(f, "Metadata::MediaDetailsBatch({} items)", batch.len())
            //}
            Self::CheckDetailsFetcherQueue => write!(f, "Metadata::CheckDetailsFetcherQueue"),
            Self::MediaDetailsLoaded(result) => match result {
                Ok(refs) => write!(f, "Metadata::MediaDetailsLoaded(Ok: {} items)", refs.len()),
                Err(e) => write!(f, "Metadata::MediaDetailsLoaded(Err: {})", e),
            },
            //Self::MediaDetailsFetched(id, result) => match result {
            //    Ok(media) => match media {
            //        Media::Movie(m) => write!(
            //            f,
            //            "Metadata::MediaDetailsFetched({}, Ok: Movie {})",
            //            id,
            //            m.title.as_str()
            //        ),
            //        Media::Series(s) => write!(
            //            f,
            //            "Metadata::MediaDetailsFetched({}, Ok: Series {})",
            //            id,
            //            s.title.as_str()
            //        ),
            //        Media::Season(s) => write!(
            //            f,
            //            "Metadata::MediaDetailsFetched({}, Ok: Season {})",
            //            id,
            //            s.id.as_str()
            //        ),
            //        Media::Episode(e) => write!(
            //            f,
            //            "Metadata::MediaDetailsFetched({}, Ok: Episode S{:02}E{:02})",
            //            id,
            //            e.season_number.value(),
            //            e.episode_number.value()
            //        ),
            //    },
            //    Err(e) => write!(f, "Metadata::MediaDetailsFetched({}, Err: {})", id, e),
            //},
            //Self::MetadataUpdated(media_id) => write!(f, "Metadata::MetadataUpdated({})", media_id),
            Self::ImageLoaded(cache_key, result) => match result {
                Ok(data) => write!(
                    f,
                    "Metadata::ImageLoaded({}, Ok: {} bytes)",
                    cache_key,
                    data.len()
                ),
                Err(e) => write!(f, "Metadata::ImageLoaded({}, Err: {})", cache_key, e),
            },
            Self::UnifiedImageLoaded(req, _) => {
                write!(f, "Metadata::UnifiedImageLoaded({:?}, <handle>)", req)
            }
            Self::UnifiedImageLoadFailed(req, err) => f
                .debug_tuple("Metadata::UnifiedImageLoadFailed")
                .field(req)
                .field(err)
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
