//! Library management view with permission-based controls

use crate::{
    common::ui_utils::icon_text,
    domains::{
        auth::permissions::{self, StatePermissionExt},
        ui::{
            messages::UiMessage, settings_ui::SettingsUiMessage, theme,
            views::admin::view_library_form,
        },
    },
    infra::theme::accent,
    state::State,
};
use chrono::{DateTime, Utc};
use ferrex_core::player_prelude::{
    ArchivedLibraryType, Library, LibraryId, ScanFailureDto,
    ScanLifecycleStatus, ScanRecoveryRequest, ScanRunDto, ScanRunEventDto,
    ScanRunMode, ScannerHealthResponse,
};
use ferrex_player_library::scan_dashboard::{
    ScanDashboardLoadState, scan_failure_display_text, scan_status_display_text,
};
#[cfg(feature = "demo")]
use iced::widget::text_input;
use iced::{
    Color, Element, Length,
    widget::{
        Space, button, column, container, progress_bar, row, scrollable, text,
    },
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
        header_row = header_row.push(
            button("Create Library")
                .on_press(SettingsUiMessage::ShowLibraryForm(None).into())
                .style(theme::Button::Primary.style()),
        );
        header_row = header_row.push(Space::new().width(10));
    }

    // Add Clear All Data button only if user can reset database
    if permissions.can_reset_database() {
        header_row = header_row.push(
            button("🗑 Clear All Data")
                .on_press(SettingsUiMessage::ShowClearDatabaseConfirm.into())
                .style(theme::Button::Destructive.style()),
        );
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
            {
                scan_button
            } else {
                scan_button.on_press(
                    SettingsUiMessage::ScanLibrary(domain_library_id).into(),
                )
            };
            action_buttons = action_buttons.push(scan_button);
            // Reset: delete and recreate library with start_scan=true
            action_buttons = action_buttons.push(
                button("Reset Library")
                    .on_press(
                        SettingsUiMessage::ResetLibrary(LibraryId(
                            library.id.as_uuid(),
                        ))
                        .into(),
                    )
                    .style(theme::Button::Secondary.style()),
            );
        }

        // Edit button (only if user has update permission)
        if permissions.has_permission("libraries:update") {
            action_buttons = action_buttons.push(
                button("Edit")
                    .on_press(
                        SettingsUiMessage::ShowLibraryForm(Some(
                            deserialize::<Library, Error>(library)
                                .expect("Failed to deserialize library"),
                        ))
                        .into(),
                    )
                    .style(theme::Button::Secondary.style()),
            );
        }

        // Delete button (only if user has delete permission)
        if permissions.has_permission("libraries:delete") {
            action_buttons = action_buttons.push(
                button("Delete")
                    .on_press(
                        SettingsUiMessage::DeleteLibrary(LibraryId(
                            library.id.as_uuid(),
                        ))
                        .into(),
                    )
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

fn scan_status_panel(state: &State) -> Element<'_, UiMessage> {
    let dashboard = &state.domains.library.state.scan_dashboard;
    let permissions = state.permission_checker();
    let can_scan_libraries = permissions.can_scan_libraries();

    let refresh_label = match dashboard.overview_state {
        ScanDashboardLoadState::Loading => "Refreshing…",
        _ => "Refresh diagnostics",
    };

    let header = row![
        row![
            icon_text(Icon::Activity),
            column![
                text("Scanner diagnostics")
                    .size(20)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text("Health, progress, history, and safe recovery controls")
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .spacing(2),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
        Space::new().width(Length::Fill),
        button(refresh_label)
            .on_press(SettingsUiMessage::RefreshScanDashboard.into())
            .style(theme::Button::Secondary.style()),
    ]
    .align_y(iced::Alignment::Center);

    let mut items = column![header].spacing(14);

    match &dashboard.overview_state {
        ScanDashboardLoadState::Idle => {
            items = items.push(diagnostics_notice_card(
                Icon::CircleGauge,
                "Diagnostics have not loaded yet",
                "Load scanner diagnostics to see queue depth, watcher health, recent scans, and recovery options.",
                Some((
                    "Load diagnostics",
                    SettingsUiMessage::RefreshScanDashboard,
                )),
            ));
        }
        ScanDashboardLoadState::Loading if dashboard.health.is_none() => {
            items = items.push(diagnostics_notice_card(
                Icon::RefreshCw,
                "Loading scanner diagnostics…",
                "Fetching scanner health, active scans, and recent scan history.",
                None,
            ));
        }
        ScanDashboardLoadState::Failed { error }
            if dashboard.health.is_none() =>
        {
            items = items.push(diagnostics_error_card(
                "Scanner diagnostics could not be loaded",
                error,
            ));
        }
        ScanDashboardLoadState::Failed { error } => {
            items = items
                .push(diagnostics_error_card("Latest refresh failed", error));
        }
        ScanDashboardLoadState::Loading | ScanDashboardLoadState::Loaded => {}
    }

    if let Some(health) = &dashboard.health {
        items = items.push(scanner_health_card(health));
    }

    items = items.push(active_and_latest_runs_panel(
        &dashboard.active_runs,
        dashboard.recent_runs.first(),
        can_scan_libraries,
    ));

    items = items.push(recent_run_history_panel(
        &dashboard.recent_runs,
        dashboard.runs_page.as_ref(),
        dashboard.selected_run_id,
    ));

    if dashboard.selected_run_id.is_some() {
        items =
            items.push(selected_run_detail_panel(state, can_scan_libraries));
    } else if !dashboard.recent_runs.is_empty() {
        items = items.push(diagnostics_notice_card(
            Icon::ListTree,
            "Select a run for timeline details",
            "Use Details on any scan run to view ordered events, terminal summary, failure summaries, and copyable IDs.",
            None,
        ));
    }

    container(items)
        .width(Length::Fill)
        .style(theme::Container::Default.style())
        .into()
}

fn diagnostics_notice_card(
    icon: Icon,
    title: &'static str,
    message: &'static str,
    action: Option<(&'static str, SettingsUiMessage)>,
) -> Element<'static, UiMessage> {
    let mut content = row![
        icon_text(icon),
        column![
            text(title)
                .size(15)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(message)
                .size(13)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(4)
        .width(Length::Fill),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);

    if let Some((label, message)) = action {
        content = content.push(
            button(label)
                .on_press(message.into())
                .style(theme::Button::Secondary.style()),
        );
    }

    container(content)
        .padding([12, 16])
        .style(theme::Container::Card.style())
        .width(Length::Fill)
        .into()
}

fn diagnostics_error_card(
    title: &'static str,
    error: &str,
) -> Element<'static, UiMessage> {
    container(
        row![
            icon_text(Icon::OctagonAlert),
            column![
                text(title).size(15).color(theme::MediaServerTheme::ERROR),
                text(error.to_string())
                    .size(13)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .spacing(4)
            .width(Length::Fill),
            button("Retry")
                .on_press(SettingsUiMessage::RefreshScanDashboard.into())
                .style(theme::Button::Secondary.style()),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding([12, 16])
    .style(theme::Container::ErrorBox.style())
    .width(Length::Fill)
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HealthSeverity {
    Healthy,
    Active,
    NeedsAttention,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScannerHealthCopy {
    label: &'static str,
    message: String,
    severity: HealthSeverity,
}

fn scanner_health_copy(health: &ScannerHealthResponse) -> ScannerHealthCopy {
    let incremental = &health.incremental;
    let queue_total = queue_depth_total(health);
    let watcher_needs_attention = incremental.watcher_error_count > 0
        || incremental.overflow_events > 0
        || incremental.stale_cursor_libraries > 0
        || incremental.stale_cursors > 0
        || incremental.last_watcher_error.is_some();
    let watcher_shortfall = incremental.watch_enabled_libraries > 0
        && incremental
            .active_watch_libraries
            .saturating_add(incremental.initializing_watch_libraries)
            < incremental.watch_enabled_libraries;

    if health.failed_runs > 0 || watcher_needs_attention || watcher_shortfall {
        ScannerHealthCopy {
            label: "Needs attention",
            message: "Scanner history or watcher health has items that may need operator review. No media or library data is changed from this panel."
                .to_string(),
            severity: HealthSeverity::NeedsAttention,
        }
    } else if health.active_scans > 0
        || queue_total > 0
        || incremental.replay_pending_events > 0
    {
        ScannerHealthCopy {
            label: "Active",
            message: "Scanner work is in progress or queued.".to_string(),
            severity: HealthSeverity::Active,
        }
    } else {
        ScannerHealthCopy {
            label: "Healthy",
            message: "Scanner queues are clear and watchers report healthy."
                .to_string(),
            severity: HealthSeverity::Healthy,
        }
    }
}

fn scanner_health_card(
    health: &ScannerHealthResponse,
) -> Element<'_, UiMessage> {
    let copy = scanner_health_copy(health);
    let status_color = health_severity_color(copy.severity);
    let incremental = &health.incremental;

    let summary = row![
        metric_chip("Scanner health", copy.label, status_color),
        metric_chip("Active scans", health.active_scans.to_string(), accent(),),
        metric_chip(
            "Recent runs retained",
            health.retained_runs.to_string(),
            theme::MediaServerTheme::TEXT_PRIMARY,
        ),
        metric_chip(
            "Failed after retries",
            health.failed_runs.to_string(),
            if health.failed_runs > 0 {
                theme::MediaServerTheme::WARNING
            } else {
                theme::MediaServerTheme::TEXT_PRIMARY
            },
        ),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);

    let queues = row![
        metric_chip(
            "Folder queue",
            health.queue_depths.folder_scan.to_string(),
            queue_depth_color(health.queue_depths.folder_scan),
        ),
        metric_chip(
            "Analyze",
            health.queue_depths.analyze.to_string(),
            queue_depth_color(health.queue_depths.analyze),
        ),
        metric_chip(
            "Metadata",
            health.queue_depths.metadata.to_string(),
            queue_depth_color(health.queue_depths.metadata),
        ),
        metric_chip(
            "Index",
            health.queue_depths.index.to_string(),
            queue_depth_color(health.queue_depths.index),
        ),
        metric_chip(
            "Images",
            health.queue_depths.image_fetch.to_string(),
            queue_depth_color(health.queue_depths.image_fetch),
        ),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    let watch_status = watcher_status_label(health);
    let cursor_status = cursor_status_label(health);

    let mut watch_details = column![
        text(copy.message)
            .size(13)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        row![
            metric_chip(
                "Watch health",
                watch_status,
                if scanner_health_copy(health).severity
                    == HealthSeverity::NeedsAttention
                {
                    theme::MediaServerTheme::WARNING
                } else {
                    theme::MediaServerTheme::SUCCESS
                },
            ),
            metric_chip(
                "Watch libraries",
                format!(
                    "{}/{} active",
                    incremental.active_watch_libraries,
                    incremental.watch_enabled_libraries
                ),
                theme::MediaServerTheme::TEXT_PRIMARY,
            ),
            metric_chip(
                "Watch roots",
                format!(
                    "{}/{} active",
                    incremental.active_watch_roots,
                    incremental.registered_watch_roots
                ),
                theme::MediaServerTheme::TEXT_PRIMARY,
            ),
            metric_chip(
                "Cursor health",
                cursor_status,
                if incremental.stale_cursors > 0
                    || incremental.replay_pending_events > 0
                {
                    theme::MediaServerTheme::WARNING
                } else {
                    theme::MediaServerTheme::SUCCESS
                },
            ),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(10);

    if let Some(error) = &incremental.last_watcher_error {
        watch_details = watch_details.push(
            text(format!("Last watcher issue: {}", truncate_path(error)))
                .size(12)
                .color(theme::MediaServerTheme::WARNING),
        );
    }

    container(
        column![
            row![
                icon_text(Icon::HeartPulse),
                text("Scanner health")
                    .size(18)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
            summary,
            text("Queue depths")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            queues,
            watch_details,
        ]
        .spacing(14),
    )
    .padding(16)
    .style(theme::Container::Card.style())
    .width(Length::Fill)
    .into()
}

fn metric_chip<'a>(
    label: &'a str,
    value: impl Into<String>,
    color: Color,
) -> Element<'a, UiMessage> {
    container(
        column![
            text(label)
                .size(11)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
            text(value.into()).size(16).color(color),
        ]
        .spacing(2),
    )
    .padding([8, 10])
    .style(theme::Container::HeaderAccent.style())
    .into()
}

fn active_and_latest_runs_panel<'a>(
    active_runs: &'a [ScanRunDto],
    latest_run: Option<&'a ScanRunDto>,
    can_scan_libraries: bool,
) -> Element<'a, UiMessage> {
    let mut content = column![section_heading(
        Icon::Radar,
        "Active / latest run",
        if active_runs.is_empty() {
            "No active scan is running right now."
        } else {
            "Live scanner progress and safe controls."
        },
    )]
    .spacing(12);

    if active_runs.is_empty() {
        if let Some(run) = latest_run {
            content = content.push(scan_run_card(
                run,
                RunCardMode::Latest,
                can_scan_libraries,
                false,
            ));
        } else {
            content = content.push(
                container(
                    column![
                        text("No scan history yet")
                            .size(15)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        text("Start a scan from a library card below to populate diagnostics.")
                            .size(13)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(6),
                )
                .padding([12, 16])
                .style(theme::Container::Card.style()),
            );
        }
    } else {
        let mut runs: Vec<&ScanRunDto> = active_runs.iter().collect();
        runs.sort_by(|a, b| b.last_event_at.cmp(&a.last_event_at));
        for run in runs.into_iter().take(4) {
            content = content.push(scan_run_card(
                run,
                RunCardMode::Active,
                can_scan_libraries,
                false,
            ));
        }
    }

    container(content)
        .padding(16)
        .style(theme::Container::Card.style())
        .width(Length::Fill)
        .into()
}

fn recent_run_history_panel<'a>(
    recent_runs: &'a [ScanRunDto],
    page: Option<&ferrex_core::player_prelude::ScanPageMeta>,
    selected_run_id: Option<Uuid>,
) -> Element<'a, UiMessage> {
    let page_label = page
        .map(|page| {
            format!("Showing {} of {} retained runs", page.count, page.total)
        })
        .unwrap_or_else(|| "Recent durable run history".to_string());

    let mut content = column![section_heading(
        Icon::History,
        "Recent scan history",
        &page_label,
    )]
    .spacing(12);

    if recent_runs.is_empty() {
        content = content.push(
            container(
                column![
                    text("No recent scan runs")
                        .size(15)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    text("Completed and failed scan runs will appear here once scanner history is available.")
                        .size(13)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                ]
                .spacing(6),
            )
            .padding([12, 16])
            .style(theme::Container::Card.style()),
        );
    } else {
        for run in recent_runs.iter().take(8) {
            content = content.push(scan_run_card(
                run,
                RunCardMode::History,
                false,
                selected_run_id == Some(run.scan_id),
            ));
        }
    }

    container(content)
        .padding(16)
        .style(theme::Container::Card.style())
        .width(Length::Fill)
        .into()
}

fn selected_run_detail_panel(
    state: &State,
    can_scan_libraries: bool,
) -> Element<'_, UiMessage> {
    let dashboard = &state.domains.library.state.scan_dashboard;
    let selected_id = dashboard.selected_run_id;

    let mut content = column![section_heading(
        Icon::ListTree,
        "Run details",
        "Timeline, terminal summary, failure summaries, and copyable IDs.",
    )]
    .spacing(14);

    if let Some(run) = &dashboard.selected_run {
        content = content.push(run_identity_panel(run));
        content = content.push(terminal_summary_panel(
            run,
            dashboard.selected_terminal_summary.as_ref(),
        ));
    } else if let Some(scan_id) = selected_id {
        content = content.push(
            container(
                row![
                    text(format!("scan_id: {scan_id}"))
                        .size(13)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                        .width(Length::Fill),
                    copy_button("Copy scan_id", scan_id.to_string()),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            )
            .padding([10, 12])
            .style(theme::Container::HeaderAccent.style()),
        );
    }

    match &dashboard.selected_run_state {
        ScanDashboardLoadState::Loading => {
            content = content.push(diagnostics_notice_card(
                Icon::RefreshCw,
                "Loading run details…",
                "Fetching ordered timeline events and failure summaries for this run.",
                None,
            ));
        }
        ScanDashboardLoadState::Failed { error } => {
            content = content.push(diagnostics_error_card(
                "Run details could not be loaded",
                error,
            ));
        }
        ScanDashboardLoadState::Idle | ScanDashboardLoadState::Loaded => {}
    }

    content = content.push(timeline_panel(&dashboard.selected_events));
    content = content.push(failure_summary_panel(
        &dashboard.selected_failures,
        can_scan_libraries,
    ));

    if let Some(replay) = &dashboard.selected_replay {
        content = content.push(
            container(
                column![
                    text("Timeline cursor")
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    text(format!(
                        "Next sequence: {} · recoverable: {}",
                        replay
                            .next_sequence
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "current".to_string()),
                        if replay.recoverable { "yes" } else { "no" }
                    ))
                    .size(12)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    text(replay.recovery_hint.clone())
                        .size(12)
                        .color(theme::MediaServerTheme::TEXT_SUBDUED),
                ]
                .spacing(6),
            )
            .padding([10, 12])
            .style(theme::Container::HeaderAccent.style()),
        );
    }

    container(content)
        .padding(16)
        .style(theme::Container::Card.style())
        .width(Length::Fill)
        .into()
}

fn run_identity_panel(run: &ScanRunDto) -> Element<'_, UiMessage> {
    container(
        column![
            row![
                metric_chip(
                    "Status",
                    safe_run_status_label(run),
                    status_color(&run.status),
                ),
                metric_chip(
                    "Progress",
                    format!(
                        "{}/{} items",
                        run.completed_items, run.total_items
                    ),
                    theme::MediaServerTheme::TEXT_PRIMARY,
                ),
                metric_chip(
                    "Retrying",
                    run.retrying_items.to_string(),
                    if run.retrying_items > 0 {
                        theme::MediaServerTheme::WARNING
                    } else {
                        theme::MediaServerTheme::TEXT_PRIMARY
                    },
                ),
                metric_chip(
                    "Needs attention",
                    attention_count_text(run),
                    if run.dead_lettered_items > 0 {
                        theme::MediaServerTheme::WARNING
                    } else {
                        theme::MediaServerTheme::TEXT_PRIMARY
                    },
                ),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
            copyable_id_row("scan_id", run.scan_id.to_string()),
            copyable_id_row("correlation_id", run.correlation_id.to_string()),
        ]
        .spacing(10),
    )
    .padding([10, 12])
    .style(theme::Container::HeaderAccent.style())
    .width(Length::Fill)
    .into()
}

fn terminal_summary_panel(
    run: &ScanRunDto,
    terminal_summary: Option<&serde_json::Value>,
) -> Element<'static, UiMessage> {
    let mut lines = column![
        text("Terminal summary")
            .size(15)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(run.status_message.clone())
            .size(13)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text(format!(
            "Completed {} of {} items · Retrying {} · {}",
            run.completed_items,
            run.total_items,
            run.retrying_items,
            attention_summary_text(run),
        ))
        .size(13)
        .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text(format!(
            "Started {} · Last update {}",
            format_timestamp(run.started_at),
            format_timestamp(run.last_event_at),
        ))
        .size(12)
        .color(theme::MediaServerTheme::TEXT_SUBDUED),
    ]
    .spacing(6);

    if let Some(terminal_at) = run.terminal_at {
        lines = lines.push(
            text(format!("Finished {}", format_timestamp(terminal_at)))
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
        );
    } else {
        lines = lines.push(
            text("Run is still active; final summary will appear when it finishes.")
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
        );
    }

    for line in terminal_summary_supplemental_lines(terminal_summary) {
        lines = lines.push(
            text(line)
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
        );
    }

    container(lines)
        .padding([10, 12])
        .style(theme::Container::Card.style())
        .width(Length::Fill)
        .into()
}

fn timeline_panel(events: &[ScanRunEventDto]) -> Element<'_, UiMessage> {
    let mut content = column![section_heading(
        Icon::ListOrdered,
        "Timeline events",
        "Ordered by scanner sequence.",
    )]
    .spacing(10);

    if events.is_empty() {
        content = content.push(
            text("No timeline events are available for this run yet.")
                .size(13)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    } else {
        let mut ordered: Vec<&ScanRunEventDto> = events.iter().collect();
        ordered.sort_by(|a, b| {
            a.sequence
                .cmp(&b.sequence)
                .then(a.occurred_at.cmp(&b.occurred_at))
        });

        for event in ordered.into_iter().take(12) {
            content = content.push(timeline_event_row(event));
        }

        if events.len() > 12 {
            content = content.push(
                text(format!(
                    "Showing first 12 of {} retained events.",
                    events.len()
                ))
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
            );
        }
    }

    container(content)
        .padding([10, 12])
        .style(theme::Container::Card.style())
        .width(Length::Fill)
        .into()
}

fn timeline_event_row(event: &ScanRunEventDto) -> Element<'_, UiMessage> {
    let status_label = if event.status_label.trim().is_empty() {
        scan_status_display_text(&event.status).label
    } else {
        event.status_label.clone()
    };

    container(
        row![
            column![
                text(format!(
                    "#{} · {} · {}",
                    event.sequence,
                    event_kind_label(&event.event_kind),
                    status_label,
                ))
                .size(13)
                .color(status_color(&event.status)),
                text(event.status_message.clone())
                    .size(12)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                text(format!(
                    "{} · Completed {}/{} · Retrying {} · {}",
                    format_timestamp(event.occurred_at),
                    event.completed_items,
                    event.total_items,
                    event.retrying_items,
                    event_attention_text(
                        event.dead_lettered_items,
                        &event.status
                    ),
                ))
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
                event
                    .current_path
                    .as_ref()
                    .map(|path| {
                        text(format!("Path: {}", truncate_path(path)))
                            .size(12)
                            .color(theme::MediaServerTheme::TEXT_SUBDUED)
                    })
                    .unwrap_or_else(|| {
                        text("Path: not reported")
                            .size(12)
                            .color(theme::MediaServerTheme::TEXT_SUBDUED)
                    }),
            ]
            .spacing(4)
            .width(Length::Fill),
            copy_button("Copy correlation", event.correlation_id.to_string()),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
    )
    .padding([8, 10])
    .style(theme::Container::HeaderAccent.style())
    .width(Length::Fill)
    .into()
}

fn failure_summary_panel(
    failures: &[ScanFailureDto],
    can_scan_libraries: bool,
) -> Element<'_, UiMessage> {
    let mut content = column![section_heading(
        Icon::TriangleAlert,
        "Failure summaries",
        "Actionable scan items with non-destructive recovery controls.",
    )]
    .spacing(10);

    if failures.is_empty() {
        content = content.push(
            text("No failure summaries for this run.")
                .size(13)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    } else {
        for failure in failures.iter().take(8) {
            content = content.push(failure_card(failure, can_scan_libraries));
        }

        if failures.len() > 8 {
            content = content.push(
                text(format!(
                    "Showing first 8 of {} failure summaries.",
                    failures.len()
                ))
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
            );
        }
    }

    container(content)
        .padding([10, 12])
        .style(theme::Container::Card.style())
        .width(Length::Fill)
        .into()
}

fn failure_card(
    failure: &ScanFailureDto,
    can_scan_libraries: bool,
) -> Element<'_, UiMessage> {
    let copy = safe_failure_copy(failure);
    let recovery_target = recovery_path_for_failure(failure);

    let mut recovery = row![
        text("Recovery is non-destructive: it queues a retry and keeps library data in place.")
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SUBDUED)
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    if !failure.retryable {
        recovery = recovery.push(
            text("Failed after retries")
                .size(12)
                .color(theme::MediaServerTheme::WARNING),
        );
    } else if !can_scan_libraries {
        recovery = recovery.push(
            text("Retry requires library scan permission")
                .size(12)
                .color(theme::MediaServerTheme::WARNING),
        );
    } else if let Some(path) = recovery_target {
        recovery = recovery.push(
            button("Retry this path")
                .on_press(
                    SettingsUiMessage::RecoverScanPath(ScanRecoveryRequest {
                        library_id: failure.library_id,
                        path,
                        correlation_id: None,
                    })
                    .into(),
                )
                .style(theme::Button::Secondary.style()),
        );
    } else {
        recovery = recovery.push(
            text("Retry path is not available")
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
        );
    }

    container(
        column![
            row![
                column![
                    text(copy.0)
                        .size(14)
                        .color(theme::MediaServerTheme::WARNING),
                    text(copy.1)
                        .size(12)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                ]
                .spacing(4)
                .width(Length::Fill),
                metric_chip(
                    "Occurrences",
                    failure.occurrences.to_string(),
                    theme::MediaServerTheme::TEXT_PRIMARY,
                ),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
            text(format!("Item: {}", truncate_path(&failure.subject_key)))
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
            text(format!(
                "First seen {} · Last seen {}",
                format_timestamp(failure.first_seen_at),
                format_timestamp(failure.last_seen_at),
            ))
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SUBDUED),
            recovery,
        ]
        .spacing(8),
    )
    .padding([10, 12])
    .style(theme::Container::HeaderAccent.style())
    .width(Length::Fill)
    .into()
}

#[derive(Debug, Clone, Copy)]
enum RunCardMode {
    Active,
    Latest,
    History,
}

fn scan_run_card<'a>(
    run: &'a ScanRunDto,
    mode: RunCardMode,
    can_scan_libraries: bool,
    selected: bool,
) -> Element<'a, UiMessage> {
    let title = match mode {
        RunCardMode::Active => "Active run",
        RunCardMode::Latest => "Latest run",
        RunCardMode::History => "History run",
    };
    let percent = run_progress_percent(run);
    let style = if selected {
        theme::Container::CardHovered.style()
    } else {
        theme::Container::HeaderAccent.style()
    };

    let mut actions = row![
        button(if selected {
            "Refresh details"
        } else {
            "Details"
        })
        .on_press(
            if selected {
                SettingsUiMessage::RefreshScanDashboardRun(run.scan_id)
            } else {
                SettingsUiMessage::SelectScanDashboardRun(run.scan_id)
            }
            .into(),
        )
        .style(theme::Button::Secondary.style()),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    if matches!(mode, RunCardMode::Active) {
        actions = add_scan_control_buttons(actions, run, can_scan_libraries);
    }

    container(
        column![
            row![
                column![
                    row![
                        text(title)
                            .size(12)
                            .color(theme::MediaServerTheme::TEXT_SUBDUED),
                        status_badge(
                            safe_run_status_label(run),
                            status_color(&run.status)
                        ),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                    text(format!(
                        "Library {} · {}",
                        run.library_id,
                        format_timestamp(run.last_event_at),
                    ))
                    .size(12)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    text(run.status_message.clone())
                        .size(12)
                        .color(theme::MediaServerTheme::TEXT_SUBDUED),
                ]
                .spacing(4)
                .width(Length::Fill),
                actions,
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
            container(progress_bar(0.0..=100.0, percent))
                .height(Length::Fixed(6.0))
                .width(Length::Fill),
            row![
                text(format!(
                    "{}% · {}/{} items",
                    percent.round(),
                    run.completed_items,
                    run.total_items,
                ))
                .size(12)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                Space::new().width(16),
                text(format!("Retrying: {}", run.retrying_items))
                    .size(12)
                    .color(if run.retrying_items > 0 {
                        theme::MediaServerTheme::WARNING
                    } else {
                        theme::MediaServerTheme::TEXT_SECONDARY
                    }),
                Space::new().width(16),
                text(attention_summary_text(run)).size(12).color(
                    if run.dead_lettered_items > 0 {
                        theme::MediaServerTheme::WARNING
                    } else {
                        theme::MediaServerTheme::TEXT_SECONDARY
                    }
                ),
                Space::new().width(16),
                text(
                    run.current_path
                        .as_deref()
                        .map(|path| format!("Current: {}", truncate_path(path)))
                        .unwrap_or_else(
                            || "Current: awaiting scanner update".to_string()
                        ),
                )
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10),
    )
    .padding([10, 12])
    .style(style)
    .width(Length::Fill)
    .into()
}

fn add_scan_control_buttons<'a>(
    mut actions: iced::widget::Row<'a, UiMessage>,
    run: &ScanRunDto,
    can_scan_libraries: bool,
) -> iced::widget::Row<'a, UiMessage> {
    if !can_scan_libraries {
        return actions.push(
            text("Controls require scan permission")
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
        );
    }

    match run.status.as_str() {
        "running" => {
            actions = actions.push(
                button("Pause")
                    .on_press(
                        SettingsUiMessage::PauseLibraryScan(
                            run.library_id,
                            run.scan_id,
                        )
                        .into(),
                    )
                    .style(theme::Button::Secondary.style()),
            );
            actions = actions.push(
                button("Cancel scan")
                    .on_press(
                        SettingsUiMessage::CancelLibraryScan(
                            run.library_id,
                            run.scan_id,
                        )
                        .into(),
                    )
                    .style(theme::Button::Destructive.style()),
            );
        }
        "paused" => {
            actions = actions.push(
                button("Resume")
                    .on_press(
                        SettingsUiMessage::ResumeLibraryScan(
                            run.library_id,
                            run.scan_id,
                        )
                        .into(),
                    )
                    .style(theme::Button::Primary.style()),
            );
            actions = actions.push(
                button("Cancel scan")
                    .on_press(
                        SettingsUiMessage::CancelLibraryScan(
                            run.library_id,
                            run.scan_id,
                        )
                        .into(),
                    )
                    .style(theme::Button::Destructive.style()),
            );
        }
        "pending" => {
            actions = actions.push(
                button("Cancel scan")
                    .on_press(
                        SettingsUiMessage::CancelLibraryScan(
                            run.library_id,
                            run.scan_id,
                        )
                        .into(),
                    )
                    .style(theme::Button::Destructive.style()),
            );
        }
        _ => {}
    }

    actions
}

fn section_heading<'a>(
    icon: Icon,
    title: impl Into<String>,
    subtitle: impl Into<String>,
) -> Element<'a, UiMessage> {
    row![
        icon_text(icon),
        column![
            text(title.into())
                .size(16)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(subtitle.into())
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(2),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center)
    .into()
}

fn status_badge<'a>(
    label: impl Into<String>,
    color: Color,
) -> Element<'a, UiMessage> {
    container(text(label.into()).size(12).color(color))
        .padding([4, 8])
        .style(theme::Container::HeaderAccent.style())
        .into()
}

