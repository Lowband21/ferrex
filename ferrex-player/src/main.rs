use iced::{
    widget::{button, text},
    Element, Font, Length, Subscription, Task,
};
use lucide_icons::{lucide_font_bytes, Icon};
use std::collections::HashMap;
use std::sync::Arc;

// Module declarations organized by functionality
// Core modules
mod config;
mod constants;
mod messages; // Domain message modules
mod prelude;
mod scroll_manager;
mod state;
mod subscriptions;
mod theme;
mod transitions;
mod updates;

// UI Components
mod components;
mod view_models;
mod views;
mod widgets;

// Media handling
mod api_types;
mod image_pipeline;
mod image_types;
mod media_library;
mod media_store;

// Authentication
mod api_client;
mod auth_dto;
mod auth_errors;
mod auth_manager;
mod auth_state;
mod auth_storage;
mod batch_metadata_fetcher;
mod hardware_fingerprint;
mod metadata_coordinator;
mod metadata_service;
mod models;
mod permissions;
mod player;
mod security;
mod server;

// Caching and performance
mod performance_config;
mod profiling;
mod service_registry;
mod unified_image_service;

// Utilities
mod util;

// External crate imports
use gstreamer as gst;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

// Internal module imports
use crate::{
    image_types::ImageRequest,
    messages::{auth, library, media, metadata, streaming, ui, DomainMessage},
    state::{State, ViewState},
    views::library::view_library,
};
use profiling::PROFILER;
use views::{
    admin::{view_admin_dashboard, view_library_management},
    movies::view_movie_detail,
    tv::{view_episode_detail, view_season_detail, view_tv_show_detail},
    view_loading_video, view_video_error,
};

// Use MediaEvent from ferrex_core
use ferrex_core::MediaEvent;

/// Helper function to create icon text
fn icon_text(icon: lucide_icons::Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

fn main() -> iced::Result {
    // Initialize logger with debug level if not set
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "ferrex_player=debug");
    }
    env_logger::init();

    let server_url =
        std::env::var("FERREX_SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

    let init = move || {
        // Initialize UnifiedImageService with proper receiver
        let (image_service, image_receiver) = unified_image_service::UnifiedImageService::new(4);

        // Initialize the global service registry
        service_registry::init_registry(image_service.clone());

        // Initialize API client and auth manager
        let api_client = crate::api_client::ApiClient::new(server_url.clone());
        let auth_manager = crate::auth_manager::AuthManager::new(api_client.clone());

        let mut state = State {
            server_url: server_url.clone(),
            loading: true,
            image_service,
            image_receiver: Arc::new(Mutex::new(Some(image_receiver))),
            api_client: Some(api_client),
            auth_manager: Some(auth_manager.clone()),
            ..Default::default()
        };

        // BatchMetadataFetcher initialization moved to after login success
        // to ensure ApiClient has authentication token

        // Initialize depth lines for the default library view
        state.background_shader_state.update_depth_lines(
            &state.view,
            state.window_size.width,
            state.window_size.height,
        );

        // Check for stored authentication
        let auth_task = Task::perform(
            async move {
                log::info!("[Auth] Checking for stored authentication...");
                let stored_auth_result = auth_manager.load_from_keychain().await;
                log::info!("[Auth] load_from_keychain completed");
                
                // Check if we have valid stored auth
                match stored_auth_result {
                    Ok(Some(stored_auth)) => {
                        log::info!("[Auth] Found stored auth for user: {}", stored_auth.user.username);
                        
                        // Check if auto-login is enabled for this device and user
                        // This checks BOTH the user's database preference AND device-specific setting
                        let device_auto_login = auth_manager.auth_storage()
                            .is_auto_login_enabled(&stored_auth.user.id)
                            .await
                            .unwrap_or(false);
                        let auto_login_enabled = stored_auth.user.preferences.auto_login_enabled && device_auto_login;
                        
                        log::info!("[Auth] Device auto-login enabled: {}", auto_login_enabled);
                        
                        if auto_login_enabled {
                            // Apply the stored auth
                            if let Err(e) = auth_manager.apply_stored_auth(stored_auth).await {
                                log::error!("[Auth] Failed to apply stored auth: {}", e);
                                Ok(None)
                            } else {
                                log::info!("[Auth] Auto-login successful");
                                Ok(Some(true))
                            }
                        } else {
                            log::info!("[Auth] Auto-login disabled");
                            Ok(Some(false))
                        }
                    }
                    Ok(None) => {
                        log::info!("[Auth] No stored auth found");
                        Ok(None)
                    }
                    Err(e) => {
                        log::error!("[Auth] Error loading stored auth: {}", e);
                        Err(e)
                    }
                }
            },
            |result| match result {
                Ok(Some(true)) => {
                    log::info!("[Auth] Auto-login enabled, sending CheckAuthStatus");
                    DomainMessage::Auth(crate::messages::auth::Message::CheckAuthStatus)
                }
                Ok(Some(false)) | Ok(None) => {
                    log::info!("[Auth] Auto-login disabled or no stored auth, sending LoadUsers");
                    DomainMessage::Auth(crate::messages::auth::Message::LoadUsers)
                }
                Err(e) => {
                    log::error!("[Auth] Error loading from keychain: {}", e);
                    DomainMessage::Auth(crate::messages::auth::Message::CheckSetupStatus)
                }
            },
        );

        // Note: Library loading will happen after authentication
        (state, auth_task)
    };

    iced::application(init, update, view)
        .subscription(subscription)
        .font(lucide_font_bytes())
        .theme(|_| theme::MediaServerTheme::theme())
        .window(iced::window::Settings {
            size: iced::Size::new(1280.0, 720.0),
            resizable: true,
            decorations: true,
            ..Default::default()
        })
        .run()
}

