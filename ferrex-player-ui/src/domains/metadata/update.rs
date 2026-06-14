use super::{
    image_service::UnifiedImageService,
    messages::MetadataMessage,
    update_handlers::unified_image::{
        UnifiedImageContext, handle_unified_image_cancelled,
        handle_unified_image_load_failed, handle_unified_image_loaded,
    },
};
use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::ui::background_ui::BackgroundMessage,
    state::State,
};
use ferrex_player_foundation::domain::DomainUpdateResult as FoundationUpdateResult;
use iced::Task;
use std::{sync::Arc, time::Instant};

/// App-shell hooks required by metadata update handlers.
pub trait MetadataUpdateContext: UnifiedImageContext {
    type AppMessage: Send + 'static;

    fn metadata_message(message: MetadataMessage) -> Self::AppMessage;
    fn image_service(&self) -> &Arc<UnifiedImageService>;
    fn nudge_image_transition(&self) -> Option<Self::AppMessage>;
}

impl UnifiedImageContext for State {
    fn image_service(&self) -> &Arc<UnifiedImageService> {
        &self.domains.metadata.state.image_service
    }

    fn extend_poster_animation_until(&mut self, until: Instant) {
        let active_until = &mut self.domains.ui.state.poster_anim_active_until;
        *active_until = Some(
            active_until
                .map(|current| current.max(until))
                .unwrap_or(until),
        );
    }
}

impl MetadataUpdateContext for State {
    type AppMessage = DomainMessage;

    fn metadata_message(message: MetadataMessage) -> Self::AppMessage {
        DomainMessage::Metadata(message)
    }

    fn image_service(&self) -> &Arc<UnifiedImageService> {
        &self.domains.metadata.state.image_service
    }

    fn nudge_image_transition(&self) -> Option<Self::AppMessage> {
        Some(DomainMessage::Ui(
            BackgroundMessage::UpdateTransitions.into(),
        ))
    }
}

pub fn update_metadata_for_context<C>(
    context: &mut C,
    message: MetadataMessage,
) -> FoundationUpdateResult<Task<C::AppMessage>, ()>
where
    C: MetadataUpdateContext + 'static,
{
    match message {
        MetadataMessage::UnifiedImageLoaded(
            request,
            handle,
            estimated_bytes,
        ) => {
            let task = handle_unified_image_loaded(
                context,
                request,
                handle,
                estimated_bytes,
            )
            .map(C::metadata_message);
            let task = if let Some(nudge) = context.nudge_image_transition() {
                Task::batch(vec![task, Task::done(nudge)])
            } else {
                task
            };
            FoundationUpdateResult::task(task)
        }
        MetadataMessage::UnifiedImageLoadFailed(request, error) => {
            FoundationUpdateResult::task(
                handle_unified_image_load_failed(context, request, error)
                    .map(C::metadata_message),
            )
        }
        MetadataMessage::UnifiedImageCancelled(request) => {
            FoundationUpdateResult::task(
                handle_unified_image_cancelled(context, request)
                    .map(C::metadata_message),
            )
        }
        MetadataMessage::ImageBlobReady(request, token) => {
            MetadataUpdateContext::image_service(context)
                .set_ready_token(&request, token);
            MetadataUpdateContext::image_service(context)
                .request_image(request);
            FoundationUpdateResult::task(Task::none())
        }
        MetadataMessage::InitializeService => {
            log::info!("Metadata service initialization requested");
            FoundationUpdateResult::task(Task::none())
        }
        MetadataMessage::MediaDetailsLoaded(result) => {
            match result {
                Ok(details) => {
                    log::info!("Media details loaded: {} items", details.len());
                }
                Err(error) => {
                    log::error!("Failed to load media details: {}", error);
                }
            }
            FoundationUpdateResult::task(Task::none())
        }
        MetadataMessage::SeriesSortingCompleted(series_refs) => {
            log::info!(
                "Series sorting completed: {} series",
                series_refs.len()
            );
            FoundationUpdateResult::task(Task::none())
        }
        MetadataMessage::ForceRescan => {
            log::info!("Force rescan requested");
            FoundationUpdateResult::task(Task::none())
        }
        MetadataMessage::NoOp => FoundationUpdateResult::task(Task::none()),
    }
}

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

    let result = update_metadata_for_context(state, message);
    DomainUpdateResult::task(result.task)
}
