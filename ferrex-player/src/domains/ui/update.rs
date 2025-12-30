use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::ui::{
        background_ui::update_background_ui, feedback_ui::update_feedback_ui,
        header_ui::update_header_ui, interaction_ui::update_interaction_ui,
        library_ui::update_library_ui, menu::poster_menu_update,
        messages::UiMessage,
        motion_controller::messages::MotionMessage as KineticMotionMessage,
        playback_ui::update_playback_ui, settings_ui::update_settings_ui,
        shell_ui::update_shell_ui,
        update_handlers::handle_virtual_carousel_message,
        update_handlers::home_focus, view_model_ui::update_view_model_ui,
        views::virtual_carousel::VirtualCarouselMessage as VCM,
        window_ui::update_window_ui,
    },
    state::State,
};

use iced::Task;
use std::time::Instant;

pub fn update_ui(state: &mut State, message: UiMessage) -> DomainUpdateResult {
    match message {
        UiMessage::FrameTick(now) => update_frame_tick(state, now),
        UiMessage::Shell(shell_msg) => update_shell_ui(state, shell_msg),
        UiMessage::Interaction(interaction_msg) => {
            update_interaction_ui(state, interaction_msg)
        }
        UiMessage::Library(library_msg) => {
            update_library_ui(state, library_msg)
        }
        UiMessage::Feedback(feedback_msg) => {
            update_feedback_ui(state, feedback_msg)
        }
        UiMessage::Window(window_msg) => update_window_ui(state, window_msg),
        UiMessage::Header(header_msg) => update_header_ui(state, header_msg),
        UiMessage::VirtualCarousel(vc_msg) => DomainUpdateResult::task(
            handle_virtual_carousel_message(state, vc_msg)
                .map(DomainMessage::Ui),
        ),
        UiMessage::Background(background_msg) => {
            update_background_ui(state, background_msg)
        }
        UiMessage::PosterMenu(menu_msg) => poster_menu_update(state, menu_msg),
        UiMessage::ViewModels(view_model_msg) => {
            update_view_model_ui(state, view_model_msg)
        }
        UiMessage::Playback(play_msg) => update_playback_ui(state, play_msg),
        #[cfg(feature = "debug-cache-overlay")]
        UiMessage::CacheOverlayTick => {
            let image_service = state.image_service.clone();
            let disk_cache = state.disk_image_cache.clone();
            let ram_max_bytes =
                state.runtime_config.image_cache_ram_max_bytes();
            let disk_max_bytes =
                state.runtime_config.image_cache_disk_max_bytes();
            let disk_ttl_days =
                state.runtime_config.image_cache_disk_ttl_days();

            let task = Task::perform(
                crate::domains::ui::views::cache_debug_overlay::sample_cache_overlay(
                    image_service,
                    disk_cache,
                    ram_max_bytes,
                    disk_max_bytes,
                    disk_ttl_days,
                ),
                UiMessage::CacheOverlayUpdated,
            )
            .map(DomainMessage::Ui);
            DomainUpdateResult::task(task)
        }
        #[cfg(feature = "debug-cache-overlay")]
        UiMessage::CacheOverlayUpdated(sample) => {
            state.domains.ui.state.cache_overlay_sample = Some(sample);
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::NoOp => DomainUpdateResult::task(Task::none()),
        UiMessage::Settings(settings_ui_message) => {
            update_settings_ui(state, settings_ui_message)
        }
    }
}

fn update_frame_tick(state: &mut State, now: Instant) -> DomainUpdateResult {
    let mut tasks: Vec<Task<DomainMessage>> = Vec::new();
    let mut events = Vec::new();

    // ========== KINETIC GRID SCROLLING ==========
    if state.domains.ui.state.motion_controller.is_active() {
        let task = crate::domains::ui::motion_controller::update::update(
            state,
            KineticMotionMessage::Tick(now),
        )
        .map(DomainMessage::Ui);
        tasks.push(task);
    }

    // ========== HOME TAB VERTICAL FOCUS ANIMATION ==========
    let in_all_curated = !state.domains.search.state.presentation.is_open()
        && matches!(
            state.domains.ui.state.scope,
            crate::domains::ui::shell_ui::Scope::Home
        )
        && matches!(
            state.tab_manager.active_tab_id(),
            crate::domains::ui::tabs::TabId::Home
        );
    if in_all_curated
        && let Some(crate::domains::ui::tabs::TabState::Home(all_state)) = state
            .tab_manager
            .get_tab(crate::domains::ui::tabs::TabId::Home)
        && all_state.focus.vertical_animator.is_active()
    {
        tasks.push(
            home_focus::handle_home_focus_tick(state, now)
                .map(DomainMessage::Ui),
        );
    }

    // ========== CAROUSEL MOTION (HORIZONTAL + SNAPPING) ==========
    // Only tick carousels that may be active in the current context.
    {
        use crate::domains::ui::tabs::TabState;
        use crate::domains::ui::types::ViewState;
        use crate::domains::ui::views::virtual_carousel::types::CarouselKey;
        let mut keys: Vec<CarouselKey> =
            match state.domains.ui.state.view.clone() {
                ViewState::SeriesDetail { series_id, .. } => {
                    vec![CarouselKey::ShowSeasons(series_id.to_uuid())]
                }
                ViewState::SeasonDetail { season_id, .. } => {
                    vec![CarouselKey::SeasonEpisodes(season_id.to_uuid())]
                }
                _ => Vec::new(),
            };

        // If in All view (curated), include its active carousel key
        if in_all_curated
            && let Some(TabState::Home(all_state)) = state
                .tab_manager
                .get_tab(crate::domains::ui::tabs::TabId::Home)
            && let Some(k) = all_state.focus.active_carousel.clone()
        {
            keys.push(k);
        }

        if !keys.is_empty() {
            let reg = &state.domains.ui.state.carousel_registry;
            let mut any_active = false;
            for key in keys {
                let scroller_active = reg
                    .get_scroller(&key)
                    .map(|s| s.is_active())
                    .unwrap_or(false);
                let animator_active = reg
                    .get_animator(&key)
                    .map(|a| a.is_active())
                    .unwrap_or(false);
                if scroller_active || animator_active {
                    any_active = true;
                    break;
                }
            }

            if any_active {
                tasks.push(
                    handle_virtual_carousel_message(
                        state,
                        VCM::MotionTick(now),
                    )
                    .map(DomainMessage::Ui),
                );
            }
        }
    }

    // ========== BACKGROUND + POSTER MENU TRANSITIONS ==========
    // When keep-alive is active, or any transitions are running, tick transitions.
    // `update_background_ui` further gates work internally.
    let poster_anim_active = state
        .domains
        .ui
        .state
        .poster_anim_active_until
        .map(|until| until >= now)
        .unwrap_or(false);

    let shader_state = &state.domains.ui.state.background_shader_state;
    let transitions_active = shader_state.color_transitions.is_transitioning()
        || shader_state.backdrop_transitions.is_transitioning()
        || shader_state.gradient_transitions.is_transitioning();

    if poster_anim_active || transitions_active {
        let res = update_background_ui(
            state,
            crate::domains::ui::background_ui::BackgroundMessage::UpdateTransitions,
        );
        tasks.push(res.task);
        events.extend(res.events);
    }

    // FrameTick also functions as a minimal keep-alive: even if no work is required
    // during this particular frame, the message itself keeps the UI repainting.
    DomainUpdateResult::with_events(Task::batch(tasks), events)
}