fn copyable_id_row<'a>(
    label: &'a str,
    value: String,
) -> Element<'a, UiMessage> {
    row![
        text(format!("{label}: {value}"))
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SECONDARY)
            .width(Length::Fill),
        copy_button(format!("Copy {label}"), value),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center)
    .into()
}

fn copy_button<'a>(
    label: impl Into<String>,
    value: String,
) -> Element<'a, UiMessage> {
    button(text(label.into()))
        .on_press(SettingsUiMessage::CopyScannerDiagnostic(value).into())
        .style(theme::Button::Secondary.style())
        .into()
}

fn safe_run_status_label(run: &ScanRunDto) -> String {
    if run.status_label.trim().is_empty() {
        scan_status_display_text(&run.status).label
    } else {
        run.status_label.clone()
    }
}

fn safe_failure_copy(failure: &ScanFailureDto) -> (String, String) {
    if failure.category == "content_not_indexed"
        || failure.message_code == "scan.no_indexable_media"
    {
        return (
            "No playable media found".to_string(),
            "No playable media files were found in this folder. Check the path or add supported media before retrying."
                .to_string(),
        );
    }

    if failure.category_label.trim().is_empty()
        || failure.message.trim().is_empty()
    {
        let display =
            scan_failure_display_text(&failure.category, &failure.message_code);
        return (display.label, display.message);
    }

    (failure.category_label.clone(), failure.message.clone())
}

