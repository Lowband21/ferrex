use crate::{
    domains::library::messages::LibraryMessage,
    infra::services::api::ApiService,
};

use ferrex_core::player_prelude::{LibraryId, SeriesID};
use iced::Task;
use std::sync::Arc;

pub fn handle_fetch_series_bundle(
    api_service: Arc<dyn ApiService>,
    library_id: LibraryId,
    series_id: SeriesID,
) -> Task<LibraryMessage> {
    Task::perform(
        async move { api_service.fetch_series_bundle(library_id, series_id).await },
        move |result| LibraryMessage::SeriesBundleLoaded {
            library_id,
            series_id,
            result: result.map_err(|e| e.to_string()),
        },
    )
}
