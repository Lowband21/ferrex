use std::sync::Arc;

use iced::{
    Application, Font, Preset, Program as IcedProgram, Settings, Theme,
};

use crate::common::messages::DomainMessage;
use crate::state::State;
use crate::{subscriptions, update, view};
use iced::Element;

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
    fn view_adapter(
        state: &State,
    ) -> Element<'_, DomainMessage, Theme, iced::Renderer> {
        if let Some(id) = state
            .windows
            .get(crate::domains::ui::windows::WindowKind::Main)
        {
            view::view(state, id)
        } else {
            // Fallback while main window id is not known yet:
            // - If not authenticated, render the auth view so tests can proceed.
            // - Otherwise, render an empty container until window id is set.
            if !state.is_authenticated {
                crate::domains::ui::views::auth::view_auth(
                    &state.domains.auth.state.auth_flow,
                    state.domains.auth.state.user_permissions.as_ref(),
                )
                .map(DomainMessage::from)
            } else {
                iced::widget::container(
                    iced::widget::Space::new()
                        .width(iced::Length::Fill)
                        .height(iced::Length::Fill),
                )
                .into()
            }
        }
    }

    iced::application(
        move || bootstrap::runtime_boot(&boot_config),
        update::update,
        view_adapter,
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
    Settings {
        id: Some("ferrex-player".to_string()),
        antialiasing: true,
        default_font: Font::MONOSPACE,
        ..Default::default()
    }
}

fn app_theme(state: &State) -> Theme {
    crate::domains::ui::theme::MediaServerTheme::theme_for_state(state, None)
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

    fn view_adapter(
        state: &State,
    ) -> Element<'_, DomainMessage, Theme, iced::Renderer> {
        if let Some(id) = state
            .windows
            .get(crate::domains::ui::windows::WindowKind::Main)
        {
            view::view(state, id)
        } else {
            if !state.is_authenticated {
                crate::domains::ui::views::auth::view_auth(
                    &state.domains.auth.state.auth_flow,
                    state.domains.auth.state.user_permissions.as_ref(),
                )
                .map(DomainMessage::from)
            } else {
                iced::widget::container(
                    iced::widget::Space::new()
                        .width(iced::Length::Fill)
                        .height(iced::Length::Fill),
                )
                .into()
            }
        }
    }

    iced::application(
        move || bootstrap::runtime_boot(&boot_config),
        update::update,
        view_adapter,
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
