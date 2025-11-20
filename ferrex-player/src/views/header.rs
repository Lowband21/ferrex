use crate::permissions::StatePermissionExt;
use crate::{
    api_types::MediaReference, constants::layout::header::HEIGHT, messages::ui::Message, theme,
    State, ViewState,
};
use ferrex_core::{api_types::MediaId, library::LibraryType, SeriesID};
use iced::{
    widget::{button, container, row, text, text_input, Space, Stack},
    Element, Length,
};
use lucide_icons::Icon;

pub fn view_header<'a>(state: &'a State) -> Element<'a, Message> {
    match &state.view {
        ViewState::Library => {
            // New header layout: Left (Home, Back, Library), Center (Search), Right (Controls)
            let left_section = row![
                // Home button
                button(
                    container(icon_text(Icon::House))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .on_press(Message::NavigateHome)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
                Space::with_width(20), // Gap between home and library tabs
                // Library tabs
                container(create_library_tabs(state)).align_y(iced::alignment::Vertical::Center),
            ]
            .align_y(iced::Alignment::Center);

            // Center section - Search (always visible)
            let center_section = container(
                row![
                    container(
                        text_input("Search...", &state.search_query)
                            .on_input(Message::UpdateSearchQuery)
                            .on_submit(Message::ExecuteSearch)
                            .style(theme::TextInput::header_search())
                            .padding([15, 12])
                            .size(14)
                            .width(Length::Fixed(300.0))
                    )
                    .height(HEIGHT)
                    .center_y(Length::Fill),
                    button(
                        container(icon_text(Icon::Search))
                            .center_x(Length::Fill)
                            .center_y(Length::Fill)
                    )
                    .on_press(Message::ExecuteSearch)
                    .style(theme::Button::HeaderIcon.style())
                    .width(Length::Fixed(HEIGHT))
                    .height(HEIGHT)
                ]
                .align_y(iced::Alignment::Center),
            )
            .height(HEIGHT)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center);

            // Right section - Controls
            let right_section = row![
                // Fullscreen toggle
                button(
                    container(icon_text(if state.is_fullscreen {
                        Icon::Minimize
                    } else {
                        Icon::Maximize
                    }))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                )
                .on_press(Message::ToggleFullscreen)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
                // Scan activity
                button(
                    container(icon_text(Icon::FileScan))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .on_press_maybe(if state.scanning || state.show_scan_progress {
                    Some(Message::ToggleScanProgress)
                } else {
                    None
                })
                .style(if state.scanning || state.show_scan_progress {
                    theme::Button::Primary.style()
                } else {
                    theme::Button::HeaderIcon.style()
                })
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
                // Admin settings (show only if user has permissions)
                {
                    let element: Element<Message> =
                        if state.permission_checker().can_view_admin_dashboard() {
                            button(
                                container(icon_text(Icon::Settings))
                                    .center_x(Length::Fill)
                                    .center_y(Length::Fill),
                            )
                            .on_press(Message::ShowLibraryManagement)
                            .style(theme::Button::HeaderIcon.style())
                            .width(Length::Fixed(HEIGHT))
                            .height(HEIGHT)
                            .into()
                        } else {
                            Space::with_width(HEIGHT).into()
                        };
                    element
                },
                // Profile placeholder
                button(
                    container(icon_text(Icon::UserPen))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .on_press(Message::ShowProfile)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            ]
            .align_y(iced::Alignment::Center);

            // Stack layout to achieve proper center alignment
            Stack::new()
                .push(
                    // Base layer: centered search
                    container(center_section)
                        .width(Length::Fill)
                        .height(HEIGHT)
                        .align_x(iced::alignment::Horizontal::Center)
                        .align_y(iced::alignment::Vertical::Center),
                )
                .push(
                    // Top layer: left and right sections
                    row![
                        container(left_section)
                            .padding([0, 15])
                            .align_y(iced::alignment::Vertical::Center),
                        Space::with_width(Length::Fill),
                        container(right_section)
                            .padding([0, 15])
                            .align_y(iced::alignment::Vertical::Center),
                    ]
                    .width(Length::Fill)
                    .height(HEIGHT),
                )
                .width(Length::Fill)
                .height(HEIGHT)
                .into()
        }
        ViewState::MovieDetail { .. }
        | ViewState::TvShowDetail { .. }
        | ViewState::SeasonDetail { .. }
        | ViewState::EpisodeDetail { .. } => {
            // Simplified header for detail views
            let left_section = row![
                // Back button
                button(
                    container(icon_text(Icon::ChevronLeft))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .on_press(Message::BackToLibrary)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            ]
            .align_y(iced::Alignment::Center);

            // Right section - same controls as library view
            let right_section = row![
                // Fullscreen toggle
                button(
                    container(icon_text(if state.is_fullscreen {
                        Icon::Minimize
                    } else {
                        Icon::Maximize
                    }))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                )
                .on_press(Message::ToggleFullscreen)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
                // Admin settings (show only if user has permissions)
                {
                    let admin_element: Element<'_, Message> =
                        if state.permission_checker().can_view_admin_dashboard() {
                            button(
                                container(icon_text(Icon::Settings))
                                    .center_x(Length::Fill)
                                    .center_y(Length::Fill),
                            )
                            .on_press(Message::ShowLibraryManagement)
                            .style(theme::Button::HeaderIcon.style())
                            .width(Length::Fixed(HEIGHT))
                            .height(HEIGHT)
                            .into()
                        } else {
                            Space::with_width(HEIGHT).into()
                        };
                    admin_element
                },
                // Profile placeholder
                button(
                    container(icon_text(Icon::UserPen))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .on_press(Message::ShowProfile)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            ]
            .align_y(iced::Alignment::Center);

            Stack::new()
                .push(
                    // Base layer: centered title
                    container(
                        text(get_detail_title(state))
                            .size(20)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    )
                    .width(Length::Fill)
                    .height(HEIGHT)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .push(
                    // Top layer: left and right sections
                    row![
                        container(left_section)
                            .padding([0, 15])
                            .align_y(iced::alignment::Vertical::Center),
                        Space::with_width(Length::Fill),
                        container(right_section)
                            .padding([0, 15])
                            .align_y(iced::alignment::Vertical::Center),
                    ]
                    .width(Length::Fill)
                    .height(HEIGHT),
                )
                .width(Length::Fill)
                .height(HEIGHT)
                .into()
        }
        ViewState::LibraryManagement => {
            let left_section = row![
                // Back button
                button(
                    container(icon_text(Icon::ChevronLeft))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .on_press(Message::BackToLibrary)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            ]
            .align_y(iced::Alignment::Center);

            Stack::new()
                .push(
                    // Base layer: centered title
                    container(
                        text("Library Management")
                            .size(20)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    )
                    .width(Length::Fill)
                    .height(HEIGHT)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .push(
                    // Top layer: left section
                    container(left_section)
                        .width(Length::Fill)
                        .height(HEIGHT)
                        .padding([0, 20])
                        .align_x(iced::alignment::Horizontal::Left)
                        .align_y(iced::alignment::Vertical::Center),
                )
                .width(Length::Fill)
                .height(HEIGHT)
                .into()
        }
        ViewState::AdminDashboard => {
            let left_section = row![
                // Back button
                button(
                    container(icon_text(Icon::ChevronLeft))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .on_press(Message::BackToLibrary)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            ]
            .align_y(iced::Alignment::Center);

            Stack::new()
                .push(
                    // Base layer: centered title
                    container(
                        text("Admin Dashboard")
                            .size(20)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    )
                    .width(Length::Fill)
                    .height(HEIGHT)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .push(
                    // Top layer: left section
                    container(left_section)
                        .width(Length::Fill)
                        .height(HEIGHT)
                        .padding([0, 20])
                        .align_x(iced::alignment::Horizontal::Left)
                        .align_y(iced::alignment::Vertical::Center),
                )
                .width(Length::Fill)
                .height(HEIGHT)
                .into()
        }
        _ => {
            // No header for other views
            Space::with_height(0).into()
        }
    }
}

fn create_library_tabs<'a>(state: &'a State) -> Element<'a, Message> {
    use crate::state::ViewMode;

    if state.libraries.is_empty() {
        // Fallback to old view mode tabs if no libraries configured
        row![
            button(container(text("All").size(14)).center_y(Length::Fill))
                .on_press(Message::SetViewMode(ViewMode::All))
                .style(theme::Button::HeaderIcon.style())
                .padding([0, 16])
                .height(HEIGHT),
            button(container(text("Movies").size(14)).center_y(Length::Fill))
                .on_press(Message::SetViewMode(ViewMode::Movies))
                .style(theme::Button::HeaderIcon.style())
                .padding([0, 16])
                .height(HEIGHT),
            button(container(text("TV Shows").size(14)).center_y(Length::Fill))
                .on_press(Message::SetViewMode(ViewMode::TvShows))
                .style(theme::Button::HeaderIcon.style())
                .padding([0, 16])
                .height(HEIGHT),
        ]
        .into()
    } else {
        // Show library tabs
        let mut tabs_vec: Vec<Element<Message>> = Vec::new();

        // Add "All" tab
        tabs_vec.push(
            button(container(text("All").size(14)).center_y(Length::Fill))
                .on_press(Message::SelectLibrary(None))
                .style(theme::Button::HeaderIcon.style())
                .padding([0, 16])
                .height(HEIGHT)
                .into(),
        );

        // Add individual library tabs
        for library in state.libraries.iter().filter(|l| l.enabled) {
            tabs_vec.push(
                button(container(text(&library.name).size(14)).center_y(Length::Fill))
                    .on_press(Message::SelectLibrary(Some(library.id.clone())))
                    .style(theme::Button::HeaderIcon.style())
                    .padding([0, 16])
                    .height(HEIGHT)
                    .into(),
            );
        }

        row(tabs_vec).into()
    }
}

fn get_detail_title(state: &State) -> String {
    match &state.view {
        ViewState::MovieDetail { movie, .. } => movie.title.as_str().to_string(),
        ViewState::TvShowDetail { series_id, .. } => {
            // NEW ARCHITECTURE: Get series from MediaStore
            if let Ok(store) = state.media_store.read() {
                if let Some(MediaReference::Series(series)) =
                    store.get(&MediaId::Series(series_id.clone()))
                {
                    series.title.as_str().to_string()
                } else {
                    "TV Show".to_string()
                }
            } else {
                "TV Show".to_string()
            }
        }
        ViewState::SeasonDetail { series_id, .. } => {
            // NEW ARCHITECTURE: Get series from MediaStore
            if let Ok(store) = state.media_store.read() {
                if let Some(MediaReference::Series(series)) =
                    store.get(&MediaId::Series(series_id.clone()))
                {
                    series.title.as_str().to_string()
                } else {
                    "Season".to_string()
                }
            } else {
                "Season".to_string()
            }
        }
        ViewState::EpisodeDetail { .. } => "Episode".to_string(),
        _ => String::new(),
    }
}

// Helper function to create icon text
fn icon_text(icon: Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(16)
}

// Get the lucide font
fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}
