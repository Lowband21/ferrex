use std::sync::Arc;

use env_logger::{Builder, Target};
use iced::{
    Application, Element, Font, Preset, Program as IcedProgram, Settings, Task,
    Theme, window,
};
use iced_aw::ICED_AW_FONT_BYTES;
use log::LevelFilter;

use crate::common::messages::DomainMessage;
use crate::domains::ui::{
    shell_ui::UiShellMessage, theme::MediaServerTheme, windows::WindowKind,
};
use crate::state::State;
use crate::{subscriptions, update, view};

pub mod bootstrap;
pub mod presets;

pub use bootstrap::AppConfig;

/// Run the installed Ferrex player binary using environment-derived settings.
pub fn run() -> crate::Result {
    init_runtime_hooks();
    run_with_config(AppConfig::from_environment())
}

/// Initialize logging and optional profiling integrations for the app runtime.
pub fn init_runtime_hooks() {
    if std::env::var("RUST_LOG").is_err() {
        init_default_logger();
        log::info!("RUST_LOG not set; using default logger");
    } else {
        let _ = env_logger::try_init();
        log::info!("Initialized logger from env");
    }

    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    crate::infra::profiling::init();

    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    log::info!("Profiling system initialized");

    #[cfg(feature = "profile-with-puffin")]
    log::info!(
        "Puffin server listening on 127.0.0.1:8585 - connect with: puffin_viewer --url 127.0.0.1:8585"
    );

    #[cfg(feature = "profile-with-tracy")]
    tracy_client::Client::start();
}

/// Run the Ferrex player daemon with an explicit app configuration.
pub fn run_with_config(config: AppConfig) -> crate::Result {
    let tenfoot_enabled = config.tenfoot_enabled();
    if tenfoot_enabled {
        log::info!("10-foot mode enabled via --10ft or FERREX_10FT=1");
    }
    let boot_config = config.clone();

    iced::daemon::<State, DomainMessage, Theme, iced_wgpu::Renderer>(
        move || daemon_boot(&boot_config, tenfoot_enabled),
        update::update,
        view::view,
    )
    .settings(daemon_settings())
    .subscription(subscriptions::subscription)
    .font(ICED_AW_FONT_BYTES)
    .font(lucide_icons::lucide_font_bytes())
    .title(daemon_title)
    .theme(|state: &State, window| {
        MediaServerTheme::theme_for_state(state, Some(window))
    })
    .run()
}

fn init_default_logger() {
    let mut builder = Builder::new();
    builder
        .target(Target::Stdout)
        .filter_level(LevelFilter::Info)
        .filter_module("ferrex_player", LevelFilter::Debug)
        .filter_module("ferrex_player_app", LevelFilter::Debug)
        .filter_module("ferrex_player_ui", LevelFilter::Debug);

    let _ = builder.try_init();
}

fn daemon_boot(
    config: &AppConfig,
    tenfoot_enabled: bool,
) -> (State, Task<DomainMessage>) {
    let (mut state, boot_task) = bootstrap::runtime_boot(config);

    let (main_id, open) = window::open(window::Settings {
        size: if tenfoot_enabled {
            iced::Size::new(1920.0, 1080.0)
        } else {
            iced::Size::new(1620.0, 1080.0)
        },
        resizable: true,
        decorations: !tenfoot_enabled,
        transparent: true,
        ..Default::default()
    });

    state.windows.set(WindowKind::Main, main_id);

    let boot = Task::batch([
        boot_task,
        open.map(|_| DomainMessage::NoOp),
        Task::done(DomainMessage::Ui(
            UiShellMessage::MainWindowOpened(main_id).into(),
        )),
    ]);

    (state, boot)
}

fn daemon_settings() -> Settings {
    Settings {
        id: Some("ferrex-player".to_string()),
        antialiasing: false,
        default_font: Font::MONOSPACE,
        #[cfg(not(target_os = "macos"))]
        vsync: false,
        #[cfg(target_os = "macos")]
        vsync: true,
        ..Default::default()
    }
}

fn daemon_title(state: &State, window_id: window::Id) -> String {
    if state
        .windows
        .get(WindowKind::Search)
        .is_some_and(|id| id == window_id)
    {
        "Ferrex Search".to_string()
    } else {
        "Ferrex Player".to_string()
    }
}

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
        view_adapter,
    )
    .settings(application_settings())
    .title("Ferrex Player")
    .subscription(subscriptions::subscription)
    .font(ICED_AW_FONT_BYTES)
    .font(lucide_icons::lucide_font_bytes())
    .theme(app_theme)
    .window(window::Settings {
        size: iced::Size::new(1280.0, 720.0),
        resizable: true,
        decorations: true,
        transparent: true,
        ..Default::default()
    })
    .presets(presets::collect(&config))
}

fn application_settings() -> Settings {
    Settings {
        id: Some("ferrex-player".to_string()),
        antialiasing: true,
        default_font: Font::MONOSPACE,
        ..Default::default()
    }
}

fn app_theme(state: &State) -> Theme {
    MediaServerTheme::theme_for_state(state, None)
}

fn view_adapter(
    state: &State,
) -> Element<'_, DomainMessage, Theme, iced::Renderer> {
    if let Some(id) = state.windows.get(WindowKind::Main) {
        view::view(state, id)
    } else if !state.is_authenticated {
        crate::domains::ui::views::auth::view_auth(
            state,
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
        view_adapter,
    )
    .settings(application_settings())
    .title("Ferrex Player")
    .subscription(subscriptions::subscription)
    .font(ICED_AW_FONT_BYTES)
    .font(lucide_icons::lucide_font_bytes())
    .theme(app_theme)
    .window(window::Settings {
        size: iced::Size::new(1280.0, 720.0),
        resizable: true,
        decorations: true,
        transparent: true,
        ..Default::default()
    })
    .presets(custom_presets)
}
