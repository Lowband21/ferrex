//! Library management view with permission-based controls

use crate::{
    common::ui_utils::icon_text,
    domains::{
        auth::permissions::{self, StatePermissionExt},
        ui::{
            LibraryMaintenanceAction, messages::UiMessage,
            settings_ui::SettingsUiMessage, theme,
            views::admin::view_library_form,
        },
    },
    infra::theme::accent,
    state::State,
};
use ferrex_core::player_prelude::{
    ArchivedLibraryType, Library, LibraryId, ScanLifecycleStatus,
    ScanPathReasonCategory, ScanPathReasonDetail, ScanRunMode, ScanSnapshotDto,
};
#[cfg(feature = "demo")]
use iced::widget::text_input;
use iced::{
    Color, Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};
use lucide_icons::Icon;
use rkyv::{deserialize, rancor::Error};
use uuid::Uuid;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_library_management(state: &State) -> Element<'_, UiMessage> {
    let permissions = state.permission_checker();
    let destructive_action_pending = state
        .domains
        .ui
        .state
        .library_maintenance_confirmation
        .is_some()
        || state
            .domains
            .ui
            .state
            .library_maintenance_in_flight
            .is_some();

    // If form is open, show the form instead
    if let Some(form_data) = &state.domains.library.state.library_form_data {
        return view_library_form(state, form_data);
    }

    let mut content = column![].spacing(20).padding(20);

    // Build header with conditional buttons
    let mut header_row =
        row![Space::new().width(Length::Fill)].align_y(iced::Alignment::Center);

    // Add Create Library button only if user has permission
    if permissions.has_permission("libraries:create") {
        let create_button =
            button("Create Library").style(theme::Button::Primary.style());
        let create_button = if destructive_action_pending {
            create_button
        } else {
            create_button
                .on_press(SettingsUiMessage::ShowLibraryForm(None).into())
        };
        header_row = header_row.push(create_button);
        header_row = header_row.push(Space::new().width(10));
    }

    // Add Clear All Data button only if user can reset database
    if permissions.can_reset_database() {
        let label = if state.domains.ui.state.library_maintenance_in_flight
            == Some(LibraryMaintenanceAction::ClearAllData)
        {
            "Clearing…"
        } else {
            "🗑 Clear All Data"
        };
        let clear_button =
            button(label).style(theme::Button::Destructive.style());
        let clear_button = if destructive_action_pending {
            clear_button
        } else {
            clear_button.on_press(
                SettingsUiMessage::ShowLibraryMaintenanceConfirm(
                    LibraryMaintenanceAction::ClearAllData,
                )
                .into(),
            )
        };
        header_row = header_row.push(clear_button);
    }

    content = content.push(header_row);

    if let Some(success_message) =
        &state.domains.library.state.library_form_success
    {
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

    if !state.domains.library.state.library_form_errors.is_empty() {
        let errors = state
            .domains
            .library
            .state
            .library_form_errors
            .iter()
            .map(|message| {
                text(message)
                    .size(14)
                    .color(theme::MediaServerTheme::ERROR_COLOR)
                    .into()
            })
            .collect::<Vec<Element<'_, UiMessage>>>();
        content = content.push(
            container(column(errors).spacing(6))
                .padding([12, 16])
                .style(theme::Container::ErrorBox.style()),
        );
    }

    content = content.push(scan_status_panel(state));

    #[cfg(feature = "demo")]
    {
        content = content.push(demo_controls_panel(state));
    }

    // Libraries list
    if !state.domains.library.state.repo_accessor.is_initialized() {
        content = content.push(
            container(
                column![
                    text("No Libraries Configured")
                        .size(24)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    Space::new().height(10),
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
                        create_library_card(state, library_id, &permissions)
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

fn create_library_card<'a>(
    state: &'a State,
    library_id: &Uuid,
    permissions: &permissions::PermissionChecker,
) -> Element<'a, UiMessage> {
    let library_opt = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_archived_library_yoke(library_id)
        .unwrap(); // This should be safe but I should handle it anyway

    if let Some(library_yoke) = library_opt {
        let library = *library_yoke.get();
        let action_pending = state
            .domains
            .ui
            .state
            .library_maintenance_confirmation
            .is_some()
            || state
                .domains
                .ui
                .state
                .library_maintenance_in_flight
                .is_some();

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
            let domain_library_id = LibraryId(library.id.as_uuid());
            let active_manual_scan =
                state.domains.library.state.active_scan_by_library_mode(
                    domain_library_id,
                    ScanRunMode::Manual,
                );
            let scan_start_pending = state
                .domains
                .library
                .state
                .is_scan_start_pending(domain_library_id, ScanRunMode::Manual);
            let scan_button_label = if scan_start_pending {
                "Starting…"
            } else if let Some(scan) = active_manual_scan {
                match &scan.status {
                    ScanLifecycleStatus::Pending => "Scan pending",
                    ScanLifecycleStatus::Running => "Scanning…",
                    ScanLifecycleStatus::Paused => "Scan paused",
                    ScanLifecycleStatus::Completed
                    | ScanLifecycleStatus::Failed
                    | ScanLifecycleStatus::Canceled => "Scan",
                }
            } else {
                "Scan"
            };

            let scan_button = button(scan_button_label)
                .style(theme::Button::Secondary.style());
            let scan_button = if active_manual_scan.is_some()
                || scan_start_pending
                || action_pending
            {
                scan_button
            } else {
                scan_button.on_press(
                    SettingsUiMessage::ScanLibrary(domain_library_id).into(),
                )
            };
            action_buttons = action_buttons.push(scan_button);
        }

        // Reset crosses the same destructive and creation authority boundary as
        // its atomic server-side delete/reinsert operation.
        if permissions.can_scan_libraries()
            && permissions.has_permission("libraries:delete")
            && permissions.has_permission("libraries:create")
        {
            let library_id = LibraryId(library.id.as_uuid());
            let label = if state.domains.ui.state.library_maintenance_in_flight
                == Some(LibraryMaintenanceAction::Reset(library_id))
            {
                "Resetting…"
            } else {
                "Reset Library"
            };
            let reset_button =
                button(label).style(theme::Button::Secondary.style());
            let reset_button = if action_pending {
                reset_button
            } else {
                reset_button.on_press(
                    SettingsUiMessage::ShowLibraryMaintenanceConfirm(
                        LibraryMaintenanceAction::Reset(library_id),
                    )
                    .into(),
                )
            };
            action_buttons = action_buttons.push(reset_button);
        }

        // Edit button (only if user has update permission)
        if permissions.has_permission("libraries:update") {
            let edit_button =
                button("Edit").style(theme::Button::Secondary.style());
            let edit_button = if action_pending {
                edit_button
            } else {
                edit_button.on_press(
                    SettingsUiMessage::ShowLibraryForm(Some(
                        deserialize::<Library, Error>(library)
                            .expect("Failed to deserialize library"),
                    ))
                    .into(),
                )
            };
            action_buttons = action_buttons.push(edit_button);
        }

        // Delete button (only if user has delete permission)
        if permissions.has_permission("libraries:delete") {
            let library_id = LibraryId(library.id.as_uuid());
            let label = if state.domains.ui.state.library_maintenance_in_flight
                == Some(LibraryMaintenanceAction::Delete(library_id))
            {
                "Deleting…"
            } else {
                "Delete"
            };
            let delete_button =
                button(label).style(theme::Button::Destructive.style());
            let delete_button = if action_pending {
                delete_button
            } else {
                delete_button.on_press(
                    SettingsUiMessage::ShowLibraryMaintenanceConfirm(
                        LibraryMaintenanceAction::Delete(library_id),
                    )
                    .into(),
                )
            };
            action_buttons = action_buttons.push(delete_button);
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
                            Space::new().width(10),
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
                Space::new().height(10),
                text(
                    "Create a library to start managing your media collection."
                )
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

fn active_scan_panel_snapshots(state: &State) -> Vec<ScanSnapshotDto> {
    let mut scans: Vec<ScanSnapshotDto> = state
        .domains
        .library
        .state
        .active_scans
        .values()
        .cloned()
        .collect();
    scans.sort_by_key(|snapshot| snapshot.started_at);
    scans
}

fn scan_status_panel(state: &State) -> Element<'_, UiMessage> {
    let scans = active_scan_panel_snapshots(state);

    if scans.is_empty() {
        if !state.domains.library.state.latest_progress.is_empty() {
            log::warn!(
                "Active scans map empty but {:} progress frames buffered; scan UI may be out of sync",
                state.domains.library.state.latest_progress.len()
            );
        }
    } else {
        log::trace!(
            "Rendering active scans panel with {} entries",
            scans.len()
        );
    }

    let header = row![
        row![
            icon_text(Icon::Activity),
            text("Scanner Status")
                .size(20)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
        Space::new().width(Length::Fill),
        text(scan_panel_count_label(&scans))
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
            Space::new().width(Length::Fill),
            button("Refresh Metrics")
                .on_press(SettingsUiMessage::FetchScanMetrics.into())
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
                    Space::new().width(Length::Fill),
                    button("Load Metrics")
                        .on_press(SettingsUiMessage::FetchScanMetrics.into())
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
                    Space::new().width(Length::Fill),
                    button("Start Scan")
                        .on_press(UiMessage::NoOp)
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

            let reason_details = progress
                .as_ref()
                .map(|event| event.reason_details.as_slice())
                .filter(|details| !details.is_empty())
                .unwrap_or(snapshot.reason_details.as_slice());

            let (
                completed_items,
                total_items,
                validated_items,
                known_unchanged_items,
                skipped_items,
                needs_attention_items,
                retrying_items,
                current_path,
            ) = if let Some(event) = &progress {
                (
                    event.completed_items,
                    event.total_items,
                    event.validated_items,
                    event.known_unchanged_items,
                    event.skipped_items,
                    event.needs_attention_items,
                    event.retrying_items,
                    event
                        .current_path
                        .clone()
                        .or(snapshot.current_path.clone()),
                )
            } else {
                (
                    snapshot.completed_items,
                    snapshot.total_items,
                    snapshot.validated_items,
                    snapshot.known_unchanged_items,
                    snapshot.skipped_items,
                    snapshot.needs_attention_items,
                    snapshot.retrying_items,
                    snapshot.current_path.clone(),
                )
            };

            let percent = if total_items > 0 {
                (completed_items as f32 / total_items as f32 * 100.0).round()
            } else {
                0.0
            };

            let status_label = scan_status_label(
                &snapshot,
                reason_details,
                completed_items,
                total_items,
                validated_items,
                known_unchanged_items,
                skipped_items,
                needs_attention_items,
                retrying_items,
            );

            let library_name = state
                .domains
                .ui
                .state
                .repo_accessor
                .get_archived_library_yoke(snapshot.library_id.as_uuid())
                .ok()
                .and_then(|opt| opt)
                .map(|yoke| yoke.get().name.to_string())
                .unwrap_or_else(|| snapshot.library_id.to_string());

            let status_badge =
                container(text(status_label.0).size(13).color(status_label.1))
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
                Space::new().width(20),
                text(format!("Validated: {validated_items}"))
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::new().width(20),
                text(format!("Unchanged: {known_unchanged_items}"))
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::new().width(20),
                text(format!("Skipped: {skipped_items}"))
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::new().width(20),
                text(format!("Retrying: {retrying_items}"))
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::new().width(20),
                text(format!("Needs attention: {needs_attention_items}"))
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::new().width(20),
                text(path_text)
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .align_y(iced::Alignment::Center);

            let progress_bar = row![
                container(
                    container(Space::new().width(Length::Fixed(percent * 3.0)))
                        .height(6)
                        .style(theme::Container::ProgressBar.style()),
                )
                .width(Length::FillPortion(3))
                .height(6)
                .style(theme::Container::ProgressBarBackground.style()),
                Space::new().width(10),
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
                            .on_press(
                                SettingsUiMessage::PauseLibraryScan(
                                    snapshot.library_id,
                                    snapshot.scan_id,
                                )
                                .into(),
                            )
                            .style(theme::Button::Secondary.style()),
                    );
                    actions = actions.push(
                        button("Cancel")
                            .on_press(
                                SettingsUiMessage::CancelLibraryScan(
                                    snapshot.library_id,
                                    snapshot.scan_id,
                                )
                                .into(),
                            )
                            .style(theme::Button::Destructive.style()),
                    );
                }
                ScanLifecycleStatus::Paused => {
                    actions = actions.push(
                        button("Resume")
                            .on_press(
                                SettingsUiMessage::ResumeLibraryScan(
                                    snapshot.library_id,
                                    snapshot.scan_id,
                                )
                                .into(),
                            )
                            .style(theme::Button::Primary.style()),
                    );
                    actions = actions.push(
                        button("Cancel")
                            .on_press(
                                SettingsUiMessage::CancelLibraryScan(
                                    snapshot.library_id,
                                    snapshot.scan_id,
                                )
                                .into(),
                            )
                            .style(theme::Button::Destructive.style()),
                    );
                }
                ScanLifecycleStatus::Pending => {
                    actions = actions.push(
                        button("Cancel")
                            .on_press(
                                SettingsUiMessage::CancelLibraryScan(
                                    snapshot.library_id,
                                    snapshot.scan_id,
                                )
                                .into(),
                            )
                            .style(theme::Button::Destructive.style()),
                    );
                }
                ScanLifecycleStatus::Completed
                | ScanLifecycleStatus::Failed
                | ScanLifecycleStatus::Canceled => {}
            }

            if scan_has_rescan_recovery(
                &snapshot,
                reason_details,
                skipped_items,
                needs_attention_items,
            ) {
                actions = actions.push(
                    button("Rescan library")
                        .on_press(
                            SettingsUiMessage::ScanLibrary(snapshot.library_id)
                                .into(),
                        )
                        .style(theme::Button::Primary.style()),
                );
            }

            let mut scan_details = column![
                row![
                    text(library_name)
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    Space::new().width(Length::Fixed(12.0)),
                    status_badge,
                ]
                .align_y(iced::Alignment::Center)
                .spacing(8),
                Space::new().height(8),
                progress_bar,
                Space::new().height(6),
                stats_row,
            ]
            .spacing(6)
            .width(Length::Fill);

            if let Some(copy) = scan_recovery_copy(
                &snapshot,
                reason_details,
                completed_items,
                total_items,
                validated_items,
                known_unchanged_items,
                skipped_items,
                needs_attention_items,
            ) {
                scan_details = scan_details.push(
                    text(copy).size(12).color(theme::MediaServerTheme::WARNING),
                );
            }

            if let Some(reason_details) = reason_details_panel(reason_details) {
                scan_details = scan_details.push(reason_details);
            }

            items = items.push(
                container(
                    row![scan_details, actions]
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

fn scan_panel_count_label(scans: &[ScanSnapshotDto]) -> String {
    let running = scans
        .iter()
        .filter(|scan| {
            matches!(
                scan.status,
                ScanLifecycleStatus::Pending
                    | ScanLifecycleStatus::Running
                    | ScanLifecycleStatus::Paused
            )
        })
        .count();
    let needs_attention = scans
        .iter()
        .filter(|scan| {
            matches!(
                scan.status,
                ScanLifecycleStatus::Completed | ScanLifecycleStatus::Failed
            ) && scan.needs_attention_items.saturating_add(scan.failed_items)
                > 0
        })
        .count();
    let skipped = scans
        .iter()
        .filter(|scan| {
            matches!(
                scan.status,
                ScanLifecycleStatus::Completed | ScanLifecycleStatus::Failed
            ) && scan.needs_attention_items == 0
                && scan.failed_items == 0
                && scan.skipped_items > 0
        })
        .count();

    if needs_attention > 0 {
        format!("{running} running • {needs_attention} needs attention")
    } else if skipped > 0 {
        format!("{running} running • {skipped} skipped")
    } else {
        format!("{running} running")
    }
}

fn scan_status_label(
    snapshot: &ScanSnapshotDto,
    reason_details: &[ScanPathReasonDetail],
    completed_items: u64,
    total_items: u64,
    validated_items: u64,
    known_unchanged_items: u64,
    skipped_items: u64,
    needs_attention_items: u64,
    retrying_items: u64,
) -> (&'static str, Color) {
    if matches!(snapshot.status, ScanLifecycleStatus::Failed)
        && needs_attention_items > 0
    {
        return ("Needs attention", theme::MediaServerTheme::ERROR);
    }
    if matches!(snapshot.status, ScanLifecycleStatus::Completed)
        && scan_is_whole_library_no_media(
            reason_details,
            completed_items,
            total_items,
            validated_items,
            known_unchanged_items,
            skipped_items,
        )
    {
        return ("No media found", theme::MediaServerTheme::WARNING);
    }
    if matches!(snapshot.status, ScanLifecycleStatus::Failed)
        && scan_is_whole_library_skipped(
            completed_items,
            total_items,
            validated_items,
            known_unchanged_items,
            skipped_items,
        )
    {
        return ("Skipped", theme::MediaServerTheme::WARNING);
    }
    if retrying_items > 0
        && matches!(snapshot.status, ScanLifecycleStatus::Running)
    {
        return ("Retrying", accent());
    }

    match snapshot.status {
        ScanLifecycleStatus::Pending => {
            ("Pending", theme::MediaServerTheme::TEXT_SECONDARY)
        }
        ScanLifecycleStatus::Running => ("Running", accent()),
        ScanLifecycleStatus::Paused => {
            ("Paused", theme::MediaServerTheme::WARNING)
        }
        ScanLifecycleStatus::Completed => {
            ("Completed", theme::MediaServerTheme::SUCCESS)
        }
        ScanLifecycleStatus::Failed => {
            ("Failed", theme::MediaServerTheme::ERROR)
        }
        ScanLifecycleStatus::Canceled => {
            ("Canceled", theme::MediaServerTheme::TEXT_SECONDARY)
        }
    }
}

fn scan_has_rescan_recovery(
    snapshot: &ScanSnapshotDto,
    reason_details: &[ScanPathReasonDetail],
    skipped_items: u64,
    needs_attention_items: u64,
) -> bool {
    matches!(
        snapshot.status,
        ScanLifecycleStatus::Completed | ScanLifecycleStatus::Failed
    ) && (needs_attention_items > 0
        || reason_details.iter().any(reason_detail_needs_rescan)
        || (matches!(snapshot.status, ScanLifecycleStatus::Failed)
            && skipped_items > 0)
        || (skipped_items > 0 && scan_has_no_media_found(reason_details)))
}

fn scan_recovery_copy(
    snapshot: &ScanSnapshotDto,
    reason_details: &[ScanPathReasonDetail],
    completed_items: u64,
    total_items: u64,
    validated_items: u64,
    known_unchanged_items: u64,
    skipped_items: u64,
    needs_attention_items: u64,
) -> Option<&'static str> {
    if !scan_has_rescan_recovery(
        snapshot,
        reason_details,
        skipped_items,
        needs_attention_items,
    ) {
        return None;
    }

    if needs_attention_items > 0 {
        Some(
            "Review the paths below, then rescan this library. Per-path retry is not available yet.",
        )
    } else if matches!(snapshot.status, ScanLifecycleStatus::Failed)
        && completed_items != total_items
    {
        Some(
            "The scan stopped before all paths finished. Rescan this library to try again.",
        )
    } else if matches!(snapshot.status, ScanLifecycleStatus::Completed)
        && scan_is_whole_library_no_media(
            reason_details,
            completed_items,
            total_items,
            validated_items,
            known_unchanged_items,
            skipped_items,
        )
    {
        Some(
            "Add supported media or update the folder, then rescan this library.",
        )
    } else {
        Some("Skipped paths can be checked again with a library rescan.")
    }
}

fn reason_details_panel(
    reason_details: &[ScanPathReasonDetail],
) -> Option<Element<'static, UiMessage>> {
    if reason_details.is_empty() {
        return None;
    }

    let mut details = column![].spacing(4);
    for detail in reason_details.iter().take(3) {
        details = details.push(
            text(reason_detail_copy(detail))
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    if reason_details.len() > 3 {
        details = details.push(
            text(format!("+{} more path notes", reason_details.len() - 3))
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
        );
    }

    Some(
        container(details)
            .padding([8, 12])
            .style(theme::Container::Default.style())
            .into(),
    )
}

fn reason_detail_copy(detail: &ScanPathReasonDetail) -> String {
    let label = reason_detail_label(detail);
    let message = detail
        .message
        .as_deref()
        .filter(|message| !contains_dead_letter_term(message))
        .unwrap_or_else(|| fallback_reason_message(&detail.reason_code));

    if let Some(path) = &detail.path {
        format!("{label}: {message} — {}", truncate_path(path))
    } else {
        format!("{label}: {message}")
    }
}

fn reason_detail_label(detail: &ScanPathReasonDetail) -> &'static str {
    match detail.category {
        ScanPathReasonCategory::KnownUnchanged => "Already scanned",
        ScanPathReasonCategory::Skipped => {
            if detail.reason_code == "no_supported_media_found" {
                "No media found"
            } else {
                "Skipped"
            }
        }
        ScanPathReasonCategory::Retrying => "Retrying",
        ScanPathReasonCategory::NeedsAttention => "Needs attention",
    }
}

fn fallback_reason_message(reason_code: &str) -> &'static str {
    match reason_code {
        "unchanged_since_last_scan" => {
            "Already up to date from a previous scan"
        }
        "path_missing" => "The path was not available during the scan",
        "no_supported_media_found" => {
            "No supported media files were found at this path"
        }
        "unsupported_media_layout" => {
            "This path does not contain a supported media layout"
        }
        "temporary_scan_issue" => "A temporary scan issue is being retried",
        _ => "Review this path and rescan when it is ready",
    }
}

fn contains_dead_letter_term(text: &str) -> bool {
    let compact: String = text
        .chars()
        .filter_map(|ch| {
            let lower = ch.to_ascii_lowercase();
            lower.is_ascii_alphanumeric().then_some(lower)
        })
        .collect();
    compact.contains("deadletter")
}

fn reason_detail_needs_rescan(detail: &ScanPathReasonDetail) -> bool {
    match detail.category {
        ScanPathReasonCategory::NeedsAttention => true,
        ScanPathReasonCategory::Skipped => matches!(
            detail.reason_code.as_str(),
            "path_missing"
                | "no_supported_media_found"
                | "unsupported_media_layout"
                | "skipped"
        ),
        ScanPathReasonCategory::KnownUnchanged
        | ScanPathReasonCategory::Retrying => false,
    }
}

fn scan_has_no_media_found(reason_details: &[ScanPathReasonDetail]) -> bool {
    reason_details
        .iter()
        .any(|detail| detail.reason_code == "no_supported_media_found")
}

fn scan_is_whole_library_no_media(
    reason_details: &[ScanPathReasonDetail],
    completed_items: u64,
    total_items: u64,
    validated_items: u64,
    known_unchanged_items: u64,
    skipped_items: u64,
) -> bool {
    scan_is_whole_library_skipped(
        completed_items,
        total_items,
        validated_items,
        known_unchanged_items,
        skipped_items,
    ) && !reason_details.is_empty()
        && reason_details.iter().all(|detail| {
            detail.category == ScanPathReasonCategory::Skipped
                && detail.reason_code == "no_supported_media_found"
        })
}

fn scan_is_whole_library_skipped(
    completed_items: u64,
    total_items: u64,
    validated_items: u64,
    known_unchanged_items: u64,
    skipped_items: u64,
) -> bool {
    total_items > 0
        && completed_items == total_items
        && skipped_items == total_items
        && validated_items == 0
        && known_unchanged_items == 0
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    fn fixed_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0)
            .expect("valid fixed timestamp")
    }

    fn reason_detail(
        category: ScanPathReasonCategory,
        reason_code: &str,
        message: Option<&str>,
    ) -> ScanPathReasonDetail {
        ScanPathReasonDetail {
            category,
            reason_code: reason_code.into(),
            message: message.map(str::to_string),
            path: Some("/media/library/missing/movie.mkv".into()),
            path_key: None,
            retryable: false,
            action_hint: Some("rescan_library".into()),
        }
    }

    fn snapshot(status: ScanLifecycleStatus) -> ScanSnapshotDto {
        let scan_id = Uuid::now_v7();
        let library_id = LibraryId::new();
        ScanSnapshotDto {
            scan_id,
            library_id,
            status,
            mode: ScanRunMode::Manual,
            completed_items: 0,
            total_items: 1,
            validated_items: 0,
            known_unchanged_items: 0,
            skipped_items: 0,
            failed_items: 0,
            needs_attention_items: 0,
            retrying_items: 0,
            correlation_id: scan_id,
            idempotency_key: "scan:test:1".into(),
            run_key: ScanRunMode::Manual.run_key(library_id),
            disposition: None,
            current_path: None,
            started_at: fixed_time(),
            terminal_at: None,
            sequence: 1,
            reason_details: Vec::new(),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn admin_active_panel_has_one_card_for_one_library_mode() {
        let mut state = State::new("http://localhost:3000".to_string());
        let scan = snapshot(ScanLifecycleStatus::Running);
        let library_id = scan.library_id;
        let scan_id = scan.scan_id;

        state
            .domains
            .library
            .state
            .active_scans
            .insert(scan_id, scan);

        let cards = active_scan_panel_snapshots(&state);

        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].library_id, library_id);
        assert_eq!(cards[0].mode, ScanRunMode::Manual);
        assert_eq!(cards[0].run_key, ScanRunMode::Manual.run_key(library_id));
    }

    #[test]
    fn reason_copy_uses_safe_attention_terms() {
        let detail = reason_detail(
            ScanPathReasonCategory::NeedsAttention,
            "needs_attention",
            None,
        );

        assert_eq!(reason_detail_label(&detail), "Needs attention");
        let copy = reason_detail_copy(&detail);
        assert!(copy.contains("Needs attention"));
        assert!(copy.contains("Review this path and rescan"));
        assert!(!copy.contains("dead_letter"));
        assert!(!copy.contains("deadletter"));
    }

    #[test]
    fn reason_copy_sanitizes_backend_dead_letter_terms() {
        let detail = reason_detail(
            ScanPathReasonCategory::NeedsAttention,
            "deadletter_queue",
            Some("dead-letter queue entry needs operator review"),
        );

        let copy = reason_detail_copy(&detail);
        let normalized = copy.to_ascii_lowercase();
        assert!(copy.contains("Needs attention"));
        assert!(copy.contains("Review this path and rescan"));
        assert!(!normalized.contains("dead_letter"));
        assert!(!normalized.contains("dead-letter"));
        assert!(!normalized.contains("deadletter"));
        assert!(!normalized.contains("dead letter"));
    }

    #[test]
    fn no_media_found_reason_gets_label_and_rescan_copy() {
        let mut scan = snapshot(ScanLifecycleStatus::Completed);
        scan.completed_items = 1;
        scan.total_items = 1;
        scan.skipped_items = 1;
        let details = vec![reason_detail(
            ScanPathReasonCategory::Skipped,
            "no_supported_media_found",
            None,
        )];

        assert_eq!(reason_detail_label(&details[0]), "No media found");
        assert_eq!(
            scan_status_label(&scan, &details, 1, 1, 0, 0, 1, 0, 0).0,
            "No media found"
        );
        assert!(scan_has_rescan_recovery(&scan, &details, 1, 0));
        assert_eq!(
            scan_recovery_copy(&scan, &details, 1, 1, 0, 0, 1, 0),
            Some(
                "Add supported media or update the folder, then rescan this library."
            )
        );
    }

    #[test]
    fn mixed_success_no_media_reason_preserves_terminal_status() {
        let details = vec![reason_detail(
            ScanPathReasonCategory::Skipped,
            "no_supported_media_found",
            None,
        )];

        for (validated_items, known_unchanged_items) in [(1, 0), (0, 1)] {
            for (status, expected_label) in [
                (ScanLifecycleStatus::Completed, "Completed"),
                (ScanLifecycleStatus::Failed, "Failed"),
            ] {
                let mut scan = snapshot(status);
                scan.completed_items =
                    validated_items + known_unchanged_items + 1;
                scan.total_items = scan.completed_items;
                scan.validated_items = validated_items;
                scan.known_unchanged_items = known_unchanged_items;
                scan.skipped_items = 1;

                assert_eq!(
                    scan_status_label(
                        &scan,
                        &details,
                        scan.completed_items,
                        scan.total_items,
                        validated_items,
                        known_unchanged_items,
                        1,
                        0,
                        0,
                    )
                    .0,
                    expected_label
                );
                assert!(scan_has_rescan_recovery(&scan, &details, 1, 0));
                assert_eq!(
                    scan_recovery_copy(
                        &scan,
                        &details,
                        scan.completed_items,
                        scan.total_items,
                        validated_items,
                        known_unchanged_items,
                        1,
                        0,
                    ),
                    Some(
                        "Skipped paths can be checked again with a library rescan."
                    )
                );
            }
        }
    }

    #[test]
    fn incomplete_failed_scan_with_empty_path_preserves_failure() {
        let mut scan = snapshot(ScanLifecycleStatus::Failed);
        scan.completed_items = 1;
        scan.total_items = 3_131;
        scan.skipped_items = 1;
        let details = vec![reason_detail(
            ScanPathReasonCategory::Skipped,
            "no_supported_media_found",
            None,
        )];

        assert_eq!(
            scan_status_label(&scan, &details, 1, 3_131, 0, 0, 1, 0, 0).0,
            "Failed"
        );
        assert_eq!(
            scan_recovery_copy(&scan, &details, 1, 3_131, 0, 0, 1, 0),
            Some(
                "The scan stopped before all paths finished. Rescan this library to try again."
            )
        );
    }

    #[test]
    fn mixed_skip_reasons_are_not_whole_library_no_media() {
        let mut scan = snapshot(ScanLifecycleStatus::Completed);
        scan.completed_items = 2;
        scan.total_items = 2;
        scan.skipped_items = 2;
        let details = vec![
            reason_detail(
                ScanPathReasonCategory::Skipped,
                "no_supported_media_found",
                None,
            ),
            reason_detail(
                ScanPathReasonCategory::Skipped,
                "path_missing",
                None,
            ),
        ];

        assert_eq!(
            scan_status_label(&scan, &details, 2, 2, 0, 0, 2, 0, 0).0,
            "Completed"
        );
        assert_eq!(
            scan_recovery_copy(&scan, &details, 2, 2, 0, 0, 2, 0),
            Some("Skipped paths can be checked again with a library rescan.")
        );
    }

    #[test]
    fn active_scan_with_early_empty_folder_preserves_active_status() {
        let details = vec![reason_detail(
            ScanPathReasonCategory::Skipped,
            "no_supported_media_found",
            None,
        )];

        for (status, expected_label) in [
            (ScanLifecycleStatus::Pending, "Pending"),
            (ScanLifecycleStatus::Running, "Running"),
        ] {
            let mut scan = snapshot(status);
            scan.completed_items = 1;
            scan.total_items = 3_131;
            scan.skipped_items = 1;

            assert_eq!(
                scan_status_label(&scan, &details, 1, 3_131, 0, 0, 1, 0, 0,).0,
                expected_label
            );
            assert_eq!(
                scan_recovery_copy(&scan, &details, 1, 3_131, 0, 0, 1, 0,),
                None
            );
        }
    }

    #[test]
    fn panel_count_reports_attention_instead_of_running_only() {
        let mut running = snapshot(ScanLifecycleStatus::Running);
        let mut failed = snapshot(ScanLifecycleStatus::Failed);
        failed.needs_attention_items = 1;
        failed.failed_items = 1;

        assert_eq!(
            scan_panel_count_label(&[running.clone(), failed]),
            "1 running • 1 needs attention"
        );

        running.retrying_items = 1;
        assert_eq!(
            scan_status_label(
                &running,
                &[],
                running.completed_items,
                running.total_items,
                running.validated_items,
                running.known_unchanged_items,
                running.skipped_items,
                running.needs_attention_items,
                running.retrying_items,
            )
            .0,
            "Retrying"
        );
    }
}

#[cfg(feature = "demo")]
fn demo_controls_panel(state: &State) -> Element<'_, UiMessage> {
    let controls = &state.domains.library.state.demo_controls;

    let header = row![
        icon_text(Icon::Sparkles),
        text("Demo Controls")
            .size(20)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    let inputs = row![
        column![
            text("Movies")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text_input("Desired movie count", &controls.movies_input)
                .on_input(SettingsUiMessage::DemoMoviesTargetChanged)
                .padding(8)
                .size(16),
            text(
                controls
                    .movies_current
                    .map(|count| format!("Current: {count}"))
                    .unwrap_or_else(|| "Current: –".into()),
            )
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SUBDUED),
        ]
        .spacing(6)
        .width(Length::FillPortion(1)),
        column![
            text("Series")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text_input("Desired series count", &controls.series_input)
                .on_input(SettingsUiMessage::DemoSeriesTargetChanged)
                .padding(8)
                .size(16),
            text(
                controls
                    .series_current
                    .map(|count| format!("Current: {count}"))
                    .unwrap_or_else(|| "Current: –".into()),
            )
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SUBDUED),
        ]
        .spacing(6)
        .width(Length::FillPortion(1)),
    ]
    .spacing(16);

    let mut actions = row![].spacing(12);

    let apply_label = if controls.is_updating {
        "Applying…"
    } else {
        "Apply Size"
    };

    let apply_button = if controls.is_updating {
        button(apply_label)
            .style(theme::Button::Secondary.style())
            .padding([10, 20])
    } else {
        button(apply_label)
            .on_press(SettingsUiMessage::DemoApplySizing)
            .style(theme::Button::Primary.style())
            .padding([10, 20])
    };

    actions = actions.push(apply_button);

    let refresh_label = if controls.is_loading {
        "Refreshing…"
    } else {
        "Refresh Status"
    };

    let refresh_button = if controls.is_loading {
        button(refresh_label)
            .style(theme::Button::Secondary.style())
            .padding([10, 20])
    } else {
        button(refresh_label)
            .on_press(SettingsUiMessage::DemoRefreshStatus)
            .style(theme::Button::Secondary.style())
            .padding([10, 20])
    };

    actions = actions.push(refresh_button);

    let info = column![
        text(
            controls
                .demo_root
                .as_ref()
                .map(|root| format!("Media root: {}", root.display()))
                .unwrap_or_else(|| "Media root: –".into()),
        )
        .size(12)
        .color(theme::MediaServerTheme::TEXT_SUBDUED),
        text(
            controls
                .demo_username
                .as_ref()
                .map(|user| format!("Demo account: {}", user))
                .unwrap_or_else(|| "Demo account: –".into()),
        )
        .size(12)
        .color(theme::MediaServerTheme::TEXT_SUBDUED),
        text(format!(
            "Registered demo libraries: {}",
            controls.demo_library_ids.len()
        ))
        .size(12)
        .color(theme::MediaServerTheme::TEXT_SUBDUED),
    ]
    .spacing(4);

    let mut content = column![header, inputs, actions, info]
        .spacing(16)
        .padding(20);

    if let Some(error) = &controls.error {
        let error_row = container(
            row![
                icon_text(Icon::OctagonAlert),
                text(error).size(14).color(theme::MediaServerTheme::ERROR)
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .padding([10, 14])
        .style(theme::Container::ErrorBox.style());
        content = content.push(error_row);
    }

    let element: Element<'_, SettingsUiMessage> = container(content)
        .width(Length::Fill)
        .style(theme::Container::Card.style())
        .into();

    element.map(UiMessage::Settings)
}
