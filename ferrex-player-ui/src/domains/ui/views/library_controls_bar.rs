use crate::{
    domains::ui::{
        messages::UiMessage,
        tabs::{TabId, TabState},
        widgets::library_sort_filter_menu,
    },
    infra::constants::layout::header::HEIGHT,
    state::State,
};
use ferrex_core::player_prelude::{LibraryId, UiResolution, UiWatchStatus};
use ferrex_model::LibraryType;
use iced::{Element, Length, widget::container};

/// Creates the library controls bar that appears below the header
/// This bar contains sort and filter controls and is only visible when viewing specific libraries
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_library_controls_bar<'a>(
    state: &'a State,
    lib_id: LibraryId,
    lib_type: &LibraryType,
) -> Option<Element<'a, UiMessage>> {
    // Only show controls for specific libraries, not the Home view

    // Get current sort settings from state
    let ui_state = &state.domains.ui.state;
    let current_sort = ui_state.sort_by;
    let current_order = ui_state.sort_order;

    let item_count = match lib_type {
        LibraryType::Movies => {
            let movie_num = state
                .tab_manager
                .get_tab(TabId::Library(lib_id))
                .and_then(|tab| match tab {
                    TabState::Library(lib_state) => {
                        Some(lib_state.grid_state.total_items)
                    }
                    _ => None,
                })
                .unwrap_or(0)
                .to_string();
            let mut ret = String::with_capacity(12);
            ret.push_str(&movie_num.to_string());
            ret.push_str(" Movies");
            ret
        }
        LibraryType::Series => {
            let series_num = state
                .tab_manager
                .get_tab(TabId::Library(lib_id))
                .and_then(|tab| match tab {
                    TabState::Library(lib_state) => {
                        Some(lib_state.grid_state.total_items)
                    }
                    _ => None,
                })
                .unwrap_or(0);
            let episode_num = state
                .domains
                .ui
                .state
                .repo_accessor
                .episode_len(&lib_id)
                .ok()?;
            let mut ret = String::with_capacity(24);
            ret.push_str(&series_num.to_string());
            ret.push_str(" Series ");
            ret.push_str(&episode_num.to_string());
            ret.push_str(" Episodes");
            ret
        }
    };

    let active_filter_count = ui_state.selected_genres.len()
        + ui_state.selected_decade.iter().count()
        + if ui_state.selected_resolution != UiResolution::Any {
            1
        } else {
            0
        }
        + if ui_state.selected_watch_status != UiWatchStatus::Any {
            1
        } else {
            0
        };
    let is_filter_open = ui_state.show_filter_panel;

    let controls = container(library_sort_filter_menu(
        current_sort,
        current_order,
        active_filter_count,
        is_filter_open,
        item_count,
    ))
    .padding([0, 0])
    .width(Length::Fill)
    .align_y(iced::Alignment::Center);

    Some(
        container(controls)
            .width(Length::Fill)
            .height(HEIGHT)
            .style(super::super::theme::Container::HeaderAccent.style())
            .align_y(iced::alignment::Vertical::Center)
            .into(),
    )
}

/// Calculate the total height needed for header + controls bar
pub fn calculate_top_bars_height(has_library_selected: bool) -> f32 {
    if has_library_selected {
        HEIGHT * 2.0 // Header + controls bar
    } else {
        HEIGHT // Just header
    }
}
