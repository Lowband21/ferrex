use crate::{
    common::focus::ids,
    common::ui_utils::{Icon, icon_text},
    domains::{
        library::{media_root_browser, types::LibraryFormData},
        ui::{messages::UiMessage, settings_ui::SettingsUiMessage, theme},
    },
    state::State,
};
use ferrex_core::player_prelude::{MediaRootEntry, MediaRootEntryKind};
use iced::{
    Element, Length,
    widget::{
        Space, button, checkbox, column, container, radio, row, scrollable,
        text, text_input,
    },
};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_library_form<'a>(
    state: &'a State,
    form_data: &'a LibraryFormData,
) -> Element<'a, UiMessage> {
    let mut content = column![].spacing(20).padding(20);
    let browser_state = &state.domains.library.state.media_root_browser;

    // Header with back button
    content = content.push(
        row![
            button(
                row![
                    icon_text(Icon::ArrowLeft),
                    text(" Back to Library Management")
                ]
                .spacing(5)
                .align_y(iced::Alignment::Center)
            )
            .on_press(SettingsUiMessage::HideLibraryForm.into())
            .style(theme::Button::Secondary.style()),
            Space::new().width(Length::Fill),
            text(if form_data.editing {
                "Edit Library"
            } else {
                "Create Library"
            })
            .size(28)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::new().width(Length::Fill),
            Space::new().width(Length::Fixed(100.0)), // Balance the back button
        ]
        .align_y(iced::Alignment::Center),
    );

    // Keep a stable widget tree before the form fields to preserve focus when
    // validation errors appear/disappear.
    let error_box: Element<'a, UiMessage> =
        if state.domains.library.state.library_form_errors.is_empty() {
            Space::new().height(Length::Fixed(0.0)).into()
        } else {
            container(
                column(
                    state
                        .domains
                        .library
                        .state
                        .library_form_errors
                        .iter()
                        .map(|error| {
                            text(error)
                                .size(14)
                                .color(theme::MediaServerTheme::ERROR_COLOR)
                                .into()
                        })
                        .collect::<Vec<_>>(),
                )
                .spacing(5),
            )
            .padding(10)
            .style(theme::Container::ErrorBox.style())
            .width(Length::Fill)
            .into()
        };
    content = content.push(error_box);

    // Form fields
    let mut form_content = column![].spacing(15);

    // Library Name
    form_content = form_content.push(
        column![
                text("Library Name")
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                text_input("Enter library name", &form_data.name)
                    .on_input(
                        |s| SettingsUiMessage::UpdateLibraryFormName(s).into()
                    )
                    .id(ids::library_form_name())
                    .padding(10)
                    .size(16),
            ]
        .spacing(5),
    );

    // Library Type
    form_content = form_content.push(
        column![
            text("Library Type")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            row![
                radio(
                    "Movies",
                    "Movies",
                    Some(form_data.library_type.as_str()),
                    |value| {
                        SettingsUiMessage::UpdateLibraryFormType(
                            value.to_string(),
                        )
                        .into()
                    }
                ),
                Space::new().width(Length::Fixed(30.0)),
                radio(
                    "TV Shows",
                    "TvShows",
                    Some(form_data.library_type.as_str()),
                    |value| {
                        SettingsUiMessage::UpdateLibraryFormType(
                            value.to_string(),
                        )
                        .into()
                    }
                ),
            ]
            .spacing(20)
        ]
        .spacing(5),
    );

    // Paths
    form_content = form_content.push(
        column![
            text("Media Paths")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text("Enter one or more paths separated by commas")
                .size(12)
                .color(theme::MediaServerTheme::TEXT_DIMMED),
            text_input(
                "e.g., /media/movies, /mnt/storage/films",
                &form_data.paths
            )
            .on_input(|s| SettingsUiMessage::UpdateLibraryFormPaths(s).into())
            .id(ids::library_form_paths())
            .padding(10)
            .size(16),
        ]
        .spacing(5),
    );

    let root_hint = browser_state
        .media_root
        .as_ref()
        .map(|root| format!("Container root: {}", root))
        .unwrap_or_else(|| {
            "Container media root not yet reported by server".into()
        });
    let mut root_info = column![
        text(root_hint)
            .size(12)
            .color(theme::MediaServerTheme::TEXT_DIMMED),
    ]
    .spacing(2);
    if browser_state.visible {
        root_info = root_info.push(
            text(format!("Browsing: {}", browser_state.display_path))
                .size(12)
                .color(theme::MediaServerTheme::TEXT_DIMMED),
        );
    }
    form_content = form_content.push(
        row![
            button("Browse Server Media Root")
                .on_press(
                    SettingsUiMessage::LibraryMediaRoot(
                        media_root_browser::Message::Open
                    )
                    .into()
                )
                .style(theme::Button::Secondary.style()),
            Space::new().width(Length::Fixed(12.0)),
            root_info.width(Length::Fill),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(10),
    );

    // Scan Interval
    form_content = form_content.push(
        column![
            text("Automatic Scan Interval (minutes)")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text("Set to 0 to disable automatic scanning")
                .size(12)
                .color(theme::MediaServerTheme::TEXT_DIMMED),
            text_input("60", &form_data.scan_interval_minutes)
                .on_input(|s| {
                    SettingsUiMessage::UpdateLibraryFormScanInterval(s).into()
                })
                .id(ids::library_form_scan_interval())
                .padding(10)
                .size(16),
        ]
        .spacing(5),
    );

    // Enabled checkbox
    form_content = form_content.push(
        checkbox(form_data.enabled)
            .on_toggle(|_| SettingsUiMessage::ToggleLibraryFormEnabled.into())
            .label("Enabled")
            .style(theme::Checkbox::style())
            .spacing(8)
            .text_size(16),
    );

    if !form_data.editing {
        form_content = form_content.push(
            checkbox(form_data.start_scan)
                .on_toggle(|_| {
                    SettingsUiMessage::ToggleLibraryFormStartScan.into()
                })
                .label("Start scan immediately")
                .style(theme::Checkbox::style())
                .spacing(8)
                .text_size(16),
        );
    }

    content = content.push(
        container(form_content.padding(20))
            .style(theme::Container::Card.style())
            .width(Length::Fill),
    );

    if browser_state.visible {
        content = content.push(
            container(media_root_browser_modal(state))
                .style(theme::Container::ModalOverlay.style())
                .width(Length::Fill)
                .height(Length::Fill),
        );
    }

    // Action buttons
    content = content.push(
        row![
            Space::new().width(Length::Fill),
            button("Cancel")
                .on_press(SettingsUiMessage::HideLibraryForm.into())
                .style(theme::Button::Secondary.style()),
            Space::new().width(Length::Fixed(10.0)),
            button(if form_data.editing {
                "Update Library"
            } else {
                "Create Library"
            })
            .on_press(SettingsUiMessage::SubmitLibraryForm.into())
            .style(theme::Button::Primary.style()),
        ]
        .align_y(iced::Alignment::Center),
    );

    scrollable(
        container(content)
            .width(Length::Fill)
            .height(Length::Shrink),
    )
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::default(),
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::Scrollable::style())
    .into()
}

