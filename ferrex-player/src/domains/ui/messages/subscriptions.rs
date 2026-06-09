use std::time::{Duration, Instant};

use super::UiMessage;
use crate::domains::search::keyboard::TenFootKeyboardDirection;
use crate::domains::ui::shell_ui::Scope;
use crate::domains::ui::{
    feedback_ui::FeedbackMessage, interaction_ui::InteractionMessage,
};

use crate::{
    common::messages::DomainMessage,
    domains::{
        search::messages::SearchMessage,
        ui::{
            motion_controller::messages::{
                Direction as Dir, MotionMessage as KM,
            },
            shell_ui::UiShellMessage,
            tabs::{TabId, TabState},
            types::ViewState,
            views::{
                tenfoot::{
                    detail::{TenFootDetailMessage, is_tenfoot_detail_route},
                    home::{TenFootHomeMessage, is_tenfoot_home_route},
                },
                virtual_carousel::{
                    messages::VirtualCarouselMessage as VCM, types::CarouselKey,
                },
            },
        },
    },
    state::State,
};

use iced::{
    Subscription,
    event::{self, Event as RuntimeEvent, Status as EventStatus},
    keyboard::{self, Key},
    window,
};

#[cfg(feature = "debug-cache-overlay")]
use iced::time;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];
    let mut needs_frame_tick = false;
    let search_open = state.domains.search.state.presentation.is_open();
    let tenfoot_home_active = is_tenfoot_home_route(state);
    let tenfoot_detail_active = is_tenfoot_detail_route(state);
    let tenfoot_playback_surface_active = state.interface_mode.is_tenfoot()
        && matches!(
            state.domains.ui.state.view,
            ViewState::Player
                | ViewState::LoadingVideo { .. }
                | ViewState::VideoError { .. }
        );

    // Delegate window lifecycle subscriptions (resize, move, focus) to the
    // window management module so secondary windows stay isolated
    subscriptions.push(
        crate::domains::ui::windows::subscriptions::subscription(state),
    );

    // Search surface keyboard interactions (overlay or detached window)
    if search_open {
        let tenfoot_search_overlay = state.interface_mode.is_tenfoot()
            && state.domains.search.state.presentation.is_overlay();
        let tenfoot_keyboard_open = tenfoot_search_overlay
            && state.domains.search.state.tenfoot_keyboard.is_open();
        subscriptions.push(event::listen().map(move |event| {
            search_surface_key_handler(
                event,
                tenfoot_search_overlay,
                tenfoot_keyboard_open,
            )
            .unwrap_or(DomainMessage::NoOp)
        }));
    }

    // Watch for close requests and close only our search window
    if let Some(search_id) = state.search_window_id {
        subscriptions.push(iced::window::close_requests().with(search_id).map(
            |(search_id, id)| {
                if id == search_id {
                    DomainMessage::Ui(UiShellMessage::CloseSearch.into())
                } else {
                    DomainMessage::NoOp
                }
            },
        ));
    }

    let in_grid_context = !search_open
        && !tenfoot_home_active
        && !tenfoot_detail_active
        && matches!(state.domains.ui.state.view, ViewState::Library)
        && matches!(state.domains.ui.state.scope, Scope::Library(_))
        && matches!(state.tab_manager.active_tab_id(), TabId::Library(_));

    if in_grid_context {
        subscriptions.push(event::listen_with(main_window_grid_key_handler));
    }

    if !search_open && tenfoot_home_active {
        subscriptions.push(event::listen_with(tenfoot_home_key_handler));
    }

    if !search_open && tenfoot_detail_active {
        subscriptions.push(event::listen_with(tenfoot_detail_key_handler));
    }

    if !search_open
        && !tenfoot_home_active
        && !tenfoot_detail_active
        && !tenfoot_playback_surface_active
    {
        subscriptions.push(event::listen().map(|ev| match ev {
            RuntimeEvent::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                ..
            }) => {
                if modifiers.control() || modifiers.alt() || modifiers.logo() {
                    return DomainMessage::NoOp;
                }
                use iced::keyboard::key::Named;
                match key {
                    Key::Named(Named::ArrowRight) => {
                        if modifiers.shift() {
                            DomainMessage::Ui(UiMessage::VirtualCarousel(
                                VCM::NextPageActive,
                            ))
                        } else {
                            // Disable kinetic hold: step by one item with snap
                            DomainMessage::Ui(UiMessage::VirtualCarousel(
                                VCM::NextItemActive,
                            ))
                        }
                    }
                    Key::Named(Named::ArrowLeft) => {
                        if modifiers.shift() {
                            DomainMessage::Ui(UiMessage::VirtualCarousel(
                                VCM::PrevPageActive,
                            ))
                        } else {
                            // Disable kinetic hold: step by one item with snap
                            DomainMessage::Ui(UiMessage::VirtualCarousel(
                                VCM::PrevItemActive,
                            ))
                        }
                    }
                    Key::Named(Named::Shift) => DomainMessage::Ui(
                        UiMessage::VirtualCarousel(VCM::SetBoostActive(true)),
                    ),
                    _ => DomainMessage::NoOp,
                }
            }
            RuntimeEvent::Keyboard(keyboard::Event::KeyReleased {
                key,
                modifiers,
                ..
            }) => {
                if modifiers.control() || modifiers.alt() || modifiers.logo() {
                    return DomainMessage::NoOp;
                }
                use iced::keyboard::key::Named;
                match key {
                    // With kinetic hold disabled for carousels, ignore Arrow releases
                    Key::Named(Named::ArrowRight) => DomainMessage::NoOp,
                    Key::Named(Named::ArrowLeft) => DomainMessage::NoOp,
                    Key::Named(Named::Shift) => DomainMessage::Ui(
                        UiMessage::VirtualCarousel(VCM::SetBoostActive(false)),
                    ),
                    _ => DomainMessage::NoOp,
                }
            }
            _ => DomainMessage::NoOp,
        }));

        // Track mouse movement globally to gate hover-driven focus switches
        subscriptions.push(event::listen().map(|ev| match ev {
            RuntimeEvent::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                DomainMessage::Ui(InteractionMessage::MouseMoved.into())
            }
            _ => DomainMessage::NoOp,
        }));
    }

    // All tab focus navigation (Up/Down to move between carousels)
    let in_all_curated = !search_open
        && !tenfoot_home_active
        && !tenfoot_detail_active
        && !tenfoot_playback_surface_active
        && matches!(state.domains.ui.state.scope, Scope::Home)
        && matches!(state.tab_manager.active_tab_id(), TabId::Home);
    if in_all_curated {
        subscriptions.push(event::listen().map(|ev| match ev {
            RuntimeEvent::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                ..
            }) => {
                if modifiers.control() || modifiers.alt() || modifiers.logo() {
                    return DomainMessage::NoOp;
                }
                use iced::keyboard::key::Named;
                match key {
                    Key::Named(Named::ArrowDown) => DomainMessage::Ui(
                        InteractionMessage::HomeFocusNext.into(),
                    ),
                    Key::Named(Named::ArrowUp) => DomainMessage::Ui(
                        InteractionMessage::HomeFocusPrev.into(),
                    ),
                    _ => DomainMessage::NoOp,
                }
            }
            _ => DomainMessage::NoOp,
        }));
    }

    #[cfg(feature = "debug-cache-overlay")]
    {
        subscriptions.push(
            time::every(Duration::from_secs(5))
                .map(|_| DomainMessage::Ui(UiMessage::CacheOverlayTick)),
        );
    }

    // Frame-synchronized ticking for any per-frame motion/animation. We only
    // subscribe once, then dispatch work via `UiMessage::FrameTick`.
    if state.domains.ui.state.motion_controller.is_active() {
        needs_frame_tick = true;
    }

    // Motion ticking for virtual carousels (All view active carousel, seasons/episodes)
    let mut carousel_motion_active = false;
    {
        use ViewState::*;
        let mut keys: Vec<CarouselKey> =
            match state.domains.ui.state.view.clone() {
                SeriesDetail { series_id, .. } => {
                    vec![CarouselKey::ShowSeasons(series_id.to_uuid())]
                }
                SeasonDetail { season_id, .. } => {
                    vec![CarouselKey::SeasonEpisodes(season_id.to_uuid())]
                }
                _ => Vec::new(),
            };
        // If in All view (curated), include its active carousel key
        if !tenfoot_home_active
            && !tenfoot_detail_active
            && matches!(state.domains.ui.state.scope, Scope::Home)
            && matches!(state.tab_manager.active_tab_id(), TabId::Home)
            && let Some(TabState::Home(all_state)) =
                state.tab_manager.get_tab(TabId::Home)
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
                carousel_motion_active = true;
            }
        }
    }
    if carousel_motion_active {
        needs_frame_tick = true;
    }

    // Vertical snapping for All view focus changes and poster keep alive
    let mut home_focus_anim_active = false;
    if in_all_curated
        && let Some(TabState::Home(all_state)) =
            state.tab_manager.get_tab(TabId::Home)
        && all_state.focus.vertical_animator.is_active()
    {
        home_focus_anim_active = true;
    }
    if home_focus_anim_active {
        needs_frame_tick = true;
    }

    let poster_anim_active = state
        .domains
        .ui
        .state
        .poster_anim_active_until
        .map(|until| until >= Instant::now())
        .unwrap_or(false);

    if poster_anim_active {
        needs_frame_tick = true;
    }

    let transitions_active = state
        .domains
        .ui
        .state
        .background_shader_state
        .color_transitions
        .is_transitioning()
        || state
            .domains
            .ui
            .state
            .background_shader_state
            .backdrop_transitions
            .is_transitioning()
        || state
            .domains
            .ui
            .state
            .background_shader_state
            .gradient_transitions
            .is_transitioning()
        || poster_anim_active;
    if transitions_active {
        needs_frame_tick = true;
    }

    if needs_frame_tick {
        subscriptions.push(
            window::frames().map(|instant| {
                DomainMessage::Ui(UiMessage::FrameTick(instant))
            }),
        );
    }

    // Toast expiry subscription - tick every 100ms when toasts are active
    if state.domains.ui.state.toast_manager.has_toasts() {
        subscriptions.push(
            iced::time::every(Duration::from_millis(100))
                .map(|_| DomainMessage::Ui(FeedbackMessage::ToastTick.into())),
        );
    }

    Subscription::batch(subscriptions)
}

