use crate::{
    domains::ui::{
        messages::Message,
        widgets::{filter_button, sort_dropdown, sort_order_toggle},
    },
    infrastructure::constants::layout::header::HEIGHT,
    state_refactored::State,
};
use iced::{
    Element, Length,
    widget::{Space, container, row},
};
use uuid::Uuid;

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
    selected_library: Option<Uuid>,
) -> Option<Element<'a, Message>> {
    // Only show controls for specific libraries, not the "All" view
    if selected_library.is_none() {
        return None;
    }

    // Get current sort settings from state
    let ui_state = &state.domains.ui.state;
    let current_sort = ui_state.sort_by;
    let current_order = ui_state.sort_order;

    let active_filter_count = ui_state.selected_genres.len()
        + ui_state.selected_decade.iter().count()
        + if ui_state.selected_resolution != ferrex_core::UiResolution::Any {
            1
        } else {
            0
        }
        + if ui_state.selected_watch_status != ferrex_core::UiWatchStatus::Any {
            1
        } else {
            0
        };
    let is_filter_open = ui_state.show_filter_panel;

    // Create the controls row
    let controls = row![
        sort_dropdown(current_sort),
        sort_order_toggle(current_order),
        filter_button(active_filter_count, is_filter_open),
        Space::with_width(Length::Fill),
    ]
    .spacing(12)
    .padding([0, 16])
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