fn media_root_browser_modal<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let browser = &state.domains.library.state.media_root_browser;
    let entries = browser.entries.clone();
    let breadcrumbs = browser.breadcrumbs.clone();

    let header = row![
        text("Browse Server Media Root")
            .size(20)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::new().width(Length::Fill),
        button("Close")
            .on_press(
                SettingsUiMessage::LibraryMediaRoot(
                    media_root_browser::Message::Close
                )
                .into()
            )
            .style(theme::Button::Secondary.style())
            .padding([6, 14]),
    ]
    .align_y(iced::Alignment::Center);

    let mut body = column![header].spacing(16);

    body = body.push(
        text(
            browser
                .media_root
                .as_ref()
                .map(|root| format!("Container root: {}", root))
                .unwrap_or_else(|| {
                    "Container media root not configured yet.".into()
                }),
        )
        .size(14)
        .color(theme::MediaServerTheme::TEXT_SECONDARY),
    );

    if !breadcrumbs.is_empty() {
        let buttons = breadcrumbs
            .into_iter()
            .map(|crumb| {
                let target = if crumb.relative_path.is_empty() {
                    None
                } else {
                    Some(crumb.relative_path.clone())
                };
                button(text(crumb.label))
                    .on_press(
                        SettingsUiMessage::LibraryMediaRoot(
                            media_root_browser::Message::Browse {
                                path: target,
                            },
                        )
                        .into(),
                    )
                    .style(theme::Button::Secondary.style())
                    .padding([4, 10])
                    .into()
            })
            .collect::<Vec<_>>();
        body =
            body.push(row(buttons).spacing(6).align_y(iced::Alignment::Center));
    }

    if let Some(error) = &browser.error {
        body = body.push(
            container(
                text(error)
                    .size(12)
                    .color(theme::MediaServerTheme::ERROR_COLOR),
            )
            .padding([8, 12])
            .style(theme::Container::ErrorBox.style()),
        );
    }

    let list: Element<'a, UiMessage> = if browser.is_loading {
        container(
            text("Loading…")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        )
        .padding([12, 0])
        .into()
    } else if entries.is_empty() {
        container(
            text("This directory is empty.")
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        )
        .padding([12, 0])
        .into()
    } else {
        let entry_elements = entries
            .into_iter()
            .map(render_media_root_entry)
            .collect::<Vec<_>>();

        scrollable(column(entry_elements).spacing(8))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::default(),
            ))
            .height(Length::Fixed(300.0))
            .style(theme::Scrollable::style())
            .into()
    };

    body = body.push(list);

    container(body)
        .padding(24)
        .style(theme::Container::Modal.style())
        .width(Length::Fixed(720.0))
        .into()
}

