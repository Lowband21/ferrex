#![feature(type_alias_impl_trait)]
use ferrex_player::*;

use env_logger::{Builder, Env, Target};
use iced::{Task, Theme};
use log::LevelFilter;
use lucide_icons::lucide_font_bytes;

use common::messages::DomainMessage;
use domains::ui::theme;
use iced::Program;
use state_refactored::State;

fn init_logger() {
    Builder::new()
        .target(Target::Stdout)
        .filter_level(LevelFilter::Warn) // Warn level for dependencies
        .filter_module("ferrex-player", LevelFilter::Debug)
        .init();
}

fn main() -> iced::Result {
    if std::env::var("RUST_LOG").is_err() {
        log::warn!("Failed to initialize logger from env, falling back to default");
        init_logger();
    } else {
        log::warn!("Initializing logger from env");
        env_logger::init();
    }

    // Initialize profiling system if enabled
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    infrastructure::profiling::init();

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

    let server_url =
        std::env::var("FERREX_SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

    let init = move || {
        // Create state using the new constructor
        let mut state = State::new(server_url.clone());

        // Initialize the global service registry
        infrastructure::service_registry::init_registry(state.image_service.clone());

        // Extract auth_service for use in the auth task
        let auth_service = state.domains.auth.state.auth_service.clone();

        let lib_id = state
            .domains
            .library
            .state
            .current_library_id
            .map(|library_id| library_id.as_uuid());

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

                        log::info!("[Auth] Auto-login enabled: {}", auto_login_enabled);

                        if auto_login_enabled {
                            // Apply the stored auth
                            match auth_service.apply_stored_auth(stored_auth).await {
                                Ok(()) => {
                                    log::info!("[Auth] Auto-login successful");
                                    Ok::<Option<bool>, String>(Some(true))
                                }
                                Err(e) => {
                                    log::error!("[Auth] Failed to apply stored auth: {}", e);
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
                    log::info!("[Auth] Auto-login enabled, sending CheckAuthStatus");
                    DomainMessage::Auth(domains::auth::messages::Message::CheckAuthStatus)
                }
                Ok(Some(false)) | Ok(None) => {
                    log::info!("[Auth] Auto-login disabled or no stored auth, sending LoadUsers");
                    DomainMessage::Auth(domains::auth::messages::Message::LoadUsers)
                }
                Err(e) => {
                    log::error!("[Auth] Error during auth check: {}", e);
                    DomainMessage::Auth(domains::auth::messages::Message::LoadUsers)
                }
            },
        );

        // Note: Library loading will happen after authentication
        (state, auth_task)
    };

    iced::application::<State, DomainMessage, Theme, iced_wgpu::Renderer>(
        init,
        update::update,
        view::view,
    )
    .subscription(subscriptions::subscription)
    .font(lucide_font_bytes())
    .theme(|_| theme::MediaServerTheme::theme())
    .present_mode(iced::PresentMode::AutoNoVsync)
    .window(iced::window::Settings {
        size: iced::Size::new(1280.0, 720.0),
        resizable: true,
        decorations: true,
        transparent: true, // Allow transparent background for Wayland subsurface video
        ..Default::default()
    })
    .run()
}
