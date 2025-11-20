use crate::domains::auth::permissions::StatePermissionExt;
use crate::infrastructure::MediaID;
use crate::{
    domains::ui::messages::Message, domains::ui::theme, domains::ui::types::ViewState,
    infrastructure::api_types::Media, infrastructure::constants::layout::header::HEIGHT,
    state_refactored::State,
};
use ferrex_core::LibraryID;
use iced::widget::text_input::Id;
use iced::{
    widget::{button, container, row, text, text_input, Space, Stack},
    Element, Length,
};
use lucide_icons::Icon;
use rkyv::deserialize;
use rkyv::rancor::Error;
use uuid::Uuid;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_header<'a>(state: &'a State) -> Element<'a, Message> {
    match &state.domains.ui.state.view {
        ViewState::Library => {
            // New header layout: Left (Home, Back if history exists, Library tabs), Center (Search), Right (Controls)
            let mut left_section_items = vec![];

            // Home button
            left_section_items.push(
                button(
                    container(icon_text(Icon::House))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(Message::NavigateHome)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            // Back button (only if navigation history exists)
            if !state.domains.ui.state.navigation_history.is_empty() {
                left_section_items.push(
                    button(
                        container(icon_text(Icon::ChevronLeft))
                            .center_x(Length::Fill)
                            .center_y(Length::Fill),
                    )
                    .on_press(Message::NavigateBack)
                    .style(theme::Button::HeaderIcon.style())
                    .width(Length::Fixed(HEIGHT))
                    .height(HEIGHT)
                    .into(),
                );
            }

            left_section_items.push(Space::with_width(20).into()); // Gap between buttons and library tabs

            // Library tabs
            left_section_items.push(
                container(create_library_tabs(state))
                    .align_y(iced::alignment::Vertical::Center)
                    .into(),
            );

            let left_section = row(left_section_items).align_y(iced::Alignment::Center);

            // Center section - Search (always visible)
            let center_section = container(
                row![
                    container(
                        text_input("Search...", &state.domains.search.state.query)
                            .id(Id::new("search-input"))
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
                .on_press_maybe(
                    if state.domains.library.state.scanning
                        || state.domains.library.state.show_scan_progress
                    {
                        Some(Message::ToggleScanProgress)
                    } else {
                        None
                    }
                )
                .style(
                    if state.domains.library.state.scanning
                        || state.domains.library.state.show_scan_progress
                    {
                        theme::Button::Primary.style()
                    } else {
                        theme::Button::HeaderIcon.style()
                    }
                )
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
            let mut left_section_items = vec![];

            // Home button
            left_section_items.push(
                button(
                    container(icon_text(Icon::House))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(Message::NavigateHome)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            // Back button (always shown in detail views since we came from somewhere)
            left_section_items.push(
                button(
                    container(icon_text(Icon::ChevronLeft))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(Message::NavigateBack)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            let left_section = row(left_section_items).align_y(iced::Alignment::Center);

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
            let mut left_section_items = vec![];

            // Home button
            left_section_items.push(
                button(
                    container(icon_text(Icon::House))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(Message::NavigateHome)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            // Back button (always shown since we came from library)
            left_section_items.push(
                button(
                    container(icon_text(Icon::ChevronLeft))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(Message::NavigateBack)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            let left_section = row(left_section_items).align_y(iced::Alignment::Center);

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
            let mut left_section_items = vec![];

            // Home button
            left_section_items.push(
                button(
                    container(icon_text(Icon::House))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(Message::NavigateHome)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            // Back button (always shown since we came from somewhere)
            left_section_items.push(
                button(
                    container(icon_text(Icon::ChevronLeft))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(Message::NavigateBack)
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            let left_section = row(left_section_items).align_y(iced::Alignment::Center);

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
    use crate::domains::ui::tabs::TabId;
    use crate::domains::ui::types::DisplayMode;

    if !state.domains.ui.state.repo_accessor.is_initialized() {
        // No libraries configured - show only curated view
        row![
            button(container(text("All").size(14)).center_y(Length::Fill))
                .on_press(Message::SetDisplayMode(DisplayMode::Curated))
                .style(theme::Button::HeaderIcon.style())
                .padding([0, 16])
                .height(HEIGHT),
        ]
        .into()
    } else {
        // Show library tabs
        let mut tabs_vec: Vec<Element<Message>> = Vec::new();

        // Check if "All" tab is active
        let is_all_active = state.tab_manager.active_tab_id() == TabId::All;
        let all_button_style = if is_all_active {
            theme::Button::Primary.style()
        } else {
            theme::Button::HeaderIcon.style()
        };

        // Add "All" tab - shows curated collections from all libraries
        tabs_vec.push(
            button(container(text("All").size(14)).center_y(Length::Fill))
                .on_press(Message::SetDisplayMode(DisplayMode::Curated))
                .style(all_button_style)
                .padding([0, 16])
                .height(HEIGHT)
                .into(),
        );

        // Add individual library tabs
        for library in state
            .domains
            .ui
            .state
            .repo_accessor
            .get_archived_libraries()
            .unwrap()
            .iter()
            .map(|l| l.get())
            .filter(|l| l.enabled)
        {
            let name: String = deserialize::<String, Error>(&library.name).unwrap();
            let id = deserialize::<LibraryID, Error>(&library.id).unwrap();
            // Check if this library tab is active
            let is_active = state.tab_manager.active_tab_id() == TabId::Library(id);
            let button_style = if is_active {
                theme::Button::Primary.style()
            } else {
                theme::Button::HeaderIcon.style()
            };

            tabs_vec.push(
                button(container(text(name).size(14)).center_y(Length::Fill))
                    .on_press(Message::SelectLibraryAndMode(id))
                    .style(button_style)
                    .padding([0, 16])
                    .height(HEIGHT)
                    .into(),
            );
        }
        row(tabs_vec).into()
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn get_detail_title(state: &State) -> String {
    let mut buff = Uuid::encode_buffer();
    match &state.domains.ui.state.view {
        ViewState::MovieDetail { movie_id, .. } => "Placeholder".to_string(), //movie,
        ViewState::TvShowDetail { series_id, .. } => {
            /*
            // Use MediaQueryService (clean architecture)
            state
                .domains
                .media
                .state
                .query_service
                .get_series_title(series_id) */
            "Null".to_string()
        }
        ViewState::SeasonDetail { series_id, .. } => {
            /*
            // Use MediaQueryService (clean architecture)
            state
                .domains
                .media
                .state
                .query_service
                .get_series_title(series_id) */
            "Null".to_string()
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
