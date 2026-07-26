//! Playback overlay views for the optional UI feature.
//!
//! Views here render the playback surface from `PlayerDomainState` without
//! owning app-shell state transitions.

use super::messages::PlayerMessage;
use super::state::{PlayerDomainState, TrackNotification};
use super::theme;
use crate::contract::{
    BackendKind, PlaybackSnapshot, PlaybackState, PresentationMode,
    PresenterState,
};
use iced::Theme;
use iced::{
    Element, Length, Padding,
    widget::{Space, column, container, mouse_area, row, text},
};

/// Backend-neutral status plate rendered over a playback surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackSurfaceStatus {
    Loading,
    Buffering { percentage: Option<u8> },
    Seeking,
    Stopping,
    AwaitingHost,
    AwaitingVideoOutput,
    PresenterFailed(String),
    PlaybackFailed(String),
}

impl PlaybackSurfaceStatus {
    /// Derive transient presentation UI exclusively from the reduced playback
    /// snapshot. Backend adapters never participate in rendering decisions.
    pub fn from_snapshot(snapshot: &PlaybackSnapshot) -> Option<Self> {
        if snapshot.presenter == PresenterState::Failed {
            return Some(Self::PresenterFailed(snapshot_error_message(
                snapshot,
                "The native video surface could not be attached.",
            )));
        }

        match snapshot.state {
            PlaybackState::Loading => Some(Self::Loading),
            PlaybackState::Buffering => Some(Self::Buffering {
                percentage: snapshot
                    .buffer
                    .percentage
                    .filter(|percentage| percentage.is_finite())
                    .map(|percentage| {
                        (percentage.clamp(0.0, 1.0) * 100.0).round() as u8
                    }),
            }),
            PlaybackState::Seeking => Some(Self::Seeking),
            PlaybackState::Stopping => Some(Self::Stopping),
            PlaybackState::Failed => Some(Self::PlaybackFailed(
                snapshot_error_message(snapshot, "Playback failed."),
            )),
            PlaybackState::Idle
            | PlaybackState::Playing
            | PlaybackState::Paused
            | PlaybackState::Ended
            | PlaybackState::Terminated => {
                if snapshot.target.presentation
                    != PresentationMode::IntegratedNative
                {
                    return None;
                }

                match snapshot.presenter {
                    PresenterState::AwaitingHost => Some(Self::AwaitingHost),
                    PresenterState::AwaitingVideoOutput => {
                        Some(Self::AwaitingVideoOutput)
                    }
                    PresenterState::Detached
                    | PresenterState::Attached
                    | PresenterState::Hidden
                    | PresenterState::Suspended
                    | PresenterState::Failed => None,
                }
            }
        }
    }

    pub const fn title(&self) -> &'static str {
        match self {
            Self::Loading => "Loading video…",
            Self::Buffering { .. } => "Buffering…",
            Self::Seeking => "Seeking…",
            Self::Stopping => "Stopping playback…",
            Self::AwaitingHost => "Preparing video surface…",
            Self::AwaitingVideoOutput => "Waiting for video output…",
            Self::PresenterFailed(_) => "Video presentation failed",
            Self::PlaybackFailed(_) => "Playback failed",
        }
    }

    pub const fn symbol(&self) -> &'static str {
        match self {
            Self::Loading | Self::Buffering { .. } => "◌",
            Self::Seeking => "↦",
            Self::Stopping => "■",
            Self::AwaitingHost | Self::AwaitingVideoOutput => "▣",
            Self::PresenterFailed(_) | Self::PlaybackFailed(_) => "!",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Buffering {
                percentage: Some(percentage),
            } => Some(format!("{percentage}% buffered")),
            Self::PresenterFailed(message) | Self::PlaybackFailed(message) => {
                Some(message.clone())
            }
            Self::Loading
            | Self::Buffering { percentage: None }
            | Self::Seeking
            | Self::Stopping
            | Self::AwaitingHost
            | Self::AwaitingVideoOutput => None,
        }
    }
}