/// Domain-aware update function that routes messages to appropriate handlers
fn update(state: &mut State, message: DomainMessage) -> Task<DomainMessage> {
    use crate::messages::DomainMessage;
    use crate::updates::update_auth::update_auth;
    use crate::updates::update_library::update_library;
    use crate::updates::update_media::update_media;
    use crate::updates::update_metadata::update_metadata;
    use crate::updates::update_streaming::update_streaming;
    use crate::updates::update_ui::update_ui;
    use crate::updates::update_user_management::update_user_management;

    // Add profiling for domain messages
    let message_name = message.name();
    PROFILER.start(&format!("update::{}", message_name));

    let result = match message {
        // Route auth messages to the auth domain handler
        DomainMessage::Auth(auth_msg) => {
            // Check for internal cross-domain events
            if let crate::messages::auth::Message::_EmitCrossDomainEvent(event) = &auth_msg {
                return messages::cross_domain::handle_event(state, event.clone());
            }
            update_auth(state, auth_msg).map(DomainMessage::Auth)
        }

        // Route library messages to the library domain handler
        DomainMessage::Library(library_msg) => {
            // Check for internal cross-domain events
            if let crate::messages::library::Message::_EmitCrossDomainEvent(event) = &library_msg {
                return messages::cross_domain::handle_event(state, event.clone());
            }
            update_library(state, library_msg).map(DomainMessage::Library)
        }

        // Route media messages to the media domain handler
        DomainMessage::Media(media_msg) => {
            // Check for internal cross-domain events
            if let crate::messages::media::Message::_EmitCrossDomainEvent(event) = &media_msg {
                return messages::cross_domain::handle_event(state, event.clone());
            }
            update_media(state, media_msg).map(DomainMessage::Media)
        }

        // Route metadata messages to the metadata domain handler
        DomainMessage::Metadata(metadata_msg) => {
            if let crate::messages::metadata::Message::_EmitCrossDomainEvent(event) = &metadata_msg
            {
                return messages::cross_domain::handle_event(state, event.clone());
            }
            update_metadata(state, metadata_msg).map(DomainMessage::Metadata)
        }

        // Route UI messages to the UI domain handler
        DomainMessage::Ui(ui_msg) => {
            // Check for internal cross-domain events
            if let crate::messages::ui::Message::_EmitCrossDomainEvent(event) = &ui_msg {
                return messages::cross_domain::handle_event(state, event.clone());
            }
            update_ui(state, ui_msg).map(DomainMessage::Ui)
        }

        // Route streaming messages to the streaming domain handler
        DomainMessage::Streaming(streaming_msg) => {
            // Check for internal cross-domain events
            if let crate::messages::streaming::Message::_EmitCrossDomainEvent(event) =
                &streaming_msg
            {
                return messages::cross_domain::handle_event(state, event.clone());
            }
            update_streaming(state, streaming_msg).map(DomainMessage::Streaming)
        }

        // Cross-domain messages
        DomainMessage::NoOp => Task::none(),

        DomainMessage::ClearError => {
            state.error_message = None;
            Task::none()
        }

        DomainMessage::Event(event) => {
            // Process cross-domain events and trigger appropriate domain actions
            messages::cross_domain::handle_event(state, event)
        }

        // Route settings messages to the settings domain handler
        DomainMessage::Settings(settings_msg) => {
            // Check for internal cross-domain events
            if let crate::messages::settings::Message::_EmitCrossDomainEvent(event) = &settings_msg {
                return messages::cross_domain::handle_event(state, event.clone());
            }
            use crate::updates::settings::update_settings;
            update_settings(state, settings_msg).map(DomainMessage::Settings)
        }

        // Route user management messages to the user management domain handler
        DomainMessage::UserManagement(user_mgmt_msg) => {
            // Check for internal cross-domain events
            if let crate::messages::user_management::Message::_EmitCrossDomainEvent(event) = &user_mgmt_msg {
                return messages::cross_domain::handle_event(state, event.clone());
            }
            update_user_management(state, user_mgmt_msg)
        }

        // Other domains not implemented yet
        _ => Task::none(),
    };

    PROFILER.end(&format!("update::{}", message_name));
    result
}

