use crate::{
    domains::ui::messages::Message,
    domains::ui::theme,
    domains::ui::views::{
        grid::grid_view,
        {all::view_all_content, scanning::overlay::create_scan_progress_overlay},
    },
    domains::ui::DisplayMode,
    state_refactored::State,
};
use iced::{
    widget::{button, column, container, row, text, Row, Space},
    Element, Length,
};
use lucide_icons::Icon;

fn library_loading() -> Element<'static, Message> {
    container(
        column![
            text("Media Library")
                .size(28)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_height(Length::Fixed(100.0)),
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

pub fn view_library(state: &State) -> Element<Message> {
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
                        Space::with_width(Length::Fill),
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
                container(Space::with_height(0)).into()
            };

        let scan_progress_section: Element<Message> = container(Space::with_height(0)).into();

        // Use MediaQueryService to check for media (clean architecture)
        let has_media = state.domains.media.state.query_service.has_any_media();

        if !has_media {
            // Empty state
            container(
                column![
                    error_section,
                    Space::with_height(Length::Fill),
                    container(
                        column![
                            text("No media files found")
                                .size(18)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            Space::with_height(20),
                            text("Click 'Scan Library' to find media files")
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        ]
                        .align_x(iced::Alignment::Center)
                        .spacing(10)
                    )
                    .align_x(iced::alignment::Horizontal::Center),
                    Space::with_height(Length::Fill)
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
            // Choose view based on display mode first, then library selection
            log::debug!("[Library View] Rendering with library.current_library_id = {:?}, ui.current_library_id = {:?}, ui.display_mode = {:?}",
                state.domains.library.state.current_library_id,
                state.domains.ui.state.current_library_id,
                state.domains.ui.state.display_mode
            );

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
                        TabState::Library(lib_state) => {
                            match lib_state.library_type {
                                LibraryType::Movies => {
                                    // Use movies from tab state
                                    let movies = lib_state.movies().unwrap_or(&[]);
                                    log::debug!(
                                        "[Library Movies View] Rendering {} movies for library {}",
                                        movies.len(),
                                        lib_state.library_id
                                    );
                                    grid_view::virtual_movie_references_grid(
                                        movies,
                                        &lib_state.grid_state,
                                        &state.domains.ui.state.hovered_media_id,
                                        Message::TabGridScrolled,
                                        state.domains.ui.state.fast_scrolling,
                                        state,
                                    )
                                }
                                LibraryType::TvShows => {
                                    // Use TV shows from tab state
                                    let shows = lib_state.tv_shows().unwrap_or(&[]);
                                    log::debug!(
                                        "[Library TV View] Rendering {} series for library {}",
                                        shows.len(),
                                        lib_state.library_id
                                    );
                                    grid_view::virtual_series_references_grid(
                                        shows,
                                        &lib_state.grid_state,
                                        &state.domains.ui.state.hovered_media_id,
                                        Message::TabGridScrolled,
                                        state.domains.ui.state.fast_scrolling,
                                        state,
                                    )
                                }
                            }
                        }
                        TabState::All(all_state) => {
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

            // Create main content with proper spacing
            let main_content = column![
                error_section,
                scan_progress_section,
                container(library_content)
                    .width(Length::Fill)
                    .height(Length::Fill)
            ];

            // Add scan progress overlay if visible
            if state.domains.library.state.show_scan_progress
                && state.domains.library.state.scan_progress.is_some()
            {
                //log::info!("Showing scan overlay - show_scan_progress: true, scan_progress: Some");
                create_scan_progress_overlay(
                    main_content,
                    &state.domains.library.state.scan_progress,
                )
            } else {
                //log::debug!(
                //    "Not showing scan overlay - show_scan_progress: {}, scan_progress: {}",
                //    state.show_scan_progress,
                //    state.scan_progress.is_some()
                //);
                main_content.into()
            }
        }
    }
}
