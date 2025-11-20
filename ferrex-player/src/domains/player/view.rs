use super::messages::Message;
use super::state::{PlayerDomainState, TrackNotification};
use super::theme;
use iced::Theme;
use iced::{
    Element, Length, Padding,
    widget::{Space, column, container, mouse_area, row, text},
};

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
    pub fn view(&self) -> iced::Element<'_, Message, Theme> {
        log::trace!(
            "PlayerState::view() called - position: {:.2}s, duration: {:.2}s, source_duration: {:?}, controls: {}",
            self.last_valid_position,
            self.last_valid_duration,
            self.source_duration,
            self.controls
        );

        // If external player is active, show a dedicated placeholder view
        if self.external_mpv_active {
            return container(
                column![
                    text("Playing Externally").size(24),
                    text(format!(
                        "Position: {:.0}s / {:.0}s",
                        self.last_valid_position, self.last_valid_duration
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

        if let Some(video) = &self.video_opt {
            let clickable_video = self.video_view(video);

            // Overlay stack: video, then controls
            let player_with_overlay: iced::Element<Message, Theme> =
                if self.controls {
                    let controls = self.controls_overlay();

                    let mut children: Vec<
                        iced::Element<Message, Theme, iced_wgpu::Renderer>,
                    > = vec![clickable_video];

                    children.push(controls);

                    iced::widget::Stack::with_children(children).into()
                } else {
                    clickable_video
                };

            let player_with_settings: iced::Element<Message, Theme> =
                if self.show_settings {
                    let settings = self.settings_panel();
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
                Message,
                Theme,
                iced_wgpu::Renderer,
            > = if self.show_quality_menu {
                let quality_menu = self.quality_menu_overlay();
                iced::widget::Stack::with_children(vec![
                    player_with_settings,
                    quality_menu,
                ])
                .into()
            } else if self.show_subtitle_menu {
                let subtitle_menu = self.subtitle_menu_overlay();
                iced::widget::Stack::with_children(vec![
                    player_with_settings,
                    subtitle_menu,
                ])
                .into()
            } else {
                player_with_settings
            };

            let player_with_notification: iced::Element<
                Message,
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
                .on_move(Message::MouseMoved)
                .on_release(Message::SeekRelease);

            container(interactive)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.is_loading_video {
            // Show loading spinner in player view
            let loading_content = column![
                // Static loading icon for now - animate later
                text("⟳").size(64),
                Space::new().height(Length::Fixed(20.0)),
                text("Loading video...")
                    .size(18)
                    .color(iced::Color::from_rgb(0.7, 0.7, 0.7)),
            ]
            .align_x(iced::Alignment::Center)
            .spacing(10);

            container(loading_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(theme::container_player)
                .into()
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
        video: &'a subwave_unified::video::SubwaveVideo,
    ) -> Element<'a, Message> {
        // Create the appropriate video player widget based on the backend
        // Determine if overlay is active (controls or menus visible)
        let overlay_active = self.controls
            || self.show_settings
            || self.show_subtitle_menu
            || self.show_quality_menu
            || self.track_notification.is_some();

        let player: iced::Element<Message, Theme, iced_wgpu::Renderer> = {
            let on_new_frame = if overlay_active {
                Some(Message::NewFrame)
            } else {
                None
            };
            video.widget(self.content_fit, on_new_frame)
        };

        // Wrap in a black background container first, then a mouse area to handle clicks
        //let video_with_background = container(player).width(Length::Fill).height(Length::Fill);

        iced::widget::mouse_area(player)
            .on_press(Message::VideoClicked)
            .into()
    }

    /// Build the controls overlay
    fn controls_overlay(&self) -> iced::Element<'_, Message, Theme> {
        // Delegate to controls.rs for the full implementation
        self.build_controls()
    }

    /// Build the track notification overlay
    fn notification_overlay<'a>(
        &self,
        notification: &'a TrackNotification,
    ) -> iced::Element<'a, Message, Theme> {
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
    ) -> iced::Element<'_, Message, Theme, iced_wgpu::Renderer> {
        // Delegate to controls.rs for the full implementation
        self.build_settings_panel()
    }

    /// Build the quality menu overlay
    fn quality_menu_overlay(
        &self,
    ) -> iced::Element<'_, Message, Theme, iced_wgpu::Renderer> {
        // Position the menu near the quality button (bottom right)
        container(row![
            Space::new().width(Length::Fill),
            container(self.build_quality_menu())
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
    ) -> iced::Element<'_, Message, Theme, iced_wgpu::Renderer> {
        // Position the menu near the subtitle button (bottom right)
        container(row![
            Space::new().width(Length::Fill),
            container(self.build_subtitle_menu())
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
    pub fn minimal_view(&self) -> Option<Element<'_, Message>> {
        self.video_opt.as_ref().map(|video| {
            let player = video
                .widget(self.content_fit, Some(Message::NewFrame))
                .map(|m| m);

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
