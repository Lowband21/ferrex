use crate::domains::metadata::messages::MetadataMessage;
use crate::state::State;
use ferrex_core::player_prelude::{ImageRequest, ImageSize};
use iced::{Task, widget::image::Handle};

/// Handle successful image load from the unified image service
pub fn handle_unified_image_loaded(
    state: &mut State,
    request: ImageRequest,
    handle: Handle,
    estimated_bytes: u64,
) -> Task<MetadataMessage> {
    //log::info!("Unified image loaded: {:?}", request);

    // Update the unified image service cache with the loaded handle
    state.domains.metadata.state.image_service.mark_loaded(
        &request,
        handle.clone(),
        estimated_bytes,
    );

    // Keep UI alive briefly to allow poster animations to play smoothly
    if matches!(request.size, ImageSize::Poster(_))
        || matches!(request.size, ImageSize::Thumbnail(_))
    {
        use std::time::{Duration, Instant};
        let until = Instant::now()
            + Duration::from_millis(
                (crate::infra::constants::layout::animation::DEFAULT_DURATION_MS
                    as f64
                    * 1.25) as u64,
            );
        let ui_until = &mut state.domains.ui.state.poster_anim_active_until;
        *ui_until = Some(ui_until.map(|u| u.max(until)).unwrap_or(until));
    }

    /*
    // Check if this is a backdrop for the current detail view
    let should_refresh = match (&state.domains.ui.state.view, &request.media_id, &request.size) {
        (
            crate::domains::ui::types::ViewState::MovieDetail { movie, .. },
            MediaID::Movie(id),
            ImageSize::backdrop(),
        ) if &movie.id == id => {
            log::info!("Loaded backdrop for current movie detail view");
            true
        }
        (
            crate::domains::ui::types::ViewState::TvShowDetail { series_id, .. },
            MediaID::Series(id),
            ImageSize::backdrop(),
        ) if series_id == id => {
            log::info!("Loaded backdrop for current TV show detail view");
            true
        }
        (
            crate::domains::ui::types::ViewState::SeasonDetail { season_id, .. },
            MediaID::Season(id),
            ImageSize::backdrop(),
        ) if season_id == id => {
            log::info!("Loaded backdrop for current season detail view");
            true
        }
        _ => false,
    }; */

    // The image is now loaded in the cache, UI will pick it up on next render
    // No need for cross-domain events here
    Task::none()
}

/// Handle failed image load from the unified image service
pub fn handle_unified_image_load_failed(
    state: &mut State,
    request: ImageRequest,
    error: String,
) -> Task<MetadataMessage> {
    log::error!("Unified image load failed: {:?} - {}", request, error);

    // Mark the request as failed in the unified image service
    state
        .domains
        .metadata
        .state
        .image_service
        .mark_failed(&request, error);

    Task::none()
}

/// Handle a cancellation signal from the image loader.
pub fn handle_unified_image_cancelled(
    state: &mut State,
    request: ImageRequest,
) -> Task<MetadataMessage> {
    log::trace!("Unified image load cancelled: {:?}", request);
    state
        .domains
        .metadata
        .state
        .image_service
        .mark_cancelled(&request);
    Task::none()
}