fn tenfoot_home_key_handler(
    event: RuntimeEvent,
    status: EventStatus,
    _window: iced::window::Id,
) -> Option<DomainMessage> {
    use crate::common::focus::{SpatialAction, SpatialDirection};
    use iced::keyboard::key::Named;

    if !matches!(status, EventStatus::Ignored) {
        return None;
    }

    let RuntimeEvent::Keyboard(keyboard::Event::KeyPressed {
        key,
        modifiers,
        ..
    }) = event
    else {
        return None;
    };

    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }

    let action = match key {
        Key::Named(Named::ArrowUp) => {
            Some(SpatialAction::Move(SpatialDirection::Up))
        }
        Key::Named(Named::ArrowDown) => {
            Some(SpatialAction::Move(SpatialDirection::Down))
        }
        Key::Named(Named::ArrowLeft) => {
            Some(SpatialAction::Move(SpatialDirection::Left))
        }
        Key::Named(Named::ArrowRight) => {
            Some(SpatialAction::Move(SpatialDirection::Right))
        }
        Key::Named(Named::Enter) => Some(SpatialAction::Activate),
        Key::Named(Named::Space) => Some(SpatialAction::Activate),
        Key::Named(Named::Escape) => Some(SpatialAction::Back),
        Key::Character(value)
            if value == "/" || value.eq_ignore_ascii_case("s") =>
        {
            Some(SpatialAction::Search)
        }
        Key::Character(value) if value.eq_ignore_ascii_case("m") => {
            Some(SpatialAction::Menu)
        }
        Key::Character(value) if value == " " => Some(SpatialAction::Activate),
        _ => None,
    }?;

    let msg = match action {
        SpatialAction::Move(direction) => TenFootHomeMessage::Move(direction),
        SpatialAction::Activate => TenFootHomeMessage::ActivateFocused,
        SpatialAction::Back => TenFootHomeMessage::Back,
        SpatialAction::Search => TenFootHomeMessage::Search,
        SpatialAction::Menu => TenFootHomeMessage::OpenFocusedMenu,
    };

    Some(DomainMessage::Ui(msg.into()))
}

