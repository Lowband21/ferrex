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
    let current_sort = state.domains.ui.state.sort_by;
    let current_order = state.domains.ui.state.sort_order;

    // TODO: Get active filter count from state when filters are implemented
    let active_filter_count = 0;
    let is_filter_open = false;

    // Create the controls row
    let controls = row![
        // Sort dropdown
        sort_dropdown(current_sort),
        Space::with_width(8),
        // Sort order toggle
        sort_order_toggle(current_order),
        Space::with_width(24),
        // Filter button
        filter_button(active_filter_count, is_filter_open),
        // Spacer to push any future controls to the right
        Space::with_width(Length::Fill),
    ]
    .padding([0, 20])
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
