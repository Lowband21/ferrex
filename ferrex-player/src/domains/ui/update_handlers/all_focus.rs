use iced::Task;
use iced::widget::{operation::scroll_to, scrollable::AbsoluteOffset};

use crate::domains::ui::messages::Message;
use crate::domains::ui::tabs::{TabId, TabState};
use crate::infra::constants::virtual_carousel::layout as vcl;
use crate::infra::constants::virtual_carousel::snap as snap_consts;
use crate::state::State;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_all_view_scrolled(
    state: &mut State,
    viewport: iced::widget::scrollable::Viewport,
) -> Task<Message> {
    if let Some(TabState::All(all_state)) =
        state.tab_manager.get_tab_mut(TabId::All)
    {
        let abs = viewport.absolute_offset();
        let bounds = viewport.bounds();
        all_state.focus.scroll_y = abs.y;
        all_state.focus.viewport_height = bounds.height;
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
pub fn handle_all_focus_next(state: &mut State) -> Task<Message> {
    // Build ordered keys without holding a mutable borrow
    let ordered = crate::domains::ui::tabs::ordered_keys_for_all_view(state);

    if let Some(TabState::All(all_state)) =
        state.tab_manager.get_tab_mut(TabId::All)
    {
        if all_state.focus.ordered_keys.is_empty() {
            all_state.focus.ordered_keys = ordered;
            if all_state.focus.active_carousel.is_none() {
                all_state.focus.active_carousel =
                    all_state.focus.ordered_keys.first().cloned();
            }
        }

        // Determine current and next indices
        let edge_eps = 2.0_f32;
        let vt = all_state.focus.scroll_y;
        let vb = vt + all_state.focus.viewport_height;
        let main_h = vcl::HEADER_HEIGHT_EST
            + vcl::HEADER_SCROLL_SPACING
            + vcl::SCROLL_HEIGHT;

        let cur_key = match &all_state.focus.active_carousel {
            Some(k) => k.clone(),
            None => {
                if let Some(first) = all_state.focus.ordered_keys.first() {
                    first.clone()
                } else {
                    return Task::none();
                }
            }
        };

        let cur_top = all_state.focus.section_top_y(&cur_key).unwrap_or(0.0);
        let cur_bottom = cur_top + main_h;
        let at_bottom_edge = cur_bottom >= vb - edge_eps;

        if let Some(next) = all_state.focus.next_key() {
            // Change selection first
            all_state.focus.active_carousel = Some(next.clone());
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
                    all_state.focus.section_top_y(&next).unwrap_or(cur_top);
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
pub fn handle_all_focus_prev(state: &mut State) -> Task<Message> {
    let ordered = crate::domains::ui::tabs::ordered_keys_for_all_view(state);
    if let Some(TabState::All(all_state)) =
        state.tab_manager.get_tab_mut(TabId::All)
    {
        if all_state.focus.ordered_keys.is_empty() {
            all_state.focus.ordered_keys = ordered;
            if all_state.focus.active_carousel.is_none() {
                all_state.focus.active_carousel =
                    all_state.focus.ordered_keys.first().cloned();
            }
        }

        let edge_eps = 2.0_f32;
        let vt = all_state.focus.scroll_y;
        let main_h = vcl::HEADER_HEIGHT_EST
            + vcl::HEADER_SCROLL_SPACING
            + vcl::SCROLL_HEIGHT;

        let cur_key = match &all_state.focus.active_carousel {
            Some(k) => k.clone(),
            None => {
                if let Some(first) = all_state.focus.ordered_keys.first() {
                    first.clone()
                } else {
                    return Task::none();
                }
            }
        };

        let cur_top = all_state.focus.section_top_y(&cur_key).unwrap_or(0.0);
        let at_top_edge = cur_top <= vt + edge_eps;

        if let Some(prev) = all_state.focus.prev_key() {
            // Change selection first
            all_state.focus.active_carousel = Some(prev.clone());
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
                    all_state.focus.section_top_y(&prev).unwrap_or(cur_top);
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

fn start_vertical_snap_to_key(
    state: &mut State,
    key: crate::domains::ui::views::virtual_carousel::types::CarouselKey,
) -> Task<Message> {
    let Some(TabState::All(all_state)) =
        state.tab_manager.get_tab_mut(TabId::All)
    else {
        return Task::none();
    };
    let Some(target_top) = all_state.focus.section_top_y(&key) else {
        return Task::none();
    };
    let current = all_state.focus.scroll_y;
    // Align near the top with a small offset for breathing room
    let target_y = (target_top - 8.0).max(0.0);
    if (target_y - current).abs() < 1.0 {
        return Task::none();
    }
    all_state.focus.vertical_animator.start(
        current,
        target_y,
        snap_consts::PAGE_DURATION_MS,
        snap_consts::EASING_KIND,
    );
    // Motion ticks will drive the scroll via handle_all_focus_tick
    Task::none()
}

fn start_vertical_snap_to_y(state: &mut State, target_y: f32) -> Task<Message> {
    let Some(TabState::All(all_state)) =
        state.tab_manager.get_tab_mut(TabId::All)
    else {
        return Task::none();
    };
    let current = all_state.focus.scroll_y;
    if (target_y - current).abs() < 1.0 {
        return Task::none();
    }
    all_state.focus.vertical_animator.start(
        current,
        target_y,
        snap_consts::PAGE_DURATION_MS,
        snap_consts::EASING_KIND,
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
pub fn handle_all_focus_tick(state: &mut State) -> Task<Message> {
    let Some(TabState::All(all_state)) =
        state.tab_manager.get_tab_mut(TabId::All)
    else {
        return Task::none();
    };

    if !all_state.focus.vertical_animator.is_active() {
        return Task::none();
    }

    if let Some(next_y) = all_state.focus.vertical_animator.tick() {
        all_state.focus.scroll_y = next_y;
        let id = all_state.focus.scrollable_id.clone();
        return scroll_to::<Message>(id, AbsoluteOffset { x: 0.0, y: next_y });
    }

    Task::none()
}