fn render_media_root_entry(
    entry: MediaRootEntry,
) -> Element<'static, UiMessage> {
    let MediaRootEntry {
        name,
        relative_path,
        kind,
        is_symlink,
    } = entry;

    let descriptor = match kind {
        MediaRootEntryKind::Directory => "Directory",
        MediaRootEntryKind::File => "File",
        MediaRootEntryKind::Other => "Entry",
    };

    let display_path = if relative_path.is_empty() {
        "/".to_string()
    } else {
        relative_path.clone()
    };

    let mut info = column![
        text(name.clone())
            .size(14)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(format!("{descriptor} • {}", display_path))
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(4);

    if is_symlink {
        info = info.push(
            text("Symbolic link")
                .size(12)
                .color(theme::MediaServerTheme::TEXT_DIMMED),
        );
    }

    let mut actions = iced::widget::Row::new().spacing(6);
    if kind == MediaRootEntryKind::Directory {
        let target = if relative_path.is_empty() {
            None
        } else {
            Some(relative_path.clone())
        };
        actions = actions.push(
            button("Open")
                .on_press(
                    SettingsUiMessage::LibraryMediaRoot(
                        media_root_browser::Message::Browse { path: target },
                    )
                    .into(),
                )
                .style(theme::Button::Secondary.style())
                .padding([6, 12]),
        );

        if name != ".." {
            actions = actions.push(
                button("Select")
                    .on_press(
                        SettingsUiMessage::LibraryMediaRoot(
                            media_root_browser::Message::PathSelected(
                                relative_path.clone(),
                            ),
                        )
                        .into(),
                    )
                    .style(theme::Button::Primary.style())
                    .padding([6, 12]),
            );
        }
    }

    let icon = match kind {
        MediaRootEntryKind::Directory => "📁",
        MediaRootEntryKind::File => "📄",
        MediaRootEntryKind::Other => "❓",
    };

    container(
        row![text(icon).size(18), info.width(Length::Fill), actions,]
            .spacing(12)
            .align_y(iced::Alignment::Center),
    )
    .padding([6, 8])
    .style(theme::Container::Default.style())
    .into()
}
