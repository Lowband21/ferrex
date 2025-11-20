use crate::domains::metadata::{image_types::ImageRequest, messages::Message};
use crate::state_refactored::State;
use iced::{widget::image::Handle, Task};

/// Handle successful image load from the unified image service
pub fn handle_unified_image_loaded(
    state: &mut State,
    request: ImageRequest,
    handle: Handle,
) -> Task<Message> {
    //log::info!("Unified image loaded: {:?}", request);

    // Update the unified image service cache with the loaded handle
    state.domains.metadata.state.image_service.mark_loaded(&request, handle.clone());

    // Check if this is a backdrop for the current detail view
    let should_refresh = match (&state.domains.ui.state.view, &request.media_id, &request.size) {
        (
            crate::domains::ui::types::ViewState::MovieDetail { movie, .. },
            ferrex_core::api_types::MediaId::Movie(id),
            crate::domains::metadata::image_types::ImageSize::Backdrop,
        ) if &movie.id == id => {
            log::info!("Loaded backdrop for current movie detail view");
            true
        }
        (
            crate::domains::ui::types::ViewState::TvShowDetail { series_id, .. },
            ferrex_core::api_types::MediaId::Series(id),
            crate::domains::metadata::image_types::ImageSize::Backdrop,
        ) if series_id == id => {
            log::info!("Loaded backdrop for current TV show detail view");
            true
        }
        (
            crate::domains::ui::types::ViewState::SeasonDetail { season_id, .. },
            ferrex_core::api_types::MediaId::Season(id),
            crate::domains::metadata::image_types::ImageSize::Backdrop,
        ) if season_id == id => {
            log::info!("Loaded backdrop for current season detail view");
            true
        }
        _ => false,
    };

    // The image is now loaded in the cache, UI will pick it up on next render
    // No need for cross-domain events here
    Task::none()
}

/// Handle failed image load from the unified image service
pub fn handle_unified_image_load_failed(
    state: &mut State,
    request: ImageRequest,
    error: String,
) -> Task<Message> {
    log::error!("Unified image load failed: {:?} - {}", request, error);

    // Mark the request as failed in the unified image service
    state.domains.metadata.state.image_service.mark_failed(&request, error);

    Task::none()
}
