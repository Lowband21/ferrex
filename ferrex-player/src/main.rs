use ferrex_player::{
    app::AppConfig,
    common::messages::DomainMessage,
    domains::{
        self,
        ui::{
            shell_ui::UiShellMessage, theme::MediaServerTheme,
            windows::WindowKind,
        },
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
        log::warn!(
            "Failed to initialize logger from env, fell back to default"
        );
    } else {
        env_logger::init();
        log::warn!("Initialized logger from env");
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
    let server_url = config.server_url().to_string();

    let init = move || {
        // Create state using the new constructor
        let mut state = State::new(server_url.clone());

        // Initialize the global service registry
        ferrex_player::infra::service_registry::init_registry(
            state.image_service.clone(),
        );

        // Extract auth_service for use in the auth task
        let auth_service = state.domains.auth.state.auth_service.clone();

        let lib_id = state
            .domains
            .ui
            .state
            .scope
            .lib_id()
            .map(|library_id| library_id.to_uuid());

        // Initialize depth lines for the default library view
        state
            .domains
            .ui
            .state
            .background_shader_state
            .update_depth_lines(
                &state.domains.ui.state.view,
                state.window_size.width,
                state.window_size.height,
                lib_id,
            );

        // Check for stored authentication
        let auth_task = Task::perform(
            async move {
                log::info!("[Auth] Checking for stored authentication...");

                match auth_service.load_from_keychain().await {
                    Ok(Some(stored_auth)) => {
                        log::info!(
                            "[Auth] Found stored auth for user: {}",
                            stored_auth.user.username
                        );

                        // Check if auto-login is enabled for this user
                        let auto_login_enabled = auth_service
                            .is_auto_login_enabled(&stored_auth.user.id)
                            .await
                            .unwrap_or(false)
                            && stored_auth.user.preferences.auto_login_enabled;

                        log::info!(
                            "[Auth] Auto-login enabled: {}",
                            auto_login_enabled
                        );

                        if auto_login_enabled {
                            // Apply the stored auth
                            match auth_service
                                .apply_stored_auth(stored_auth)
                                .await
                            {
                                Ok(()) => {
                                    log::info!("[Auth] Auto-login successful");
                                    Ok::<Option<bool>, String>(Some(true))
                                }
                                Err(e) => {
                                    log::error!(
                                        "[Auth] Failed to apply stored auth: {}",
                                        e
                                    );
                                    Ok::<Option<bool>, String>(Some(false))
                                }
                            }
                        } else {
                            log::info!("[Auth] Auto-login disabled");
                            Ok::<Option<bool>, String>(Some(false))
                        }
                    }
                    Ok(None) => {
                        log::info!("[Auth] No stored auth found");
                        Ok::<Option<bool>, String>(None)
                    }
                    Err(e) => {
                        log::error!("[Auth] Error loading stored auth: {}", e);
                        Ok::<Option<bool>, String>(None)
                    }
                }
            },
            |result| match result {
                Ok(Some(true)) => {
                    log::info!(
                        "[Auth] Auto-login enabled, sending CheckAuthStatus"
                    );
                    DomainMessage::Auth(
                        domains::auth::messages::AuthMessage::CheckAuthStatus,
                    )
                }
                Ok(Some(false)) | Ok(None) => {
                    log::info!(
                        "[Auth] Auto-login disabled or no stored auth, sending LoadUsers"
                    );
                    DomainMessage::Auth(
                        domains::auth::messages::AuthMessage::LoadUsers,
                    )
                }
                Err(e) => {
                    log::error!("[Auth] Error during auth check: {}", e);
                    DomainMessage::Auth(
                        domains::auth::messages::AuthMessage::LoadUsers,
                    )
                }
            },
        );

        (state, auth_task)
    };

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
            let (mut state, auth_task) = init();

            // Explicitly open the main window for daemon-based multi-window
            let (main_id, open) = window::open(window::Settings {
                size: iced::Size::new(1620.0, 1080.0),
                resizable: true,
                decorations: true,
                transparent: true,
                ..Default::default()
            });

            // Track main window id immediately
            state.windows.set(WindowKind::Main, main_id);

            let boot = Task::batch([
                auth_task,
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
