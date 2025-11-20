use crate::domains::ui::theme;
use crate::domains::ui::views::grid::{
    movie_reference_card_with_state, series_reference_card_with_state,
};
use crate::domains::ui::{
    messages::Message, views::carousel::windowed_media_carousel,
};
use crate::infra::LibraryType;
use crate::state::State;
use ferrex_core::player_prelude::{MovieID, SeriesID};
use iced::{
    Element, Length,
    widget::{column, container, scrollable, text},
};

// Helper function for carousel view used in All mode
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_all_content<'a>(state: &'a State) -> Element<'a, Message> {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infra::profiling_scopes::scopes::VIEW_RENDER);

    let watch_state_opt = state.domains.media.state.get_watch_state();

    let mut content = column![].spacing(30).padding(20);
    let mut added_count = 0;

    log::debug!(
        "view_all_content: library_info has {} libraries",
        state.tab_manager.library_info().len()
    );
    for (library_id, library_type) in state.tab_manager.library_info() {
        log::debug!(
            "Processing library {} of type {:?}",
            library_id,
            library_type
        );
        match library_type {
            LibraryType::Movies => {
                let movies_ids = state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_sorted_index_by_library(
                        library_id,
                        state.domains.ui.state.sort_by,
                        state.domains.ui.state.sort_order,
                    );
                if let Ok(movies_ids) = movies_ids {
                    log::debug!(
                        "All view: Found {} movies for library {}",
                        movies_ids.len(),
                        library_id
                    );
                    let carousel = windowed_media_carousel(
                        library_type.to_string(),
                        "Movies",
                        movies_ids.len(),
                        &state.domains.ui.state.movies_carousel,
                        move |idx| {
                            movies_ids.get(idx).map(|uuid| {
                                let is_hovered = state
                                    .domains
                                    .ui
                                    .state
                                    .hovered_media_id
                                    .as_ref()
                                    == Some(uuid);

                                let item_watch_progress = if let Some(
                                    watch_state,
                                ) = watch_state_opt
                                {
                                    watch_state.get_watch_progress(uuid)
                                } else {
                                    None
                                };
                                let movie_id = MovieID(*uuid);
                                movie_reference_card_with_state(
                                    state,
                                    movie_id,
                                    is_hovered,
                                    false,
                                    item_watch_progress,
                                )
                            })
                        },
                    );
                    added_count += 1;
                    content = content.push(carousel);
                }
            }
            LibraryType::Series => {
                let series_ids_result = state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_sorted_index_by_library(
                        library_id,
                        state.domains.ui.state.sort_by,
                        state.domains.ui.state.sort_order,
                    );
                if let Ok(series_ids) = series_ids_result {
                    log::debug!(
                        "All view: Found {} series for library {}",
                        series_ids.len(),
                        library_id
                    );
                    let carousel = windowed_media_carousel(
                        library_type.to_string(),
                        "TV Shows",
                        series_ids.len(),
                        &state.domains.ui.state.tv_carousel,
                        move |idx| {
                            series_ids.get(idx).map(|uuid| {
                                let is_hovered = state
                                    .domains
                                    .ui
                                    .state
                                    .hovered_media_id
                                    .as_ref()
                                    == Some(uuid);

                                let item_watch_progress = if let Some(
                                    watch_state,
                                ) = watch_state_opt
                                {
                                    watch_state.get_watch_progress(uuid)
                                } else {
                                    None
                                };
                                let series_id = SeriesID(*uuid);
                                series_reference_card_with_state(
                                    state,
                                    series_id,
                                    is_hovered,
                                    false,
                                    item_watch_progress,
                                )
                            })
                        },
                    );
                    added_count += 1;
                    content = content.push(carousel);
                }
            }
        }
    }

    if added_count == 0 && !state.loading {
        content = content.push(
            container(
                column![
                    text("📁").size(64),
                    text("No media files found")
                        .size(24)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    text("Click 'Scan Library' to search for media files")
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                ]
                .spacing(10)
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .padding(100) // Add padding instead of fill height for visual centering
            .align_x(iced::alignment::Horizontal::Center),
        );
    }

    scrollable(content)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::default(),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