fn snapshot_error_message(
    snapshot: &PlaybackSnapshot,
    fallback: &'static str,
) -> String {
    snapshot
        .last_error
        .as_ref()
        .map(|error| error.message.trim())
        .filter(|message| !message.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// Status for the active surface. In-process and retained external playback
/// both expose the same reduced snapshot; pre-session URL loading is the only
/// compatibility state read here.
pub fn playback_surface_status(
    state: &PlayerDomainState,
) -> Option<PlaybackSurfaceStatus> {
    if let Some(snapshot) = state.playback_snapshot() {
        PlaybackSurfaceStatus::from_snapshot(snapshot)
    } else if state.is_loading_video || state.is_resolving_stream_url {
        Some(PlaybackSurfaceStatus::Loading)
    } else {
        None
    }
}

fn native_root_drag_policy(
    target: Option<crate::contract::PlaybackTarget>,
    has_native_host: bool,
    is_macos: bool,
) -> bool {
    is_macos
        && has_native_host
        && target == Some(crate::contract::PlaybackTarget::MPV_INTEGRATED)
}

/// Whether central player-surface presses should be handed to AppKit as native
/// root-window drags instead of using the ordinary video-click action.
///
/// The native host check keeps pre-host and fallback rendering on the canonical
/// click path. The platform condition is compile-time so Windows and Linux
/// retain their existing behavior.
pub fn native_root_drag_surface_enabled(
    snapshot: Option<&PlaybackSnapshot>,
    native_host_window: Option<iced::window::Id>,
) -> bool {
    native_root_drag_policy(
        snapshot.map(|snapshot| snapshot.target),
        native_host_window.is_some(),
        cfg!(target_os = "macos"),
    )
}

/// Select the canonical action for a central video/background press.
pub fn central_surface_press_message(
    snapshot: Option<&PlaybackSnapshot>,
    native_host_window: Option<iced::window::Id>,
) -> PlayerMessage {
    if native_root_drag_surface_enabled(snapshot, native_host_window) {
        PlayerMessage::BeginNativeRootDrag
    } else {
        PlayerMessage::VideoClicked
    }
}

fn shield_native_root_drag<'a>(
    surface: iced::Element<'a, PlayerMessage, Theme, iced_wgpu::Renderer>,
    enabled: bool,
) -> iced::Element<'a, PlayerMessage, Theme, iced_wgpu::Renderer> {
    if enabled {
        mouse_area(surface)
            .on_press(PlayerMessage::ShowControls)
            .into()
    } else {
        surface
    }
}

