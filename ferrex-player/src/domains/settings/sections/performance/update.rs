//! Performance section update handlers

use super::messages::PerformanceMessage;
use super::state::EasingKind;
use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Main message handler for performance section
pub fn handle_message(
    state: &mut State,
    message: PerformanceMessage,
) -> DomainUpdateResult {
    match message {
        // Scrolling
        PerformanceMessage::SetScrollDebounceMs(ms) => {
            set_scroll_debounce_ms(state, ms)
        }
        PerformanceMessage::SetScrollTickNs(ns) => {
            set_scroll_tick_ns(state, ns)
        }
        PerformanceMessage::SetScrollDecayTauMs(ms) => {
            set_scroll_decay_tau_ms(state, ms)
        }
        PerformanceMessage::SetScrollBaseVelocity(v) => {
            set_scroll_base_velocity(state, v)
        }
        PerformanceMessage::SetScrollMaxVelocity(v) => {
            set_scroll_max_velocity(state, v)
        }
        PerformanceMessage::SetScrollMinStopVelocity(v) => {
            set_scroll_min_stop_velocity(state, v)
        }
        PerformanceMessage::SetScrollRampMs(ms) => {
            set_scroll_ramp_ms(state, ms)
        }
        PerformanceMessage::SetScrollBoostMultiplier(m) => {
            set_scroll_boost_multiplier(state, m)
        }
        PerformanceMessage::SetScrollEasing(e) => set_scroll_easing(state, e),

        // Texture Upload
        PerformanceMessage::SetTextureMaxUploadsPerFrame(n) => {
            set_texture_max_uploads_per_frame(state, n)
        }

        // Prefetch
        PerformanceMessage::SetPrefetchRowsAbove(n) => {
            set_prefetch_rows_above(state, n)
        }
        PerformanceMessage::SetPrefetchRowsBelow(n) => {
            set_prefetch_rows_below(state, n)
        }
        PerformanceMessage::SetPrefetchKeepAliveMs(ms) => {
            set_prefetch_keep_alive_ms(state, ms)
        }

        // Carousel
        PerformanceMessage::SetCarouselPrefetchItems(n) => {
            set_carousel_prefetch_items(state, n)
        }
        PerformanceMessage::SetCarouselBackgroundItems(n) => {
            set_carousel_background_items(state, n)
        }
        PerformanceMessage::SetCarouselBaseVelocity(v) => {
            set_carousel_base_velocity(state, v)
        }
        PerformanceMessage::SetCarouselMaxVelocity(v) => {
            set_carousel_max_velocity(state, v)
        }
        PerformanceMessage::SetCarouselBoostMultiplier(m) => {
            set_carousel_boost_multiplier(state, m)
        }
        PerformanceMessage::SetCarouselRampMs(ms) => {
            set_carousel_ramp_ms(state, ms)
        }
        PerformanceMessage::SetCarouselDecayTauMs(ms) => {
            set_carousel_decay_tau_ms(state, ms)
        }
        PerformanceMessage::SetCarouselItemSnapMs(ms) => {
            set_carousel_item_snap_ms(state, ms)
        }
        PerformanceMessage::SetCarouselPageSnapMs(ms) => {
            set_carousel_page_snap_ms(state, ms)
        }
        PerformanceMessage::SetCarouselHoldTapThresholdMs(ms) => {
            set_carousel_hold_tap_threshold_ms(state, ms)
        }
        PerformanceMessage::SetCarouselSnapEpsilon(e) => {
            set_carousel_snap_epsilon(state, e)
        }
        PerformanceMessage::SetCarouselAnchorSettleMs(ms) => {
            set_carousel_anchor_settle_ms(state, ms)
        }

        // Animation Effects
        PerformanceMessage::SetAnimationHoverScale(s) => {
            set_animation_hover_scale(state, s)
        }
        PerformanceMessage::SetAnimationHoverTransitionMs(ms) => {
            set_animation_hover_transition_ms(state, ms)
        }
        PerformanceMessage::SetAnimationHoverScaleDownDelayMs(ms) => {
            set_animation_hover_scale_down_delay_ms(state, ms)
        }
    }
}

// Scrolling handlers - accept String for UI-visible fields, parse and validate
fn set_scroll_debounce_ms(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(ms) = value.parse::<u64>()
        && (5..=200).contains(&ms)
    {
        state.domains.settings.performance.scroll_debounce_ms = ms;
    }
    DomainUpdateResult::none()
}

fn set_scroll_tick_ns(state: &mut State, ns: u64) -> DomainUpdateResult {
    let _ = (state, ns);
    DomainUpdateResult::none()
}

fn set_scroll_decay_tau_ms(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(ms) = value.parse::<u64>()
        && (50..=1000).contains(&ms)
    {
        state.domains.settings.performance.scroll_decay_tau_ms = ms;
    }
    DomainUpdateResult::none()
}

fn set_scroll_base_velocity(state: &mut State, v: f32) -> DomainUpdateResult {
    let _ = (state, v);
    DomainUpdateResult::none()
}

fn set_scroll_max_velocity(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(v) = value.parse::<f32>()
        && (1.0..=20.0).contains(&v)
    {
        state.domains.settings.performance.scroll_max_velocity = v;
    }
    DomainUpdateResult::none()
}

fn set_scroll_min_stop_velocity(
    state: &mut State,
    v: f32,
) -> DomainUpdateResult {
    let _ = (state, v);
    DomainUpdateResult::none()
}

