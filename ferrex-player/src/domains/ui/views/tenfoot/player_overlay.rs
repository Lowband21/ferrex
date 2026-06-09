//! 10-foot player overlay and controller action handling.
//!
//! This module keeps the existing player/video pipeline intact and layers a
//! TV-scale transport HUD over it only while the existing player controls are
//! visible. Keyboard/controller focus is derived from the rendered control
//! rectangles and resolved with the shared spatial focus scorer.

use std::sync::{Mutex, MutexGuard};

use crate::{
    common::{
        controller_input::{
            ControllerButton, ControllerEvent, ControllerInputMapper,
        },
        focus::{
            FocusLayoutRect, FocusMargins, SpatialAction, SpatialDirection,
            SpatialFocusBuilder, SpatialFocusId, SpatialFocusState,
            SpatialFocusable,
        },
        messages::DomainMessage,
        ui_utils::lucide_font,
    },
    domains::{
        player::{messages::PlayerMessage, state::PlayerDomainState},
        ui::{shell_ui::UiShellMessage, theme::MediaServerTheme},
    },
    infra::constants::player::seeking::{
        SEEK_BACKWARD_COURSE, SEEK_FORWARD_COURSE,
    },
    state::State,
};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Shadow,
    Subscription, Theme, Vector,
    event::{self, Event as RuntimeEvent},
    keyboard::{self, Key, Modifiers, key::Named},
    widget::{Space, button, column, container, mouse_area, row, text},
};
use lucide_icons::Icon;

const PLAY_ID: &str = "10ft.player.play_pause";
const REWIND_ID: &str = "10ft.player.seek_back";
const FORWARD_ID: &str = "10ft.player.seek_forward";
const PREVIOUS_ID: &str = "10ft.player.previous";
const NEXT_ID: &str = "10ft.player.next";
const SUBTITLE_ID: &str = "10ft.player.subtitles";
const AUDIO_ID: &str = "10ft.player.audio";
const FULLSCREEN_ID: &str = "10ft.player.fullscreen";
const EXIT_ID: &str = "10ft.player.exit";

#[derive(Debug, Default)]
struct OverlayRuntime {
    focused: Option<SpatialFocusId>,
    controls_hidden: bool,
}

static OVERLAY_RUNTIME: Mutex<OverlayRuntime> = Mutex::new(OverlayRuntime {
    focused: None,
    controls_hidden: false,
});

fn overlay_runtime() -> MutexGuard<'static, OverlayRuntime> {
    OVERLAY_RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, Copy)]
struct PlayerOverlayLayout;

impl PlayerOverlayLayout {
    const MIN_VIEWPORT_W: f32 = 800.0;
    const MIN_VIEWPORT_H: f32 = 480.0;

    const TOP_PAD_TOP: f32 = 28.0;
    const TOP_PAD_X: f32 = 44.0;
    const TOP_PAD_BOTTOM: f32 = 62.0;

    const PANEL_PAD_TOP: f32 = 18.0;
    const PANEL_PAD_RIGHT: f32 = 44.0;
    const PANEL_PAD_BOTTOM: f32 = 26.0;
    const PANEL_PAD_LEFT: f32 = 44.0;
    const PANEL_COLUMN_GAP: f32 = 16.0;

    const TIME_LABEL_W: f32 = 124.0;
    const PROGRESS_ROW_GAP: f32 = 18.0;
    const PROGRESS_ROW_H: f32 = 32.0;
    const PROGRESS_H: f32 = 18.0;
    const PROGRESS_VISUAL_H: f32 = 8.0;

    const CONTROL_ROW_GAP: f32 = 28.0;
    const CONTROL_ROW_H: f32 = Self::PLAY_H;

    const TRANSPORT_GAP: f32 = 10.0;
    const TRANSPORT_SMALL_W: f32 = 64.0;
    const TRANSPORT_SMALL_H: f32 = 58.0;
    const PLAY_W: f32 = 82.0;
    const PLAY_H: f32 = 70.0;

    const COMMAND_GAP: f32 = 10.0;
    const COMMAND_H: f32 = 52.0;
    const FOCUS_MARGIN_X: f32 = 8.0;
    const FOCUS_MARGIN_Y: f32 = 6.0;
    const SUBTITLE_W: f32 = 72.0;
    const AUDIO_W: f32 = 92.0;
    const FULLSCREEN_W: f32 = 96.0;
    const EXIT_W: f32 = 82.0;

    const PANEL_H: f32 = Self::PANEL_PAD_TOP
        + Self::PROGRESS_ROW_H
        + Self::PANEL_COLUMN_GAP
        + Self::CONTROL_ROW_H
        + Self::PANEL_PAD_BOTTOM;

    fn focus_margins() -> FocusMargins {
        FocusMargins::symmetric(Self::FOCUS_MARGIN_X, Self::FOCUS_MARGIN_Y)
    }

    fn focus_layout(layout: FocusLayoutRect) -> FocusLayoutRect {
        layout.expanded(Self::focus_margins())
    }

    fn viewport_width(width: f32) -> f32 {
        width.max(Self::MIN_VIEWPORT_W)
    }

    fn viewport_height(height: f32) -> f32 {
        height.max(Self::MIN_VIEWPORT_H)
    }

    fn top_bar_padding() -> Padding {
        Padding {
            top: Self::TOP_PAD_TOP,
            right: Self::TOP_PAD_X,
            bottom: Self::TOP_PAD_BOTTOM,
            left: Self::TOP_PAD_X,
        }
    }

