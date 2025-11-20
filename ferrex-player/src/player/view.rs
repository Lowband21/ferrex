use super::state::{AspectRatio, PlayerState, TrackNotification};
use super::theme;
use crate::messages::media::Message;
use iced::{
    widget::{button, column, container, mouse_area, row, stack, text, Space},
    ContentFit, Element, Length, Padding,
};
use iced_video_player::VideoPlayer;

impl PlayerState {
    /// Build the main player view
    pub fn view(&self) -> Element<Message> {
        log::trace!("PlayerState::view() called - position: {:.2}s, duration: {:.2}s, source_duration: {:?}, controls: {}",
            self.position, self.duration, self.source_duration, self.controls);

        if let Some(video) = &self.video_opt {
            // Create the clickable video
            let clickable_video = self.video_view(video);

            // Create overlay if controls are visible
            let player_with_overlay: Element<Message> = if self.controls {
                stack![clickable_video, self.controls_overlay()].into()
            } else {
                clickable_video
            };

            // Add settings panel if visible
            let player_with_settings = if self.show_settings {
                stack![
                    player_with_overlay,
                    // Position settings panel in bottom right, near the settings button
                    container(row![
                        Space::with_width(Length::Fill),
                        container(self.settings_panel())
                            .style(theme::container_settings_panel_wrapper),
                        Space::with_width(Length::Fixed(80.0)), // Offset from right edge
                    ])
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_y(iced::alignment::Vertical::Bottom)
                    .padding(Padding {
                        top: 0.0,
                        right: 0.0,
                        bottom: 100.0,
                        left: 0.0
                    }) // Position above controls
                ]
                .into()
            } else {
                player_with_overlay
            };

            // Add subtitle menu if visible
            let player_with_menus = if self.show_quality_menu {
                stack![player_with_settings, self.quality_menu_overlay()].into()
            } else if self.show_subtitle_menu {
                stack![player_with_settings, self.subtitle_menu_overlay()].into()
            } else {
                player_with_settings
            };

            // Add track notification overlay if present
            let player_with_notification = if let Some(notification) = &self.track_notification {
                stack![player_with_menus, self.notification_overlay(notification)].into()
            } else {
                player_with_menus
            };

            // Wrap with mouse movement detection
            let interactive = mouse_area(player_with_notification).on_move(|_| Message::MouseMoved);

            container(interactive)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.is_loading_video {
            // Show loading spinner in player view
            let loading_content = column![
                // Static loading icon for now - can be animated later
                text("⟳").size(64), // Using a refresh/loading unicode symbol
                Space::with_height(Length::Fixed(20.0)),
                text("Loading video...")
                    .size(18)
                    .color(iced::Color::from_rgb(0.7, 0.7, 0.7)),
            ]
            .align_x(iced::Alignment::Center)
            .spacing(10);

            // Full screen player-style container with loading spinner
            container(loading_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(theme::container_player)
                .into()
        } else {
            // No video loaded and not loading
            container(
                column![
                    text("No video loaded").size(24),
                    Space::with_height(Length::Fixed(20.0)),
                    button("Back to Library")
                        .on_press(Message::BackToLibrary)
                        .style(theme::button_player),
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
    fn video_view<'a>(&self, video: &'a iced_video_player::Video) -> Element<'a, Message> {
        let player = VideoPlayer::new(video)
            .width(Length::Fill)
            .height(Length::Fill)
            .on_new_frame(Message::NewFrame)
            .on_seek_done(Message::SeekDone);

        let player = match self.aspect_ratio {
            AspectRatio::Fill => player.content_fit(ContentFit::Cover),
            AspectRatio::Fit => player.content_fit(ContentFit::Contain),
            AspectRatio::Stretch => player.content_fit(ContentFit::Fill),
            AspectRatio::Original => player.content_fit(ContentFit::None),
        };

        // Wrap in a mouse area to handle clicks
        iced::widget::mouse_area(container(player).width(Length::Fill).height(Length::Fill))
            .on_press(Message::VideoClicked)
            .into()
    }

    /// Build the controls overlay
    fn controls_overlay(&self) -> Element<Message> {
        // Delegate to controls.rs for the full implementation
        self.build_controls()
    }

    /// Build the track notification overlay
    fn notification_overlay<'a>(
        &self,
        notification: &'a TrackNotification,
    ) -> Element<'a, Message> {
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
    fn settings_panel(&self) -> Element<Message> {
        // Delegate to controls.rs for the full implementation
        self.build_settings_panel()
    }

    /// Build the subtitle menu overlay
    fn quality_menu_overlay(&self) -> Element<Message> {
        // Position the menu near the quality button (bottom right)
        container(row![
            Space::with_width(Length::Fill),
            container(self.build_quality_menu()).style(theme::container_subtitle_menu_wrapper),
            Space::with_width(Length::Fixed(200.0)), // Offset from right edge
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

    fn subtitle_menu_overlay(&self) -> Element<Message> {
        // Position the menu near the subtitle button (bottom right)
        container(row![
            Space::with_width(Length::Fill),
            container(self.build_subtitle_menu()).style(theme::container_subtitle_menu_wrapper),
            Space::with_width(Length::Fixed(100.0)), // Offset from right edge
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
    pub fn minimal_view(&self) -> Option<Element<Message>> {
        self.video_opt.as_ref().map(|video| {
            let player = VideoPlayer::new(video)
                .width(Length::Fill)
                .height(Length::Fixed(200.0))
                .on_new_frame(Message::NewFrame)
                .on_seek_done(Message::SeekDone);

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