fn tenfoot_detail_key_handler(
    event: RuntimeEvent,
    status: EventStatus,
    _window: iced::window::Id,
) -> Option<DomainMessage> {
    use crate::common::focus::{SpatialAction, SpatialDirection};
    use iced::keyboard::key::Named;

    if !matches!(status, EventStatus::Ignored) {
        return None;
    }

    let RuntimeEvent::Keyboard(keyboard::Event::KeyPressed {
        key,
        modifiers,
        ..
    }) = event
    else {
        return None;
    };

    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }

    let action = match key {
        Key::Named(Named::ArrowUp) => {
            Some(SpatialAction::Move(SpatialDirection::Up))
        }
        Key::Named(Named::ArrowDown) => {
            Some(SpatialAction::Move(SpatialDirection::Down))
        }
        Key::Named(Named::ArrowLeft) => {
            Some(SpatialAction::Move(SpatialDirection::Left))
        }
        Key::Named(Named::ArrowRight) => {
            Some(SpatialAction::Move(SpatialDirection::Right))
        }
        Key::Named(Named::Enter) => Some(SpatialAction::Activate),
        Key::Named(Named::Space) => Some(SpatialAction::Activate),
        Key::Named(Named::Escape) => Some(SpatialAction::Back),
        Key::Named(Named::Backspace) => Some(SpatialAction::Back),
        Key::Character(value) if value.eq_ignore_ascii_case("b") => {
            Some(SpatialAction::Back)
        }
        Key::Character(value) if value == " " => Some(SpatialAction::Activate),
        _ => None,
    }?;

    let msg = match action {
        SpatialAction::Move(direction) => TenFootDetailMessage::Move(direction),
        SpatialAction::Activate => TenFootDetailMessage::ActivateFocused,
        SpatialAction::Back => TenFootDetailMessage::Back,
        SpatialAction::Search | SpatialAction::Menu => {
            TenFootDetailMessage::Back
        }
    };

    Some(DomainMessage::Ui(msg.into()))
}

