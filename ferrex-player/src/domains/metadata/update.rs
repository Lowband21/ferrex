use super::messages::MetadataMessage;
use crate::common::messages::{DomainMessage, DomainUpdateResult};
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
    message: MetadataMessage,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infra::profiling_scopes::scopes::METADATA_UPDATE);

    match message {
        MetadataMessage::InitializeService => {
            log::info!("Metadata service initialization requested");
            // TODO: Initialize metadata service if needed
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        MetadataMessage::MediaDetailsLoaded(result) => {
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
        MetadataMessage::SeriesSortingCompleted(series_refs) => {
            log::info!(
                "Series sorting completed: {} series",
                series_refs.len()
            );
            // TODO: Update UI with sorted series
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        MetadataMessage::ForceRescan => {
            log::info!("Force rescan requested");
            // TODO: Trigger forced rescan of media library
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        MetadataMessage::UnifiedImageLoaded(
            request,
            handle,
            estimated_bytes,
        ) => {
            let meta_task = crate::domains::metadata::update_handlers::unified_image::handle_unified_image_loaded(state, request, handle, estimated_bytes)
                .map(DomainMessage::Metadata);
            // Nudge UI to render promptly to avoid coalesced updates
            let ui_nudge = Task::done(DomainMessage::Ui(
                crate::domains::ui::background_ui::BackgroundMessage::UpdateTransitions
                    .into(),
            ));
            DomainUpdateResult::task(Task::batch(vec![meta_task, ui_nudge]))
        }
        MetadataMessage::UnifiedImageLoadFailed(request, error) => {
            let task = crate::domains::metadata::update_handlers::unified_image::handle_unified_image_load_failed(state, request, error);
            DomainUpdateResult::task(task.map(DomainMessage::Metadata))
        }
        MetadataMessage::UnifiedImageCancelled(request) => {
            let task = crate::domains::metadata::update_handlers::unified_image::handle_unified_image_cancelled(state, request);
            DomainUpdateResult::task(task.map(DomainMessage::Metadata))
        }
        MetadataMessage::ImageBlobReady(request, token) => {
            state.image_service.set_ready_token(&request, token);
            state.image_service.request_image(request);
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
        MetadataMessage::NoOp => {
            DomainUpdateResult::task(Task::none().map(DomainMessage::Metadata))
        }
    }
}
