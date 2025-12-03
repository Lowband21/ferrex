//! Theme section view
//!
//! Renders the theme settings section with:
//! - GPU-accelerated HSLuv color picker wheel
//! - Harmony mode selection (None, Complementary, Triadic, Split-Comp)
//! - Lightness slider
//! - Color preview swatches

use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Element, Length};

use crate::domains::settings::sections::theme::messages::ThemeMessage;
use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::settings_ui::SettingsUiMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::domains::ui::widgets::setting_controls::{
    setting_dropdown, setting_section, setting_slider,
};
use crate::infra::color::HarmonyMode;
use crate::infra::shader_widgets::color_picker::{
    ColorPicker, ColorPickerMessage,
};
use crate::state::State;

/// Render the theme settings section
pub fn view_theme_section<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let size_provider = &state.domains.ui.state.size_provider;
    let fonts = size_provider.font;
    let scale = size_provider.scale;
    let theme_state = &state.domains.settings.theme;
    let accent_config = &theme_state.accent_color;

    // Get the display color
    let display_color = theme_state.effective_accent();

    // Base size for color picker - larger for better visibility
    let picker_base_size = 500.0;

    let mut content = column![].spacing(24).padding(20);

    // Header
    content = content.push(
        text("Theme")
            .size(fonts.title_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
    );

    // Accent Color section
    content = content.push(setting_section(
        "Accent Color",
        Some("Choose your preferred accent color using the color wheel"),
        fonts,
    ));

    // Color picker widget - responsive to scale
    let picker: Element<'a, UiMessage> =
        ColorPicker::responsive(accent_config, picker_base_size, scale)
            .on_change(color_picker_message_to_ui)
            .into();

    // Color info text - format as hex
    let color_hex = format!(
        "#{:02X}{:02X}{:02X}",
        (display_color.r * 255.0) as u8,
        (display_color.g * 255.0) as u8,
        (display_color.b * 255.0) as u8,
    );

    // Color picker left-aligned with info to the right
    content = content.push(
        row![
            picker,
            Space::new().width(24),
            column![
                // Color preview swatch
                container(Space::new().width(60).height(60)).style(move |_| {
                    container::Style {
                        background: Some(iced::Background::Color(
                            display_color,
                        )),
                        border: iced::Border {
                            color: MediaServerTheme::BORDER_COLOR,
                            width: 2.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }
                }),
                Space::new().height(8),
                text("Current Color")
                    .size(fonts.caption)
                    .color(MediaServerTheme::TEXT_SECONDARY),
                text(color_hex)
                    .size(fonts.body)
                    .color(MediaServerTheme::TEXT_PRIMARY),
            ]
            .spacing(4),
        ]
        .align_y(iced::Alignment::Start),
    );

    content = content.push(Space::new().height(16));

    // Harmony Mode dropdown
    content = content.push(setting_dropdown(
        "Color Harmony",
        &HarmonyMode::ALL,
        accent_config.harmony_mode,
        |mode| {
            SettingsUiMessage::Theme(ThemeMessage::SetHarmonyMode(mode)).into()
        },
        fonts,
    ));

    content = content.push(Space::new().height(8));

    // Lightness slider
    content = content.push(setting_slider(
        "Lightness",
        accent_config.lightness,
        0.0..=100.0,
        "%",
        0,
        |v| {
            SettingsUiMessage::Theme(ThemeMessage::SetAccentLightness(v)).into()
        },
        fonts,
    ));

    content = content.push(Space::new().height(16));

    // Reset button
    let reset_button = button(
        text("Reset to Default")
            .size(fonts.caption)
            .color(MediaServerTheme::TEXT_PRIMARY),
    )
    .padding([8, 16])
    .style(theme::Button::Secondary.style())
    .on_press(SettingsUiMessage::Theme(ThemeMessage::ResetToDefault).into());

    content = content.push(reset_button);

    // Show complement colors if harmony is enabled
    if accent_config.harmony_mode != HarmonyMode::None {
        content = content.push(Space::new().height(24));
        content = content.push(setting_section(
            "Harmony Colors",
            Some("These colors are automatically generated based on your selection"),
            fonts,
        ));

        let all_colors = accent_config.all_colors();
        let mut color_row = row![].spacing(12);

        for (i, color) in all_colors.iter().enumerate() {
            let label = match i {
                0 => "Primary",
                1 => "Complement 1",
                2 => "Complement 2",
                _ => "Color",
            };

            let hex = format!(
                "#{:02X}{:02X}{:02X}",
                (color.r * 255.0) as u8,
                (color.g * 255.0) as u8,
                (color.b * 255.0) as u8,
            );

            // Copy color to avoid borrowing issue
            let swatch_color = *color;
            let swatch =
                container(Space::new().width(40).height(40)).style(move |_| {
                    container::Style {
                        background: Some(iced::Background::Color(swatch_color)),
                        border: iced::Border {
                            color: MediaServerTheme::BORDER_COLOR,
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    }
                });

            color_row = color_row.push(
                column![
                    swatch,
                    text(label)
                        .size(fonts.small)
                        .color(MediaServerTheme::TEXT_SECONDARY),
                    text(hex)
                        .size(fonts.small)
                        .color(MediaServerTheme::TEXT_SUBDUED),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center),
            );
        }

        content = content.push(color_row);
    }

    // Wrap in scrollable container
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

/// Convert color picker messages to UI messages
fn color_picker_message_to_ui(msg: ColorPickerMessage) -> UiMessage {
    match msg {
        ColorPickerMessage::HueSatChanged { hue, saturation } => {
            SettingsUiMessage::Theme(ThemeMessage::SetAccentHueSat {
                hue,
                saturation,
            })
            .into()
        }
        ColorPickerMessage::HueChanged(hue) => {
            SettingsUiMessage::Theme(ThemeMessage::SetAccentHueSat {
                hue,
                saturation: 100.0,
            })
            .into()
        }
        ColorPickerMessage::DragStarted(_) => {
            SettingsUiMessage::Theme(ThemeMessage::PickerDragStarted).into()
        }
        ColorPickerMessage::DragEnded => {
            SettingsUiMessage::Theme(ThemeMessage::PickerDragEnded).into()
        }
        ColorPickerMessage::SaturationChanged(_)
        | ColorPickerMessage::HoverChanged(_) => UiMessage::NoOp,
    }
}