    fn panel_padding() -> Padding {
        Padding {
            top: Self::PANEL_PAD_TOP,
            right: Self::PANEL_PAD_RIGHT,
            bottom: Self::PANEL_PAD_BOTTOM,
            left: Self::PANEL_PAD_LEFT,
        }
    }

    fn panel_y(viewport_height: f32) -> f32 {
        Self::viewport_height(viewport_height) - Self::PANEL_H
    }

    fn content_width(viewport_width: f32) -> f32 {
        (Self::viewport_width(viewport_width)
            - Self::PANEL_PAD_LEFT
            - Self::PANEL_PAD_RIGHT)
            .max(0.0)
    }

    fn progress_rect(
        viewport_width: f32,
        viewport_height: f32,
    ) -> FocusLayoutRect {
        let content_width = Self::content_width(viewport_width);
        FocusLayoutRect::new(
            Self::PANEL_PAD_LEFT + Self::TIME_LABEL_W + Self::PROGRESS_ROW_GAP,
            Self::panel_y(viewport_height)
                + Self::PANEL_PAD_TOP
                + (Self::PROGRESS_ROW_H - Self::PROGRESS_H) / 2.0,
            (content_width
                - Self::TIME_LABEL_W * 2.0
                - Self::PROGRESS_ROW_GAP * 2.0)
                .max(0.0),
            Self::PROGRESS_H,
        )
    }

    fn control_row_y(viewport_height: f32) -> f32 {
        Self::panel_y(viewport_height)
            + Self::PANEL_PAD_TOP
            + Self::PROGRESS_ROW_H
            + Self::PANEL_COLUMN_GAP
    }

    fn transport_row_width() -> f32 {
        Self::TRANSPORT_SMALL_W * 4.0 + Self::PLAY_W + Self::TRANSPORT_GAP * 4.0
    }

    fn transport_row_x(viewport_width: f32) -> f32 {
        let content_width = Self::content_width(viewport_width);
        let side_slot = ((content_width
            - Self::transport_row_width()
            - Self::CONTROL_ROW_GAP * 2.0)
            / 2.0)
            .max(0.0);

        Self::PANEL_PAD_LEFT + side_slot + Self::CONTROL_ROW_GAP
    }

    fn transport_button_size(id: &str) -> (f32, f32) {
        if id == PLAY_ID {
            (Self::PLAY_W, Self::PLAY_H)
        } else {
            (Self::TRANSPORT_SMALL_W, Self::TRANSPORT_SMALL_H)
        }
    }

    fn transport_button_rect(
        viewport_width: f32,
        viewport_height: f32,
        id: &str,
    ) -> FocusLayoutRect {
        let row_x = Self::transport_row_x(viewport_width);
        let small_pitch = Self::TRANSPORT_SMALL_W + Self::TRANSPORT_GAP;
        let x = match id {
            PREVIOUS_ID => row_x,
            REWIND_ID => row_x + small_pitch,
            PLAY_ID => row_x + small_pitch * 2.0,
            FORWARD_ID => {
                row_x + small_pitch * 2.0 + Self::PLAY_W + Self::TRANSPORT_GAP
            }
            NEXT_ID => {
                row_x
                    + small_pitch * 2.0
                    + Self::PLAY_W
                    + Self::TRANSPORT_GAP
                    + Self::TRANSPORT_SMALL_W
                    + Self::TRANSPORT_GAP
            }
            _ => row_x,
        };
        let (width, height) = Self::transport_button_size(id);

        FocusLayoutRect::new(
            x,
            Self::control_row_y(viewport_height)
                + (Self::CONTROL_ROW_H - height) / 2.0,
            width,
            height,
        )
    }

    fn command_button_width(id: &str) -> f32 {
        match id {
            SUBTITLE_ID => Self::SUBTITLE_W,
            AUDIO_ID => Self::AUDIO_W,
            FULLSCREEN_ID => Self::FULLSCREEN_W,
            EXIT_ID => Self::EXIT_W,
            _ => Self::AUDIO_W,
        }
    }

    fn command_row_width() -> f32 {
        Self::SUBTITLE_W
            + Self::AUDIO_W
            + Self::FULLSCREEN_W
            + Self::EXIT_W
            + Self::COMMAND_GAP * 3.0
    }

    fn command_row_x(viewport_width: f32) -> f32 {
        Self::PANEL_PAD_LEFT + Self::content_width(viewport_width)
            - Self::command_row_width()
    }