/// Render a static status plate without requesting a frame-rate redraw loop.
pub fn playback_status_overlay(
    status: PlaybackSurfaceStatus,
) -> Element<'static, PlayerMessage, Theme, iced_wgpu::Renderer> {
    let mut content = column![
        text(status.symbol())
            .size(44)
            .color(iced::Color::from_rgb(0.85, 0.85, 0.85)),
        text(status.title()).size(20).color(iced::Color::WHITE),
    ]
    .align_x(iced::Alignment::Center)
    .spacing(10);

    if let Some(detail) = status.detail() {
        content = content.push(
            text(detail)
                .size(14)
                .color(iced::Color::from_rgb(0.72, 0.72, 0.72)),
        );
    }

    container(
        container(content)
            .padding([18, 26])
            .style(theme::container_playback_status),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl PlayerDomainState {
    /// Build the main player view
    /// Note: Returns wgpu renderer elements since waylandsink video playback requires GPU acceleration
    pub fn view(
        &self,
        native_host_window: Option<iced::window::Id>,
    ) -> iced::Element<'_, PlayerMessage, Theme> {
        let playback = self.playback_snapshot();
        log::trace!(
            "PlayerState::view() called - position: {:.2}s, duration: {:?}, target: {:?}, controls: {}",
            playback
                .map(|snapshot| snapshot.position.as_secs_f64())
                .unwrap_or_default(),
            playback.and_then(|snapshot| snapshot.duration),
            playback.map(|snapshot| snapshot.target),
            self.controls
        );

        // The retained process path also projects into PlaybackSnapshot. The
        // view chooses presentation from the neutral target and never reads
        // process liveness or IPC fields directly.
        if let Some(snapshot) = playback.filter(|snapshot| {
            snapshot.target.backend == BackendKind::ExternalMpv
        }) {
            let duration = snapshot
                .duration
                .map(|duration| duration.as_secs_f64())
                .unwrap_or_default();
            return container(
                column![
                    text("Playing Externally").size(24),
                    text(format!(
                        "Position: {:.0}s / {:.0}s",
                        snapshot.position.as_secs_f64(),
                        duration
                    ))
                    .size(18),
                    Space::new().height(Length::Fixed(20.0)),
                    text("External player (MPV) is handling playback").size(14),
                    text("The app will restore when playback ends").size(14),
                ]
                .align_x(iced::Alignment::Center)
                .spacing(10),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .into();
        }

        let native_root_drag =
            native_root_drag_surface_enabled(playback, native_host_window);
        let central_press =
            central_surface_press_message(playback, native_host_window);
        if let Some(video) = self.playback_widget(native_host_window) {
            let clickable_video = self.video_view(video, central_press);

            // Overlay stack: video, snapshot-derived transient status, then
            // controls. The status plate is static and never drives redraws.
            let mut children: Vec<
                iced::Element<PlayerMessage, Theme, iced_wgpu::Renderer>,
            > = vec![clickable_video];

            if let Some(status) = playback_surface_status(self) {
                children.push(playback_status_overlay(status));
            }
            if self.controls {
                children.push(self.controls_overlay(native_root_drag));
            }

            let player_with_overlay: iced::Element<PlayerMessage, Theme> =
                iced::widget::Stack::with_children(children).into();

            let player_with_settings: iced::Element<PlayerMessage, Theme> =
                if self.show_settings {
                    let settings = shield_native_root_drag(
                        self.settings_panel(),
                        native_root_drag,
                    );
                    let positioned_settings = container(row![
                        Space::new().width(Length::Fill),
                        container(settings)
                            .style(theme::container_settings_panel_wrapper),
                        Space::new().width(Length::Fixed(80.0)), // Offset from right edge
                    ])
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_y(iced::alignment::Vertical::Bottom)
                    .padding(Padding {
                        top: 0.0,
                        right: 0.0,
                        bottom: 100.0,
                        left: 0.0,
                    }); // Position above controls

                    iced::widget::Stack::with_children(vec![
                        player_with_overlay,
                        positioned_settings.into(),
                    ])
                    .into()
                } else {
                    player_with_overlay
                };

            let player_with_menus: iced::Element<
                PlayerMessage,
                Theme,
                iced_wgpu::Renderer,
            > = if self.show_quality_menu {
                let quality_menu = self.quality_menu_overlay(native_root_drag);
                iced::widget::Stack::with_children(vec![
                    player_with_settings,
                    quality_menu,
                ])
                .into()
            } else if self.show_subtitle_menu {
                let subtitle_menu =
                    self.subtitle_menu_overlay(native_root_drag);
                iced::widget::Stack::with_children(vec![
                    player_with_settings,
                    subtitle_menu,
                ])
                .into()
            } else {
                player_with_settings
            };

            let player_with_notification: iced::Element<
                PlayerMessage,
                Theme,
                iced_wgpu::Renderer,
            > = if let Some(notification) = &self.track_notification {
                let notification_overlay =
                    self.notification_overlay(notification);
                iced::widget::Stack::with_children(vec![
                    player_with_menus,
                    notification_overlay,
                ])
                .into()
            } else {
                player_with_menus
            };

            // Wrap with mouse movement detection and release handling for seek bar
            let interactive = mouse_area(player_with_notification)
                .on_move(PlayerMessage::MouseMoved)
                .on_release(PlayerMessage::SeekRelease);

            container(interactive)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(status) = playback_surface_status(self) {
            playback_status_overlay(status)
        } else {
            container(
                column![
                    text("No video loaded")
                        .size(24)
                        .color(iced::Color::from_rgb(0.7, 0.7, 0.7)),
                ]
                .align_x(iced::Alignment::Center)
                .spacing(10),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(theme::container_player)
            .into()
        }
    }

    /// Build the video player view
    fn video_view<'a>(
        &self,
        player: Element<'a, PlayerMessage>,
        press: PlayerMessage,
    ) -> Element<'a, PlayerMessage> {
        // Presentation redraw cadence is backend-owned. Player state arrives
        // through copied event signals or the bounded legacy snapshot timer,
        // never through a decoded-frame callback.
        iced::widget::mouse_area(player).on_press(press).into()
    }

    /// Build the controls overlay
    fn controls_overlay(
        &self,
        shield_native_drag: bool,
    ) -> iced::Element<'_, PlayerMessage, Theme> {
        // Delegate to controls.rs for the full implementation
        self.build_controls(shield_native_drag)
    }

    /// Build the track notification overlay
    fn notification_overlay<'a>(
        &self,
        notification: &'a TrackNotification,
    ) -> iced::Element<'a, PlayerMessage, Theme> {
        container(
            container(
                text(&notification.message)
                    .size(18)
                    .color([1.0, 1.0, 1.0, 0.9]),
            )
            .padding(15)
            .style(theme::container_notification),
        )
        .width(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .padding(50)
        .into()
    }

    /// Build the settings panel
    fn settings_panel(
        &self,
    ) -> iced::Element<'_, PlayerMessage, Theme, iced_wgpu::Renderer> {
        // Delegate to controls.rs for the full implementation
        self.build_settings_panel()
    }

    /// Build the quality menu overlay
    fn quality_menu_overlay(
        &self,
        shield_native_drag: bool,
    ) -> iced::Element<'_, PlayerMessage, Theme, iced_wgpu::Renderer> {
        // Position the menu near the quality button (bottom right)
        container(row![
            Space::new().width(Length::Fill),
            container(shield_native_root_drag(
                self.build_quality_menu(),
                shield_native_drag,
            ))
            .style(theme::container_subtitle_menu_wrapper),
            Space::new().width(Length::Fixed(200.0)), // Offset from right edge
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 100.0,
            left: 0.0,
        }) // Position above controls
        .into()
    }

    fn subtitle_menu_overlay(
        &self,
        shield_native_drag: bool,
    ) -> iced::Element<'_, PlayerMessage, Theme, iced_wgpu::Renderer> {
        // Position the menu near the subtitle button (bottom right)
        container(row![
            Space::new().width(Length::Fill),
            container(shield_native_root_drag(
                self.build_subtitle_menu(),
                shield_native_drag,
            ))
            .style(theme::container_subtitle_menu_wrapper),
            Space::new().width(Length::Fixed(100.0)), // Offset from right edge
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 100.0,
            left: 0.0,
        }) // Position above controls
        .into()
    }

    /// Build a minimal player view for embedding (e.g., in library view)
    pub fn minimal_view(&self) -> Option<Element<'_, PlayerMessage>> {
        self.playback_widget(None).map(|player| {
            container(player)
                .width(Length::Fill)
                .style(theme::container_player)
                .into()
        })
    }
}