fn search_surface_key_handler(
    event: RuntimeEvent,
    tenfoot_search_overlay: bool,
    tenfoot_keyboard_open: bool,
) -> Option<DomainMessage> {
    use iced::keyboard::key::Named;

    if let RuntimeEvent::Keyboard(keyboard::Event::KeyPressed {
        key,
        modifiers,
        ..
    }) = event
    {
        if modifiers.control() || modifiers.alt() || modifiers.logo() {
            return None;
        }

        if tenfoot_search_overlay && tenfoot_keyboard_open {
            return match key {
                Key::Named(Named::Escape) => {
                    Some(DomainMessage::Search(SearchMessage::HandleEscape))
                }
                Key::Named(Named::Enter) => Some(DomainMessage::Search(
                    SearchMessage::TenFootKeyboardActivate,
                )),
                Key::Named(Named::ArrowUp) => Some(DomainMessage::Search(
                    SearchMessage::TenFootKeyboardMove(
                        TenFootKeyboardDirection::Up,
                    ),
                )),
                Key::Named(Named::ArrowDown) => Some(DomainMessage::Search(
                    SearchMessage::TenFootKeyboardMove(
                        TenFootKeyboardDirection::Down,
                    ),
                )),
                Key::Named(Named::ArrowLeft) => Some(DomainMessage::Search(
                    SearchMessage::TenFootKeyboardMove(
                        TenFootKeyboardDirection::Left,
                    ),
                )),
                Key::Named(Named::ArrowRight) => Some(DomainMessage::Search(
                    SearchMessage::TenFootKeyboardMove(
                        TenFootKeyboardDirection::Right,
                    ),
                )),
                _ => None,
            };
        }

        match key {
            Key::Named(Named::Escape) => {
                Some(DomainMessage::Search(SearchMessage::HandleEscape))
            }
            Key::Named(Named::Enter) => {
                Some(DomainMessage::Search(SearchMessage::SelectCurrent))
            }
            Key::Named(Named::ArrowUp) => {
                Some(DomainMessage::Search(SearchMessage::SelectPrevious))
            }
            Key::Named(Named::ArrowDown) => {
                Some(DomainMessage::Search(SearchMessage::SelectNext))
            }
            Key::Named(Named::ArrowLeft) | Key::Named(Named::ArrowRight)
                if tenfoot_search_overlay =>
            {
                Some(DomainMessage::Search(SearchMessage::ShowTenFootKeyboard))
            }
            Key::Character(value) if modifiers.shift() => None,
            Key::Character(value) if value.eq_ignore_ascii_case("k") => {
                Some(DomainMessage::Search(SearchMessage::SelectPrevious))
            }
            Key::Character(value) if value.eq_ignore_ascii_case("j") => {
                Some(DomainMessage::Search(SearchMessage::SelectNext))
            }
            _ => None,
        }
    } else {
        None
    }
}

