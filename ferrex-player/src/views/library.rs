use crate::{
    messages::ui::Message,
    state::ViewMode,
    theme,
    views::grid::grid_view,
    views::{all::view_all_content, scanning::overlay::create_scan_progress_overlay},
    State,
};
use iced::{
    widget::{button, column, container, row, text, Row, Space, Stack},
    Element, Length,
};
use lucide_icons::Icon;

// Helper function to create icon text
fn icon_text(icon: Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

// Get icon character string
fn icon_char(icon: Icon) -> String {
    icon.unicode().to_string()
}

// Get the lucide font
fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}

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
        let library_tabs = if state.libraries.is_empty() {
            // Fallback to old view mode tabs if no libraries configured
            log::debug!("No libraries configured, showing view mode tabs");
            row![
                button(text("All").size(16))
                    .on_press(Message::SetViewMode(ViewMode::All))
                    .style(if state.view_mode == ViewMode::All {
                        theme::Button::Primary.style()
                    } else {
                        theme::Button::Secondary.style()
                    })
                    .padding([8, 16]),
                Space::with_width(10),
                button(text("Movies").size(16))
                    .on_press(Message::SetViewMode(ViewMode::Movies))
                    .style(if state.view_mode == ViewMode::Movies {
                        theme::Button::Primary.style()
                    } else {
                        theme::Button::Secondary.style()
                    })
                    .padding([8, 16]),
                Space::with_width(10),
                button(text("TV Shows").size(16))
                    .on_press(Message::SetViewMode(ViewMode::TvShows))
                    .style(if state.view_mode == ViewMode::TvShows {
                        theme::Button::Primary.style()
                    } else {
                        theme::Button::Secondary.style()
                    })
                    .padding([8, 16]),
            ]
            .spacing(10) // Add explicit spacing to ensure buttons don't overlap
        } else {
            // Show library tabs
            let mut tabs_vec: Vec<Element<Message>> = Vec::new();

            // Add "All Libraries" tab
            tabs_vec.push(
                button(
                    row![
                        text("📚📺").size(14),
                        Space::with_width(5),
                        text("All Libraries").size(16),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::SelectLibrary(None))
                .style(if state.current_library_id.is_none() {
                    theme::Button::Primary.style()
                } else {
                    theme::Button::Secondary.style()
                })
                .padding([8, 16])
                .into(),
            );

            for library in &state.libraries {
                if !tabs_vec.is_empty() {
                    tabs_vec.push(Space::with_width(10).into());
                }

                // Library type icon
                let icon = if library.library_type == crate::api_types::LibraryType::Movies {
                    Icon::Film
                } else {
                    Icon::Tv
                };

                // Library status indicator
                let status_indicator = if library.enabled {
                    if state.scanning && state.active_scan_id.is_some() {
                        Icon::Loader
                    } else {
                        Icon::Cloud
                    }
                } else {
                    Icon::CloudOff
                };

                let tab_content = column![row![
                    icon_text(icon).size(14),
                    Space::with_width(3),
                    icon_text(status_indicator).size(10),
                    Space::with_width(5),
                    text(&library.name).size(16),
                ]
                .align_y(iced::Alignment::Center),]
                .spacing(2)
                .align_x(iced::Alignment::Center);

                tabs_vec.push(
                    button(tab_content)
                        .on_press(Message::SelectLibrary(Some(library.id.clone())))
                        .style(if state.current_library_id.as_ref() == Some(&library.id) {
                            theme::Button::Primary.style()
                        } else {
                            theme::Button::Secondary.style()
                        })
                        .padding([8, 12])
                        .into(),
                );
            }

            Row::with_children(tabs_vec)
        };

        // Header is now handled at a higher level in main.rs

        // Error message if any
        let error_section: Element<Message> = if let Some(error) = &state.error_message {
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

        // Scan progress section - SAVED FOR FUTURE ADMIN PAGE
        /* Inline scan progress implementation - commented out in favor of overlay
        let scan_progress_section: Element<Message> = if let Some(progress) = &state.scan_progress {
            let progress_percentage = if progress.total_files > 0 {
                (progress.scanned_files as f32 / progress.total_files as f32) * 100.0
            } else {
                0.0
            };

            let eta_text = if let Some(eta) = progress.estimated_time_remaining {
                let seconds = eta.as_secs();
                if seconds < 60 {
                    format!("ETA: {} seconds", seconds)
                } else {
                    let minutes = seconds / 60;
                    let remaining_seconds = seconds % 60;
                    format!("ETA: {}:{:02}", minutes, remaining_seconds)
                }
            } else {
                "Calculating ETA...".to_string()
            };

            let current_file_text = if let Some(file) = &progress.current_file {
                // Extract just the filename from the path for cleaner display
                let filename = std::path::Path::new(file)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(file);
                format!("Processing: {}", filename)
            } else {
                "Scanning directories...".to_string()
            };

            let status_text = match progress.status {
                ScanStatus::Pending => "Preparing scan...",
                ScanStatus::Scanning => "Scanning files...",
                ScanStatus::Processing => "Processing metadata...",
                ScanStatus::Completed => "Scan completed!",
                ScanStatus::Failed => "Scan failed",
                ScanStatus::Cancelled => "Scan cancelled",
            };

            container(
                column![
                    row![
                        text(status_text)
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        Space::with_width(Length::Fill),
                        container(
                            container(Space::with_width(Length::Fixed(
                                progress_percentage * 0.01 * 200.0
                            )))
                            .height(3)
                            .style(theme::Container::ProgressBar.style())
                        )
                        .width(200)
                        .height(3)
                        .style(theme::Container::ProgressBarBackground.style()),
                        Space::with_width(10),
                        text(format!("{:.0}%", progress_percentage))
                            .size(12)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        Space::with_width(15),
                        text(eta_text)
                            .size(12)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .align_y(iced::Alignment::Center),
                    row![
                        text(format!(
                            "{}/{} files • {} stored • {} metadata",
                            progress.scanned_files, progress.total_files,
                            progress.stored_files, progress.metadata_fetched
                        ))
                        .size(11)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        Space::with_width(Length::Fill),
                        if !progress.errors.is_empty() {
                            Element::from(
                                text(format!("{} errors", progress.errors.len()))
                                    .size(11)
                                    .color(theme::MediaServerTheme::ERROR),
                            )
                        } else {
                            Element::from(Space::with_width(0))
                        },
                    ],
                    text(current_file_text)
                        .size(10)
                        .color(theme::MediaServerTheme::TEXT_DIMMED),
                ]
                .spacing(3),
            )
            .padding(10)
            .style(theme::Container::Card.style())
            .into()
        } else {
            container(Space::with_height(0)).into()
        };
        */
        let scan_progress_section: Element<Message> = container(Space::with_height(0)).into();

        // NEW ARCHITECTURE: Check if we have any media in MediaStore
        let has_media = if let Ok(store) = state.media_store.read() {
            !store.get_all_movies().is_empty() || !store.get_all_series().is_empty()
        } else {
            false
        };

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
            // Choose view based on mode OR selected library type
            let library_content = if let Some(library_id) = &state.current_library_id {
                // A specific library is selected
                if let Some(selected_library) = state.libraries.iter().find(|l| l.id == *library_id)
                {
                    // Use library type to determine which view to show
                    use crate::api_types::LibraryType;

                    match selected_library.library_type {
                        LibraryType::Movies => {
                            // Use movie references grid from ViewModel
                            let movies = state.movies_view_model.all_movies();
                            //log::info!("Library view: Rendering {} movies from MoviesViewModel", movies.len());
                            grid_view::virtual_movie_references_grid(
                                movies,
                                state.movies_view_model.grid_state(),
                                &state.hovered_media_id,
                                Message::MoviesGridScrolled,
                                state.fast_scrolling,
                                state,
                            )
                        }
                        LibraryType::TvShows => {
                            // Use series references grid from ViewModel
                            let series = state.tv_view_model.all_series();
                            //log::info!("Library view: Rendering {} series from TvViewModel", series.len());
                            grid_view::virtual_series_references_grid(
                                series,
                                state.tv_view_model.grid_state(),
                                &state.hovered_media_id,
                                Message::TvShowsGridScrolled,
                                state.fast_scrolling,
                                state,
                            )
                        }
                    }
                } else {
                    // Library not found, show all content
                    view_all_content(state)
                }
            } else {
                // No specific library selected, use view mode
                match state.view_mode {
                    ViewMode::All => view_all_content(state),
                    ViewMode::Movies => {
                        //log::debug!(
                        //    "Rendering Movies view with {} movie references",
                        //    state.movies_view_model.all_movies().len()
                        //);
                        // Use virtual grid view for movie references from ViewModel
                        grid_view::virtual_movie_references_grid(
                            state.movies_view_model.all_movies(),
                            state.movies_view_model.grid_state(),
                            &state.hovered_media_id,
                            Message::MoviesGridScrolled,
                            state.fast_scrolling,
                            state,
                        )
                    }
                    ViewMode::TvShows => {
                        //log::debug!(
                        //    "Rendering TvShows view with {} series references",
                        //    state.tv_view_model.all_series().len()
                        //);
                        // Use virtual grid view for series references from ViewModel
                        grid_view::virtual_series_references_grid(
                            state.tv_view_model.all_series(),
                            state.tv_view_model.grid_state(),
                            &state.hovered_media_id,
                            Message::TvShowsGridScrolled,
                            state.fast_scrolling,
                            state,
                        )
                    }
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
            if state.show_scan_progress && state.scan_progress.is_some() {
                //log::info!("Showing scan overlay - show_scan_progress: true, scan_progress: Some");
                create_scan_progress_overlay(main_content, &state.scan_progress)
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