    fn command_button_rect(
        viewport_width: f32,
        viewport_height: f32,
        id: &str,
    ) -> FocusLayoutRect {
        let x = match id {
            SUBTITLE_ID => Self::command_row_x(viewport_width),
            AUDIO_ID => {
                Self::command_row_x(viewport_width)
                    + Self::SUBTITLE_W
                    + Self::COMMAND_GAP
            }
            FULLSCREEN_ID => {
                Self::command_row_x(viewport_width)
                    + Self::SUBTITLE_W
                    + Self::COMMAND_GAP
                    + Self::AUDIO_W
                    + Self::COMMAND_GAP
            }
            EXIT_ID => {
                Self::command_row_x(viewport_width)
                    + Self::SUBTITLE_W
                    + Self::COMMAND_GAP
                    + Self::AUDIO_W
                    + Self::COMMAND_GAP
                    + Self::FULLSCREEN_W
                    + Self::COMMAND_GAP
            }
            _ => Self::command_row_x(viewport_width),
        };
        let width = Self::command_button_width(id);

        FocusLayoutRect::new(
            x,
            Self::control_row_y(viewport_height)
                + (Self::CONTROL_ROW_H - Self::COMMAND_H) / 2.0,
            width,
            Self::COMMAND_H,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TenFootPlayerInputSnapshot {
    has_internal_video: bool,
    external_active: bool,
    overlay_visible: bool,
    viewport_width_bits: u32,
    viewport_height_bits: u32,
}

impl TenFootPlayerInputSnapshot {
    fn from_state(state: &State) -> Self {
        let player = &state.domains.player.state;
        let external_active = player.external_mpv_active;
        let has_internal_video = player.video_opt.is_some() && !external_active;
        let overlay_visible = overlay_controls_visible(player);

        Self {
            has_internal_video,
            external_active,
            overlay_visible,
            viewport_width_bits: state.window_size.width.to_bits(),
            viewport_height_bits: state.window_size.height.to_bits(),
        }
    }

    fn focusables(self) -> Vec<SpatialFocusable> {
        focusables_for_viewport(
            f32::from_bits(self.viewport_width_bits),
            f32::from_bits(self.viewport_height_bits),
            self.has_internal_video && self.overlay_visible,
        )
    }

    fn handle_event(&self, event: RuntimeEvent) -> Option<DomainMessage> {
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

        let action = spatial_action_for_player_key(key, modifiers)?;
        Some(self.handle_action(action))
    }

    fn handle_action(&self, action: SpatialAction) -> DomainMessage {
        if !self.has_internal_video {
            return match action {
                SpatialAction::Back if !self.external_active => {
                    player_message(PlayerMessage::NavigateBack)
                }
                SpatialAction::Search => {
                    DomainMessage::Ui(UiShellMessage::OpenSearchOverlay.into())
                }
                _ => DomainMessage::NoOp,
            };
        }

        let focusables = self.focusables();

        match action {
            SpatialAction::Move(direction) => {
                show_overlay_controls();
                move_focus(&focusables, direction);
                player_message(PlayerMessage::ShowControls)
            }
            SpatialAction::Activate => {
                show_overlay_controls();
                if !self.overlay_visible {
                    return player_message(PlayerMessage::ShowControls);
                }

                let focused = focused_id_for_focusables(&focusables);
                player_message(player_message_for_focus(focused.as_str()))
            }
            SpatialAction::Back => {
                if self.overlay_visible {
                    hide_overlay_controls();
                    player_message(PlayerMessage::ShowControls)
                } else {
                    player_message(PlayerMessage::NavigateBack)
                }
            }
            SpatialAction::Search => {
                show_overlay_controls();
                DomainMessage::Ui(UiShellMessage::OpenSearchOverlay.into())
            }
            SpatialAction::Menu => {
                show_overlay_controls();
                focus_id(SUBTITLE_ID, &focusables);
                player_message(PlayerMessage::ShowControls)
            }
        }
    }
}

fn player_message(message: PlayerMessage) -> DomainMessage {
    DomainMessage::Player(message)
}

fn spatial_action_for_player_key(
    key: Key,
    _modifiers: Modifiers,
) -> Option<SpatialAction> {
    let button = match key {
        Key::Named(Named::ArrowUp) => ControllerButton::DPadUp,
        Key::Named(Named::ArrowDown) => ControllerButton::DPadDown,
        Key::Named(Named::ArrowLeft) => ControllerButton::DPadLeft,
        Key::Named(Named::ArrowRight) => ControllerButton::DPadRight,
        Key::Named(Named::Enter) | Key::Named(Named::Space) => {
            ControllerButton::South
        }
        Key::Named(Named::Escape) | Key::Named(Named::Backspace) => {
            ControllerButton::East
        }
        Key::Character(value) if value == "/" => ControllerButton::Select,
        Key::Character(value) if value.eq_ignore_ascii_case("s") => {
            ControllerButton::Select
        }
        Key::Character(value) if value.eq_ignore_ascii_case("b") => {
            ControllerButton::East
        }
        Key::Character(value) if value.eq_ignore_ascii_case("m") => {
            ControllerButton::Start
        }
        _ => return None,
    };

    Some(
        ControllerInputMapper::new()
            .handle_event(ControllerEvent::ButtonPressed(button)),
    )
}

/// Player-view keyboard/controller subscription for 10-foot mode.
pub fn keyboard_subscription(state: &State) -> Subscription<DomainMessage> {
    if !state.interface_mode.is_tenfoot()
        || state.domains.search.state.presentation.is_open()
        || !matches!(
            state.domains.ui.state.view,
            crate::domains::ui::types::ViewState::Player
        )
    {
        return Subscription::none();
    }

    let snapshot = TenFootPlayerInputSnapshot::from_state(state);
    event::listen().with(snapshot).map(|(snapshot, event)| {
        snapshot.handle_event(event).unwrap_or(DomainMessage::NoOp)
    })
}

/// Build the 10-foot player view while preserving the existing video widget.
pub fn view_player(state: &State) -> Element<'_, PlayerMessage, Theme> {
    let player = &state.domains.player.state;

    if player.external_mpv_active {
        return external_player_view(player);
    }

    if let Some(video) = &player.video_opt {
        let video_surface: Element<
            '_,
            PlayerMessage,
            Theme,
            iced_wgpu::Renderer,
        > = mouse_area(
            video.widget(player.content_fit, Some(PlayerMessage::NewFrame)),
        )
        .on_press(PlayerMessage::VideoClicked)
        .into();

        let mut layers: Vec<
            Element<'_, PlayerMessage, Theme, iced_wgpu::Renderer>,
        > = vec![video_surface];

        if overlay_controls_visible(player) {
            layers.push(overlay(state));
        }

        if let Some(notification) = &player.track_notification {
            layers.push(notification_overlay(&notification.message));
        }

        let stacked: Element<'_, PlayerMessage, Theme, iced_wgpu::Renderer> =
            iced::widget::Stack::with_children(layers).into();

        let interactive = mouse_area(stacked)
            .on_move(PlayerMessage::MouseMoved)
            .on_release(PlayerMessage::SeekRelease);

        return container(interactive)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(player_container_style)
            .into();
    }

    if player.is_loading_video {
        return loading_player_view();
    }

    no_video_view()
}

/// 10-foot loading surface for the pre-player `ViewState::LoadingVideo` route.
pub fn view_loading_status(
    url: &str,
) -> Element<'static, PlayerMessage, Theme> {
    let target = if url.is_empty() {
        "Resolving playback stream".to_string()
    } else {
        "Preparing playback stream".to_string()
    };

    centered_status_view("Loading video…", target)
}

fn overlay(
    state: &State,
) -> Element<'_, PlayerMessage, Theme, iced_wgpu::Renderer> {
    let player = &state.domains.player.state;
    let focused_id = focused_id_for_viewport(
        state.window_size.width,
        state.window_size.height,
        true,
    );
    let title = player
        .current_media
        .as_ref()
        .map(|media| media.filename.clone())
        .unwrap_or_else(|| "Now playing".to_string());

