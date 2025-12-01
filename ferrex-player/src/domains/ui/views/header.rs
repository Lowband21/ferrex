use crate::{
    common::ui_utils::icon_text_with_size,
    domains::{
        auth::permissions::StatePermissionExt,
        ui::{
            messages::UiMessage,
            settings_ui::SettingsUiMessage,
            shell_ui::{Scope, UiShellMessage},
            theme,
            types::ViewState,
        },
    },
    infra::constants::layout::header::HEIGHT,
    state::State,
};

use ferrex_core::player_prelude::LibraryId;

use iced::{
    Element, Length,
    widget::{Space, Stack, button, container, row, text},
};

use rkyv::{deserialize, rancor::Error};

use lucide_icons::Icon;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_header<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;

    match &state.domains.ui.state.view {
        ViewState::Library => {
            // New header layout: Left (Home, Back if history exists, Library tabs), Center (Search), Right (Controls)
            let mut left_section_items = vec![];

            // Home button
            left_section_items.push(
                button(
                    container(icon_text_with_size(Icon::House, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::NavigateHome.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            // Back button (only if navigation history exists)
            if !state.domains.ui.state.navigation_history.is_empty() {
                left_section_items.push(
                    button(
                        container(icon_text_with_size(Icon::ChevronLeft, 16.0))
                            .center_x(Length::Fill)
                            .center_y(Length::Fill),
                    )
                    .on_press(UiShellMessage::NavigateBack.into())
                    .style(theme::Button::HeaderIcon.style())
                    .width(Length::Fixed(HEIGHT))
                    .height(HEIGHT)
                    .into(),
                );
            }

            left_section_items.push(Space::new().width(20).into()); // Gap between buttons and library tabs

            // Library tabs
            left_section_items.push(
                container(create_library_tabs(state))
                    .align_y(iced::alignment::Vertical::Center)
                    .into(),
            );

            let left_section =
                row(left_section_items).align_y(iced::Alignment::Center);

            // Center section left empty to keep layout balanced
            let center_section =
                container(Space::new().width(Length::Shrink).height(HEIGHT))
                    .width(Length::Shrink)
                    .height(HEIGHT)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center);

            // Right section - Controls
            let search_button = button(
                container(icon_text_with_size(Icon::Search, 16.0))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .on_press(UiShellMessage::OpenSearchWindow.into())
            .style(theme::Button::HeaderIcon.style())
            .width(Length::Fixed(HEIGHT))
            .height(HEIGHT);

            let right_section = row![
                search_button,
                button(
                    container(icon_text_with_size(
                        if state.is_fullscreen {
                            Icon::Minimize
                        } else {
                            Icon::Maximize
                        },
                        16.0,
                    ))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::ToggleFullscreen.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            ]
            .align_y(iced::Alignment::Center);

            let mut right_section = right_section;

            if !state.domains.library.state.active_scans.is_empty() {
                let active_count =
                    state.domains.library.state.active_scans.len();
                right_section = right_section.push(
                    container(
                        row![
                            icon_text_with_size(Icon::FileScan, 16.0),
                            text(format!(" {}", active_count))
                                .size(fonts.caption as f32)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        ]
                        .spacing(6)
                        .align_y(iced::Alignment::Center),
                    )
                    .padding([0, 12])
                    .style(theme::Container::HeaderAccent.style()),
                );
            }

            right_section = right_section.push({
                let element: Element<UiMessage> = if state
                    .permission_checker()
                    .can_view_admin_dashboard()
                {
                    button(
                        container(icon_text_with_size(Icon::Settings, 16.0))
                            .center_x(Length::Fill)
                            .center_y(Length::Fill),
                    )
                    .on_press(SettingsUiMessage::ShowLibraryManagement.into())
                    .style(theme::Button::HeaderIcon.style())
                    .width(Length::Fixed(HEIGHT))
                    .height(HEIGHT)
                    .into()
                } else {
                    Space::new().width(HEIGHT).into()
                };
                element
            });

            right_section = right_section.push(
                button(
                    container(icon_text_with_size(Icon::UserPen, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(SettingsUiMessage::ShowProfile.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            );

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
                            .padding([0, 0])
                            .align_y(iced::alignment::Vertical::Center),
                        Space::new().width(Length::Fill),
                        container(right_section)
                            .padding([0, 0])
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
        | ViewState::SeriesDetail { .. }
        | ViewState::SeasonDetail { .. }
        | ViewState::EpisodeDetail { .. } => {
            // Detail views header with global search in the center
            let mut left_section_items = vec![];

            // Home button
            left_section_items.push(
                button(
                    container(icon_text_with_size(Icon::House, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::NavigateHome.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            // Back button (always shown in detail views since we came from somewhere)
            left_section_items.push(
                button(
                    container(icon_text_with_size(Icon::ChevronLeft, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::NavigateBack.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            let left_section =
                row(left_section_items).align_y(iced::Alignment::Center);

            let center_section =
                container(Space::new().width(Length::Shrink).height(HEIGHT))
                    .width(Length::Shrink)
                    .height(HEIGHT)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center);

            // Right section - same controls as library view
            let search_button = button(
                container(icon_text_with_size(Icon::Search, 16.0))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .on_press(UiShellMessage::OpenSearchWindow.into())
            .style(theme::Button::HeaderIcon.style())
            .width(Length::Fixed(HEIGHT))
            .height(HEIGHT);

            let right_section = row![
                search_button,
                // Fullscreen toggle
                button(
                    container(icon_text_with_size(
                        if state.is_fullscreen {
                            Icon::Minimize
                        } else {
                            Icon::Maximize
                        },
                        16.0,
                    ))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                )
                .on_press(UiShellMessage::ToggleFullscreen.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
                // Admin settings (show only if user has permissions)
                {
                    let admin_element: Element<'_, UiMessage> = if state
                        .permission_checker()
                        .can_view_admin_dashboard()
                    {
                        button(
                            container(icon_text_with_size(
                                Icon::Settings,
                                16.0,
                            ))
                            .center_x(Length::Fill)
                            .center_y(Length::Fill),
                        )
                        .on_press(
                            SettingsUiMessage::ShowLibraryManagement.into(),
                        )
                        .style(theme::Button::HeaderIcon.style())
                        .width(Length::Fixed(HEIGHT))
                        .height(HEIGHT)
                        .into()
                    } else {
                        Space::new().width(HEIGHT).into()
                    };
                    admin_element
                },
                button(
                    container(icon_text_with_size(Icon::UserPen, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .on_press(SettingsUiMessage::ShowProfile.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            ]
            .align_y(iced::Alignment::Center);

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
                            .padding([0, 0])
                            .align_y(iced::alignment::Vertical::Center),
                        Space::new().width(Length::Fill),
                        container(right_section)
                            .padding([0, 0])
                            .align_y(iced::alignment::Vertical::Center),
                    ]
                    .width(Length::Fill)
                    .height(HEIGHT),
                )
                .width(Length::Fill)
                .height(HEIGHT)
                .into()
        }
        ViewState::AdminDashboard => {
            // Generic header for admin dashboard with back/home and controls
            let mut left_section_items = vec![];

            // Home button
            left_section_items.push(
                button(
                    container(icon_text_with_size(Icon::House, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::NavigateHome.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            // Back button
            left_section_items.push(
                button(
                    container(icon_text_with_size(Icon::ChevronLeft, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(SettingsUiMessage::HideAdminDashboard.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            let left_section =
                row(left_section_items).align_y(iced::Alignment::Center);

            // Right section - Controls
            let mut right_section = row![
                // Fullscreen toggle
                button(
                    container(icon_text_with_size(
                        if state.is_fullscreen {
                            Icon::Minimize
                        } else {
                            Icon::Maximize
                        },
                        16.0,
                    ))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                )
                .on_press(UiShellMessage::ToggleFullscreen.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            ]
            .align_y(iced::Alignment::Center);

            // Library management (admin) button
            right_section = right_section.push({
                let element: Element<UiMessage> = if state
                    .permission_checker()
                    .can_view_admin_dashboard()
                {
                    button(
                        container(icon_text_with_size(Icon::Settings, 16.0))
                            .center_x(Length::Fill)
                            .center_y(Length::Fill),
                    )
                    .on_press(SettingsUiMessage::ShowLibraryManagement.into())
                    .style(theme::Button::HeaderIcon.style())
                    .width(Length::Fixed(HEIGHT))
                    .height(HEIGHT)
                    .into()
                } else {
                    Space::new().width(HEIGHT).into()
                };
                element
            });

            // Users management button
            right_section = right_section.push({
                let element: Element<UiMessage> =
                    if state.permission_checker().can_view_users() {
                        button(
                            container(icon_text_with_size(Icon::Users, 16.0))
                                .center_x(Length::Fill)
                                .center_y(Length::Fill),
                        )
                        .on_press(SettingsUiMessage::ShowUserManagement.into())
                        .style(theme::Button::HeaderIcon.style())
                        .width(Length::Fixed(HEIGHT))
                        .height(HEIGHT)
                        .into()
                    } else {
                        Space::new().width(HEIGHT).into()
                    };
                element
            });

            // Profile button
            right_section = right_section.push(
                button(
                    container(icon_text_with_size(Icon::UserPen, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(SettingsUiMessage::ShowProfile.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            );

            Stack::new()
                .push(
                    // Base layer: centered title
                    container(
                        text("Admin Dashboard")
                            .size(fonts.subtitle as f32)
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
                            .padding([0, 0])
                            .align_y(iced::alignment::Vertical::Center),
                        Space::new().width(Length::Fill),
                        container(right_section)
                            .padding([0, 0])
                            .align_y(iced::alignment::Vertical::Center),
                    ]
                    .width(Length::Fill)
                    .height(HEIGHT),
                )
                .width(Length::Fill)
                .height(HEIGHT)
                .into()
        }
        ViewState::UserSettings => {
            // Simple header for user settings view
            let left_section = row![
                button(
                    container(icon_text_with_size(Icon::House, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::NavigateHome.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
                button(
                    container(icon_text_with_size(Icon::ChevronLeft, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::NavigateBack.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            ]
            .align_y(iced::Alignment::Center);

            let mut right_section = row![
                button(
                    container(icon_text_with_size(
                        if state.is_fullscreen {
                            Icon::Minimize
                        } else {
                            Icon::Maximize
                        },
                        16.0,
                    ))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                )
                .on_press(UiShellMessage::ToggleFullscreen.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            ]
            .align_y(iced::Alignment::Center);

            right_section = right_section.push(
                button(
                    container(icon_text_with_size(Icon::UserPen, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(SettingsUiMessage::ShowProfile.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT),
            );

            Stack::new()
                .push(
                    container(
                        text("User Settings")
                            .size(fonts.subtitle as f32)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    )
                    .width(Length::Fill)
                    .height(HEIGHT)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .push(
                    row![
                        container(left_section)
                            .padding([0, 0])
                            .align_y(iced::alignment::Vertical::Center),
                        Space::new().width(Length::Fill),
                        container(right_section)
                            .padding([0, 0])
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
                    container(icon_text_with_size(Icon::House, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::NavigateHome.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            // Back button (always shown since we came from library)
            left_section_items.push(
                button(
                    container(icon_text_with_size(Icon::ChevronLeft, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::NavigateBack.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            let left_section =
                row(left_section_items).align_y(iced::Alignment::Center);

            Stack::new()
                .push(
                    // Base layer: centered title
                    container(
                        text("Library Management")
                            .size(fonts.subtitle as f32)
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
                        .padding([0, 0])
                        .align_x(iced::alignment::Horizontal::Left)
                        .align_y(iced::alignment::Vertical::Center),
                )
                .width(Length::Fill)
                .height(HEIGHT)
                .into()
        }
        // Note: Duplicate AdminDashboard branch removed (handled above)
        ViewState::AdminUsers => {
            // Header for User Management view
            let mut left_section_items = vec![];

            // Home button
            left_section_items.push(
                button(
                    container(icon_text_with_size(Icon::House, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::NavigateHome.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            // Back button
            left_section_items.push(
                button(
                    container(icon_text_with_size(Icon::ChevronLeft, 16.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .on_press(UiShellMessage::NavigateBack.into())
                .style(theme::Button::HeaderIcon.style())
                .width(Length::Fixed(HEIGHT))
                .height(HEIGHT)
                .into(),
            );

            let left_section =
                row(left_section_items).align_y(iced::Alignment::Center);

            Stack::new()
                .push(
                    container(
                        text("User Management")
                            .size(fonts.subtitle as f32)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    )
                    .width(Length::Fill)
                    .height(HEIGHT)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .push(
                    container(left_section)
                        .width(Length::Fill)
                        .height(HEIGHT)
                        .padding([0, 0])
                        .align_x(iced::alignment::Horizontal::Left)
                        .align_y(iced::alignment::Vertical::Center),
                )
                .width(Length::Fill)
                .height(HEIGHT)
                .into()
        }
        _ => {
            // No header for other views
            Space::new().height(0).into()
        }
    }
}

fn create_library_tabs<'a>(state: &'a State) -> Element<'a, UiMessage> {
    use crate::domains::ui::tabs::TabId;

    let fonts = &state.domains.ui.state.size_provider.font;

    if !state.domains.ui.state.repo_accessor.is_initialized() {
        // No libraries configured - show only curated view
        row![
            button(
                container(text("Home").size(fonts.caption))
                    .center_y(Length::Fill)
            )
            .on_press(UiShellMessage::SelectScope(Scope::Home).into())
            .style(theme::Button::HeaderIcon.style())
            .padding([0, 16])
            .height(HEIGHT),
        ]
        .into()
    } else {
        // Show library tabs
        let mut tabs_vec: Vec<Element<UiMessage>> = Vec::new();

        // Check if Home tab is active
        let is_all_active = state.tab_manager.active_tab_id() == TabId::Home;
        let all_button_style = if is_all_active {
            theme::Button::HeaderTabActive.style()
        } else {
            theme::Button::HeaderIcon.style()
        };

        // Add Home tab - shows curated collections from all libraries
        tabs_vec.push(
            button(
                container(text("Home").size(fonts.caption))
                    .center_y(Length::Fill),
            )
            .on_press(UiShellMessage::SelectScope(Scope::Home).into())
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
            let name: String =
                deserialize::<String, Error>(&library.name).unwrap();
            let id = deserialize::<LibraryId, Error>(&library.id).unwrap();
            // Check if this library tab is active
            let is_active =
                state.tab_manager.active_tab_id() == TabId::Library(id);
            let button_style = if is_active {
                theme::Button::HeaderTabActive.style()
            } else {
                theme::Button::HeaderIcon.style()
            };

            tabs_vec.push(
                button(
                    container(text(name).size(fonts.caption))
                        .center_y(Length::Fill),
                )
                .on_press(
                    UiShellMessage::SelectScope(Scope::Library(id)).into(),
                )
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
    match &state.domains.ui.state.view {
        ViewState::MovieDetail { .. } => "Placeholder".to_string(), //movie,
        ViewState::SeriesDetail { .. } => {
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
        ViewState::SeasonDetail { .. } => {
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
