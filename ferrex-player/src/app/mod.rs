use std::sync::Arc;

use iced::{
    Application, Font, Preset, Program as IcedProgram, Settings, Theme,
};

use crate::common::messages::DomainMessage;
use crate::state::State;
use crate::{subscriptions, update, view};

pub mod bootstrap;
pub mod presets;

pub use bootstrap::AppConfig;

/// Build the Ferrex application using the provided configuration.
pub fn application(
    config: AppConfig,
) -> Application<
    impl IcedProgram<State = State, Message = DomainMessage, Theme = Theme>,
> {
    let config = Arc::new(config);

    let boot_config = Arc::clone(&config);
    iced::application(
        move || bootstrap::runtime_boot(&boot_config),
        update::update,
        view::view,
    )
    .settings(default_settings())
    .title("Ferrex Player")
    .subscription(subscriptions::subscription)
    .font(iced_aw::ICED_AW_FONT_BYTES)
    .font(lucide_icons::lucide_font_bytes())
    .theme(app_theme)
    .window(iced::window::Settings {
        size: iced::Size::new(1280.0, 720.0),
        resizable: true,
        decorations: true,
        transparent: true,
        ..Default::default()
    })
    .presets(presets::collect(&config))
}

fn default_settings() -> Settings {
    let mut settings = Settings::default();
    settings.id = Some("ferrex-player".to_string());
    settings.antialiasing = true;
    settings.default_font = Font::MONOSPACE;
    settings
}

fn app_theme(_: &State) -> Theme {
    crate::domains::ui::theme::MediaServerTheme::theme()
}

/// Convenience helper for tests to construct an application with custom presets.
pub fn application_with_presets(
    config: AppConfig,
    custom_presets: Vec<Preset<State, DomainMessage>>,
) -> Application<
    impl IcedProgram<State = State, Message = DomainMessage, Theme = Theme>,
> {
    let config = Arc::new(config);
    let boot_config = Arc::clone(&config);

    iced::application(
        move || bootstrap::runtime_boot(&boot_config),
        update::update,
        view::view,
    )
    .settings(default_settings())
    .title("Ferrex Player")
    .subscription(subscriptions::subscription)
    .font(iced_aw::ICED_AW_FONT_BYTES)
    .font(lucide_icons::lucide_font_bytes())
    .theme(app_theme)
    .window(iced::window::Settings {
        size: iced::Size::new(1280.0, 720.0),
        resizable: true,
        decorations: true,
        transparent: true,
        ..Default::default()
    })
    .presets(custom_presets)
}