/// Helper functions for formatting time
pub fn format_time(seconds: f64) -> String {
    let total_seconds = seconds as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let secs = total_seconds % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{:02}:{:02}", minutes, secs)
    }
}

/// Calculate seek position from slider interaction
pub fn calculate_seek_position(x: f32, width: f32, duration: f64) -> f64 {
    let normalized = (x / width).clamp(0.0, 1.0);
    normalized as f64 * duration
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{
        BufferState, PlaybackCapabilities, PlaybackError, PlaybackErrorKind,
        PlaybackTarget, SessionGeneration,
    };

    fn snapshot(target: PlaybackTarget) -> PlaybackSnapshot {
        PlaybackSnapshot::new(
            SessionGeneration::INITIAL,
            target,
            PlaybackCapabilities::default(),
        )
    }

    #[test]
    fn transient_surface_status_is_derived_from_snapshot_state() {
        let mut snapshot = snapshot(PlaybackTarget::MPV_NATIVE_WINDOW);

        snapshot.state = PlaybackState::Loading;
        assert_eq!(
            PlaybackSurfaceStatus::from_snapshot(&snapshot),
            Some(PlaybackSurfaceStatus::Loading)
        );

        snapshot.state = PlaybackState::Buffering;
        snapshot.buffer = BufferState {
            buffering: true,
            percentage: Some(0.426),
            ..BufferState::default()
        };
        let status = PlaybackSurfaceStatus::from_snapshot(&snapshot)
            .expect("buffering has a status plate");
        assert_eq!(
            status,
            PlaybackSurfaceStatus::Buffering {
                percentage: Some(43)
            }
        );
        assert_eq!(status.detail().as_deref(), Some("43% buffered"));

        snapshot.state = PlaybackState::Seeking;
        assert_eq!(
            PlaybackSurfaceStatus::from_snapshot(&snapshot),
            Some(PlaybackSurfaceStatus::Seeking)
        );

        snapshot.state = PlaybackState::Playing;
        assert_eq!(PlaybackSurfaceStatus::from_snapshot(&snapshot), None);
    }

    #[test]
    fn integrated_presenter_readiness_has_an_explicit_status() {
        let mut snapshot = snapshot(PlaybackTarget::MPV_INTEGRATED);
        snapshot.state = PlaybackState::Playing;

        snapshot.presenter = PresenterState::AwaitingHost;
        assert_eq!(
            PlaybackSurfaceStatus::from_snapshot(&snapshot),
            Some(PlaybackSurfaceStatus::AwaitingHost)
        );

        snapshot.presenter = PresenterState::AwaitingVideoOutput;
        assert_eq!(
            PlaybackSurfaceStatus::from_snapshot(&snapshot),
            Some(PlaybackSurfaceStatus::AwaitingVideoOutput)
        );

        snapshot.presenter = PresenterState::Attached;
        assert_eq!(PlaybackSurfaceStatus::from_snapshot(&snapshot), None);

        // Detached is neutral until a presenter lifecycle explicitly begins;
        // this preserves the legacy integrated GStreamer path during rollout.
        snapshot.target = PlaybackTarget::GSTREAMER_INTEGRATED;
        snapshot.presenter = PresenterState::Detached;
        assert_eq!(PlaybackSurfaceStatus::from_snapshot(&snapshot), None);

        // A native-window target does not wait on an Iced presenter either.
        snapshot.target = PlaybackTarget::MPV_NATIVE_WINDOW;
        assert_eq!(PlaybackSurfaceStatus::from_snapshot(&snapshot), None);
    }

    #[test]
    fn failures_use_structured_snapshot_errors_with_safe_fallback_text() {
        let mut snapshot = snapshot(PlaybackTarget::MPV_NATIVE_WINDOW);
        snapshot.state = PlaybackState::Failed;
        assert_eq!(
            PlaybackSurfaceStatus::from_snapshot(&snapshot),
            Some(PlaybackSurfaceStatus::PlaybackFailed(
                "Playback failed.".to_string()
            ))
        );

        snapshot.last_error = Some(PlaybackError::new(
            PlaybackErrorKind::UnsupportedMedia,
            "Unsupported codec",
        ));
        assert_eq!(
            PlaybackSurfaceStatus::from_snapshot(&snapshot),
            Some(PlaybackSurfaceStatus::PlaybackFailed(
                "Unsupported codec".to_string()
            ))
        );

        snapshot.state = PlaybackState::Playing;
        snapshot.presenter = PresenterState::Failed;
        assert_eq!(
            PlaybackSurfaceStatus::from_snapshot(&snapshot),
            Some(PlaybackSurfaceStatus::PresenterFailed(
                "Unsupported codec".to_string()
            ))
        );
    }

    #[test]
    fn native_root_drag_replaces_video_click_only_for_integrated_macos_host() {
        assert!(native_root_drag_policy(
            Some(PlaybackTarget::MPV_INTEGRATED),
            true,
            true,
        ));
        assert!(!native_root_drag_policy(
            Some(PlaybackTarget::MPV_INTEGRATED),
            false,
            true,
        ));
        assert!(!native_root_drag_policy(
            Some(PlaybackTarget::MPV_NATIVE_WINDOW),
            true,
            true,
        ));
        assert!(!native_root_drag_policy(
            Some(PlaybackTarget::MPV_INTEGRATED),
            true,
            false,
        ));
        assert!(!native_root_drag_policy(None, true, true));
    }
}