fn recovery_path_for_failure(failure: &ScanFailureDto) -> Option<String> {
    let subject = failure.subject_key.trim();
    if subject.starts_with('/')
        || subject.starts_with("~/")
        || subject.as_bytes().get(1).is_some_and(|byte| *byte == b':')
    {
        Some(subject.to_string())
    } else {
        None
    }
}

fn terminal_summary_supplemental_lines(
    terminal_summary: Option<&serde_json::Value>,
) -> Vec<String> {
    let Some(summary) = terminal_summary else {
        return Vec::new();
    };
    let Some(object) = summary.as_object() else {
        return Vec::new();
    };

    let mut lines = Vec::new();
    if let Some(source) = object.get("source").and_then(|value| value.as_str())
    {
        lines.push(format!("Summary source: {source}"));
    }
    if let Some(sequence) =
        object.get("sequence").and_then(|value| value.as_u64())
    {
        lines.push(format!("Final event sequence: {sequence}"));
    }
    if let Some(path) =
        object.get("current_path").and_then(|value| value.as_str())
    {
        lines.push(format!("Last reported path: {}", truncate_path(path)));
    }

    lines
}

fn watcher_status_label(health: &ScannerHealthResponse) -> String {
    let incremental = &health.incremental;
    if incremental.watcher_error_count > 0
        || incremental.last_watcher_error.is_some()
    {
        format!(
            "Needs attention · {} issue(s)",
            incremental.watcher_error_count
        )
    } else if incremental.initializing_watch_libraries > 0 {
        format!(
            "Starting · {} initializing",
            incremental.initializing_watch_libraries
        )
    } else if incremental.watch_enabled_libraries == 0 {
        "No watched libraries".to_string()
    } else if incremental.active_watch_libraries
        < incremental.watch_enabled_libraries
    {
        "Needs attention".to_string()
    } else {
        "Healthy".to_string()
    }
}

