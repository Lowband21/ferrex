use super::library_filter_panel::library_filter_panel;
use crate::{
    domains::ui::{
        feedback_ui::FeedbackMessage,
        interaction_ui::InteractionMessage,
        messages::UiMessage,
        shell_ui::Scope,
        theme,
        views::{
            grid::{
                virtual_movie_references_grid, virtual_series_references_grid,
            },
            home::view_home_content,
        },
        widgets::collect_cached_handles_for_media,
    },
    state::State,
};
use ferrex_core::player_prelude::ImageSize;
use ferrex_model::MediaType;
use iced::{
    Element, Length,
    widget::{Space, button, column, container, row, text},
};
use uuid::Uuid;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn library_loading() -> Element<'static, UiMessage> {
    // Note: This function returns 'static Element, so it cannot access state.
    // Font sizes remain hardcoded here as semantic tokens require state access.
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
pub fn view_library(state: &State) -> Element<'_, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;

    // debug timing disabled in tests to simplify renderer unification

    if state.loading {
        // Loading state
        library_loading()
    } else {
        // LEGACY: Error message if any
        let error_section: Element<UiMessage> =
            if let Some(error) = &state.domains.ui.state.error_message {
                container(
                    row![
                        text(error).color(theme::MediaServerTheme::ERROR),
                        Space::new().width(Length::Fill),
                        button("×")
                            .on_press(FeedbackMessage::ClearError.into())
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
                                .size(fonts.body_lg)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            Space::new().height(20),
                            text("Click 'Scan Library' to find media files")
                                .size(fonts.caption)
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
            let library_content = match state.domains.ui.state.scope {
                Scope::Home => {
                    // Always show all content in Curated mode, regardless of library selection
                    view_home_content(state)
                }
                Scope::Library(_) => {
                    // Use the tab system to get the active tab
                    use crate::domains::ui::tabs::TabState;
                    use crate::infra::api_types::LibraryType;

                    let active_tab = state.tab_manager.active_tab();
                    match active_tab {
                        TabState::Library(lib_state) => match lib_state
                            .library_type
                        {
                            LibraryType::Movies => {
                                // Preload textures for visible rows and a small prefetch window
                                let visible_range =
                                    lib_state.grid_state.visible_range.clone();
                                let preload_range = lib_state.grid_state.get_preload_range(crate::infra::constants::layout::virtual_grid::PREFETCH_ROWS_ABOVE);
                                let mut ids: Vec<Uuid> = Vec::new();
                                if let Some(slice) = lib_state
                                    .cached_index_ids
                                    .get(visible_range.clone())
                                {
                                    ids.extend(slice.iter().copied());
                                }
                                if let Some(slice) = lib_state
                                    .cached_index_ids
                                    .get(preload_range)
                                {
                                    ids.extend(slice.iter().copied());
                                }
                                // Deduplicate
                                ids.sort_unstable();
                                ids.dedup();
                                let handles = collect_cached_handles_for_media(
                                    ids.into_iter(),
                                    MediaType::Movie,
                                    ImageSize::poster(),
                                );
                                let budget = crate::infra::constants::performance_config::texture_upload::MAX_UPLOADS_PER_FRAME as usize;
                                //let preloader = texture_preloader(handles, budget);

                                let grid = virtual_movie_references_grid(
                                    &lib_state.cached_index_ids,
                                    &lib_state.grid_state,
                                    &state.domains.ui.state.hovered_media_id,
                                    |vp| {
                                        InteractionMessage::TabGridScrolled(vp)
                                            .into()
                                    },
                                    state,
                                );

                                // column![preloader, grid].into()
                                grid
                            }
                            LibraryType::Series => {
                                let visible_range =
                                    lib_state.grid_state.visible_range.clone();
                                let preload_range = lib_state.grid_state.get_preload_range(crate::infra::constants::layout::virtual_grid::PREFETCH_ROWS_ABOVE);
                                let mut ids: Vec<Uuid> = Vec::new();
                                if let Some(slice) = lib_state
                                    .cached_index_ids
                                    .get(visible_range.clone())
                                {
                                    ids.extend(slice.iter().copied());
                                }
                                if let Some(slice) = lib_state
                                    .cached_index_ids
                                    .get(preload_range)
                                {
                                    ids.extend(slice.iter().copied());
                                }
                                ids.sort_unstable();
                                ids.dedup();
                                let handles = collect_cached_handles_for_media(
                                    ids.into_iter(),
                                    MediaType::Series,
                                    ImageSize::poster(),
                                );
                                // let preloader = texture_preloader(handles, budget);

                                let grid = virtual_series_references_grid(
                                    &lib_state.cached_index_ids,
                                    &lib_state.grid_state,
                                    &state.domains.ui.state.hovered_media_id,
                                    |vp| {
                                        InteractionMessage::TabGridScrolled(vp)
                                            .into()
                                    },
                                    state,
                                );

                                // column![preloader, grid].into()
                                grid
                            }
                        },
                        TabState::Home(_all_state) => {
                            // Use the AllViewModel from all_state
                            view_home_content(state)
                        }
                    }
                }
                _ => {
                    // Other modes not implemented yet
                    view_home_content(state)
                }
            };

            let filter_panel: Option<Element<UiMessage>> =
                if state.domains.ui.state.show_filter_panel {
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

            main_content.into()
        }
    }
}
