use super::messages::Message;
use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::state_refactored::State;
use iced::Task;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update_metadata(state: &mut State, message: Message) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infrastructure::profiling_scopes::scopes::METADATA_UPDATE);

    match message {
        Message::InitializeService => {
            log::info!("Metadata service initialization requested");
            // TODO: Initialize metadata service if needed
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
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
                    DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
                }
                Err(e) => {
                    log::error!("Failed to load media details: {}", e);
                    DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
                }
            }
        }
        //Message::MediaDetailsFetched(media_id, result) => {
        //    match result {
        //        Ok(media_ref) => {
        //            log::info!("Media details fetched for {:?}", media_id);
        //            // Delegate to MediaDetailsUpdated
        //            DomainUpdateResult::task(
        //                Task::done(Message::MediaDetailsUpdated(media_ref))
        //                    .map(DomainMessage::Metadata),
        //            )
        //        }
        //        Err(e) => {
        //            log::error!("Failed to fetch media details for {:?}: {}", media_id, e);
        //            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        //        }
        //    }
        //}
        //Message::MetadataUpdated(media_id) => {
        //    log::info!("Metadata updated for {:?}", media_id);
        //    // TODO: Trigger UI refresh for affected media
        //    DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        //}
        //Message::MediaOrganized(media_files, tv_shows) => {
        //    log::info!(
        //        "Media organized: {} files, {} shows",
        //        media_files.len(),
        //        tv_shows.len()
        //    );
        //    // TODO: Update state with organized media
        //    DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        //}
        Message::SeriesSortingCompleted(series_refs) => {
            log::info!("Series sorting completed: {} series", series_refs.len());
            // TODO: Update UI with sorted series
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        Message::MediaEventReceived(event) => {
            log::debug!("Media event received: {:?}", event);
            // TODO: Process media event
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        Message::MediaEventsError(error) => {
            log::error!("Media events error: {}", error);
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        Message::ForceRescan => {
            log::info!("Force rescan requested");
            // TODO: Trigger forced rescan of media library
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        Message::RefreshShowMetadata(series_id) => todo!(),
        Message::RefreshSeasonMetadata(season_id, _) => todo!(),
        Message::RefreshEpisodeMetadata(episode_id) => todo!(),
        Message::ShowMetadataRefreshed(_) => todo!(),
        Message::ShowMetadataRefreshFailed(_, _) => todo!(),
        Message::BatchMetadataComplete => {
            log::info!("Batch metadata processing completed");
            state.loading = false;
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        //Message::MediaDetailsUpdated(media_reference) => {
        //    log::debug!("Single media details updated");
        //    // Process single media update through the batch handler for consistency
        //    DomainUpdateResult::task(
        //        state
        //            .handle_media_details_batch(vec![media_reference])
        //            .discard()
        //            .map(DomainMessage::Metadata),
        //    )
        //}
        //Message::MediaDetailsBatch(media_references) => {
        //    log::info!(
        //        "Processing batch of {} media details",
        //        media_references.len()
        //    );
        //    // Delegate to the state's batch handler which updates MediaStore efficiently
        //    DomainUpdateResult::task(
        //        state
        //            .handle_media_details_batch(media_references)
        //            .map(|_| Message::BatchMetadataComplete)
        //            .map(DomainMessage::Metadata),
        //    )
        //}
        Message::CheckDetailsFetcherQueue => {
            log::debug!("CheckDetailsFetcherQueue - deprecated with new batch metadata service");
            // This is no longer needed with the new metadata service
            // The service sends MediaDetailsUpdated messages directly
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        //Message::FetchBatchMetadata(libraries_data) => {
        //    log::info!(
        //        "Fetching batch metadata for {} libraries",
        //        libraries_data.len()
        //    );

        //    // Execute the batch metadata fetcher directly on background thread
        //    if let Some(fetcher) = &state.batch_metadata_fetcher {
        //        let fetcher_clone = std::sync::Arc::clone(fetcher);

        //        // Spawn the metadata fetching directly - no Iced tasks
        //        tokio::spawn(async move {
        //            log::info!("[Metadata] Starting batch metadata fetch");
        //            // Process libraries will now emit events directly, not return tasks
        //            fetcher_clone
        //                .process_libraries_with_verification(libraries_data)
        //                .await;
        //            log::info!("[Metadata] Batch metadata fetch initiated");
        //        });

        //        // Return immediately - processing happens in background
        //        DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        //    } else {
        //        log::error!("BatchMetadataFetcher not initialized");
        //        DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        //    }
        //}
        Message::ImageLoaded(_, items) => todo!(),
        Message::UnifiedImageLoaded(request, handle) => {
            let task = crate::domains::metadata::update_handlers::unified_image::handle_unified_image_loaded(state, request, handle);
            DomainUpdateResult::task(task.map(DomainMessage::Metadata))
        }
        Message::UnifiedImageLoadFailed(request, error) => {
            let task = crate::domains::metadata::update_handlers::unified_image::handle_unified_image_load_failed(state, request, error);
            DomainUpdateResult::task(task.map(DomainMessage::Metadata))
        }
        Message::NoOp => DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata)),
    }
}
