pub mod image_cache_cleanup_subscription;
pub mod image_loading_subscription;
pub mod subscription;
pub mod subscriptions;

use crate::api_types::{MediaReference, SeriesReference};
use crate::media_library::MediaFile;
use crate::models::{SeasonDetails, TvShow, TvShowDetails};
use crate::MediaEvent;
use ferrex_core::{EpisodeID, MediaId, SeasonID, SeriesID};
use std::collections::HashMap;

#[derive(Clone)]
pub enum Message {
    // Metadata service
    InitializeService,

    // Metadata fetching
    //FetchMetadata(MediaId), // Fetch metadata for media_id
    //MetadataFetched(MediaId, Result<(), String>), // media_id, result

    // TV show metadata
    //TvShowLoaded(String, Result<TvShowDetails, String>), // show_name, result
    //SeasonLoaded(String, u32, Result<SeasonDetails, String>), // show_name, season_num, result
    RefreshShowMetadata(SeriesID), // Refresh metadata for all episodes in a show
    RefreshSeasonMetadata(SeasonID, u32), // Refresh metadata for all episodes in a season
    RefreshEpisodeMetadata(EpisodeID), // Refresh metadata for a single episode
    ShowMetadataRefreshed(String), // show_name
    ShowMetadataRefreshFailed(String, String), // show_name, error

    // Batch operations
    BatchMetadataComplete,
    MediaDetailsUpdated(MediaReference), // Full details fetched for a media item
    MediaDetailsBatch(Vec<MediaReference>), // Batch of media details for efficient processing
    CheckDetailsFetcherQueue,            // Check if background fetcher has completed items
    MediaDetailsLoaded(Result<Vec<MediaReference>, String>), // Full media details loaded
    MediaDetailsFetched(MediaId, Result<MediaReference, String>), // Single media detail fetched
    MetadataUpdated(MediaId),            // Generic metadata update notification

    // Background organization
    MediaOrganized(Vec<MediaFile>, HashMap<String, TvShow>), // Media organized by show
    SeriesSortingCompleted(Vec<SeriesReference>),            // Series sorted in background

    // Media events from server
    MediaEventReceived(MediaEvent),
    MediaEventsError(String),

    // Force rescan
    ForceRescan,

    // Image loading
    ImageLoaded(String, Result<Vec<u8>, String>), // cache_key, result
    UnifiedImageLoaded(
        crate::image_types::ImageRequest,
        iced::widget::image::Handle,
    ),
    UnifiedImageLoadFailed(crate::image_types::ImageRequest, String),