    let status = player_status_label(player);
    let duration = player.source_duration.unwrap_or(player.last_valid_duration);
    let remaining = (duration - player.last_valid_position).max(0.0);
    let ratio = progress_ratio(player.last_valid_position, duration);
    let elapsed_time =
        crate::domains::player::view::format_time(player.last_valid_position);
    let remaining_time = if duration > 0.0 {
        format!("-{}", crate::domains::player::view::format_time(remaining))
    } else {
        "--:--".to_string()
    };

    let top_bar = container(
        row![
            column![
                text(status).size(14).color(MediaServerTheme::ACCENT),
                text(title).size(28).color(Color::WHITE),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            text("10-foot player")
                .size(18)
                .color(MediaServerTheme::TEXT_SECONDARY),
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(PlayerOverlayLayout::top_bar_padding())
    .style(top_gradient_style);

    let progress_layout = PlayerOverlayLayout::progress_rect(
        state.window_size.width,
        state.window_size.height,
    );
    let seek = container(
        mouse_area(progress_bar(ratio)).on_press(PlayerMessage::SeekBarPressed),
    )
    .width(Length::Fixed(progress_layout.width));

    let transport = row![
        transport_button(
            PREVIOUS_ID,
            Icon::SkipBack,
            PlayerMessage::PreviousEpisode,
            &focused_id,
            false,
        ),
        transport_button(
            REWIND_ID,
            Icon::Rewind,
            PlayerMessage::SeekRelative(SEEK_BACKWARD_COURSE),
            &focused_id,
            false,
        ),
        transport_button(
            PLAY_ID,
            if player.is_playing() {
                Icon::Pause
            } else {
                Icon::Play
            },
            PlayerMessage::PlayPause,
            &focused_id,
            true,
        ),
        transport_button(
            FORWARD_ID,
            Icon::FastForward,
            PlayerMessage::SeekRelative(SEEK_FORWARD_COURSE),
            &focused_id,
            false,
        ),
        transport_button(
            NEXT_ID,
            Icon::SkipForward,
            PlayerMessage::NextEpisode,
            &focused_id,
            false,
        ),
    ]
    .spacing(PlayerOverlayLayout::TRANSPORT_GAP)
    .align_y(Alignment::Center);

    let subtitle_label = if player.subtitles_enabled {
        "CC on"
    } else {
        "CC"
    };

    let command_row = row![
        command_button(
            SUBTITLE_ID,
            subtitle_label,
            PlayerMessage::CycleSubtitleSimple,
            &focused_id,
        ),
        command_button(
            AUDIO_ID,
            "Audio",
            PlayerMessage::CycleAudioTrack,
            &focused_id,
        ),
        command_button(
            FULLSCREEN_ID,
            if player.is_fullscreen {
                "Window"
            } else {
                "Full"
            },
            PlayerMessage::ToggleFullscreen,
            &focused_id,
        ),
        command_button(
            EXIT_ID,
            "Exit",
            PlayerMessage::NavigateBack,
            &focused_id,
        ),
    ]
    .spacing(PlayerOverlayLayout::COMMAND_GAP)
    .align_y(Alignment::Center);

    let bottom_panel = container(
        column![
            row![
                container(text(elapsed_time).size(24).color(Color::WHITE))
                    .width(Length::Fixed(PlayerOverlayLayout::TIME_LABEL_W))
                    .align_x(iced::alignment::Horizontal::Right),
                seek,
                container(text(remaining_time).size(24).color(Color::WHITE))
                    .width(Length::Fixed(PlayerOverlayLayout::TIME_LABEL_W))
                    .align_x(iced::alignment::Horizontal::Left),
            ]
            .spacing(PlayerOverlayLayout::PROGRESS_ROW_GAP)
            .align_y(Alignment::Center)
            .height(Length::Fixed(PlayerOverlayLayout::PROGRESS_ROW_H)),
            row![
                container(
                    column![
                        text(action_label(&focused_id))
                            .size(22)
                            .color(MediaServerTheme::ACCENT),
                        text("Esc hides • Enter selects • / or S searches")
                            .size(15)
                            .color(MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(2),
                )
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Left),
                container(transport).width(Length::Shrink),
                container(command_row)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right),
            ]
            .spacing(PlayerOverlayLayout::CONTROL_ROW_GAP)
            .align_y(Alignment::Center)
            .height(Length::Fixed(PlayerOverlayLayout::CONTROL_ROW_H)),
        ]
        .spacing(PlayerOverlayLayout::PANEL_COLUMN_GAP),
    )
    .width(Length::Fill)
    .height(Length::Fixed(PlayerOverlayLayout::PANEL_H))
    .padding(PlayerOverlayLayout::panel_padding())
    .style(panel_style);

    container(column![
        top_bar,
        Space::new().height(Length::Fill),
        bottom_panel
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn player_status_label(player: &PlayerDomainState) -> &'static str {
    if player.seeking {
        "SEEKING"
    } else if player.dragging {
        "SCRUBBING"
    } else if player.is_playing() {
        "PLAYING"
    } else {
        "PAUSED"
    }
}

fn action_label(focused_id: &str) -> &'static str {
    match focused_id {
        PLAY_ID => "Play / pause",
        REWIND_ID => "Seek back 15 seconds",
        FORWARD_ID => "Seek forward 30 seconds",
        PREVIOUS_ID => "Previous episode",
        NEXT_ID => "Next episode",
        SUBTITLE_ID => "Cycle subtitles",
        AUDIO_ID => "Cycle audio track",
        FULLSCREEN_ID => "Toggle fullscreen",
        EXIT_ID => "Exit player",
        _ => "Player controls",
    }
}

fn progress_bar(
    ratio: f64,
) -> Element<'static, PlayerMessage, Theme, iced_wgpu::Renderer> {
    let (played, remaining) = progress_portions(ratio);

    container(row![
        container(
            Space::new()
                .height(Length::Fixed(PlayerOverlayLayout::PROGRESS_VISUAL_H,))
        )
        .width(Length::FillPortion(played))
        .style(progress_style),
        container(
            Space::new()
                .height(Length::Fixed(PlayerOverlayLayout::PROGRESS_VISUAL_H,))
        )
        .width(Length::FillPortion(remaining))
        .style(progress_track_style),
    ])
    .width(Length::Fill)
    .height(Length::Fixed(PlayerOverlayLayout::PROGRESS_H))
    .center_y(Length::Fill)
    .into()
}

fn progress_ratio(position: f64, duration: f64) -> f64 {
    if duration <= 0.0 {
        0.0
    } else {
        (position / duration).clamp(0.0, 1.0)
    }
}

fn progress_portions(ratio: f64) -> (u16, u16) {
    let played = (progress_ratio(ratio, 1.0) * 1000.0).round() as u16;
    (played.max(1), 1000u16.saturating_sub(played).max(1))
}

fn transport_button(
    id: &'static str,
    icon: Icon,
    message: PlayerMessage,
    focused_id: &str,
    prominent: bool,
) -> Element<'static, PlayerMessage, Theme, iced_wgpu::Renderer> {
    let focused = focused_id == id;
    let size = if prominent { 34 } else { 27 };
    let (width, height) = PlayerOverlayLayout::transport_button_size(id);

    let glyph = text(icon.unicode())
        .font(lucide_font())
        .size(size)
        .line_height(1.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .center();

    button(glyph)
        .on_press(message)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .padding(0)
        .style(move |theme, status| {
            overlay_button_style(theme, status, focused, prominent)
        })
        .into()
}

fn command_button(
    id: &'static str,
    label: impl Into<String>,
    message: PlayerMessage,
    focused_id: &str,
) -> Element<'static, PlayerMessage, Theme, iced_wgpu::Renderer> {
    let focused = focused_id == id;
    let width = PlayerOverlayLayout::command_button_width(id);

    let label = text(label.into())
        .size(18)
        .line_height(1.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .center();

    button(label)
        .on_press(message)
        .width(Length::Fixed(width))
        .height(Length::Fixed(PlayerOverlayLayout::COMMAND_H))
        .padding(0)
        .style(move |theme, status| {
            overlay_button_style(theme, status, focused, false)
        })
        .into()
}

fn notification_overlay<'a>(
    message: &'a str,
) -> Element<'a, PlayerMessage, Theme, iced_wgpu::Renderer> {
    container(
        container(text(message).size(28).color(Color::WHITE))
            .padding([18, 28])
            .style(notification_style),
    )
    .width(Length::Fill)
    .padding(Padding {
        top: 160.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    })
    .align_x(iced::alignment::Horizontal::Center)
    .into()
}

fn external_player_view(
    player: &PlayerDomainState,
) -> Element<'static, PlayerMessage, Theme> {
    let position =
        crate::domains::player::view::format_time(player.last_valid_position);
    let duration =
        crate::domains::player::view::format_time(player.last_valid_duration);
    centered_status_view(
        "Playing externally",
        format!("{position} / {duration} • MPV is handling playback"),
    )
}

