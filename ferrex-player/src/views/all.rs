use iced::{
    widget::{column, container, scrollable, text},
    Element, Length,
};

use crate::{messages::ui::Message, state::State, theme, views::carousel::windowed_media_carousel};

// Helper function for carousel view used in All mode
pub fn view_all_content(state: &State) -> Element<Message> {
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
            |idx| {
                series_list.get(idx).map(|series| {
                    let is_hovered = state.hovered_media_id.as_ref().map(|s| s.as_str())
                        == Some(series.id.as_str());
                    crate::components::series_reference_card(series, is_hovered)
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
            |idx| {
                movies_list.get(idx).map(|movie| {
                    let is_hovered = state.hovered_media_id.as_ref().map(|s| s.as_str())
                        == Some(movie.id.as_str());
                    crate::components::movie_reference_card_with_state(
                        movie,
                        is_hovered,
                        false,
                        Some(state),
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
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center),
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
