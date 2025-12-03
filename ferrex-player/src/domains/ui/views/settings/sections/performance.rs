//! Performance section view
//!
//! Renders the performance settings section content including:
//! - Grid Scrolling: Kinetic scrolling parameters
//! - Carousel Motion: Carousel navigation parameters
//! - Snap Animations: Snap behavior timing
//! - Animation Effects: Visual feedback timing
//! - GPU/Memory: Texture loading and prefetch

use iced::widget::{Space, column, container, scrollable};
use iced::{Element, Length};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::settings_ui::RuntimeConfigMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::domains::ui::widgets::setting_controls::{
    setting_row, setting_section, setting_slider, setting_slider_u64,
    setting_slider_usize,
};
use crate::state::State;

/// Render the performance settings section
pub fn view_performance_section<'a>(
    state: &'a State,
) -> Element<'a, UiMessage> {
    let config = &state.runtime_config;
    let fonts = state.domains.ui.state.size_provider.font;

    let mut content = column![].spacing(24).padding(20).max_width(700);

    // ========== GRID SCROLLING ==========
    content = content.push(setting_section(
        "Grid Scrolling",
        Some("Keyboard navigation in the library grid"),
        fonts,
    ));

    content = content.push(setting_row(vec![
        setting_slider_u64(
            "Debounce",
            config.scroll_debounce_ms(),
            10..=200,
            "ms",
            |v| RuntimeConfigMessage::ScrollDebounce(v).into(),
            fonts,
        ),
        setting_slider(
            "Base Velocity",
            config.scroll_base_velocity(),
            0.5..=5.0,
            "rows/s",
            1,
            |v| RuntimeConfigMessage::ScrollBaseVelocity(v).into(),
            fonts,
        ),
        setting_slider(
            "Max Velocity",
            config.scroll_max_velocity(),
            2.0..=20.0,
            "rows/s",
            1,
            |v| RuntimeConfigMessage::ScrollMaxVelocity(v).into(),
            fonts,
        ),
    ]));

    content = content.push(setting_row(vec![
        setting_slider_u64(
            "Decay Time",
            config.scroll_decay_tau_ms(),
            50..=500,
            "ms",
            |v| RuntimeConfigMessage::ScrollDecayTau(v).into(),
            fonts,
        ),
        setting_slider_u64(
            "Ramp Duration",
            config.scroll_ramp_ms(),
            100..=2000,
            "ms",
            |v| RuntimeConfigMessage::ScrollRamp(v).into(),
            fonts,
        ),
        setting_slider(
            "Boost Multiplier",
            config.scroll_boost_multiplier(),
            1.0..=5.0,
            "x",
            1,
            |v| RuntimeConfigMessage::ScrollBoost(v).into(),
            fonts,
        ),
    ]));

    content = content.push(Space::new().height(12));

    // ========== CAROUSEL MOTION ==========
    content = content.push(setting_section(
        "Carousel Motion",
        Some("Horizontal navigation in detail views"),
        fonts,
    ));

    content = content.push(setting_row(vec![
        setting_slider(
            "Base Velocity",
            config.carousel_base_velocity(),
            0.5..=5.0,
            "items/s",
            1,
            |v| RuntimeConfigMessage::CarouselBaseVelocity(v).into(),
            fonts,
        ),
        setting_slider(
            "Max Velocity",
            config.carousel_max_velocity(),
            2.0..=20.0,
            "items/s",
            1,
            |v| RuntimeConfigMessage::CarouselMaxVelocity(v).into(),
            fonts,
        ),
        setting_slider_u64(
            "Decay Time",
            config.carousel_decay_tau_ms(),
            50..=500,
            "ms",
            |v| RuntimeConfigMessage::CarouselDecayTau(v).into(),
            fonts,
        ),
    ]));

    content = content.push(setting_row(vec![
        setting_slider_u64(
            "Ramp Duration",
            config.carousel_ramp_ms(),
            100..=2000,
            "ms",
            |v| RuntimeConfigMessage::CarouselRamp(v).into(),
            fonts,
        ),
        setting_slider(
            "Boost Multiplier",
            config.carousel_boost_multiplier(),
            1.0..=5.0,
            "x",
            1,
            |v| RuntimeConfigMessage::CarouselBoost(v).into(),
            fonts,
        ),
    ]));

    content = content.push(Space::new().height(12));

    // ========== SNAP ANIMATIONS ==========
    content = content.push(setting_section(
        "Snap Animations",
        Some("Scroll snapping behavior timing"),
        fonts,
    ));

    content = content.push(setting_row(vec![
        setting_slider_u64(
            "Item Snap",
            config.snap_item_duration_ms(),
            50..=500,
            "ms",
            |v| RuntimeConfigMessage::SnapItemDuration(v).into(),
            fonts,
        ),
        setting_slider_u64(
            "Page Snap",
            config.snap_page_duration_ms(),
            100..=800,
            "ms",
            |v| RuntimeConfigMessage::SnapPageDuration(v).into(),
            fonts,
        ),
        setting_slider_u64(
            "Hold Threshold",
            config.snap_hold_threshold_ms(),
            50..=500,
            "ms",
            |v| RuntimeConfigMessage::SnapHoldThreshold(v).into(),
            fonts,
        ),
    ]));

    content = content.push(setting_row(vec![setting_slider(
        "Snap Epsilon",
        config.snap_epsilon_fraction(),
        0.001..=0.1,
        "",
        3,
        |v| RuntimeConfigMessage::SnapEpsilon(v).into(),
        fonts,
    )]));

    content = content.push(Space::new().height(12));

    // ========== ANIMATION EFFECTS ==========
    content = content.push(setting_section(
        "Animation Effects",
        Some("Visual feedback and transitions"),
        fonts,
    ));

    content = content.push(setting_row(vec![
        setting_slider(
            "Hover Scale",
            config.animation_hover_scale(),
            1.0..=1.2,
            "x",
            2,
            |v| RuntimeConfigMessage::HoverScale(v).into(),
            fonts,
        ),
        setting_slider_u64(
            "Hover Transition",
            config.animation_hover_transition_ms(),
            50..=500,
            "ms",
            |v| RuntimeConfigMessage::HoverTransition(v).into(),
            fonts,
        ),
        setting_slider_u64(
            "Default Duration",
            config.animation_default_duration_ms(),
            50..=500,
            "ms",
            |v| RuntimeConfigMessage::AnimationDuration(v).into(),
            fonts,
        ),
    ]));

    content = content.push(setting_row(vec![
        setting_slider_u64(
            "Initial Fade",
            config.animation_texture_fade_initial_ms(),
            0..=500,
            "ms",
            |v| RuntimeConfigMessage::TextureFadeInitial(v).into(),
            fonts,
        ),
        setting_slider_u64(
            "Texture Fade",
            config.animation_texture_fade_ms(),
            50..=500,
            "ms",
            |v| RuntimeConfigMessage::TextureFade(v).into(),
            fonts,
        ),
    ]));

    content = content.push(Space::new().height(12));

    // ========== GPU/MEMORY ==========
    content = content.push(setting_section(
        "GPU & Memory",
        Some("Texture loading and caching"),
        fonts,
    ));

    content = content.push(setting_row(vec![
        setting_slider_usize(
            "Prefetch Above",
            config.prefetch_rows_above(),
            0..=10,
            "rows",
            |v| RuntimeConfigMessage::PrefetchRowsAbove(v).into(),
            fonts,
        ),
        setting_slider_usize(
            "Prefetch Below",
            config.prefetch_rows_below(),
            0..=10,
            "rows",
            |v| RuntimeConfigMessage::PrefetchRowsBelow(v).into(),
            fonts,
        ),
        setting_slider_u64(
            "Keep Alive",
            config.keep_alive_ms(),
            1000..=30000,
            "ms",
            |v| RuntimeConfigMessage::KeepAlive(v).into(),
            fonts,
        ),
    ]));

    content = content.push(setting_row(vec![
        setting_slider_usize(
            "Carousel Prefetch",
            config.carousel_prefetch_items(),
            1..=20,
            "items",
            |v| RuntimeConfigMessage::CarouselPrefetch(v).into(),
            fonts,
        ),
        setting_slider_usize(
            "Carousel Background",
            config.carousel_background_items(),
            1..=10,
            "items",
            |v| RuntimeConfigMessage::CarouselBackground(v).into(),
            fonts,
        ),
    ]));

    // Wrap in scrollable for long content
    let scrollable_content =
        scrollable(content)
            .height(Length::Fill)
            .style(|theme, status| {
                let mut style = scrollable::default(theme, status);
                style.container.background = Some(iced::Background::Color(
                    MediaServerTheme::SURFACE_DIM,
                ));
                style
            });

    container(scrollable_content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::Container::Default.style())
        .into()
}