fn cursor_status_label(health: &ScannerHealthResponse) -> String {
    let incremental = &health.incremental;
    if incremental.stale_cursors > 0 || incremental.stale_cursor_libraries > 0 {
        format!(
            "Needs attention · {} stale cursor(s)",
            incremental.stale_cursors
        )
    } else if incremental.replay_pending_events > 0 {
        format!(
            "Retrying · {} pending event(s)",
            incremental.replay_pending_events
        )
    } else if let Some(lag) = incremental.replay_lag_ms {
        format!("Healthy · replay lag {}", format_duration_ms(lag))
    } else {
        "Healthy".to_string()
    }
}

fn queue_depth_total(health: &ScannerHealthResponse) -> usize {
    health.queue_depths.folder_scan
        + health.queue_depths.analyze
        + health.queue_depths.metadata
        + health.queue_depths.index
        + health.queue_depths.image_fetch
}

fn queue_depth_color(depth: usize) -> Color {
    if depth > 0 {
        accent()
    } else {
        theme::MediaServerTheme::TEXT_PRIMARY
    }
}

fn health_severity_color(severity: HealthSeverity) -> Color {
    match severity {
        HealthSeverity::Healthy => theme::MediaServerTheme::SUCCESS,
        HealthSeverity::Active => accent(),
        HealthSeverity::NeedsAttention => theme::MediaServerTheme::WARNING,
    }
}

