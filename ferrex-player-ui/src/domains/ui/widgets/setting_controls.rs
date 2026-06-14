//! Reusable setting control widgets
//!
//! Provides slider, stepper, and dropdown widgets for settings UI with
//! consistent styling and layout.

use std::ops::RangeInclusive;

use iced::widget::{
    Space, button, column, container, pick_list, row, slider, text, text_input,
};
use iced::{Alignment, Border, Color, Element, Length};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::theme::{self, Button, MediaServerTheme};
use crate::infra::design_tokens::{FontTokens, ScalePreset};
use crate::infra::theme::accent;

/// Create a slider control with label, value display, and unit
///
/// # Arguments
/// * `label` - The setting name shown above the slider
/// * `value` - Current value
/// * `range` - Min/max range for the slider
/// * `unit` - Unit label (e.g., "ms", "rows/s")
/// * `decimals` - Number of decimal places to show
/// * `on_change` - Message to send on change
/// * `fonts` - Font tokens for scaled sizing
pub fn setting_slider(
    label: &'static str,
    value: f32,
    range: RangeInclusive<f32>,
    unit: &'static str,
    decimals: usize,
    on_change: impl Fn(f32) -> UiMessage + 'static,
    fonts: FontTokens,
) -> Element<'static, UiMessage> {
    let value_str: String = match decimals {
        0 => format!("{:.0} {}", value, unit),
        1 => format!("{:.1} {}", value, unit),
        2 => format!("{:.2} {}", value, unit),
        _ => format!("{:.3} {}", value, unit),
    };

    column![
        // Label row with value display
        row![
            text(label)
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            text(value_str)
                .size(fonts.small)
                .color(MediaServerTheme::TEXT_SUBDUED),
        ]
        .align_y(Alignment::Center),
        // Slider
        slider(range, value, on_change)
            .step(0.01)
            .width(Length::Fill)
            .style(theme::Slider::style()),
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

/// Create a slider control for u64 values (milliseconds)
pub fn setting_slider_u64(
    label: &'static str,
    value: u64,
    range: RangeInclusive<u64>,
    unit: &'static str,
    on_change: impl Fn(u64) -> UiMessage + 'static,
    fonts: FontTokens,
) -> Element<'static, UiMessage> {
    let value_f32 = value as f32;
    let min = *range.start() as f32;
    let max = *range.end() as f32;
    let value_str = format!("{} {}", value, unit);

    column![
        row![
            text(label)
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            text(value_str)
                .size(fonts.small)
                .color(MediaServerTheme::TEXT_SUBDUED),
        ]
        .align_y(Alignment::Center),
        slider(min..=max, value_f32, move |v| on_change(v as u64))
            .step(1.0)
            .width(Length::Fill)
            .style(theme::Slider::style()),
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

/// Create a slider control for i32 values (integers)
pub fn setting_slider_i32(
    label: &'static str,
    value: i32,
    range: RangeInclusive<i32>,
    unit: &'static str,
    on_change: impl Fn(i32) -> UiMessage + 'static,
    fonts: FontTokens,
) -> Element<'static, UiMessage> {
    let value_f32 = value as f32;
    let min = *range.start() as f32;
    let max = *range.end() as f32;
    let value_str = format!("{} {}", value, unit);

    column![
        row![
            text(label)
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            text(value_str)
                .size(fonts.small)
                .color(MediaServerTheme::TEXT_SUBDUED),
        ]
        .align_y(Alignment::Center),
        slider(min..=max, value_f32, move |v| on_change(v as i32))
            .step(1.0)
            .width(Length::Fill)
            .style(theme::Slider::style()),
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

/// Create a slider control for usize values
pub fn setting_slider_usize(
    label: &'static str,
    value: usize,
    range: RangeInclusive<usize>,
    unit: &'static str,
    on_change: impl Fn(usize) -> UiMessage + 'static,
    fonts: FontTokens,
) -> Element<'static, UiMessage> {
    let value_f32 = value as f32;
    let min = *range.start() as f32;
    let max = *range.end() as f32;
    let value_str = format!("{} {}", value, unit);

    column![
        row![
            text(label)
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            text(value_str)
                .size(fonts.small)
                .color(MediaServerTheme::TEXT_SUBDUED),
        ]
        .align_y(Alignment::Center),
        slider(min..=max, value_f32, move |v| on_change(v as usize))
            .step(1.0)
            .width(Length::Fill)
            .style(theme::Slider::style()),
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

/// Create a slider control for f64 values (seconds for seeking)
pub fn setting_slider_f64(
    label: &'static str,
    value: f64,
    range: RangeInclusive<f64>,
    unit: &'static str,
    decimals: usize,
    on_change: impl Fn(f64) -> UiMessage + 'static,
    fonts: FontTokens,
) -> Element<'static, UiMessage> {
    let value_f32 = value as f32;
    let min = *range.start() as f32;
    let max = *range.end() as f32;
    let value_str: String = match decimals {
        0 => format!("{:.0} {}", value, unit),
        1 => format!("{:.1} {}", value, unit),
        _ => format!("{:.2} {}", value, unit),
    };

    column![
        row![
            text(label)
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            text(value_str)
                .size(fonts.small)
                .color(MediaServerTheme::TEXT_SUBDUED),
        ]
        .align_y(Alignment::Center),
        slider(min..=max, value_f32, move |v| on_change(v as f64))
            .step(0.5)
            .width(Length::Fill)
            .style(theme::Slider::style()),
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

/// Create a section header with an optional description
pub fn setting_section(
    title: &'static str,
    description: Option<&'static str>,
    fonts: FontTokens,
) -> Element<'static, UiMessage> {
    let mut col = column![
        text(title)
            .size(fonts.body)
            .color(MediaServerTheme::TEXT_PRIMARY),
        // Divider line
        container(Space::new().height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(
                    MediaServerTheme::BORDER_COLOR
                )),
                ..Default::default()
            }),
    ]
    .spacing(6);

    if let Some(desc) = description {
        col = col.push(
            text(desc)
                .size(fonts.small)
                .color(MediaServerTheme::TEXT_SUBDUED),
        );
    }

    col.spacing(8).into()
}

/// Create a horizontal row of setting controls
pub fn setting_row(
    controls: Vec<Element<'static, UiMessage>>,
) -> Element<'static, UiMessage> {
    let mut r = row![].spacing(24).align_y(Alignment::End);
    for control in controls {
        r = r.push(control);
    }
    r.into()
}

/// Create a visual button group for scale preset selection
///
/// Displays preset buttons in two rows with active state indication.
/// Active preset uses Primary style, inactive uses Secondary.
pub fn scale_preset_buttons<'a>(
    current_scale: f32,
    on_select: impl Fn(ScalePreset) -> UiMessage + Clone + 'static,
    fonts: FontTokens,
) -> Element<'a, UiMessage> {
    // Determine which preset matches the current scale (within tolerance)
    let active_preset = ScalePreset::ALL
        .iter()
        .find(|p| (p.scale_factor() - current_scale).abs() < 0.01)
        .copied();

    // Build first row: Compact, Default, Large
    let row1_presets = [
        ScalePreset::Compact,
        ScalePreset::Default,
        ScalePreset::Large,
    ];
    let mut row1 = row![].spacing(12);
    for preset in row1_presets {
        let is_active = active_preset == Some(preset);
        let on_select = on_select.clone();
        row1 = row1.push(preset_button(preset, is_active, on_select, fonts));
    }

    // Build second row: Huge, TV
    let row2_presets = [ScalePreset::Huge, ScalePreset::TV];
    let mut row2 = row![].spacing(12);
    for preset in row2_presets {
        let is_active = active_preset == Some(preset);
        let on_select = on_select.clone();
        row2 = row2.push(preset_button(preset, is_active, on_select, fonts));
    }

    column![row1, row2].spacing(12).into()
}