fn set_scroll_ramp_ms(state: &mut State, ms: u64) -> DomainUpdateResult {
    let _ = (state, ms);
    DomainUpdateResult::none()
}

fn set_scroll_boost_multiplier(
    state: &mut State,
    m: f32,
) -> DomainUpdateResult {
    let _ = (state, m);
    DomainUpdateResult::none()
}

fn set_scroll_easing(state: &mut State, e: EasingKind) -> DomainUpdateResult {
    let _ = (state, e);
    DomainUpdateResult::none()
}

// Texture Upload handlers (legacy - now dynamically calculated based on framerate)
fn set_texture_max_uploads_per_frame(
    state: &mut State,
    n: u32,
) -> DomainUpdateResult {
    let _ = (state, n);
    DomainUpdateResult::none()
}

// Prefetch handlers
fn set_prefetch_rows_above(state: &mut State, n: usize) -> DomainUpdateResult {
    let _ = (state, n);
    DomainUpdateResult::none()
}

fn set_prefetch_rows_below(state: &mut State, n: usize) -> DomainUpdateResult {
    let _ = (state, n);
    DomainUpdateResult::none()
}

fn set_prefetch_keep_alive_ms(
    state: &mut State,
    ms: u64,
) -> DomainUpdateResult {
    let _ = (state, ms);
    DomainUpdateResult::none()
}

// Carousel handlers
fn set_carousel_prefetch_items(
    state: &mut State,
    n: usize,
) -> DomainUpdateResult {
    let _ = (state, n);
    DomainUpdateResult::none()
}

fn set_carousel_background_items(
    state: &mut State,
    n: usize,
) -> DomainUpdateResult {
    let _ = (state, n);
    DomainUpdateResult::none()
}

fn set_carousel_base_velocity(state: &mut State, v: f32) -> DomainUpdateResult {
    let _ = (state, v);
    DomainUpdateResult::none()
}

fn set_carousel_max_velocity(state: &mut State, v: f32) -> DomainUpdateResult {
    let _ = (state, v);
    DomainUpdateResult::none()
}

fn set_carousel_boost_multiplier(
    state: &mut State,
    m: f32,
) -> DomainUpdateResult {
    let _ = (state, m);
    DomainUpdateResult::none()
}

fn set_carousel_ramp_ms(state: &mut State, ms: u64) -> DomainUpdateResult {
    let _ = (state, ms);
    DomainUpdateResult::none()
}

fn set_carousel_decay_tau_ms(state: &mut State, ms: u64) -> DomainUpdateResult {
    let _ = (state, ms);
    DomainUpdateResult::none()
}

fn set_carousel_item_snap_ms(state: &mut State, ms: u64) -> DomainUpdateResult {
    let _ = (state, ms);
    DomainUpdateResult::none()
}

fn set_carousel_page_snap_ms(state: &mut State, ms: u64) -> DomainUpdateResult {
    let _ = (state, ms);
    DomainUpdateResult::none()
}

fn set_carousel_hold_tap_threshold_ms(
    state: &mut State,
    ms: u64,
) -> DomainUpdateResult {
    let _ = (state, ms);
    DomainUpdateResult::none()
}

fn set_carousel_snap_epsilon(state: &mut State, e: f32) -> DomainUpdateResult {
    let _ = (state, e);
    DomainUpdateResult::none()
}

fn set_carousel_anchor_settle_ms(
    state: &mut State,
    ms: u64,
) -> DomainUpdateResult {
    let _ = (state, ms);
    DomainUpdateResult::none()
}

// Animation Effects handlers
fn set_animation_hover_scale(
    state: &mut State,
    scale: f32,
) -> DomainUpdateResult {
    // Clamp to reasonable range: 1.0 (no scale) to 1.2 (20% scale)
    let clamped = scale.clamp(1.0, 1.2);
    state.domains.settings.performance.animation_hover_scale = clamped;
    state.runtime_config.animation_hover_scale = Some(clamped);
    crate::infra::shader_widgets::poster::set_hover_scale(clamped);
    state.runtime_config.mark_dirty();
    DomainUpdateResult::none()
}

fn set_animation_hover_transition_ms(
    state: &mut State,
    ms: u64,
) -> DomainUpdateResult {
    // Clamp to reasonable range: 50ms (fast) to 500ms (slow)
    let clamped = ms.clamp(50, 500);
    state
        .domains
        .settings
        .performance
        .animation_hover_transition_ms = clamped;
    state.runtime_config.animation_hover_transition_ms = Some(clamped);
    crate::infra::shader_widgets::poster::set_hover_transition_ms(clamped);
    state.runtime_config.mark_dirty();
    DomainUpdateResult::none()
}

fn set_animation_hover_scale_down_delay_ms(
    state: &mut State,
    ms: u64,
) -> DomainUpdateResult {
    // Clamp to reasonable range: 0ms (immediate) to 500ms (noticeable)
    let clamped = ms.clamp(0, 500);
    state
        .domains
        .settings
        .performance
        .animation_hover_scale_down_delay_ms = clamped;
    state.runtime_config.animation_hover_scale_down_delay_ms = Some(clamped);
    crate::infra::shader_widgets::poster::set_hover_scale_down_delay_ms(
        clamped,
    );
    state.runtime_config.mark_dirty();
    DomainUpdateResult::none()
}
