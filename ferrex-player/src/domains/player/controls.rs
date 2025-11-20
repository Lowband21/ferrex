use super::theme;
use super::track_selection::format_subtitle_track;
use crate::domains::player::video_backend::{
    AlgorithmParams, AudioTrack, SubtitleTrack, ToneMappingAlgorithm, ToneMappingPreset,
};
use crate::domains::player::{messages::Message, state::PlayerDomainState};
use crate::domains::search::metrics::ConnectionQuality;
use iced::Theme;
use iced::{
    widget::{
        button, checkbox, column, container, mouse_area, pick_list, row, slider, stack, text, Space,
    },
    Alignment, Element, Length,
};
use iced::{ContentFit, Font};
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

impl PlayerDomainState {
    /// Build the full controls overlay
    pub fn build_controls(&self) -> iced::Element<Message, Theme> {
        let controls = column![
            // Top bar with title and buttons
            container(
                row![
                    // Left side - navigation buttons with spacing
                    row![
                        icon_button(Icon::ArrowLeft, Some(Message::NavigateBack)),
                        Space::with_width(Length::Fixed(5.0)),
                        icon_button(Icon::House, Some(Message::NavigateHome)),
                    ]
                    .align_y(Alignment::Center),
                    // Center - Title with HDR indicator
                    container(
                        row![
                            text(
                                self.current_media
                                    .as_ref()
                                    .map(|m| m.display_title())
                                    .unwrap_or_else(|| "Unknown".to_string())
                            )
                            .size(18)
                            .color([1.0, 1.0, 1.0, 1.0]),
                            // HDR indicator
                            if self.is_hdr_content {
                                row![
                                    Space::with_width(Length::Fixed(10.0)),
                                    container(text("HDR").size(12).color([1.0, 0.8, 0.0, 1.0]))
                                        .padding([2, 6])
                                        .style(theme::container_hdr_badge),
                                ]
                            } else {
                                row![]
                            }
                        ]
                        .align_y(Alignment::Center)
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
                    .padding(
                        crate::infrastructure::constants::player_controls::CONTROL_BUTTONS_PADDING
                    )
                    .width(Length::Fill),
            ]
            .spacing(0)
            .width(Length::Fill),
        ];
        controls.into()
    }

    /// Build the custom seek bar
    ///
    /// The seek bar has a visual height of 4px but a hit zone of 30px for easier interaction.
    /// Mouse clicks are validated to be within 7x the visual bar height (28px) vertically to prevent
    /// accidental seeks when clicking elsewhere on the screen.
    fn build_seek_bar(&self) -> Element<Message> {
        let bar_height = super::state::SEEK_BAR_VISUAL_HEIGHT;
        let hit_area_height =
            crate::infrastructure::constants::player_controls::SEEK_BAR_HIT_ZONE_HEIGHT;

        // Calculate percentages
        // Use source duration if available (for HLS this is the full media duration)
        let duration = self.source_duration.unwrap_or(self.duration);
        let played_percentage = if duration > 0.0 {
            let percentage = (self.position / duration).clamp(0.0, 1.0);
            percentage
        } else {
            log::debug!(
                "Seek bar: No duration available (source: {:?}, duration: {})",
                self.source_duration,
                self.duration
            );
            0.0
        };

        // Calculate buffered percentage - streaming domain should update this value
        let buffered_percentage = self.buffered_percentage.clamp(played_percentage, 1.0); // Buffered can't be less than played

        // Use FillPortion for dynamic resizing based on percentages
        let played_portion = (played_percentage * 1000.0).max(1.0) as u16;
        let _buffered_portion = ((buffered_percentage - played_percentage) * 1000.0)
            .max(0.0)
            .min(50.0) as u16;
        let unplayed_portion = (1000u16)
            .saturating_sub(played_portion)
            //.saturating_sub(buffered_portion)
            .max(1);

        // Build the visual seek bar (4px height)
        let seek_bar_visual = container(row![
            container(Space::with_height(bar_height))
                .width(Length::FillPortion(played_portion))
                .style(theme::container_seek_bar_progress),
            //container(Space::with_height(bar_height))
            //    .width(Length::FillPortion(buffered_portion))
            //    .style(theme::container_seek_bar_buffered),
            container(Space::with_height(bar_height))
                .width(Length::FillPortion(unplayed_portion))
                .style(move |theme| theme::container_seek_bar_background(theme, self.seek_bar_hovered)),
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
        // Mouse move and release are now handled at the player view level for global tracking
        .into()
    }

    /// Build the main control buttons
    fn build_control_buttons(&self) -> Element<Message> {
        // Use source duration if available (for HLS this is the full media duration)
        let duration = self.source_duration.unwrap_or(self.duration);

        //log::debug!(
        //    "Building control buttons - position: {:.2}s, duration: {:.2}s, source_duration: {:?}",
        //    self.position,
        //    self.duration,
        //    self.source_duration
        //);

        row![
            // Left section (1/3 width) - Time display
            container(
                text(if duration > 0.0 {
                    format!(
                        "{} / {}",
                        super::view::format_time(self.position),
                        super::view::format_time(duration)
                    )
                } else {
                    format!("{} / --:--", super::view::format_time(self.position))
                })
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
                    // Stop - navigates back
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
                    .height(
                        crate::infrastructure::constants::player_controls::CONTROL_BUTTONS_HEIGHT
                    )
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
                    // Video settings button (always show)
                    button(text(Icon::Gauge.unicode()).font(lucide_font()).size(20))
                        .on_press(Message::ToggleQualityMenu)
                        .style(if self.show_quality_menu {
                            theme::button_player_active
                        } else {
                            theme::button_transparent
                        })
                        .padding(8),
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
    pub fn build_settings_panel(&self) -> iced::Element<Message, Theme> {
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
                            ContentFit::Contain,
                            ContentFit::Cover,
                            ContentFit::Fill,
                            ContentFit::None,
                            ContentFit::ScaleDown,
                        ][..],
                        Some(self.content_fit),
                        Message::SetContentFit
                    )
                    .width(Length::Fixed(120.0))
                    .style(theme::pick_list_dark::<ContentFit>),
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
                // HDR information
                if self.is_hdr_content {
                    container(
                        column![
                            text("HDR Information").size(13).style(theme::text_muted),
                            Space::with_height(Length::Fixed(4.0)),
                            text("HDR content detected")
                                .size(11)
                                .style(theme::text_bright),
                            if let Some(media) = &self.current_media {
                                if let Some(metadata) = &media.metadata {
                                    let mut metadata_column = column![].spacing(2);

                                    if let Some(bit_depth) = metadata.bit_depth {
                                        metadata_column = metadata_column.push(
                                            text(format!("Bit depth: {}", bit_depth))
                                                .size(11)
                                                .style(theme::text_dim),
                                        );
                                    }

                                    if let Some(color_transfer) = &metadata.color_transfer {
                                        metadata_column = metadata_column.push(
                                            text(format!("Transfer: {}", color_transfer))
                                                .size(11)
                                                .style(theme::text_dim),
                                        );
                                    }

                                    if let Some(color_primaries) = &metadata.color_primaries {
                                        metadata_column = metadata_column.push(
                                            text(format!("Primaries: {}", color_primaries))
                                                .size(11)
                                                .style(theme::text_dim),
                                        );
                                    }

                                    metadata_column
                                } else {
                                    column![]
                                }
                            } else {
                                column![]
                            },
                        ]
                        .spacing(2),
                    )
                    .padding(6)
                    .style(theme::container_subtle)
                } else {
                    container(Space::with_height(0))
                },
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

    /// Build the quality/tone mapping menu popup
    pub fn build_quality_menu(&self) -> iced::Element<Message, Theme, iced_wgpu::Renderer> {
        let mut content = column![
            // Header
            row![
                text("Video Settings").size(16).style(theme::text_bright),
                Space::with_width(Length::Fill),
                button(text(Icon::X.unicode()).font(lucide_font()).size(16))
                    .on_press(Message::ToggleQualityMenu)
                    .style(theme::button_ghost)
                    .padding(2),
            ]
            .align_y(Alignment::Center),
            Space::with_height(Length::Fixed(15.0)),
        ]
        .spacing(5);

        // Tone mapping enabled checkbox
        let tone_mapping_enabled_checkbox =
            checkbox("Enable Tone Mapping", self.tone_mapping_config.enabled)
                .on_toggle(Message::ToggleToneMapping)
                .size(14)
                .text_size(14);

        content = content.push(tone_mapping_enabled_checkbox);
        content = content.push(Space::with_height(Length::Fixed(10.0)));

        // Preset selection
        let preset_names = vec![
            "Custom",
            "Default",
            "Film (ACES)",
            "Game Classic",
            "HDR Bright",
            "Natural",
            "Vivid",
        ];

        let current_preset_name = match self.tone_mapping_config.preset {
            ToneMappingPreset::Custom => "Custom",
            ToneMappingPreset::Default => "Default",
            ToneMappingPreset::Film => "Film (ACES)",
            ToneMappingPreset::GameClassic => "Game Classic",
            ToneMappingPreset::HdrBright => "HDR Bright",
            ToneMappingPreset::Natural => "Natural",
            ToneMappingPreset::Vivid => "Vivid",
        };

        content = content.push(
            row![
                text("Preset:").size(14),
                Space::with_width(Length::Fill),
                pick_list(preset_names, Some(current_preset_name), |selected| {
                    use crate::domains::player::video_backend::ToneMappingPreset;
                    let preset = match selected {
                        "Custom" => ToneMappingPreset::Custom,
                        "Default" => ToneMappingPreset::Default,
                        "Film (ACES)" => ToneMappingPreset::Film,
                        "Game Classic" => ToneMappingPreset::GameClassic,
                        "HDR Bright" => ToneMappingPreset::HdrBright,
                        "Natural" => ToneMappingPreset::Natural,
                        "Vivid" => ToneMappingPreset::Vivid,
                        _ => ToneMappingPreset::Default,
                    };
                    Message::SetToneMappingPreset(preset)
                },)
                .width(Length::Fixed(150.0))
                .style(theme::pick_list_dark::<&str>)
                .text_size(14),
            ]
            .align_y(Alignment::Center),
        );

        content = content.push(Space::with_height(Length::Fixed(10.0)));

        // Algorithm selection
        let algorithm_names = vec![
            "None",
            "Reinhard",
            "Reinhard Extended",
            "ACES Filmic",
            "Hable (Uncharted 2)",
        ];

        let current_algorithm_name = match self.tone_mapping_config.algorithm {
            ToneMappingAlgorithm::None => "None",
            ToneMappingAlgorithm::Reinhard => "Reinhard",
            ToneMappingAlgorithm::ReinhardExtended => "Reinhard Extended",
            ToneMappingAlgorithm::AcesFilmic => "ACES Filmic",
            ToneMappingAlgorithm::Hable => "Hable (Uncharted 2)",
        };

        content = content.push(
            row![
                text("Algorithm:").size(14),
                Space::with_width(Length::Fill),
                pick_list(algorithm_names, Some(current_algorithm_name), |selected| {
                    let algorithm = match selected {
                        "None" => ToneMappingAlgorithm::None,
                        "Reinhard" => ToneMappingAlgorithm::Reinhard,
                        "Reinhard Extended" => ToneMappingAlgorithm::ReinhardExtended,
                        "ACES Filmic" => ToneMappingAlgorithm::AcesFilmic,
                        "Hable (Uncharted 2)" => ToneMappingAlgorithm::Hable,
                        _ => ToneMappingAlgorithm::None,
                    };
                    Message::SetToneMappingAlgorithm(algorithm)
                },)
                .width(Length::Fixed(150.0))
                .style(theme::pick_list_dark::<&str>)
                .text_size(14),
            ]
            .align_y(Alignment::Center),
        );

        // Algorithm-specific parameters
        if self.tone_mapping_config.enabled {
            content = content.push(Space::with_height(Length::Fixed(10.0)));
            content = content.push(
                text("─── Algorithm Settings ─────────")
                    .size(12)
                    .style(theme::text_muted),
            );
            content = content.push(Space::with_height(Length::Fixed(5.0)));

            match self.tone_mapping_config.algorithm {
                ToneMappingAlgorithm::ReinhardExtended => {
                    let (white_point, exposure, saturation_boost) =
                        match &self.tone_mapping_config.algorithm_params {
                            AlgorithmParams::ReinhardExtended {
                                white_point,
                                exposure,
                                saturation_boost,
                            } => (*white_point, *exposure, *saturation_boost),
                            _ => (4.0, 1.5, 1.2),
                        };

                    // White Point slider
                    content = content.push(
                        row![
                            text("White Point:").size(12),
                            Space::with_width(Length::Fixed(10.0)),
                            slider(1.0..=10.0, white_point, Message::SetToneMappingWhitePoint)
                                .step(0.1)
                                .width(Length::Fixed(120.0))
                                .style(theme::slider_volume),
                            Space::with_width(Length::Fixed(5.0)),
                            text(format!("{:.1}", white_point))
                                .size(12)
                                .style(theme::text_muted),
                        ]
                        .align_y(Alignment::Center),
                    );

                    // Exposure slider
                    content = content.push(
                        row![
                            text("Exposure:").size(12),
                            Space::with_width(Length::Fixed(10.0)),
                            slider(0.5..=3.0, exposure, Message::SetToneMappingExposure)
                                .step(0.1)
                                .width(Length::Fixed(120.0))
                                .style(theme::slider_volume),
                            Space::with_width(Length::Fixed(5.0)),
                            text(format!("{:.1}", exposure))
                                .size(12)
                                .style(theme::text_muted),
                        ]
                        .align_y(Alignment::Center),
                    );

                    // Saturation boost slider
                    content = content.push(
                        row![
                            text("Saturation:").size(12),
                            Space::with_width(Length::Fixed(10.0)),
                            slider(
                                0.8..=1.5,
                                saturation_boost,
                                Message::SetToneMappingSaturationBoost
                            )
                            .step(0.05)
                            .width(Length::Fixed(120.0))
                            .style(theme::slider_volume),
                            Space::with_width(Length::Fixed(5.0)),
                            text(format!("{:.2}", saturation_boost))
                                .size(12)
                                .style(theme::text_muted),
                        ]
                        .align_y(Alignment::Center),
                    );
                }
                ToneMappingAlgorithm::Hable => {
                    let (shoulder, linear_strength, linear_angle, toe_strength) =
                        match &self.tone_mapping_config.algorithm_params {
                            AlgorithmParams::Hable {
                                shoulder_strength,
                                linear_strength,
                                linear_angle,
                                toe_strength,
                            } => (
                                *shoulder_strength,
                                *linear_strength,
                                *linear_angle,
                                *toe_strength,
                            ),
                            _ => (0.15, 0.5, 0.01, 0.2),
                        };

                    // Shoulder strength slider
                    content = content.push(
                        row![
                            text("Shoulder:").size(12),
                            Space::with_width(Length::Fixed(10.0)),
                            slider(0.0..=1.0, shoulder, Message::SetHableShoulderStrength)
                                .step(0.01)
                                .width(Length::Fixed(120.0))
                                .style(theme::slider_volume),
                            Space::with_width(Length::Fixed(5.0)),
                            text(format!("{:.2}", shoulder))
                                .size(12)
                                .style(theme::text_muted),
                        ]
                        .align_y(Alignment::Center),
                    );

                    // Linear strength slider
                    content = content.push(
                        row![
                            text("Linear Str:").size(12),
                            Space::with_width(Length::Fixed(10.0)),
                            slider(0.0..=1.0, linear_strength, Message::SetHableLinearStrength)
                                .step(0.01)
                                .width(Length::Fixed(120.0))
                                .style(theme::slider_volume),
                            Space::with_width(Length::Fixed(5.0)),
                            text(format!("{:.2}", linear_strength))
                                .size(12)
                                .style(theme::text_muted),
                        ]
                        .align_y(Alignment::Center),
                    );

                    // Linear angle slider
                    content = content.push(
                        row![
                            text("Linear Angle:").size(12),
                            Space::with_width(Length::Fixed(10.0)),
                            slider(0.0..=0.1, linear_angle, Message::SetHableLinearAngle)
                                .step(0.001)
                                .width(Length::Fixed(120.0))
                                .style(theme::slider_volume),
                            Space::with_width(Length::Fixed(5.0)),
                            text(format!("{:.3}", linear_angle))
                                .size(12)
                                .style(theme::text_muted),
                        ]
                        .align_y(Alignment::Center),
                    );

                    // Toe strength slider
                    content = content.push(
                        row![
                            text("Toe Strength:").size(12),
                            Space::with_width(Length::Fixed(10.0)),
                            slider(0.0..=1.0, toe_strength, Message::SetHableToeStrength)
                                .step(0.01)
                                .width(Length::Fixed(120.0))
                                .style(theme::slider_volume),
                            Space::with_width(Length::Fixed(5.0)),
                            text(format!("{:.2}", toe_strength))
                                .size(12)
                                .style(theme::text_muted),
                        ]
                        .align_y(Alignment::Center),
                    );
                }
                ToneMappingAlgorithm::Reinhard => {
                    content = content.push(
                        text("Basic Reinhard (no parameters)")
                            .size(12)
                            .style(theme::text_muted),
                    );
                }
                ToneMappingAlgorithm::None => {
                    content = content.push(
                        text("Passthrough (no tone mapping)")
                            .size(12)
                            .style(theme::text_muted),
                    );
                }
                ToneMappingAlgorithm::AcesFilmic => {
                    content = content.push(
                        text("ACES Filmic (no parameters)")
                            .size(12)
                            .style(theme::text_muted),
                    );
                }
            }

            // Display settings
            content = content.push(Space::with_height(Length::Fixed(10.0)));
            content = content.push(
                text("─── Display Settings ───────────")
                    .size(12)
                    .style(theme::text_muted),
            );
            content = content.push(Space::with_height(Length::Fixed(5.0)));

            // Monitor brightness slider
            content = content.push(
                row![
                    text("Monitor (nits):").size(12),
                    Space::with_width(Length::Fixed(10.0)),
                    slider(
                        100.0..=1000.0,
                        self.tone_mapping_config.monitor_brightness,
                        Message::SetMonitorBrightness
                    )
                    .step(50.0)
                    .width(Length::Fixed(120.0))
                    .style(theme::slider_volume),
                    Space::with_width(Length::Fixed(5.0)),
                    text(format!(
                        "{:.0}",
                        self.tone_mapping_config.monitor_brightness
                    ))
                    .size(12)
                    .style(theme::text_muted),
                ]
                .align_y(Alignment::Center),
            );

            // General adjustments
            content = content.push(Space::with_height(Length::Fixed(10.0)));
            content = content.push(
                text("─── Adjustments ────────────────")
                    .size(12)
                    .style(theme::text_muted),
            );
            content = content.push(Space::with_height(Length::Fixed(5.0)));

            // Brightness adjustment slider (not shown for Hable since it uses exposure only)
            if !matches!(
                self.tone_mapping_config.algorithm,
                ToneMappingAlgorithm::Hable
            ) {
                content = content.push(
                    row![
                        text("Brightness:").size(12),
                        Space::with_width(Length::Fixed(10.0)),
                        slider(
                            -1.0..=1.0,
                            self.tone_mapping_config.brightness_adjustment,
                            Message::SetToneMappingBrightness
                        )
                        .step(0.05)
                        .width(Length::Fixed(120.0))
                        .style(theme::slider_volume),
                        Space::with_width(Length::Fixed(5.0)),
                        text(format!(
                            "{:.2}",
                            self.tone_mapping_config.brightness_adjustment
                        ))
                        .size(12)
                        .style(theme::text_muted),
                    ]
                    .align_y(Alignment::Center),
                );
            }

            // Contrast adjustment slider (not shown for Hable since curve parameters provide control)
            if !matches!(
                self.tone_mapping_config.algorithm,
                ToneMappingAlgorithm::Hable
            ) {
                content = content.push(
                    row![
                        text("Contrast:").size(12),
                        Space::with_width(Length::Fixed(10.0)),
                        slider(
                            0.5..=2.0,
                            self.tone_mapping_config.contrast_adjustment,
                            Message::SetToneMappingContrast
                        )
                        .step(0.05)
                        .width(Length::Fixed(120.0))
                        .style(theme::slider_volume),
                        Space::with_width(Length::Fixed(5.0)),
                        text(format!(
                            "{:.2}",
                            self.tone_mapping_config.contrast_adjustment
                        ))
                        .size(12)
                        .style(theme::text_muted),
                    ]
                    .align_y(Alignment::Center),
                );
            }

            // Exposure adjustment slider
            content = content.push(
                row![
                    text("Exposure:").size(12),
                    Space::with_width(Length::Fixed(10.0)),
                    slider(
                        0.5..=3.0,
                        self.tone_mapping_config.exposure_adjustment,
                        Message::SetToneMappingExposure
                    )
                    .step(0.1)
                    .width(Length::Fixed(120.0))
                    .style(theme::slider_volume),
                    Space::with_width(Length::Fixed(5.0)),
                    text(format!(
                        "{:.1}",
                        self.tone_mapping_config.exposure_adjustment
                    ))
                    .size(12)
                    .style(theme::text_muted),
                ]
                .align_y(Alignment::Center),
            );

            // Saturation adjustment slider
            content = content.push(
                row![
                    text("Saturation:").size(12),
                    Space::with_width(Length::Fixed(10.0)),
                    slider(
                        0.0..=2.0,
                        self.tone_mapping_config.saturation_adjustment,
                        Message::SetToneMappingSaturation
                    )
                    .step(0.05)
                    .width(Length::Fixed(120.0))
                    .style(theme::slider_volume),
                    Space::with_width(Length::Fixed(5.0)),
                    text(format!(
                        "{:.2}",
                        self.tone_mapping_config.saturation_adjustment
                    ))
                    .size(12)
                    .style(theme::text_muted),
                ]
                .align_y(Alignment::Center),
            );
        }

        container(content.padding(20))
            .style(theme::container_subtitle_menu)
            .width(Length::Fixed(350.0))
            .into()
    }

    /// Build the subtitle menu popup
    pub fn build_subtitle_menu(&self) -> iced::Element<Message, Theme, iced_wgpu::Renderer> {
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
