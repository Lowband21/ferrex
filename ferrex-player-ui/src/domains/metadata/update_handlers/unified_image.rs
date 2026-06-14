use crate::{
    domains::metadata::{
        image_service::UnifiedImageService, messages::MetadataMessage,
    },
    infra::constants,
};
use ferrex_core::player_prelude::{ImageRequest, ImageSize};
use iced::{Task, widget::image::Handle};
use std::{sync::Arc, time::Instant};

/// Explicit image/UI ports needed by unified image handlers.
pub trait UnifiedImageContext {
    fn image_service(&self) -> &Arc<UnifiedImageService>;
    fn extend_poster_animation_until(&mut self, until: Instant);
}

pub fn handle_unified_image_loaded<C>(
    context: &mut C,
    request: ImageRequest,
    handle: Handle,
    estimated_bytes: u64,
) -> Task<MetadataMessage>
where
    C: UnifiedImageContext,
{
    context
        .image_service()
        .mark_loaded(&request, handle, estimated_bytes);

    if matches!(request.size, ImageSize::Poster(_))
        || matches!(request.size, ImageSize::Thumbnail(_))
    {
        use std::time::Duration;
        let until = Instant::now()
            + Duration::from_millis(
                (constants::layout::animation::DEFAULT_DURATION_MS as f64
                    * 1.25) as u64,
            );
        context.extend_poster_animation_until(until);
    }

    Task::none()
}

pub fn handle_unified_image_load_failed<C>(
    context: &mut C,
    request: ImageRequest,
    error: String,
) -> Task<MetadataMessage>
where
    C: UnifiedImageContext,
{
    log::error!("Unified image load failed: {:?} - {}", request, error);
    context.image_service().mark_failed(&request, error);
    Task::none()
}

pub fn handle_unified_image_cancelled<C>(
    context: &mut C,
    request: ImageRequest,
) -> Task<MetadataMessage>
where
    C: UnifiedImageContext,
{
    log::trace!("Unified image load cancelled: {:?}", request);
    context.image_service().mark_cancelled(&request);
    Task::none()
}
