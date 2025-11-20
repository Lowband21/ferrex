use super::library_filter_panel::library_filter_panel;
use crate::{
    domains::ui::{
        DisplayMode,
        messages::Message,
        theme,
        views::{
            all::view_all_content,
            grid::{virtual_movie_references_grid, virtual_series_references_grid},
        },
        widgets::{collect_cached_handles_for_media, texture_preloader},
    },
    state_refactored::State,
};
use ferrex_core::player_prelude::{ImageSize, ImageType};
use iced::{
    Element, Length,
    widget::{Space, button, column, container, row, text},
};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn library_loading() -> Element<'static, Message> {
    container(
        column![
            text("Media Library")
                .size(28)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::new().height(Length::Fixed(100.0)),
            text("Loading library...")
                .size(20)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(20)
        .align_x(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center)
    .padding(20)
    .style(theme::Container::Default.style())
    .into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_library(state: &State) -> Element<'_, Message> {
    let view_library = iced::debug::time("view::view_library");

    if state.loading {
        // Loading state
        library_loading()
    } else {
        // LEGACY: Error message if any
        let error_section: Element<Message> =
            if let Some(error) = &state.domains.ui.state.error_message {
                container(
                    row![
                        text(error).color(theme::MediaServerTheme::ERROR),
                        Space::new().width(Length::Fill),
                        button("×")
                            .on_press(Message::ClearError)
                            .style(theme::Button::Text.style()),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .padding(10)
                .style(theme::Container::Card.style())
                .into()
            } else {
                container(Space::new().height(0)).into()
            };

        if !state.domains.ui.state.repo_accessor.is_initialized() {
            // Empty state
            container(
                column![
                    error_section,
                    Space::new().height(Length::Fill),
                    container(
                        column![
                            text("No media files found")
                                .size(18)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            Space::new().height(20),
                            text("Click 'Scan Library' to find media files")
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        ]
                        .align_x(iced::Alignment::Center)
                        .spacing(10)
                    )
                    .align_x(iced::alignment::Horizontal::Center),
                    Space::new().height(Length::Fill)
                ]
                .spacing(20)
                .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .style(theme::Container::Default.style())
            .into()
        } else {
            // Check display mode FIRST to ensure Curated mode always shows all content
            let library_content = match state.domains.ui.state.display_mode {
                DisplayMode::Curated => {
                    // Always show all content in Curated mode, regardless of library selection
                    view_all_content(state)
                }
                DisplayMode::Library => {
                    // Use the tab system to get the active tab
                    use crate::domains::ui::tabs::TabState;
                    use crate::infrastructure::api_types::LibraryType;

                    let active_tab = state.tab_manager.active_tab();
                    match active_tab {
                        TabState::Library(lib_state) => match lib_state.library_type {
                            LibraryType::Movies => {
                                // Compute a small prefetch set and preload their textures into the atlas
                                let preload_range = lib_state.grid_state.get_preload_range(crate::infrastructure::constants::layout::virtual_grid::PREFETCH_ROWS_ABOVE);
                                let ids_slice =
                                    lib_state.cached_index_ids.get(preload_range).unwrap_or(&[]);
                                let handles = collect_cached_handles_for_media(
                                    ids_slice.iter().copied(),
                                    ImageType::Movie,
                                    ImageSize::Poster,
                                );
                                let budget = crate::infrastructure::constants::performance_config::texture_upload::MAX_UPLOADS_PER_FRAME as usize;
                                let preloader = texture_preloader(handles, budget);

                                let grid = virtual_movie_references_grid(
                                    &lib_state.cached_index_ids,
                                    &lib_state.grid_state,
                                    &state.domains.ui.state.hovered_media_id,
                                    Message::TabGridScrolled,
                                    state,
                                );

                                column![preloader, grid].into()
                            }
                            LibraryType::Series => {
                                let preload_range = lib_state.grid_state.get_preload_range(crate::infrastructure::constants::layout::virtual_grid::PREFETCH_ROWS_ABOVE);
                                let ids_slice =
                                    lib_state.cached_index_ids.get(preload_range).unwrap_or(&[]);
                                let handles = collect_cached_handles_for_media(
                                    ids_slice.iter().copied(),
                                    ImageType::Series,
                                    ImageSize::Poster,
                                );
                                let budget = crate::infrastructure::constants::performance_config::texture_upload::MAX_UPLOADS_PER_FRAME as usize;
                                let preloader = texture_preloader(handles, budget);

                                let grid = virtual_series_references_grid(
                                    &lib_state.cached_index_ids,
                                    &lib_state.grid_state,
                                    &state.domains.ui.state.hovered_media_id,
                                    Message::TabGridScrolled,
                                    state,
                                );

                                column![preloader, grid].into()
                            }
                        },
                        TabState::All(_all_state) => {
                            // Use the AllViewModel from all_state
                            view_all_content(state)
                        }
                    }
                }
                _ => {
                    // Other modes not implemented yet
                    view_all_content(state)
                }
            };

            let filter_panel: Option<Element<Message>> = if state.domains.ui.state.show_filter_panel
            {
                Some(library_filter_panel(state))
            } else {
                None
            };

            // Create main content with proper spacing
            let mut main_col = column![error_section];
            if let Some(panel) = filter_panel {
                main_col = main_col.push(panel);
            }
            let main_content = main_col.push(
                container(library_content)
                    .width(Length::Fill)
                    .height(Length::Fill),
            );

            view_library.finish();
            main_content.into()
        }
    }
}
