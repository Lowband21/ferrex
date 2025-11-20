//! Library management view with permission-based controls

use std::sync::Arc;

use crate::{
    common::ui_utils::icon_text,
    domains::{
        auth::permissions::{self, StatePermissionExt},
        ui::{messages::Message, theme, views::admin::view_library_form},
    },
    infrastructure::repository::accessor::{Accessor, ReadOnly},
    state_refactored::State,
};
use ferrex_core::{
    ArchivedLibraryType, LibraryID,
    api_types::{ScanLifecycleStatus, ScanSnapshotDto},
    types::library::{ArchivedLibrary, Library},
};
use iced::{
    Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};
use lucide_icons::Icon;
use rkyv::{deserialize, rancor::Error, util::AlignedVec};
use uuid::Uuid;
use yoke::Yoke;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_library_management(state: &State) -> Element<'_, Message> {
    let permissions = state.permission_checker();

    // Check if user has permission to view libraries
    if !permissions.can_view_library_settings() {
        return container(
            column![
                text("Access Denied")
                    .size(32)
                    .color(theme::MediaServerTheme::ERROR),
                Space::with_height(20),
                text("You don't have permission to view library settings")
                    .size(16)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::with_height(40),
                button("Back to Admin Dashboard")
                    .on_press(Message::HideLibraryManagement)
                    .style(theme::Button::Secondary.style())
                    .padding([12, 20]),
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();
    }

    // If form is open, show the form instead
    if let Some(form_data) = &state.domains.library.state.library_form_data {
        return view_library_form(state, form_data);
    }

    let mut content = column![].spacing(20).padding(20);

    // Build header with conditional buttons
    let mut header_row = row![
        button(
            row![icon_text(Icon::ArrowLeft), text(" Back to Library")]
                .spacing(5)
                .align_y(iced::Alignment::Center)
        )
        .on_press(Message::HideLibraryManagement)
        .style(theme::Button::Secondary.style()),
        Space::with_width(Length::Fill),
        text("Library Management")
            .size(28)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::with_width(Length::Fill),
    ]
    .align_y(iced::Alignment::Center);

    // Add Create Library button only if user has permission
    if permissions.has_permission("libraries:create") {
        header_row = header_row.push(
            button("Create Library")
                .on_press(Message::ShowLibraryForm(None))
                .style(theme::Button::Primary.style()),
        );
        header_row = header_row.push(Space::with_width(10));
    }

    // Add Clear All Data button only if user can reset database
    if permissions.can_reset_database() {
        header_row = header_row.push(
            button("🗑 Clear All Data")
                .on_press(Message::ShowClearDatabaseConfirm)
                .style(theme::Button::Destructive.style()),
        );
    }

    content = content.push(header_row);

    if let Some(success_message) = &state.domains.library.state.library_form_success {
        let success_row = row![
            icon_text(Icon::Check),
            text(success_message)
                .size(16)
                .color(theme::MediaServerTheme::SUCCESS),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);

        let success_card = container(success_row)
            .padding([12, 16])
            .style(theme::Container::SuccessBox.style());

        content = content.push(success_card);
    }

    content = content.push(active_scans_panel(state));

    // Libraries list
    if !state.domains.library.state.repo_accessor.is_initialized() {
        content = content.push(
            container(
                column![
                    text("No Libraries Configured")
                        .size(24)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    Space::with_height(10),
                    text("Create a library to start managing your media collection.")
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_SUBDUED),
                ]
                .spacing(10)
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        );
    } else {
        let libraries_list = scrollable(
            column(
                state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .libraries_index()
                    .expect("Failed to lock repository")
                    .iter()
                    .map(|library_id| {
                        create_library_card(
                            state.domains.ui.state.repo_accessor.clone(),
                            library_id,
                            &permissions,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .spacing(15),
        );

        content = content.push(libraries_list);
    }

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

type LibraryYoke = Yoke<&'static ArchivedLibrary, Arc<AlignedVec>>;

fn create_library_card<'a>(
    repo_accessor: Accessor<ReadOnly>,
    library_id: &Uuid,
    //library: &'a LibraryYoke,
    permissions: &permissions::PermissionChecker,
) -> Element<'a, Message> {
    let library_opt = repo_accessor.get_archived_library_yoke(library_id).unwrap(); // This should be safe but I should handle it anyway

    if let Some(library_yoke) = library_opt {
        let library = *library_yoke.get();

        let library_type_icon = match library.library_type {
            ArchivedLibraryType::Movies => "🎬",
            ArchivedLibraryType::Series => "📺",
        };

        let status_text = if library.enabled {
            text("Enabled").color(theme::MediaServerTheme::SUCCESS)
        } else {
            text("Disabled").color(theme::MediaServerTheme::TEXT_SUBDUED)
        };

        let mut action_buttons = row![].spacing(10);

        // Scan button (only if user has scan permission)
        if permissions.can_scan_libraries() && library.enabled {
            action_buttons = action_buttons.push(
                button("Scan")
                    .on_press(Message::ScanLibrary(LibraryID(library.id.as_uuid())))
                    .style(theme::Button::Secondary.style()),
            );
            // Reset: delete and recreate library with start_scan=true
            action_buttons = action_buttons.push(
                button("Reset Library")
                    .on_press(Message::ResetLibrary(LibraryID(library.id.as_uuid())))
                    .style(theme::Button::Secondary.style()),
            );
        }

        // Edit button (only if user has update permission)
        if permissions.has_permission("libraries:update") {
            action_buttons = action_buttons.push(
                button("Edit")
                    .on_press(Message::ShowLibraryForm(Some(
                        deserialize::<Library, Error>(library)
                            .expect("Failed to deserialize library"),
                    )))
                    .style(theme::Button::Secondary.style()),
            );
        }

        // Delete button (only if user has delete permission)
        if permissions.has_permission("libraries:delete") {
            action_buttons = action_buttons.push(
                button("Delete")
                    .on_press(Message::DeleteLibrary(LibraryID(library.id.as_uuid())))
                    .style(theme::Button::Destructive.style()),
            );
        }

        container(
            row![
                // Library icon and info
                row![
                    text(library_type_icon).size(24),
                    column![
                        row![
                            text(library.name.to_string())
                                .size(18)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            Space::with_width(10),
                            status_text,
                        ]
                        .align_y(iced::Alignment::Center),
                        text(
                            library
                                .paths
                                .first()
                                .expect("Invalid or non existant library path")
                                .to_string()
                        )
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(5),
                ]
                .spacing(15)
                .align_y(iced::Alignment::Center)
                .width(Length::Fill),
                // Action buttons
                action_buttons,
            ]
            .align_y(iced::Alignment::Center)
            .padding(20),
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill)
        .into()
    } else {
        container(
            column![
                text("No Libraries Configured")
                    .size(24)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::with_height(10),
                text("Create a library to start managing your media collection.")
                    .size(16)
                    .color(theme::MediaServerTheme::TEXT_SUBDUED),
            ]
            .spacing(10)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }
}

fn active_scans_panel(state: &State) -> Element<'_, Message> {
    let mut scans: Vec<ScanSnapshotDto> = state
        .domains
        .library
        .state
        .active_scans
        .values()
        .cloned()
        .collect();
    scans.sort_by_key(|snapshot| snapshot.started_at);

    if scans.is_empty() {
        if !state.domains.library.state.latest_progress.is_empty() {
            log::warn!(
                "Active scans map empty but {:} progress frames buffered; scan UI may be out of sync",
                state.domains.library.state.latest_progress.len()
            );
        } else {
            log::debug!("Active scans panel rendered with no active scans");
        }
    } else {
        log::debug!("Rendering active scans panel with {} entries", scans.len());
    }

    let header = row![
        row![
            icon_text(Icon::Activity),
            text("Active Scans")
                .size(20)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
        Space::with_width(Length::Fill),
        text(format!("{} running", scans.len()))
            .size(14)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .align_y(iced::Alignment::Center);

    let mut items = column![header].spacing(12);
    // Metrics panel summary
    if let Some(metrics) = &state.domains.library.state.scan_metrics {
        let q = &metrics.queue_depths;
        let summary = row![
            text(format!(
                "Queue depths — scan:{} analyze:{} metadata:{} index:{} images:{}",
                q.folder_scan, q.analyze, q.metadata, q.index, q.image_fetch
            ))
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::with_width(Length::Fill),
            button("Refresh Metrics")
                .on_press(Message::FetchScanMetrics)
                .style(theme::Button::Secondary.style())
        ]
        .align_y(iced::Alignment::Center);

        items = items.push(
            container(summary)
                .padding([8, 12])
                .style(theme::Container::Default.style()),
        );
    } else {
        items = items.push(
            container(
                row![
                    text("Scanner metrics not loaded")
                        .size(12)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    Space::with_width(Length::Fill),
                    button("Load Metrics")
                        .on_press(Message::FetchScanMetrics)
                        .style(theme::Button::Secondary.style()),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding([8, 12])
            .style(theme::Container::Default.style()),
        );
    }
    if scans.is_empty() {
        items = items.push(
            container(
                row![
                    text("No active scans at the moment")
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    Space::with_width(Length::Fill),
                    button("Start Scan")
                        .on_press(Message::NoOp)
                        .style(theme::Button::Secondary.style()),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding([12, 16])
            .style(theme::Container::Card.style()),
        );
    } else {
        for snapshot in scans {
            let progress = state
                .domains
                .library
                .state
                .latest_progress
                .get(&snapshot.scan_id)
                .cloned();

            let (completed_items, total_items, retrying_items, dead_lettered_items, current_path) =
                if let Some(event) = &progress {
                    (
                        event.completed_items,
                        event.total_items,
                        event.retrying_items.unwrap_or(snapshot.retrying_items),
                        event
                            .dead_lettered_items
                            .unwrap_or(snapshot.dead_lettered_items),
                        event.current_path.clone().or(snapshot.current_path.clone()),
                    )
                } else {
                    (
                        snapshot.completed_items,
                        snapshot.total_items,
                        snapshot.retrying_items,
                        snapshot.dead_lettered_items,
                        snapshot.current_path.clone(),
                    )
                };

            let percent = if total_items > 0 {
                (completed_items as f32 / total_items as f32 * 100.0).round()
            } else {
                0.0
            };

            let status_label = match snapshot.status {
                ScanLifecycleStatus::Pending => {
                    ("Pending", theme::MediaServerTheme::TEXT_SECONDARY)
                }
                ScanLifecycleStatus::Running => ("Running", theme::MediaServerTheme::ACCENT_BLUE),
                ScanLifecycleStatus::Paused => ("Paused", theme::MediaServerTheme::WARNING),
                ScanLifecycleStatus::Completed => ("Completed", theme::MediaServerTheme::SUCCESS),
                ScanLifecycleStatus::Failed => ("Failed", theme::MediaServerTheme::ERROR),
                ScanLifecycleStatus::Canceled => {
                    ("Canceled", theme::MediaServerTheme::TEXT_SECONDARY)
                }
            };

            let library_name = state
                .domains
                .ui
                .state
                .repo_accessor
                .get_archived_library_yoke(&snapshot.library_id.as_uuid())
                .ok()
                .and_then(|opt| opt)
                .map(|yoke| yoke.get().name.to_string())
                .unwrap_or_else(|| snapshot.library_id.to_string());

            let status_badge = container(text(status_label.0).size(13).color(status_label.1))
                .padding([4, 8])
                .style(theme::Container::HeaderAccent.style());

            let path_text = current_path
                .as_deref()
                .map(|path| format!("Current: {}", truncate_path(path)))
                .unwrap_or_else(|| "Awaiting items".to_string());

            let stats_row = row![
                text(format!("{completed_items}/{total_items} items"))
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                Space::with_width(20),
                text(format!("Retries: {retrying_items}"))
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::with_width(20),
                text(format!("Dead-lettered: {dead_lettered_items}"))
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::with_width(20),
                text(path_text)
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .align_y(iced::Alignment::Center);

            let progress_bar = row![
                container(
                    container(Space::with_width(Length::Fixed(percent * 3.0)))
                        .height(6)
                        .style(theme::Container::ProgressBar.style()),
                )
                .width(Length::FillPortion(3))
                .height(6)
                .style(theme::Container::ProgressBarBackground.style()),
                Space::with_width(10),
                text(format!("{percent:.0}%"))
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            ]
            .align_y(iced::Alignment::Center);

            let mut actions = row![].spacing(8);
            match snapshot.status {
                ScanLifecycleStatus::Running => {
                    actions = actions.push(
                        button("Pause")
                            .on_press(Message::PauseLibraryScan(
                                snapshot.library_id,
                                snapshot.scan_id,
                            ))
                            .style(theme::Button::Secondary.style()),
                    );
                    actions = actions.push(
                        button("Cancel")
                            .on_press(Message::CancelLibraryScan(
                                snapshot.library_id,
                                snapshot.scan_id,
                            ))
                            .style(theme::Button::Destructive.style()),
                    );
                }
                ScanLifecycleStatus::Paused => {
                    actions = actions.push(
                        button("Resume")
                            .on_press(Message::ResumeLibraryScan(
                                snapshot.library_id,
                                snapshot.scan_id,
                            ))
                            .style(theme::Button::Primary.style()),
                    );
                    actions = actions.push(
                        button("Cancel")
                            .on_press(Message::CancelLibraryScan(
                                snapshot.library_id,
                                snapshot.scan_id,
                            ))
                            .style(theme::Button::Destructive.style()),
                    );
                }
                ScanLifecycleStatus::Pending => {
                    actions = actions.push(
                        button("Cancel")
                            .on_press(Message::CancelLibraryScan(
                                snapshot.library_id,
                                snapshot.scan_id,
                            ))
                            .style(theme::Button::Destructive.style()),
                    );
                }
                ScanLifecycleStatus::Completed
                | ScanLifecycleStatus::Failed
                | ScanLifecycleStatus::Canceled => {}
            }

            items = items.push(
                container(
                    row![
                        column![
                            row![
                                text(library_name)
                                    .size(16)
                                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                                Space::with_width(Length::Fixed(12.0)),
                                status_badge,
                            ]
                            .align_y(iced::Alignment::Center)
                            .spacing(8),
                            Space::with_height(8),
                            progress_bar,
                            Space::with_height(6),
                            stats_row,
                        ]
                        .spacing(6)
                        .width(Length::Fill),
                        actions,
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .padding(16)
                .style(theme::Container::Card.style()),
            );
        }
    }

    container(items)
        .width(Length::Fill)
        .style(theme::Container::Default.style())
        .into()
}

fn truncate_path(path: &str) -> String {
    const MAX_LEN: usize = 48;
    if path.len() <= MAX_LEN {
        path.to_string()
    } else {
        let tail = &path[path.len() - (MAX_LEN.saturating_sub(3))..];
        format!("…{}", tail)
    }
}
