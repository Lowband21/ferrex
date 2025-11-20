use super::messages::Message;
use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::domains::ui::messages as ui;
use crate::state::State;
use iced::Task;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update_metadata(
    state: &mut State,
    message: Message,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infra::profiling_scopes::scopes::METADATA_UPDATE);

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
                    DomainUpdateResult::task(
                        Task::none().map(DomainMessage::Metadata),
                    )
                }
                Err(e) => {
                    log::error!("Failed to load media details: {}", e);
                    DomainUpdateResult::task(
                        Task::none().map(DomainMessage::Metadata),
                    )
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
            log::info!(
                "Series sorting completed: {} series",
                series_refs.len()
            );
            // TODO: Update UI with sorted series
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        Message::ForceRescan => {
            log::info!("Force rescan requested");
            // TODO: Trigger forced rescan of media library
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        Message::ImageLoaded(_, items) => todo!(),
        Message::UnifiedImageLoaded(request, handle) => {
            let meta_task = crate::domains::metadata::update_handlers::unified_image::handle_unified_image_loaded(state, request, handle)
                .map(DomainMessage::Metadata);
            // Nudge UI to render promptly to avoid coalesced updates
            let ui_nudge =
                Task::done(DomainMessage::Ui(ui::Message::UpdateTransitions));
            DomainUpdateResult::task(Task::batch(vec![meta_task, ui_nudge]))
        }
        Message::UnifiedImageLoadFailed(request, error) => {
            let task = crate::domains::metadata::update_handlers::unified_image::handle_unified_image_load_failed(state, request, error);
            DomainUpdateResult::task(task.map(DomainMessage::Metadata))
        }
        Message::UnifiedImageCancelled(request) => {
            let task = crate::domains::metadata::update_handlers::unified_image::handle_unified_image_cancelled(state, request);
            DomainUpdateResult::task(task.map(DomainMessage::Metadata))
        }
        Message::NoOp => {
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
    }
}