fn loading_player_view() -> Element<'static, PlayerMessage, Theme> {
    centered_status_view("Loading video…", "Preparing couch playback")
}

fn no_video_view() -> Element<'static, PlayerMessage, Theme> {
    centered_status_view(
        "No video loaded",
        "Choose something to play from 10-foot home",
    )
}

fn centered_status_view(
    title: impl Into<String>,
    subtitle: impl Into<String>,
) -> Element<'static, PlayerMessage, Theme> {
    container(
        column![
            text(title.into()).size(44).color(Color::WHITE),
            text(subtitle.into())
                .size(26)
                .color(MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(16)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(player_container_style)
    .into()
}

fn overlay_controls_visible(player: &PlayerDomainState) -> bool {
    let has_internal_video =
        player.video_opt.is_some() && !player.external_mpv_active;
    if !has_internal_video {
        return false;
    }

    let mut runtime = overlay_runtime();
    if !player.controls {
        runtime.controls_hidden = false;
        return false;
    }

    !runtime.controls_hidden
}

fn hide_overlay_controls() {
    overlay_runtime().controls_hidden = true;
}

fn show_overlay_controls() {
    overlay_runtime().controls_hidden = false;
}

fn play_focus_id() -> SpatialFocusId {
    SpatialFocusId::from(PLAY_ID)
}

fn focusables_for_viewport(
    viewport_width: f32,
    viewport_height: f32,
    visible: bool,
) -> Vec<SpatialFocusable> {
    let mut builder: SpatialFocusBuilder = SpatialFocusBuilder::new();

    builder
        .push_layout_if(
            PLAY_ID,
            PlayerOverlayLayout::focus_layout(
                PlayerOverlayLayout::transport_button_rect(
                    viewport_width,
                    viewport_height,
                    PLAY_ID,
                ),
            ),
            visible,
            true,
        )
        .push_layout_if(
            PREVIOUS_ID,
            PlayerOverlayLayout::focus_layout(
                PlayerOverlayLayout::transport_button_rect(
                    viewport_width,
                    viewport_height,
                    PREVIOUS_ID,
                ),
            ),
            visible,
            true,
        )
        .push_layout_if(
            REWIND_ID,
            PlayerOverlayLayout::focus_layout(
                PlayerOverlayLayout::transport_button_rect(
                    viewport_width,
                    viewport_height,
                    REWIND_ID,
                ),
            ),
            visible,
            true,
        )
        .push_layout_if(
            FORWARD_ID,
            PlayerOverlayLayout::focus_layout(
                PlayerOverlayLayout::transport_button_rect(
                    viewport_width,
                    viewport_height,
                    FORWARD_ID,
                ),
            ),
            visible,
            true,
        )
        .push_layout_if(
            NEXT_ID,
            PlayerOverlayLayout::focus_layout(
                PlayerOverlayLayout::transport_button_rect(
                    viewport_width,
                    viewport_height,
                    NEXT_ID,
                ),
            ),
            visible,
            true,
        )
        .push_layout_if(
            SUBTITLE_ID,
            PlayerOverlayLayout::focus_layout(
                PlayerOverlayLayout::command_button_rect(
                    viewport_width,
                    viewport_height,
                    SUBTITLE_ID,
                ),
            ),
            visible,
            true,
        )
        .push_layout_if(
            AUDIO_ID,
            PlayerOverlayLayout::focus_layout(
                PlayerOverlayLayout::command_button_rect(
                    viewport_width,
                    viewport_height,
                    AUDIO_ID,
                ),
            ),
            visible,
            true,
        )
        .push_layout_if(
            FULLSCREEN_ID,
            PlayerOverlayLayout::focus_layout(
                PlayerOverlayLayout::command_button_rect(
                    viewport_width,
                    viewport_height,
                    FULLSCREEN_ID,
                ),
            ),
            visible,
            true,
        )
        .push_layout_if(
            EXIT_ID,
            PlayerOverlayLayout::focus_layout(
                PlayerOverlayLayout::command_button_rect(
                    viewport_width,
                    viewport_height,
                    EXIT_ID,
                ),
            ),
            visible,
            true,
        );

    builder.build()
}

fn focused_id_for_viewport(
    viewport_width: f32,
    viewport_height: f32,
    visible: bool,
) -> String {
    let focusables =
        focusables_for_viewport(viewport_width, viewport_height, visible);
    focused_id_for_focusables(&focusables).as_str().to_string()
}

fn focused_id_for_focusables(
    focusables: &[SpatialFocusable],
) -> SpatialFocusId {
    let mut runtime = overlay_runtime();
    resolve_runtime_focus(&mut runtime, focusables)
}

fn focus_id(
    id: &'static str,
    focusables: &[SpatialFocusable],
) -> SpatialFocusId {
    let mut runtime = overlay_runtime();
    let mut focus_state = spatial_state_from_runtime(&runtime, focusables);
    let target = SpatialFocusId::from(id);
    if focus_state.focus(target.clone()) {
        runtime.focused = Some(target.clone());
        target
    } else {
        resolve_runtime_focus(&mut runtime, focusables)
    }
}

fn move_focus(
    focusables: &[SpatialFocusable],
    direction: SpatialDirection,
) -> SpatialFocusId {
    let mut runtime = overlay_runtime();
    let mut focus_state = spatial_state_from_runtime(&runtime, focusables);
    let focused = focus_state
        .move_focus(direction)
        .cloned()
        .unwrap_or_else(play_focus_id);
    runtime.focused = Some(focused.clone());
    focused
}

fn resolve_runtime_focus(
    runtime: &mut OverlayRuntime,
    focusables: &[SpatialFocusable],
) -> SpatialFocusId {
    if focusables.is_empty() {
        return runtime.focused.clone().unwrap_or_else(play_focus_id);
    }

    if let Some(focused) = runtime.focused.as_ref()
        && focusables
            .iter()
            .any(|candidate| candidate.enabled && &candidate.id == focused)
    {
        return focused.clone();
    }

    let fallback = focusables
        .iter()
        .find(|candidate| candidate.enabled)
        .map(|candidate| candidate.id.clone())
        .unwrap_or_else(play_focus_id);
    runtime.focused = Some(fallback.clone());
    fallback
}

fn spatial_state_from_runtime(
    runtime: &OverlayRuntime,
    focusables: &[SpatialFocusable],
) -> SpatialFocusState {
    let mut state = SpatialFocusState::default();
    state.set_focusables(focusables.to_vec());
    if let Some(focused) = runtime.focused.as_ref() {
        state.focus(focused.clone());
    }
    state
}

fn player_message_for_focus(focused_id: &str) -> PlayerMessage {
    match focused_id {
        PLAY_ID => PlayerMessage::PlayPause,
        REWIND_ID => PlayerMessage::SeekRelative(SEEK_BACKWARD_COURSE),
        FORWARD_ID => PlayerMessage::SeekRelative(SEEK_FORWARD_COURSE),
        PREVIOUS_ID => PlayerMessage::PreviousEpisode,
        NEXT_ID => PlayerMessage::NextEpisode,
        SUBTITLE_ID => PlayerMessage::CycleSubtitleSimple,
        AUDIO_ID => PlayerMessage::CycleAudioTrack,
        FULLSCREEN_ID => PlayerMessage::ToggleFullscreen,
        EXIT_ID => PlayerMessage::NavigateBack,
        _ => PlayerMessage::ShowControls,
    }
}

fn player_container_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(Color::BLACK)),
        ..Default::default()
    }
}