fn view(state: &State) -> Element<DomainMessage> {
    use crate::widgets::{background_shader, BackgroundEffect};
    use iced::widget::Stack;

    PROFILER.start("view");

    // Check for first-run setup
    if matches!(state.view, ViewState::FirstRunSetup) {
        let first_run_content =
            crate::views::first_run::view_first_run(state, &state.first_run_state)
                .map(DomainMessage::from);
        PROFILER.end("view");
        return first_run_content;
    }

    // Check authentication state
    if !state.is_authenticated {
        log::debug!("[Auth] Not authenticated, showing auth view");
        let auth_content = views::view_auth(&state.auth_flow, state.user_permissions.as_ref()).map(DomainMessage::from);
        PROFILER.end("view");
        return auth_content;
    }

    // Get the view content
    let content = match &state.view {
        ViewState::Library => view_library(state).map(DomainMessage::from),
        ViewState::LibraryManagement => view_library_management(state).map(DomainMessage::from),
        ViewState::AdminDashboard => view_admin_dashboard(state).map(DomainMessage::from),
        ViewState::FirstRunSetup => {
            crate::views::first_run::view_first_run(state, &state.first_run_state)
                .map(DomainMessage::from)
        }
        ViewState::Player => view_player(state).map(DomainMessage::from),
        ViewState::LoadingVideo { url } => view_loading_video(state, url).map(DomainMessage::from),
        ViewState::VideoError { message } => view_video_error(message).map(DomainMessage::from),
        ViewState::MovieDetail { movie, .. } => {
            view_movie_detail(state, movie).map(DomainMessage::from)
        }
        ViewState::TvShowDetail { series_id, .. } => {
            view_tv_show_detail(state, series_id.as_str()).map(DomainMessage::from)
        }
        ViewState::SeasonDetail {
            series_id,
            season_id,
            ..
        } => view_season_detail(state, series_id, season_id).map(DomainMessage::from),
        ViewState::EpisodeDetail { episode_id, .. } => {
            view_episode_detail(state, episode_id).map(DomainMessage::from)
        }
        ViewState::UserSettings => {
            crate::views::settings::view_user_settings(state).map(DomainMessage::from)
        }
    };

    // Add header if the view needs it
    let content_with_header = if state.view.has_header() {
        use crate::views::header::view_header;
        use iced::widget::{column, container, scrollable};

        let header = view_header(state).map(DomainMessage::from);

        // Wrap header in a container with opaque background
        let header_container = container(header)
            .width(Length::Fill)
            .style(theme::Container::Header.style());

        // Check if this is a detail view that needs scrollable content
        let scrollable_content = match &state.view {
            ViewState::MovieDetail { .. }
            | ViewState::TvShowDetail { .. }
            | ViewState::SeasonDetail { .. }
            | ViewState::EpisodeDetail { .. } => {
                // Wrap content in scrollable for detail views
                scrollable(content)
                    .on_scroll(|viewport| {
                        DomainMessage::from(messages::ui::Message::DetailViewScrolled(viewport))
                    })
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
            _ => {
                // Library and other views already have their own scrollable
                content
            }
        };

        column![header_container, scrollable_content]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        content
    };

    // Use ViewState helper methods for cleaner logic
    let result = if state.view.has_background() {
        // Note: Theme colors and backdrops are now handled in the update function
        // when view changes occur, updating state.background_shader_state

        // Create background shader from persistent state
        let mut bg_shader = background_shader()
            .colors(
                state.background_shader_state.primary_color,
                state.background_shader_state.secondary_color,
            )
            .scroll_offset(state.background_shader_state.scroll_offset)
            .gradient_center(state.background_shader_state.gradient_center);

        // For detail views, tell shader to offset backdrop by header height
        if matches!(
            &state.view,
            ViewState::MovieDetail { .. }
                | ViewState::TvShowDetail { .. }
                | ViewState::SeasonDetail { .. }
                | ViewState::EpisodeDetail { .. }
        ) {
            bg_shader = bg_shader.header_offset(crate::constants::layout::header::HEIGHT);
        }

        // Get backdrop from image service based on current view (reactive approach)
        let backdrop_handle = match &state.view {
            ViewState::MovieDetail { movie, .. } => {
                let request = crate::image_types::ImageRequest::new(
                    ferrex_core::api_types::MediaId::Movie(movie.id.clone()),
                    crate::image_types::ImageSize::Backdrop,
                );
                state.image_service.get(&request)
            }
            ViewState::TvShowDetail { series_id, .. } => {
                let request = crate::image_types::ImageRequest::new(
                    ferrex_core::api_types::MediaId::Series(series_id.clone()),
                    crate::image_types::ImageSize::Backdrop,
                );
                state.image_service.get(&request)
            }
            ViewState::SeasonDetail { season_id, .. } => {
                let request = crate::image_types::ImageRequest::new(
                    ferrex_core::api_types::MediaId::Season(season_id.clone()),
                    crate::image_types::ImageSize::Backdrop,
                );
                state.image_service.get(&request)
            }
            _ => None,
        };

        // Add backdrop if available
        if let Some(handle) = backdrop_handle {
            // Use BackdropGradient effect with dummy fade values (shader calculates them dynamically)
            bg_shader = bg_shader
                .backdrop_with_fade(handle, 0.75, 1.0) // Values ignored by shader
                .backdrop_aspect_mode(state.background_shader_state.backdrop_aspect_mode);
        } else {
            // No backdrop, use gradient effect
            bg_shader = bg_shader.effect(BackgroundEffect::Gradient);
        }

        // Add depth layout from state
        if !state
            .background_shader_state
            .depth_layout
            .regions
            .is_empty()
        {
            bg_shader =
                bg_shader.with_depth_layout(state.background_shader_state.depth_layout.clone());
        }

        // Create a stack with background as base layer
        // Convert bg_shader to Element first, then map from ui::Message to DomainMessage
        let bg_shader_element: Element<ui::Message> = bg_shader.into();
        let bg_shader_mapped: Element<DomainMessage> = bg_shader_element.map(DomainMessage::from);

        Stack::new()
            .push(bg_shader_mapped)
            .push(content_with_header)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        // For player view, no background
        content_with_header
    };

    PROFILER.end("view");

    result
}

// Get the lucide font
fn lucide_font() -> Font {
    Font::with_name("lucide")
}

fn view_player(state: &State) -> Element<media::Message> {
    state.player.view()
}

fn subscription(state: &State) -> Subscription<DomainMessage> {
    // Delegate to the centralized subscription composition
    crate::subscriptions::subscription(state)
}