fn status_color(status: &str) -> Color {
    match status {
        "completed" => theme::MediaServerTheme::SUCCESS,
        "failed" => theme::MediaServerTheme::ERROR,
        "paused" => theme::MediaServerTheme::WARNING,
        "running" | "pending" => accent(),
        "canceled" | "cancelled" => theme::MediaServerTheme::TEXT_SECONDARY,
        _ => theme::MediaServerTheme::TEXT_PRIMARY,
    }
}

fn run_progress_percent(run: &ScanRunDto) -> f32 {
    if run.total_items == 0 {
        0.0
    } else {
        ((run.completed_items as f32 / run.total_items as f32) * 100.0)
            .clamp(0.0, 100.0)
    }
}

fn attention_count_text(run: &ScanRunDto) -> String {
    run.dead_lettered_items.to_string()
}

fn attention_summary_text(run: &ScanRunDto) -> String {
    event_attention_text(run.dead_lettered_items, &run.status)
}

fn event_attention_text(count: u64, status: &str) -> String {
    if count == 0 {
        "No items need attention".to_string()
    } else if status == "failed" {
        format!("Failed after retries: {count}")
    } else {
        format!("Needs attention: {count}")
    }
}

fn event_kind_label(kind: &str) -> &'static str {
    match kind {
        "progress" => "Progress",
        "terminal" => "Terminal update",
        "failure" | "failed" => "Failed after retries",
        "retry" | "retrying" => "Retrying",
        "skipped" | "skip" => "Skipped",
        _ => "Scanner update",
    }
}

