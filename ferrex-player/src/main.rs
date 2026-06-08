use ferrex_player::{
    app::{AppConfig, bootstrap::runtime_boot},
    common::messages::DomainMessage,
    domains::ui::{
        shell_ui::UiShellMessage, theme::MediaServerTheme, windows::WindowKind,
    },
    state::State,
    subscriptions, update, view,
};

use env_logger::{Builder, Target};
use log::LevelFilter;

use iced::{Font, Task, Theme, window};
use iced_aw::ICED_AW_FONT_BYTES;

fn init_logger() {
    Builder::new()
        .target(Target::Stdout)
        .filter_level(LevelFilter::Info)
        .filter_module("ferrex_player", LevelFilter::Debug)
        .init();
}

fn main() -> iced::Result {
    if std::env::var("RUST_LOG").is_err() {
        init_logger();
        log::info!("RUST_LOG not set; using default logger");
    } else {
        env_logger::init();
        log::info!("Initialized logger from env");
    }

    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    ferrex_player::infra::profiling::init();

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

    let config = AppConfig::from_environment();
    let tenfoot_enabled = config.tenfoot_enabled();
    if tenfoot_enabled {
        log::info!("10-foot mode enabled via --10ft or FERREX_10FT=1");
    }
    let boot_config = config.clone();

    let settings = iced::Settings {
        id: Some("ferrex-player".to_string()),
        antialiasing: false,
        default_font: Font::MONOSPACE,
        #[cfg(not(target_os = "macos"))]
        vsync: false,
        #[cfg(target_os = "macos")]
        vsync: true,
        ..Default::default()
    };

    iced::daemon::<State, DomainMessage, Theme, iced_wgpu::Renderer>(
        move || {
            let (mut state, boot_task) = runtime_boot(&boot_config);

            // Explicitly open the main window for daemon-based multi-window
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

            // Track main window id immediately
            state.windows.set(WindowKind::Main, main_id);

            let boot = Task::batch([
                boot_task,
                open.map(|_| DomainMessage::NoOp),
                Task::done(DomainMessage::Ui(
                    UiShellMessage::MainWindowOpened(main_id).into(),
                )),
            ]);

            (state, boot)
        },
        update::update,
        view::view,
    )
    .settings(settings)
    .subscription(subscriptions::subscription)
    .font(ICED_AW_FONT_BYTES)
    .font(lucide_icons::lucide_font_bytes())
    .title(|state: &State, window_id| {
        if state
            .windows
            .get(WindowKind::Search)
            .is_some_and(|id| id == window_id)
        {
            "Ferrex Search".to_string()
        } else {
            "Ferrex Player".to_string()
        }
    })
    .theme(|state: &State, window| {
        MediaServerTheme::theme_for_state(state, Some(window))
    })
    .run()
}