    // Internal cross-domain coordination
    #[doc(hidden)]
    _EmitCrossDomainEvent(crate::messages::CrossDomainEvent),

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
            Message::MediaDetailsUpdated(_) => "Metadata::MediaDetailsUpdated",
            Message::MediaDetailsBatch(_) => "Metadata::MediaDetailsBatch",
            Message::CheckDetailsFetcherQueue => "Metadata::CheckDetailsFetcherQueue",
            Message::MediaDetailsLoaded(_) => "Metadata::MediaDetailsLoaded",
            Message::MediaDetailsFetched(_, _) => "Metadata::MediaDetailsFetched",
            Message::MetadataUpdated(_) => "Metadata::MetadataUpdated",
            Message::ImageLoaded(_, _) => "Metadata::ImageLoaded",
            Message::UnifiedImageLoaded(_, _) => "Metadata::UnifiedImageLoaded",
            Message::UnifiedImageLoadFailed(_, _) => "Metadata::UnifiedImageLoadFailed",
            Message::MediaOrganized(_, _) => "Metadata::MediaOrganized",
            Message::SeriesSortingCompleted(_) => "Metadata::SeriesSortingCompleted",
            Message::MediaEventReceived(_) => "Metadata::MediaEventReceived",
            Message::MediaEventsError(_) => "Metadata::MediaEventsError",
            Message::ForceRescan => "Metadata::ForceRescan",
            Message::_EmitCrossDomainEvent(_) => "Metadata::_EmitCrossDomainEvent",
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
            Self::MediaDetailsUpdated(media) => {
                // Show summary instead of full details
                match media {
                    MediaReference::Movie(m) => write!(
                        f,
                        "Metadata::MediaDetailsUpdated(Movie: {})",
                        m.title.as_str()
                    ),
                    MediaReference::Series(s) => write!(
                        f,
                        "Metadata::MediaDetailsUpdated(Series: {})",
                        s.title.as_str()
                    ),
                    MediaReference::Season(s) => write!(
                        f,
                        "Metadata::MediaDetailsUpdated(Season: {})",
                        s.id.as_str()
                    ),
                    MediaReference::Episode(e) => write!(
                        f,
                        "Library::MediaDetailsUpdated(Series ID: {}, Episode: S{:02}E{:02})",
                        e.series_id,
                        e.season_number.value(),
                        e.episode_number.value()
                    ),
                }
            }
            Self::MediaDetailsBatch(batch) => {
                // Show count instead of full vector contents
                write!(f, "Metadata::MediaDetailsBatch({} items)", batch.len())
            }
            Self::CheckDetailsFetcherQueue => write!(f, "Metadata::CheckDetailsFetcherQueue"),
            Self::MediaDetailsLoaded(result) => match result {
                Ok(refs) => write!(f, "Metadata::MediaDetailsLoaded(Ok: {} items)", refs.len()),
                Err(e) => write!(f, "Metadata::MediaDetailsLoaded(Err: {})", e),
            },
            Self::MediaDetailsFetched(id, result) => match result {
                Ok(media) => match media {
                    MediaReference::Movie(m) => write!(
                        f,
                        "Metadata::MediaDetailsFetched({}, Ok: Movie {})",
                        id,
                        m.title.as_str()
                    ),
                    MediaReference::Series(s) => write!(
                        f,
                        "Metadata::MediaDetailsFetched({}, Ok: Series {})",
                        id,
                        s.title.as_str()
                    ),
                    MediaReference::Season(s) => write!(
                        f,
                        "Metadata::MediaDetailsFetched({}, Ok: Season {})",
                        id,
                        s.id.as_str()
                    ),
                    MediaReference::Episode(e) => write!(
                        f,
                        "Library::MediaDetailsFetched(Series ID: {}, Episode: S{:02}E{:02})",
                        e.series_id,
                        e.season_number.value(),
                        e.episode_number.value()
                    ),
                },
                Err(e) => write!(f, "Metadata::MediaDetailsFetched({}, Err: {})", id, e),
            },
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
            Self::MediaOrganized(files, shows) => write!(
                f,
                "Metadata::MediaOrganized({} files, {} shows)",
                files.len(),
                shows.len()
            ),
            Self::SeriesSortingCompleted(series) => write!(
                f,
                "Metadata::SeriesSortingCompleted({} series)",
                series.len()
            ),
            Self::MediaEventReceived(_) => write!(f, "Metadata::MediaEventReceived(<event>)"),
            Self::MediaEventsError(err) => write!(f, "Metadata::MediaEventsError({})", err),
            Self::ForceRescan => write!(f, "Metadata::ForceRescan"),
            Self::_EmitCrossDomainEvent(event) => {
                write!(f, "Metadata::_EmitCrossDomainEvent({:?})", event)
            }
            Self::NoOp => write!(f, "Metadata::NoOp"),
            Message::InitializeService => todo!(),
            Message::RefreshShowMetadata(series_id) => todo!(),
            Message::RefreshSeasonMetadata(season_id, _) => todo!(),
            Message::RefreshEpisodeMetadata(episode_id) => todo!(),
            Message::ShowMetadataRefreshed(_) => todo!(),
            Message::ShowMetadataRefreshFailed(_, _) => todo!(),
            Message::BatchMetadataComplete => todo!(),
            Message::MediaDetailsUpdated(media_reference) => todo!(),
            Message::MediaDetailsBatch(media_references) => todo!(),
            Message::CheckDetailsFetcherQueue => todo!(),
            Message::MediaDetailsLoaded(media_references) => todo!(),
            Message::MediaDetailsFetched(media_id, media_reference) => todo!(),
            Message::MetadataUpdated(media_id) => todo!(),
            Message::MediaOrganized(media_files, hash_map) => todo!(),
            Message::SeriesSortingCompleted(series_references) => todo!(),
            Message::MediaEventReceived(media_event) => todo!(),
            Message::MediaEventsError(_) => todo!(),
            Message::ForceRescan => todo!(),
            Message::ImageLoaded(_, items) => todo!(),
            Message::UnifiedImageLoaded(image_request, handle) => todo!(),
            Message::UnifiedImageLoadFailed(image_request, _) => todo!(),
            Message::_EmitCrossDomainEvent(cross_domain_event) => todo!(),
            Message::NoOp => todo!(),
        }
    }
}

/// Metadata domain events
#[derive(Clone, Debug)]
pub enum MetadataEvent {
    MetadataUpdated(MediaId),
    BatchMetadataReady(Vec<MediaReference>),
    ImageReady(String, iced::widget::image::Handle),
}
