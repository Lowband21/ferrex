//! Display section view
//!
//! Renders the display settings section with unified scaling controls:
//! - Scale preset buttons (Compact, Default, Large, Huge, TV)
//! - Custom scale slider with on_release (0.5x - 3.0x)
//! - Manual numeric entry for precise values
//! - Poster quality settings for library and detail views

use iced::widget::{Space, column, container, scrollable, text};
use iced::{Element, Length};

use crate::domains::settings::sections::display::messages::DisplayMessage;
use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::settings_ui::SettingsUiMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::domains::ui::widgets::setting_controls::{
    scale_preset_buttons, scale_slider, setting_dropdown, setting_row,
    setting_section, setting_slider,
};
use crate::state::State;
use ferrex_core::player_prelude::UserScale;
use ferrex_model::PosterSize;

/// Render the display settings section
pub fn view_display_section<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let fonts = state.domains.ui.state.size_provider.font;
    let scaling_ctx = &state.domains.ui.state.scaling_context;
    let user_scale = scaling_ctx.user_scale;
    let preview_scale = state.domains.ui.state.scale_slider_preview;
    let scale_text = &state.domains.ui.state.scale_text_input;

    let mut content = column![].spacing(24).padding(20).max_width(700);

    // Header
    content = content.push(
        text("Display")
            .size(fonts.title_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
    );

    // Interface Scale section
    content = content.push(setting_section(
        "Interface Scale",
        Some("Adjust the size of all UI elements including text"),
        fonts,
    ));

    // Scale preset buttons
    content = content.push(scale_preset_buttons(
        user_scale,
        |preset| SettingsUiMessage::SetScalePreset(preset).into(),
        fonts,
    ));

    content = content.push(Space::new().height(12));

    // Scale slider with on_release behavior (only applies when released)
    content = content.push(scale_slider(
        "Fine Tune",
        user_scale,
        preview_scale,
        0.5..=3.0,
        |v| SettingsUiMessage::ScaleSliderPreview(v).into(),
        |v| SettingsUiMessage::SetUserScale(UserScale::Custom(v)).into(),
        |s| SettingsUiMessage::ScaleTextInput(s).into(),
        scale_text,
        fonts,
    ));

    content = content.push(Space::new().height(24));

    let display_state = &state.domains.settings.display;

    // Library Grid section
    content = content.push(setting_section(
        "Library Grid",
        Some("Poster spacing and layout for grid views"),
        fonts,
    ));
    content = content.push(setting_slider(
        "Poster gap",
        display_state.grid_poster_gap,
        0.0..=80.0,
        "scaled units",
        1,
        |value| {
            SettingsUiMessage::Display(DisplayMessage::SetGridPosterGap(
                format!("{value:.1}"),
            ))
            .into()
        },
        fonts,
    ));

    content = content.push(Space::new().height(24));

    // Scrollbar section
    content = content.push(setting_section(
        "Scrollbars",
        Some("Adjust minimum scrollbar thumb size for easier grabbing"),
        fonts,
    ));
    content = content.push(setting_slider(
        "Minimum thumb length",
        display_state.scrollbar_scroller_min_length_px,
        2.0..=120.0,
        "px",
        0,
        |value| {
            SettingsUiMessage::Display(
                DisplayMessage::SetScrollbarScrollerMinLength(format!(
                    "{value:.0}"
                )),
            )
            .into()
        },
        fonts,
    ));

    content = content.push(Space::new().height(24));

    // Poster Quality section
    content = content.push(setting_section(
        "Poster Quality",
        Some("Image quality for posters in different contexts"),
        fonts,
    ));

    content = content.push(setting_row(vec![
        setting_dropdown(
            "Library Grid",
            &PosterSize::ALL,
            display_state.library_poster_quality,
            |q| {
                SettingsUiMessage::Display(
                    DisplayMessage::SetLibraryPosterQuality(q),
                )
                .into()
            },
            fonts,
        ),
        setting_dropdown(
            "Detail View",
            &PosterSize::ALL,
            display_state.detail_poster_quality,
            |q| {
                SettingsUiMessage::Display(
                    DisplayMessage::SetDetailPosterQuality(q),
                )
                .into()
            },
            fonts,
        ),
    ]));

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