/// Create a single preset button with active state styling
fn preset_button<'a>(
    preset: ScalePreset,
    is_active: bool,
    on_select: impl Fn(ScalePreset) -> UiMessage + 'static,
    fonts: FontTokens,
) -> Element<'a, UiMessage> {
    let label =
        format!("{}\n{:.1}x", preset.display_name(), preset.scale_factor());

    let style = if is_active {
        Button::Primary.style()
    } else {
        Button::Secondary.style()
    };

    // Scale button dimensions with font size (base: 120x56 at caption=14)
    let scale_factor = fonts.caption / 14.0;
    let btn_width = (120.0 * scale_factor).round();
    let btn_height = (56.0 * scale_factor).round();

    button(
        container(
            text(label)
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_PRIMARY)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .width(Length::Fixed(btn_width))
    .height(Length::Fixed(btn_height))
    .style(style)
    .on_press(on_select(preset))
    .into()
}

/// Create scale slider with on_release behavior and manual text entry
///
/// The slider only applies scale changes when released, avoiding feedback
/// loops during drag. Manual entry allows precise numeric input.
///
/// # Arguments
/// * `label` - The setting name shown above the slider
/// * `value` - Current scale value
/// * `preview_value` - Optional preview value shown during drag (before release)
/// * `range` - Min/max range for the slider
/// * `on_preview` - Message to send during drag (for preview display only)
/// * `on_release` - Message to send on slider release (applies the change)
/// * `on_text_input` - Message to send when text input changes
/// * `text_value` - Current text input value
/// * `fonts` - Font tokens for scaled sizing
pub fn scale_slider<'a>(
    label: &'static str,
    value: f32,
    preview_value: Option<f32>,
    range: RangeInclusive<f32>,
    on_preview: impl Fn(f32) -> UiMessage + 'static,
    on_release: impl Fn(f32) -> UiMessage + Clone + 'static,
    on_text_input: impl Fn(String) -> UiMessage + 'static,
    text_value: &'a str,
    fonts: FontTokens,
) -> Element<'a, UiMessage> {
    let min = *range.start();
    let max = *range.end();

    // Use preview value for display if dragging, otherwise use actual value
    let display_value = preview_value.unwrap_or(value).clamp(min, max);
    let slider_value = display_value;

    let value_str = format!("{:.0}%", display_value * 100.0);
    let on_release_clone = on_release.clone();

    column![
        // Label row with percentage display
        row![
            text(label)
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            text(value_str)
                .size(fonts.small)
                .color(MediaServerTheme::TEXT_SUBDUED),
        ]
        .align_y(Alignment::Center),
        // Slider row
        row![
            slider(min..=max, slider_value, on_preview)
                .step(0.01)
                .on_release(on_release_clone(display_value))
                .width(Length::Fill)
                .style(theme::Slider::style()),
            // Manual entry text input
            text_input("1.0", text_value)
                .on_input(on_text_input)
                .on_submit(on_release(
                    text_value.parse::<f32>().unwrap_or(value).clamp(min, max)
                ))
                .width(Length::Fixed(60.0))
                .size(fonts.small)
                .style(|_theme, status| {
                    let (background, border_color) = match status {
                        text_input::Status::Active => (
                            Color::from_rgba(0.1, 0.1, 0.1, 0.8),
                            MediaServerTheme::BORDER_COLOR,
                        ),
                        text_input::Status::Hovered => (
                            Color::from_rgba(0.15, 0.15, 0.15, 0.9),
                            MediaServerTheme::BORDER_COLOR,
                        ),
                        text_input::Status::Focused { is_hovered } => {
                            if is_hovered {
                                (
                                    Color::from_rgba(0.15, 0.15, 0.15, 1.00),
                                    accent(),
                                )
                            } else {
                                (
                                    Color::from_rgba(0.15, 0.15, 0.15, 0.95),
                                    accent(),
                                )
                            }
                        }
                        text_input::Status::Disabled => (
                            Color::from_rgba(0.05, 0.05, 0.05, 0.5),
                            MediaServerTheme::BORDER_COLOR,
                        ),
                    };

                    text_input::Style {
                        background: iced::Background::Color(background),
                        border: Border {
                            color: border_color,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        icon: MediaServerTheme::TEXT_SUBDUED,
                        placeholder: MediaServerTheme::TEXT_DIMMED,
                        value: MediaServerTheme::TEXT_PRIMARY,
                        selection: accent().scale_alpha(0.3),
                    }
                }),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

/// Create a dropdown control with label for settings
///
/// # Arguments
/// * `label` - The setting name shown above the dropdown
/// * `options` - Slice of available options (must implement Display + Clone + PartialEq)
/// * `selected` - Currently selected option
/// * `on_change` - Message to send on change
/// * `fonts` - Font tokens for scaled sizing
pub fn setting_dropdown<'a, T>(
    label: &'static str,
    options: &'static [T],
    selected: T,
    on_change: impl Fn(T) -> UiMessage + 'static,
    fonts: FontTokens,
) -> Element<'a, UiMessage>
where
    T: std::fmt::Display + Clone + Copy + PartialEq + 'static,
{
    column![
        text(label)
            .size(fonts.caption)
            .color(MediaServerTheme::TEXT_SECONDARY),
        pick_list(options, Some(selected), on_change)
            .width(Length::Fixed(140.0))
            .style(|_theme, status| {
                let (background, border_color) = match status {
                    pick_list::Status::Active => (
                        Color::from_rgba(0.1, 0.1, 0.1, 0.8),
                        MediaServerTheme::BORDER_COLOR,
                    ),
                    pick_list::Status::Hovered => {
                        (Color::from_rgba(0.15, 0.15, 0.15, 0.9), accent())
                    }
                    pick_list::Status::Opened { .. } => {
                        (Color::from_rgba(0.15, 0.15, 0.15, 0.95), accent())
                    }
                };

                pick_list::Style {
                    text_color: MediaServerTheme::TEXT_PRIMARY,
                    placeholder_color: MediaServerTheme::TEXT_DIMMED,
                    handle_color: MediaServerTheme::TEXT_SECONDARY,
                    background: iced::Background::Color(background),
                    border: Border {
                        color: border_color,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                }
            }),
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}
