use crate::{messages::metadata::Message, state::State};
use iced::Task;

/// Handle metadata domain messages by routing to appropriate handlers
pub fn update_metadata(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::InitializeService => {
            log::info!("Metadata service initialization requested");
            // TODO: Initialize metadata service if needed
            Task::none()
        }
        /*
        Message::TvShowLoaded(show_name, result) => {
            let legacy_msg = Message::TvShowLoaded(show_name, result);
            let legacy_task = update(state, legacy_msg);
            // TvShowLoaded doesn't produce a metadata message in return, discard the result
            legacy_task.discard()
        }
        Message::SeasonLoaded(show_name, season_num, result) => {
            let legacy_msg = Message::SeasonLoaded(show_name, season_num, result);
            let legacy_task = update(state, legacy_msg);
            // SeasonLoaded doesn't produce a metadata message in return, discard the result
            legacy_task.discard()
        } */
        Message::MediaDetailsLoaded(result) => {
            match result {
                Ok(details) => {
                    log::info!("Media details loaded: {} items", details.len());
                    // TODO: Process loaded media details
                    Task::none()
                }
                Err(e) => {
                    log::error!("Failed to load media details: {}", e);
                    Task::none()
                }
            }
        }
        Message::MediaDetailsFetched(media_id, result) => {
            match result {
                Ok(media_ref) => {
                    log::info!("Media details fetched for {:?}", media_id);
                    // Delegate to MediaDetailsUpdated
                    Task::done(Message::MediaDetailsUpdated(media_ref))
                }
                Err(e) => {
                    log::error!("Failed to fetch media details for {:?}: {}", media_id, e);
                    Task::none()
                }
            }
        }
        Message::MetadataUpdated(media_id) => {
            log::info!("Metadata updated for {:?}", media_id);
            // TODO: Trigger UI refresh for affected media
            Task::none()
        }
        Message::MediaOrganized(media_files, tv_shows) => {
            log::info!(
                "Media organized: {} files, {} shows",
                media_files.len(),
                tv_shows.len()
            );
            // TODO: Update state with organized media
            Task::none()
        }
        Message::SeriesSortingCompleted(series_refs) => {
            log::info!("Series sorting completed: {} series", series_refs.len());
            // TODO: Update UI with sorted series
            Task::none()
        }
        Message::MediaEventReceived(event) => {
            log::debug!("Media event received: {:?}", event);
            // TODO: Process media event
            Task::none()
        }
        Message::MediaEventsError(error) => {
            log::error!("Media events error: {}", error);
            Task::none()
        }
        Message::ForceRescan => {
            log::info!("Force rescan requested");
            // TODO: Trigger forced rescan of media library
            Task::none()
        }
        Message::RefreshShowMetadata(series_id) => todo!(),
        Message::RefreshSeasonMetadata(season_id, _) => todo!(),
        Message::RefreshEpisodeMetadata(episode_id) => todo!(),
        Message::ShowMetadataRefreshed(_) => todo!(),
        Message::ShowMetadataRefreshFailed(_, _) => todo!(),
        Message::BatchMetadataComplete => {
            log::info!("Batch metadata processing completed");
            state.loading = false;
            Task::none()
        }
        Message::MediaDetailsUpdated(media_reference) => {
            log::debug!("Single media details updated");
            // Process single media update through the batch handler for consistency
            state
                .handle_media_details_batch(vec![media_reference])
                .discard()
        }
        Message::MediaDetailsBatch(media_references) => {
            log::info!(
                "Processing batch of {} media details",
                media_references.len()
            );
            // Delegate to the state's batch handler which updates MediaStore efficiently
            state
                .handle_media_details_batch(media_references)
                .map(|_| Message::BatchMetadataComplete)
        }
        Message::CheckDetailsFetcherQueue => {
            log::debug!("CheckDetailsFetcherQueue - deprecated with new batch metadata service");
            // This is no longer needed with the new metadata service
            // The service sends MediaDetailsUpdated messages directly
            Task::none()
        }
        Message::ImageLoaded(_, items) => todo!(),
        Message::UnifiedImageLoaded(request, handle) => {
            crate::updates::unified_image::handle_unified_image_loaded(state, request, handle)
        }
        Message::UnifiedImageLoadFailed(request, error) => {
            crate::updates::unified_image::handle_unified_image_load_failed(state, request, error)
        }
        Message::_EmitCrossDomainEvent(_) => {
            // This should be handled by the main update loop, not here
            log::warn!("_EmitCrossDomainEvent should be handled by main update loop");
            Task::none()
        }
        Message::NoOp => Task::none(),
    }
}
