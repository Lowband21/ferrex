use crate::domains::ui::messages::Message;
use crate::domains::ui::theme;
use crate::domains::ui::views::carousel::windowed_media_carousel;
use crate::infrastructure::api_types::MediaId;
use crate::state_refactored::State;
use iced::{
    widget::{column, container, scrollable, text},
    Element, Length,
};
use tokio::sync::watch;

// Helper function for carousel view used in All mode
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_all_content(state: &State) -> Element<Message> {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infrastructure::profiling_scopes::scopes::VIEW_RENDER);

    let watch_state_opt = state.domains.media.state.get_watch_state();

    let mut content = column![].spacing(30).padding(20);

    // TV Shows carousel - use ViewModel
    let series_list = state.all_view_model.all_series();
    //log::info!("view_all_content: series_list has {} items", series_list.len());
    if !series_list.is_empty() {
        // Use windowed carousel for performance
        let tv_carousel = windowed_media_carousel(
            "tv_shows".to_string(),
            "TV Shows",
            series_list.len(),
            state.all_view_model.tv_carousel(),
            move |idx| {
                series_list.get(idx).map(|series| {
                    let series_id = series.id.clone();
                    let is_hovered = state.domains.ui.state.hovered_media_id.as_ref().map(|s| s)
                        == Some(series_id.as_ref());

                    let item_watch_progress = if let Some(watch_state) = watch_state_opt {
                        watch_state.get_watch_progress(&MediaId::from(series_id))
                    } else {
                        None
                    };
                    crate::domains::ui::components::series_reference_card_with_state(
                        series,
                        is_hovered,
                        false,
                        item_watch_progress,
                    )
                })
            },
        );

        content = content.push(tv_carousel);
    }

    // Movies carousel - use ViewModel
    let movies_list = state.all_view_model.all_movies();
    //log::info!("view_all_content: movies_list has {} items", movies_list.len());
    if !movies_list.is_empty() {
        // Use windowed carousel for performance
        let movies_carousel = windowed_media_carousel(
            "movies".to_string(),
            "Movies",
            movies_list.len(),
            state.all_view_model.movies_carousel(),
            move |idx| {
                movies_list.get(idx).map(|movie| {
                    let movie_id = movie.id.clone();
                    let is_hovered = state
                        .domains
                        .ui
                        .state
                        .hovered_media_id
                        .as_ref()
                        .map(|s| s.as_ref())
                        == Some(movie_id.as_ref());

                    let item_watch_progress = if let Some(watch_state) = watch_state_opt {
                        watch_state.get_watch_progress(&MediaId::from(movie_id))
                    } else {
                        None
                    };
                    crate::domains::ui::components::movie_reference_card_with_state(
                        movie,
                        is_hovered,
                        false,
                        item_watch_progress,
                    )
                })
            },
        );

        content = content.push(movies_carousel);
    }

    // If no content
    let has_media = !movies_list.is_empty() || !series_list.is_empty();

    if !has_media && !state.loading {
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
