use std::time::Instant;

use iced::Task;
use iced::widget::{operation::scroll_to, scrollable::AbsoluteOffset};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::tabs::{self, TabId, TabState};
use crate::infra::constants::virtual_carousel::layout as vcl;
use crate::state::State;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_home_scrolled(
    state: &mut State,
    viewport: iced::widget::scrollable::Viewport,
) -> Task<UiMessage> {
    if let Some(TabState::Home(home_state)) =
        state.tab_manager.get_tab_mut(TabId::Home)
    {
        let abs = viewport.absolute_offset();
        let bounds = viewport.bounds();
        home_state.focus.scroll_y = abs.y;
        home_state.focus.viewport_height = bounds.height;
    }

    // Keep background noise deterministically anchored to the Home scroll position.
    // We only update Y here; horizontal offset is driven by carousel ViewportChanged events.
    state
        .domains
        .ui
        .state
        .background_shader_state
        .set_vertical_scroll_px(viewport.absolute_offset().y);
    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_home_focus_next(state: &mut State) -> Task<UiMessage> {
    // Build ordered keys without holding a mutable borrow
    let ordered = tabs::ordered_keys_for_home(state);

    if let Some(TabState::Home(home_state)) =
        state.tab_manager.get_tab_mut(TabId::Home)
    {
        if home_state.focus.ordered_keys.is_empty() {
            home_state.focus.ordered_keys = ordered;
            if home_state.focus.active_carousel.is_none() {
                home_state.focus.active_carousel =
                    home_state.focus.ordered_keys.first().cloned();
            }
        }

        // Determine current and next indices
        let edge_eps = 2.0_f32;
        let vt = home_state.focus.scroll_y;
        let vb = vt + home_state.focus.viewport_height;
        let main_h = vcl::HEADER_HEIGHT_EST
            + vcl::HEADER_SCROLL_SPACING
            + vcl::SCROLL_HEIGHT;

        let cur_key = match &home_state.focus.active_carousel {
            Some(k) => k.clone(),
            None => {
                if let Some(first) = home_state.focus.ordered_keys.first() {
                    first.clone()
                } else {
                    return Task::none();
                }
            }
        };

        let cur_top = home_state.focus.section_top_y(&cur_key).unwrap_or(0.0);
        let cur_bottom = cur_top + main_h;
        let at_bottom_edge = cur_bottom >= vb - edge_eps;

        if let Some(next) = home_state.focus.next_key() {
            // Change selection first
            home_state.focus.active_carousel = Some(next.clone());
            // Sync carousel keyboard focus so arrow keys follow the rail selection
            state
                .domains
                .ui
                .state
                .carousel_focus
                .set_keyboard_active(Some(next.clone()));

            if at_bottom_edge {
                // Compute minimal push scroll so next is fully in view
                let next_top =
                    home_state.focus.section_top_y(&next).unwrap_or(cur_top);
                let next_bottom = next_top + main_h;
                let overshoot = next_bottom - vb;
                if overshoot > edge_eps {
                    let target_y = vt + overshoot;
                    return start_vertical_snap_to_y(state, target_y);
                }
            }
        }
    }
    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_home_focus_prev(state: &mut State) -> Task<UiMessage> {
    let ordered = tabs::ordered_keys_for_home(state);
    if let Some(TabState::Home(home_state)) =
        state.tab_manager.get_tab_mut(TabId::Home)
    {
        if home_state.focus.ordered_keys.is_empty() {
            home_state.focus.ordered_keys = ordered;
            if home_state.focus.active_carousel.is_none() {
                home_state.focus.active_carousel =
                    home_state.focus.ordered_keys.first().cloned();
            }
        }

        let edge_eps = 2.0_f32;
        let vt = home_state.focus.scroll_y;
        let _main_h = vcl::HEADER_HEIGHT_EST
            + vcl::HEADER_SCROLL_SPACING
            + vcl::SCROLL_HEIGHT;

        let cur_key = match &home_state.focus.active_carousel {
            Some(k) => k.clone(),
            None => {
                if let Some(first) = home_state.focus.ordered_keys.first() {
                    first.clone()
                } else {
                    return Task::none();
                }
            }
        };

        let cur_top = home_state.focus.section_top_y(&cur_key).unwrap_or(0.0);
        let at_top_edge = cur_top <= vt + edge_eps;

        if let Some(prev) = home_state.focus.prev_key() {
            // Change selection first
            home_state.focus.active_carousel = Some(prev.clone());
            // Sync carousel keyboard focus so arrow keys follow the rail selection
            state
                .domains
                .ui
                .state
                .carousel_focus
                .set_keyboard_active(Some(prev.clone()));

            if at_top_edge {
                // Minimal push scroll so previous is fully in view above
                let prev_top =
                    home_state.focus.section_top_y(&prev).unwrap_or(cur_top);
                let overshoot = vt - prev_top;
                if overshoot > edge_eps {
                    let mut target_y = vt - overshoot;
                    if target_y < 0.0 {
                        target_y = 0.0;
                    }
                    return start_vertical_snap_to_y(state, target_y);
                }
            }
        }
    }
    Task::none()
}

// fn start_vertical_snap_to_key(
//     state: &mut State,
//     key: crate::domains::ui::views::virtual_carousel::types::CarouselKey,
// ) -> Task<UiMessage> {
//     // Read snap animation settings from runtime config
//     let snap_page_duration = state.runtime_config.snap_page_duration_ms();
//     let snap_easing = state.runtime_config.snap_easing().to_u8();

//     let Some(TabState::Home(home_state)) =
//         state.tab_manager.get_tab_mut(TabId::Home)
//     else {
//         return Task::none();
//     };
//     let Some(target_top) = home_state.focus.section_top_y(&key) else {
//         return Task::none();
//     };
//     let current = home_state.focus.scroll_y;
//     // Align near the top with a small offset for breathing room
//     let target_y = (target_top - 8.0).max(0.0);
//     if (target_y - current).abs() < 1.0 {
//         return Task::none();
//     }
//     home_state.focus.vertical_animator.start_at(
//         current,
//         target_y,
//         snap_page_duration,
//         snap_easing,
//         Instant::now(),
//     );
//     // Motion ticks will drive the scroll via handle_home_focus_tick
//     Task::none()
// }

fn start_vertical_snap_to_y(
    state: &mut State,
    target_y: f32,
) -> Task<UiMessage> {
    // Read snap animation settings from runtime config
    let snap_page_duration = state.runtime_config.snap_page_duration_ms();
    let snap_easing = state.runtime_config.snap_easing().to_u8();

    let Some(TabState::Home(home_state)) =
        state.tab_manager.get_tab_mut(TabId::Home)
    else {
        return Task::none();
    };
    let current = home_state.focus.scroll_y;
    if (target_y - current).abs() < 1.0 {
        return Task::none();
    }
    home_state.focus.vertical_animator.start_at(
        current,
        target_y,
        snap_page_duration,
        snap_easing,
        Instant::now(),
    );
    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_home_focus_tick(
    state: &mut State,
    now: Instant,
) -> Task<UiMessage> {
    let Some(TabState::Home(home_state)) =
        state.tab_manager.get_tab_mut(TabId::Home)
    else {
        return Task::none();
    };

    if !home_state.focus.vertical_animator.is_active_at(now) {
        return Task::none();
    }

    // Use frame-synchronized timestamp for smooth animation
    if let Some(next_y) = home_state.focus.vertical_animator.tick_at(now) {
        home_state.focus.scroll_y = next_y;
        state
            .domains
            .ui
            .state
            .background_shader_state
            .set_vertical_scroll_px(next_y);
        let id = home_state.focus.scrollable_id.clone();
        return scroll_to::<UiMessage>(
            id,
            AbsoluteOffset { x: 0.0, y: next_y },
        );
    }

    Task::none()
}
