use super::state::{AspectRatio, PlayerState};
use super::theme;
use super::track_selection::format_subtitle_track;
use crate::Message;
use iced::Font;
use iced::{
    widget::{button, column, container, mouse_area, pick_list, row, slider, stack, text, Space},
    Alignment, Element, Length,
};
use iced_video_player::{AudioTrack, SubtitleTrack};
use lucide_icons::Icon;

/// Get the lucide font
fn lucide_font() -> Font {
    Font::with_name("lucide")
}

/// Helper function to create icon text
fn icon_text(icon: Icon) -> iced::widget::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

/// Helper function to create a control button with icon
fn icon_button(icon: Icon, message: Option<Message>) -> Element<'static, Message> {
    let btn = button(icon_text(icon))
        .style(theme::button_transparent as fn(&iced::Theme, button::Status) -> button::Style);

    if let Some(msg) = message {
        btn.on_press(msg)
    } else {
        btn
    }
    .padding(8)
    .into()
}

impl PlayerState {
    /// Build the full controls overlay
    pub fn build_controls(&self) -> Element<Message> {
        column![
            // Top bar with title and buttons
            container(
                row![
                    // Left side - back button
                    icon_button(Icon::ArrowLeft, Some(Message::BackToLibrary)),
                    // Center - Title
                    container(
                        text(
                            self.current_media
                                .as_ref()
                                .map(|m| m.display_title())
                                .unwrap_or_else(|| "Unknown".to_string())
                        )
                        .size(18)
                        .color([1.0, 1.0, 1.0, 1.0])
                    )
                    .width(Length::Fill)
                    .center_x(Length::Fill),
                    // Right side - fullscreen button in top
                    icon_button(
                        if self.is_fullscreen {
                            Icon::Minimize2
                        } else {
                            Icon::Maximize2
                        },
                        Some(Message::ToggleFullscreen)
                    ),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .padding(15)
            )
            .width(Length::Fill),
            // Spacer to push controls to bottom
            Space::with_height(Length::Fill),
            // Bottom controls
            column![
                // Seek bar - no padding so it reaches edges
                self.build_seek_bar(),
                // Spacer between seek bar and controls (40px to match bottom padding)
                //Space::with_height(Length::Fixed(15.0)),
                // Control buttons - with padding
                container(self.build_control_buttons())
                    .padding(40) // Same padding on all sides
                    .width(Length::Fill),
            ]
            .spacing(0)
            .width(Length::Fill),
        ]
        .into()
    }

    /// Build the custom seek bar
    fn build_seek_bar(&self) -> Element<Message> {
        let bar_height = 4.0;
        let hit_area_height = 30.0;

        // Calculate percentages
        let played_percentage = if self.duration > 0.0 {
            (self.position / self.duration).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let buffer_duration_as_percent = if self.duration > 0.0 {
            5.0 / self.duration // Buffer duration should be retrieved instead of hardcoded
        } else {
            0.0
        };

        // Use FillPortion for dynamic resizing based on percentages
        let played_portion = (played_percentage * 1000.0).max(1.0) as u16;
        let buffered_portion =
            (self.buffered_percentage * buffer_duration_as_percent * 1000.0).max(1.0) as u16;
        let unplayed_portion = (1000u16)
            .saturating_sub(played_portion)
            .saturating_sub(buffered_portion)
            .max(1);

        // Build the visual seek bar (4px height)
        let seek_bar_visual = container(row![
            container(Space::with_height(bar_height))
                .width(Length::FillPortion(played_portion))
                .style(theme::container_seek_bar_progress),
            container(Space::with_height(bar_height))
                .width(Length::FillPortion(buffered_portion))
                .style(theme::container_seek_bar_buffered),
            container(Space::with_height(bar_height))
                .width(Length::FillPortion(unplayed_portion))
                .style(theme::container_seek_bar_background),
        ])
        .width(Length::Fill)
        .height(bar_height);

        // Create interactive area using mouse_area
        // Note: For proper seek position calculation, we need the seek bar's bounds.
        // This would ideally be set via a layout event, but iced doesn't currently provide that.
        // As a workaround, we assume the seek bar spans the full window width.
        mouse_area(
            // Stack: transparent hit area with visual bar centered
            container(stack![
                // Transparent hit area
                container(Space::new(Length::Fill, hit_area_height)),
                // Visual bar centered vertically
                container(seek_bar_visual)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_y(Length::Fill),
            ])
            .width(Length::Fill)
            .height(hit_area_height),
        )
        .on_press(Message::SeekBarPressed)
        .on_move(Message::SeekBarMoved)
        .on_release(Message::SeekRelease)
        .into()
    }

    /// Build the main control buttons
    fn build_control_buttons(&self) -> Element<Message> {
        row![
            // Left section (1/3 width) - Time display
            container(
                text(format!(
                    "{} / {}",
                    super::view::format_time(self.position),
                    super::view::format_time(self.duration)
                ))
                .size(14)
                .color([1.0, 1.0, 1.0, 1.0])
            )
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Left),
            // Center section (1/3 width) - Playback controls
            container(
                row![
                    // Previous episode (disabled for now)
                    icon_button(Icon::SkipBack, None),
                    // Seek backward
                    icon_button(Icon::Rewind, Some(Message::SeekBackward)),
                    // Play/Pause
                    button(
                        text(if self.is_playing() {
                            Icon::Pause.unicode()
                        } else {
                            Icon::Play.unicode()
                        })
                        .font(lucide_font())
                        .size(24)
                    )
                    .on_press(Message::PlayPause)
                    .style(theme::button_transparent)
                    .padding(8),
                    // Seek forward
                    icon_button(Icon::FastForward, Some(Message::SeekForward)),
                    // Next episode (disabled for now)
                    icon_button(Icon::SkipForward, None),
                    // Stop
                    icon_button(Icon::Square, Some(Message::Stop)),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
            )
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
            // Right section (1/3 width) - Volume and settings controls
            container(
                row![
                    // Volume controls
                    button(
                        text(if self.is_muted {
                            Icon::VolumeX.unicode()
                        } else {
                            Icon::Volume2.unicode()
                        })
                        .font(lucide_font())
                        .size(20)
                    )
                    .on_press(Message::ToggleMute)
                    .style(theme::button_transparent)
                    .padding(8),
                    container(
                        slider(0.0..=1.0, self.volume, Message::SetVolume)
                            .step(0.01)
                            .width(Length::Fixed(80.0))
                            .style(theme::slider_volume)
                    )
                    .height(36.0)
                    .align_y(iced::alignment::Vertical::Center),
                    Space::with_width(Length::Fixed(20.0)),
                    // Subtitle button (with indicator if text subtitles are available)
                    if self
                        .available_subtitle_tracks
                        .iter()
                        .any(|t| t.is_text_based())
                    {
                        button(
                            text(Icon::MessageSquare.unicode())
                                .font(lucide_font())
                                .size(20),
                        )
                        .on_press(Message::CycleSubtitleSimple)
                        .style(if self.subtitles_enabled {
                            theme::button_player_active
                        } else {
                            theme::button_transparent
                        })
                        .padding(8)
                    } else {
                        // No subtitles available - show disabled button
                        button(
                            text(Icon::MessageSquare.unicode())
                                .font(lucide_font())
                                .size(20)
                                .style(theme::text_dim),
                        )
                        .style(theme::button_player_disabled)
                        .padding(8)
                    },
                    // Settings button
                    button(text(Icon::Settings.unicode()).font(lucide_font()).size(20))
                        .on_press(Message::ToggleSettings)
                        .style(theme::button_transparent)
                        .padding(8),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
            )
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right),
        ]
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .into()
    }

    /// Build the settings panel content
    pub fn build_settings_panel(&self) -> Element<Message> {
        container(
            column![
                // Header
                row![
                    text("Player Settings").size(20).style(theme::text_bright),
                    Space::with_width(Length::Fill),
                    button(text(Icon::X.unicode()).font(lucide_font()).size(20))
                        .on_press(Message::ToggleSettings)
                        .style(theme::button_ghost)
                        .padding(4),
                ]
                .align_y(Alignment::Center),
                Space::with_height(Length::Fixed(10.0)),
                // Playback section
                text("Playback").size(15).style(theme::text_muted),
                Space::with_height(Length::Fixed(8.0)),
                // Playback speed
                row![
                    text("Speed:").size(14),
                    Space::with_width(Length::Fill),
                    pick_list(
                        &[0.5, 0.75, 1.0, 1.25, 1.5, 2.0][..],
                        Some(self.playback_speed),
                        Message::SetPlaybackSpeed
                    )
                    .width(Length::Fixed(100.0))
                    .style(theme::pick_list_dark::<f64>),
                ]
                .align_y(Alignment::Center),
                Space::with_height(Length::Fixed(10.0)),
                // Aspect ratio
                row![
                    text("Aspect Ratio:").size(14),
                    Space::with_width(Length::Fill),
                    pick_list(
                        &[
                            AspectRatio::Original,
                            AspectRatio::Fit,
                            AspectRatio::Fill,
                            AspectRatio::Stretch,
                        ][..],
                        Some(self.aspect_ratio),
                        Message::SetAspectRatio
                    )
                    .width(Length::Fixed(120.0))
                    .style(theme::pick_list_dark::<AspectRatio>),
                ]
                .align_y(Alignment::Center),
                Space::with_height(Length::Fixed(15.0)),
                // Audio & Subtitles section
                text("Audio & Subtitles").size(15).style(theme::text_muted),
                Space::with_height(Length::Fixed(8.0)),
                // Audio track selection
                self.build_audio_track_selector(),
                Space::with_height(Length::Fixed(10.0)),
                // Subtitle controls
                self.build_subtitle_controls(),
                Space::with_height(Length::Fixed(10.0)),
                // Keyboard shortcuts info (more compact)
                container(
                    column![
                        text("Shortcuts").size(13).style(theme::text_muted),
                        Space::with_height(Length::Fixed(4.0)),
                        text("Space: Play/Pause").size(11).style(theme::text_dim),
                        text("A/S: Audio/Subtitle cycle")
                            .size(11)
                            .style(theme::text_dim),
                        text("Shift+S: Subtitle menu")
                            .size(11)
                            .style(theme::text_dim),
                        text("M: Mute • F: Fullscreen")
                            .size(11)
                            .style(theme::text_dim),
                        text("←→: Seek ±15s").size(11).style(theme::text_dim),
                    ]
                    .spacing(2)
                )
                .padding(6)
                .style(theme::container_subtle),
            ]
            .spacing(5)
            .padding(12),
        )
        .width(Length::Fixed(300.0))
        .height(Length::Shrink)
        .style(theme::container_settings_panel)
        .into()
    }

    /// Build audio track selector
    fn build_audio_track_selector(&self) -> Element<Message> {
        if self.available_audio_tracks.is_empty() {
            text("No audio tracks available")
                .size(14)
                .style(theme::text_dim)
                .into()
        } else {
            row![
                text("Audio Track:").size(14),
                Space::with_width(Length::Fill),
                pick_list(
                    self.available_audio_tracks.clone(),
                    self.available_audio_tracks
                        .get(self.current_audio_track as usize)
                        .cloned(),
                    |track| Message::AudioTrackSelected(track.index)
                )
                .width(Length::Fixed(200.0))
                .style(theme::pick_list_dark::<AudioTrack>)
                .text_size(14),
            ]
            .align_y(Alignment::Center)
            .into()
        }
    }

    /// Build subtitle controls for settings panel
    fn build_subtitle_controls(&self) -> Element<Message> {
        if self.available_subtitle_tracks.is_empty() {
            text("No subtitle tracks available")
                .size(14)
                .style(theme::text_dim)
                .into()
        } else {
            // Create options list with "Disabled" as first option
            let mut subtitle_options = vec![SubtitleOption::Disabled];
            for track in &self.available_subtitle_tracks {
                // Only show text-based subtitles in the UI
                if track.is_text_based() {
                    subtitle_options.push(SubtitleOption::Track(track.clone()));
                } else {
                    log::debug!(
                        "Filtering out non-text subtitle track {}: codec={:?}",
                        track.index,
                        track.codec
                    );
                }
            }

            // Determine current selection
            let current_selection = if !self.subtitles_enabled {
                Some(SubtitleOption::Disabled)
            } else {
                self.current_subtitle_track.and_then(|idx| {
                    self.available_subtitle_tracks
                        .get(idx as usize)
                        .map(|track| SubtitleOption::Track(track.clone()))
                })
            };

            row![
                text("Subtitles:").size(14),
                Space::with_width(Length::Fill),
                pick_list(subtitle_options, current_selection, |option| match option {
                    SubtitleOption::Disabled => Message::SubtitleTrackSelected(None),
                    SubtitleOption::Track(track) =>
                        Message::SubtitleTrackSelected(Some(track.index)),
                })
                .width(Length::Fixed(200.0))
                .style(theme::pick_list_dark::<SubtitleOption>)
                .text_size(14),
            ]
            .align_y(Alignment::Center)
            .into()
        }
    }

    /// Build the subtitle menu popup
    pub fn build_subtitle_menu(&self) -> Element<Message> {
        container(
            column![
                // Header
                row![
                    text("Subtitles").size(16).style(theme::text_bright),
                    Space::with_width(Length::Fill),
                    button(text(Icon::X.unicode()).font(lucide_font()).size(16))
                        .on_press(Message::ToggleSubtitleMenu)
                        .style(theme::button_ghost)
                        .padding(2),
                ]
                .align_y(Alignment::Center),
                Space::with_height(Length::Fixed(15.0)),
                // Disabled option
                button({
                    let check_icon: Element<Message> = if !self.subtitles_enabled {
                        text(Icon::Check.unicode())
                            .font(lucide_font())
                            .size(14)
                            .into()
                    } else {
                        Space::with_width(Length::Fixed(14.0)).into()
                    };

                    row![
                        check_icon,
                        Space::with_width(Length::Fixed(8.0)),
                        text("Disabled").size(14),
                    ]
                    .align_y(Alignment::Center)
                })
                .on_press(Message::SubtitleTrackSelected(None))
                .width(Length::Fill)
                .style(theme::button_menu_item)
                .padding([6, 10]),
                Space::with_height(Length::Fixed(5.0)),
                // Subtitle tracks (text-based only)
                column(
                    self.available_subtitle_tracks
                        .iter()
                        .filter(|track| track.is_text_based())
                        .map(|track| {
                            let is_selected = self.subtitles_enabled
                                && self.current_subtitle_track == Some(track.index);

                            button({
                                let check_icon: Element<Message> = if is_selected {
                                    text(Icon::Check.unicode())
                                        .font(lucide_font())
                                        .size(14)
                                        .into()
                                } else {
                                    Space::with_width(Length::Fixed(14.0)).into()
                                };

                                row![
                                    check_icon,
                                    Space::with_width(Length::Fixed(8.0)),
                                    text(format_subtitle_track(track)).size(14),
                                ]
                                .align_y(Alignment::Center)
                            })
                            .on_press(Message::SubtitleTrackSelected(Some(track.index)))
                            .width(Length::Fill)
                            .style(theme::button_menu_item)
                            .padding([6, 10])
                            .into()
                        })
                        .collect::<Vec<_>>()
                )
                .spacing(2),
            ]
            .spacing(5)
            .padding(15),
        )
        .width(Length::Fixed(280.0))
        .style(theme::container_subtitle_menu)
        .into()
    }
}

// SubtitleOption for pick_list in settings
#[derive(Debug, Clone, PartialEq, Eq)]
enum SubtitleOption {
    Disabled,
    Track(SubtitleTrack),
}

impl std::fmt::Display for SubtitleOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubtitleOption::Disabled => write!(f, "Disabled"),
            SubtitleOption::Track(track) => write!(f, "{}", format_subtitle_track(track)),
        }
    }
}

// AspectRatio display implementation for pick_list
impl std::fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AspectRatio::Original => write!(f, "Original"),
            AspectRatio::Fill => write!(f, "Fill"),
            AspectRatio::Fit => write!(f, "Fit"),
            AspectRatio::Stretch => write!(f, "Stretch"),
        }
    }
}