fn top_gradient_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Gradient(
            iced::gradient::Linear::new(iced::Radians(
                std::f32::consts::PI / 2.0,
            ))
            .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.82))
            .add_stop(1.0, Color::from_rgba(0.0, 0.0, 0.0, 0.0))
            .into(),
        )),
        ..Default::default()
    }
}

fn panel_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Gradient(
            iced::gradient::Linear::new(iced::Radians(
                3.0 * std::f32::consts::PI / 2.0,
            ))
            .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.90))
            .add_stop(1.0, Color::from_rgba(0.0, 0.0, 0.0, 0.48))
            .into(),
        )),
        border: Border::default(),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            offset: Vector::new(0.0, -4.0),
            blur_radius: 18.0,
        },
        ..Default::default()
    }
}

fn notification_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(Color::from_rgba(
            0.0, 0.0, 0.0, 0.82,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.18),
            width: 1.0,
            radius: 16.0.into(),
        },
        ..Default::default()
    }
}

fn progress_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(MediaServerTheme::ACCENT)),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn progress_track_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            1.0, 1.0, 1.0, 0.20,
        ))),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn overlay_button_style(
    _theme: &Theme,
    status: button::Status,
    focused: bool,
    prominent: bool,
) -> button::Style {
    let hovered =
        matches!(status, button::Status::Hovered | button::Status::Pressed);
    let accent = MediaServerTheme::ACCENT;
    let background = if focused {
        Color::from_rgba(0.55, 0.12, 0.72, 0.20)
    } else if prominent {
        Color::from_rgba(1.0, 1.0, 1.0, 0.09)
    } else if hovered {
        Color::from_rgba(1.0, 1.0, 1.0, 0.08)
    } else {
        Color::TRANSPARENT
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: if focused { accent } else { Color::WHITE },
        border: Border {
            color: if focused {
                accent
            } else if prominent {
                Color::from_rgba(1.0, 1.0, 1.0, 0.18)
            } else {
                Color::TRANSPARENT
            },
            width: if focused || prominent { 1.5 } else { 0.0 },
            radius: if prominent { 35.0 } else { 8.0 }.into(),
        },
        shadow: if focused {
            Shadow {
                color: MediaServerTheme::ACCENT_GLOW,
                offset: Vector::new(0.0, 0.0),
                blur_radius: 18.0,
            }
        } else {
            Shadow::default()
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_ratio_clamps_to_playable_range() {
        assert_eq!(progress_ratio(30.0, 120.0), 0.25);
        assert_eq!(progress_ratio(-5.0, 120.0), 0.0);
        assert_eq!(progress_ratio(180.0, 120.0), 1.0);
        assert_eq!(progress_ratio(30.0, 0.0), 0.0);
    }

    #[test]
    fn progress_portions_remain_non_zero_for_layout() {
        assert_eq!(progress_portions(0.0), (1, 1000));
        assert_eq!(progress_portions(0.5), (500, 500));
        assert_eq!(progress_portions(1.0), (1000, 1));
    }

    #[test]
    fn overlay_focusables_filter_hidden_controls() {
        let focusables = focusables_for_viewport(1280.0, 800.0, false);

        assert!(focusables.is_empty());
    }

    #[test]
    fn overlay_focusables_keep_play_default_and_layout_ordered_rects() {
        let focusables = focusables_for_viewport(1920.0, 1080.0, true);
        let ids: Vec<&str> = focusables
            .iter()
            .map(|focusable| focusable.id.as_str())
            .collect();

        assert_eq!(
            ids,
            vec![
                PLAY_ID,
                PREVIOUS_ID,
                REWIND_ID,
                FORWARD_ID,
                NEXT_ID,
                SUBTITLE_ID,
                AUDIO_ID,
                FULLSCREEN_ID,
                EXIT_ID,
            ]
        );

        let play = focusables
            .iter()
            .find(|focusable| focusable.id.as_str() == PLAY_ID)
            .expect("play focusable");
        let transport_x = PlayerOverlayLayout::transport_row_x(1920.0);
        assert_eq!(
            transport_x,
            (1920.0 - PlayerOverlayLayout::transport_row_width()) / 2.0
        );
        assert_eq!(
            play.rect,
            PlayerOverlayLayout::focus_layout(
                PlayerOverlayLayout::transport_button_rect(
                    1920.0, 1080.0, PLAY_ID,
                ),
            )
            .into_focus_rect()
        );
        assert_eq!(
            play.rect.x,
            transport_x
                + (PlayerOverlayLayout::TRANSPORT_SMALL_W
                    + PlayerOverlayLayout::TRANSPORT_GAP)
                    * 2.0
                - PlayerOverlayLayout::FOCUS_MARGIN_X
        );
        assert_eq!(
            play.rect.y,
            PlayerOverlayLayout::control_row_y(1080.0)
                - PlayerOverlayLayout::FOCUS_MARGIN_Y
        );
    }

    #[test]
    fn overlay_command_focusables_align_to_right_edge() {
        let focusables = focusables_for_viewport(1920.0, 1080.0, true);
        let subtitle = focusables
            .iter()
            .find(|focusable| focusable.id.as_str() == SUBTITLE_ID)
            .expect("subtitle focusable");
        let fullscreen = focusables
            .iter()
            .find(|focusable| focusable.id.as_str() == FULLSCREEN_ID)
            .expect("fullscreen focusable");
        let exit = focusables
            .iter()
            .find(|focusable| focusable.id.as_str() == EXIT_ID)
            .expect("exit focusable");

        assert_eq!(
            subtitle.rect.x,
            1920.0
                - PlayerOverlayLayout::PANEL_PAD_RIGHT
                - PlayerOverlayLayout::command_row_width()
                - PlayerOverlayLayout::FOCUS_MARGIN_X
        );
        assert_eq!(
            fullscreen.rect.x,
            subtitle.rect.x
                + PlayerOverlayLayout::SUBTITLE_W
                + PlayerOverlayLayout::COMMAND_GAP
                + PlayerOverlayLayout::AUDIO_W
                + PlayerOverlayLayout::COMMAND_GAP
        );
        assert_eq!(
            exit.rect.x + exit.rect.width,
            1920.0 - PlayerOverlayLayout::PANEL_PAD_RIGHT
                + PlayerOverlayLayout::FOCUS_MARGIN_X
        );
    }

    #[test]
    fn overlay_progress_rect_uses_rendered_time_label_slots() {
        let rect = PlayerOverlayLayout::progress_rect(1280.0, 800.0);

        assert_eq!(
            rect.x,
            PlayerOverlayLayout::PANEL_PAD_LEFT
                + PlayerOverlayLayout::TIME_LABEL_W
                + PlayerOverlayLayout::PROGRESS_ROW_GAP
        );
        assert_eq!(
            rect.width,
            1280.0
                - PlayerOverlayLayout::PANEL_PAD_LEFT
                - PlayerOverlayLayout::PANEL_PAD_RIGHT
                - PlayerOverlayLayout::TIME_LABEL_W * 2.0
                - PlayerOverlayLayout::PROGRESS_ROW_GAP * 2.0
        );
        assert_eq!(
            rect.y,
            PlayerOverlayLayout::panel_y(800.0)
                + PlayerOverlayLayout::PANEL_PAD_TOP
                + (PlayerOverlayLayout::PROGRESS_ROW_H
                    - PlayerOverlayLayout::PROGRESS_H)
                    / 2.0
        );
    }

    #[test]
    fn overlay_focus_rects_stay_inside_800p_viewport() {
        let focusables = focusables_for_viewport(1280.0, 800.0, true);

        for focusable in focusables {
            assert!(focusable.rect.x >= 0.0, "{focusable:?}");
            assert!(focusable.rect.y >= 0.0, "{focusable:?}");
            assert!(
                focusable.rect.x + focusable.rect.width <= 1280.0,
                "{focusable:?}"
            );
            assert!(
                focusable.rect.y + focusable.rect.height <= 800.0,
                "{focusable:?}"
            );
        }
    }

    #[test]
    fn overlay_spatial_focus_moves_using_layout_rectangles() {
        let focusables = focusables_for_viewport(1920.0, 1080.0, true);
        let mut state = SpatialFocusState::default();
        state.set_focusables(focusables);
        assert!(state.focus(PLAY_ID));

        assert_eq!(
            state
                .move_focus(SpatialDirection::Right)
                .map(SpatialFocusId::as_str),
            Some(FORWARD_ID)
        );
        assert_eq!(
            state
                .move_focus(SpatialDirection::Right)
                .map(SpatialFocusId::as_str),
            Some(NEXT_ID)
        );
        assert_eq!(
            state
                .move_focus(SpatialDirection::Right)
                .map(SpatialFocusId::as_str),
            Some(SUBTITLE_ID)
        );
    }

    #[test]
    fn overlay_focus_ids_map_to_existing_player_messages() {
        assert!(matches!(
            player_message_for_focus(PLAY_ID),
            PlayerMessage::PlayPause
        ));
        assert!(matches!(
            player_message_for_focus(REWIND_ID),
            PlayerMessage::SeekRelative(SEEK_BACKWARD_COURSE)
        ));
        assert!(matches!(
            player_message_for_focus(AUDIO_ID),
            PlayerMessage::CycleAudioTrack
        ));
        assert!(matches!(
            player_message_for_focus(EXIT_ID),
            PlayerMessage::NavigateBack
        ));
    }
}