fn format_timestamp(timestamp: DateTime<Utc>) -> String {
    timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn format_duration_ms(milliseconds: u64) -> String {
    if milliseconds >= 1_000 {
        format!("{:.1}s", milliseconds as f32 / 1_000.0)
    } else {
        format!("{milliseconds}ms")
    }
}

fn truncate_path(path: &str) -> String {
    const MAX_LEN: usize = 56;
    if path.len() <= MAX_LEN {
        path.to_string()
    } else {
        let tail = &path[path.len() - (MAX_LEN.saturating_sub(3))..];
        format!("…{}", tail)
    }
}

#[cfg(test)]
mod scanner_diagnostics_ui_tests {
    use super::*;
    use chrono::TimeZone;
    use ferrex_core::api::scan::{IncrementalScanStatusView, ScanQueueDepths};
    use ferrex_core::player_prelude::{ScanSnapshotDto, ScanStartDisposition};

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

    fn scan_snapshot(library_id: LibraryId, scan_id: Uuid) -> ScanSnapshotDto {
        ScanSnapshotDto {
            scan_id,
            library_id,
            status: ScanLifecycleStatus::Running,
            mode: ScanRunMode::Manual,
            completed_items: 0,
            total_items: 10,
            validated_items: 0,
            known_unchanged_items: 0,
            skipped_items: 0,
            failed_items: 0,
            needs_attention_items: 0,
            retrying_items: 0,
            correlation_id: scan_id,
            idempotency_key: format!("scan:{scan_id}:1"),
            run_key: ScanRunMode::Manual.run_key(library_id),
            disposition: Some(ScanStartDisposition::Created),
            current_path: None,
            started_at: chrono::Utc::now(),
            terminal_at: None,
            sequence: 1,
            reason_details: Vec::new(),
        }
    }

    fn health() -> ScannerHealthResponse {
        ScannerHealthResponse {
            queue_depths: ScanQueueDepths {
                folder_scan: 0,
                manifest_scan: 0,
                analyze: 0,
                metadata: 0,
                index: 0,
                image_fetch: 0,
            },
            active_scans: 0,
            retained_runs: 0,
            failed_runs: 0,
            incremental: IncrementalScanStatusView::default(),
        }
    }

    fn failure(message_code: &str, subject_key: &str) -> ScanFailureDto {
        let now = Utc.timestamp_opt(1, 0).single().unwrap();
        ScanFailureDto {
            scan_id: Uuid::from_u128(1),
            library_id: LibraryId(Uuid::from_u128(2)),
            subject_key: subject_key.to_string(),
            category: "content_not_indexed".to_string(),
            category_label: String::new(),
            message_code: message_code.to_string(),
            message: String::new(),
            occurrences: 1,
            first_seen_at: now,
            last_seen_at: now,
            retryable: true,
            debug: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn admin_active_panel_has_one_card_for_one_library_mode() {
        let mut state = State::new("http://localhost:3000".to_string());
        let library_id = LibraryId(Uuid::now_v7());
        let scan_id = Uuid::now_v7();

        state
            .domains
            .library
            .state
            .active_scans
            .insert(scan_id, scan_snapshot(library_id, scan_id));

        let cards = active_scan_panel_snapshots(&state);

        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].library_id, library_id);
        assert_eq!(cards[0].mode, ScanRunMode::Manual);
        assert_eq!(cards[0].run_key, ScanRunMode::Manual.run_key(library_id));
    }

    #[test]
    fn scanner_health_copy_flags_watcher_issues() {
        let mut health = health();
        health.incremental.watch_enabled_libraries = 1;
        health.incremental.active_watch_libraries = 0;
        health.incremental.watcher_error_count = 1;

        let copy = scanner_health_copy(&health);

        assert_eq!(copy.label, "Needs attention");
        assert_eq!(copy.severity, HealthSeverity::NeedsAttention);
    }

    #[test]
    fn primary_attention_copy_avoids_internal_terms() {
        let text = event_attention_text(3, "failed");

        assert_eq!(text, "Failed after retries: 3");
        assert!(!text.to_ascii_lowercase().contains("dead-letter"));
    }

    #[test]
    fn no_media_failure_uses_playable_media_copy() {
        let failure = failure("scan.no_indexable_media", "/media/empty");
        let copy = safe_failure_copy(&failure);

        assert_eq!(copy.0, "No playable media found");
        assert!(copy.1.contains("playable media"));
    }

    #[test]
    fn recovery_is_only_offered_for_path_subjects() {
        let path_failure =
            failure("scan.folder_permission_denied", "/media/movies");
        let opaque_failure =
            failure("scan.folder_permission_denied", "movie:123");

        assert_eq!(
            recovery_path_for_failure(&path_failure),
            Some("/media/movies".to_string())
        );
        assert_eq!(recovery_path_for_failure(&opaque_failure), None);
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
