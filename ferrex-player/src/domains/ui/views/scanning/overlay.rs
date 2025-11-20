use crate::{
    common::{Icon, icon_text},
    domains::ui::theme,
};
use iced::{
    Element, Length,
    widget::{Space, button, column, container, row, text},
};

use crate::domains::ui::messages::Message;
use crate::infrastructure::api_types::{ScanProgress, ScanStatus};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn create_scan_progress_overlay<'a>(
    content: impl Into<Element<'a, Message>>,
    scan_progress: &Option<ScanProgress>,
) -> Element<'a, Message> {
    log::info!("Creating scan overlay function called");
    use iced::widget::{mouse_area, stack};

    let base_content = content.into();

    if let Some(progress) = scan_progress {
        log::info!(
            "Scan progress data available: status={:?}, files={}/{}",
            progress.status,
            progress.scanned_files,
            progress.total_files
        );
        let progress_percentage = if progress.total_files > 0 {
            (progress.scanned_files as f32 / progress.total_files as f32) * 100.0
        } else {
            0.0
        };

        let eta_text = if let Some(eta) = progress.estimated_time_remaining {
            let seconds = eta.as_secs();
            if seconds < 60 {
                format!("{} sec", seconds)
            } else {
                let minutes = seconds / 60;
                let remaining_seconds = seconds % 60;
                format!("{}:{:02}", minutes, remaining_seconds)
            }
        } else {
            "Calculating...".to_string()
        };

        let current_file_text = if let Some(file) = &progress.current_file {
            let filename = std::path::Path::new(file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file);
            filename.to_string()
        } else {
            "Scanning...".to_string()
        };

        let status_text = match progress.status {
            ScanStatus::Pending => "Preparing",
            ScanStatus::Scanning => "Scanning",
            ScanStatus::Processing => "Processing",
            ScanStatus::Completed => "Completed",
            ScanStatus::Failed => "Failed",
            ScanStatus::Cancelled => "Cancelled",
        };

        // Calculate scan speed
        let scan_speed = if progress.total_files > 0 && progress.scanned_files > 0 {
            // Estimate based on scan time (this is a rough calculation)
            // In a real implementation, you'd track actual scan start time
            let estimated_scan_time =
                progress.scanned_files as f32 / (progress.scanned_files as f32 / 60.0); // Rough estimate
            if estimated_scan_time > 0.0 {
                progress.scanned_files as f32 / estimated_scan_time
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Create overlay content
        // Add a semi-transparent background with blur effect
        let background = container(Space::new(Length::Fill, Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(
                    0.0, 0.0, 0.0, 0.3,
                ))),
                ..Default::default()
            });

        // Enhanced library info
        let library_info = if let Some(library) = progress.path.split('/').last() {
            format!("📁 {}", library)
        } else {
            format!("📁 {}", progress.path)
        };

        let overlay_content = container(
            container(
                column![
                    // Enhanced Header with library info
                    column![
                        row![
                            text("🔄 Library Scan")
                                .size(18)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            Space::with_width(Length::Fill),
                            button(icon_text(Icon::X))
                                .on_press(Message::ToggleScanProgress)
                                .style(theme::Button::Text.style())
                                .padding(5)
                        ]
                        .align_y(iced::Alignment::Center),
                        text(library_info)
                            .size(13)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(3),
                    Space::with_height(15),
                    // Progress bar
                    row![
                        text(status_text)
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        Space::with_width(Length::Fill),
                        container(
                            container(Space::with_width(Length::Fixed(
                                progress_percentage * 0.01 * 250.0
                            )))
                            .height(4)
                            .style(theme::Container::ProgressBar.style())
                        )
                        .width(250)
                        .height(4)
                        .style(theme::Container::ProgressBarBackground.style()),
                        Space::with_width(10),
                        text(format!("{:.0}%", progress_percentage))
                            .size(13)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .align_y(iced::Alignment::Center),
                    Space::with_height(10),
                    // Enhanced Stats Grid
                    column![
                        // First row of stats
                        row![
                            column![
                                text("📂 Files")
                                    .size(11)
                                    .color(theme::MediaServerTheme::TEXT_DIMMED),
                                text(format!(
                                    "{}/{}",
                                    progress.scanned_files, progress.total_files
                                ))
                                .size(13)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            ],
                            Space::with_width(25),
                            column![
                                text("💾 Stored")
                                    .size(11)
                                    .color(theme::MediaServerTheme::TEXT_DIMMED),
                                text(format!("{}", progress.stored_files))
                                    .size(13)
                                    .color(iced::Color::from_rgb(0.0, 0.8, 0.0)),
                            ],
                            Space::with_width(25),
                            column![
                                text("🏷️ Metadata")
                                    .size(11)
                                    .color(theme::MediaServerTheme::TEXT_DIMMED),
                                text(format!("{}", progress.metadata_fetched))
                                    .size(13)
                                    .color(iced::Color::from_rgb(0.0, 0.6, 1.0)),
                            ],
                            Space::with_width(25),
                            column![
                                text("⏱️ ETA")
                                    .size(11)
                                    .color(theme::MediaServerTheme::TEXT_DIMMED),
                                text(eta_text)
                                    .size(13)
                                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            ],
                        ],
                        Space::with_height(8),
                        // Second row with additional stats
                        row![
                            column![
                                text("⚡ Speed")
                                    .size(11)
                                    .color(theme::MediaServerTheme::TEXT_DIMMED),
                                text(if scan_speed > 0.0 {
                                    format!("{:.1} files/min", scan_speed * 60.0)
                                } else {
                                    "Calculating...".to_string()
                                })
                                .size(13)
                                .color(iced::Color::from_rgb(1.0, 0.6, 0.0)),
                            ],
                            Space::with_width(25),
                            column![
                                text("📊 Success Rate")
                                    .size(11)
                                    .color(theme::MediaServerTheme::TEXT_DIMMED),
                                text(if progress.scanned_files > 0 {
                                    format!(
                                        "{:.1}%",
                                        (progress.stored_files as f32
                                            / progress.scanned_files as f32)
                                            * 100.0
                                    )
                                } else {
                                    "N/A".to_string()
                                })
                                .size(13)
                                .color(iced::Color::from_rgb(0.0, 0.8, 0.0)),
                            ],
                            Space::with_width(25),
                            if !progress.errors.is_empty() {
                                Element::from(column![
                                    text("❌ Errors")
                                        .size(11)
                                        .color(theme::MediaServerTheme::TEXT_DIMMED),
                                    text(format!("{}", progress.errors.len()))
                                        .size(13)
                                        .color(theme::MediaServerTheme::ERROR),
                                ])
                            } else {
                                Element::from(column![
                                    text("✅ Status")
                                        .size(11)
                                        .color(theme::MediaServerTheme::TEXT_DIMMED),
                                    text("No errors")
                                        .size(13)
                                        .color(iced::Color::from_rgb(0.0, 0.8, 0.0)),
                                ])
                            },
                            Space::with_width(Length::Fill),
                        ]
                    ]
                    .spacing(2),
                    Space::with_height(10),
                    // Enhanced Current file section
                    container(
                        column![
                            row![
                                text("📄 Currently Processing")
                                    .size(11)
                                    .color(theme::MediaServerTheme::TEXT_DIMMED),
                                Space::with_width(Length::Fill),
                                // Add a small pulse animation indicator
                                text("●")
                                    .size(10)
                                    .color(iced::Color::from_rgb(0.0, 1.0, 0.0)),
                            ]
                            .align_y(iced::Alignment::Center),
                            container(
                                text(if current_file_text.chars().count() > 50 {
                                    let chars: Vec<char> = current_file_text.chars().collect();
                                    let start_index = chars.len().saturating_sub(47);
                                    format!(
                                        "...{}",
                                        chars[start_index..].iter().collect::<String>()
                                    )
                                } else {
                                    current_file_text.clone()
                                })
                                .size(12)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY)
                            )
                            .width(Length::Fill)
                            .padding([2, 0]),
                        ]
                        .spacing(3)
                    )
                    .width(Length::Fill)
                    .padding([8, 12])
                    .style(move |_| container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgba(
                            0.1, 0.1, 0.1, 0.5
                        ))),
                        border: iced::Border {
                            color: iced::Color::from_rgba(0.3, 0.3, 0.3, 0.3),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }),
                ]
                .spacing(5)
                .width(450),
            )
            .padding(20)
            .style(theme::Container::Card.style())
            .width(Length::Shrink)
            .height(Length::Shrink),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Top)
        .padding(40);

        // Stack the overlay on top of the main content
        log::info!("Rendering scan overlay stack");
        stack![
            base_content,
            // Semi-transparent background
            mouse_area(background).on_press(Message::ToggleScanProgress),
            // Overlay content
            overlay_content
        ]
        .into()
    } else {
        base_content
    }
}