fn main_window_grid_key_handler(
    event: RuntimeEvent,
    status: EventStatus,
    _window: iced::window::Id,
) -> Option<DomainMessage> {
    use iced::keyboard::key::Named;

    if !matches!(status, EventStatus::Ignored) {
        return None;
    }

    match event {
        RuntimeEvent::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            repeat,
            ..
        }) => {
            if modifiers.control() || modifiers.alt() || modifiers.logo() {
                return None;
            }
            // Ignore OS auto-repeat; motion controller handles continuous motion
            if repeat {
                return None;
            }
            match key {
                Key::Named(Named::ArrowDown) => Some(DomainMessage::Ui(
                    InteractionMessage::KineticScroll(KM::Start(Dir::Down))
                        .into(),
                )),
                Key::Named(Named::ArrowUp) => Some(DomainMessage::Ui(
                    InteractionMessage::KineticScroll(KM::Start(Dir::Up))
                        .into(),
                )),
                Key::Named(Named::Shift) => Some(DomainMessage::Ui(
                    InteractionMessage::KineticScroll(KM::SetBoost(true))
                        .into(),
                )),
                _ => None,
            }
        }
        RuntimeEvent::Keyboard(keyboard::Event::KeyReleased {
            key,
            modifiers,
            ..
        }) => {
            if modifiers.control() || modifiers.alt() || modifiers.logo() {
                return None;
            }
            match key {
                Key::Named(Named::ArrowDown) => Some(DomainMessage::Ui(
                    InteractionMessage::KineticScroll(KM::Stop(Dir::Down))
                        .into(),
                )),
                Key::Named(Named::ArrowUp) => Some(DomainMessage::Ui(
                    InteractionMessage::KineticScroll(KM::Stop(Dir::Up)).into(),
                )),
                Key::Named(Named::Shift) => Some(DomainMessage::Ui(
                    InteractionMessage::KineticScroll(KM::SetBoost(false))
                        .into(),
                )),
                _ => None,
            }
        }
        _ => None,
    }
}

// No standalone handler needed; we capture the key with event::listen().
