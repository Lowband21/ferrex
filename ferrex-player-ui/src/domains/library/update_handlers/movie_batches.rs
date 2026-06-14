use crate::{
    domains::library::messages::LibraryMessage,
    infra::services::api::ApiService,
};

use ferrex_core::player_prelude::{LibraryId, MovieBatchId};
use iced::Task;
use std::sync::Arc;

pub fn handle_fetch_movie_reference_batch(
    api_service: Arc<dyn ApiService>,
    library_id: LibraryId,
    batch_id: MovieBatchId,
) -> Task<LibraryMessage> {
    Task::perform(
        async move {
            api_service
                .fetch_movie_reference_batch(library_id, batch_id)
                .await
        },
        move |result| LibraryMessage::MovieBatchLoaded {
            library_id,
            batch_id,
            result: result.map_err(|e| e.to_string()),
        },
    )
}
