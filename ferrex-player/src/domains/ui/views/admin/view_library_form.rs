use crate::{
    common::ui_utils::{Icon, icon_text},
    domains::library::{messages::Message, types::LibraryFormData},
    domains::ui::theme,
    state_refactored::State,
};
use iced::{
    Element, Length,
    widget::{
        Space, button, checkbox, column, container, radio, row, scrollable, text, text_input,
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
) -> Element<'a, Message> {
    let mut content = column![].spacing(20).padding(20);

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
            .on_press(Message::HideLibraryForm)
            .style(theme::Button::Secondary.style()),
            Space::with_width(Length::Fill),
            text(if form_data.editing {
                "Edit Library"
            } else {
                "Create Library"
            })
            .size(28)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_width(Length::Fill),
            Space::with_width(Length::Fixed(100.0)), // Balance the back button
        ]
        .align_y(iced::Alignment::Center),
    );

    // Error messages
    if !state.domains.library.state.library_form_errors.is_empty() {
        content = content.push(
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
            .width(Length::Fill),
        );
    }

    // Form fields
    let mut form_content = column![].spacing(15);

    // Library Name
    form_content = form_content.push(
        column![
            text("Library Name")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text_input("Enter library name", &form_data.name)
                .on_input(Message::UpdateLibraryFormName)
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
                    |value| Message::UpdateLibraryFormType(value.to_string())
                ),
                Space::with_width(Length::Fixed(30.0)),
                radio(
                    "TV Shows",
                    "TvShows",
                    Some(form_data.library_type.as_str()),
                    |value| Message::UpdateLibraryFormType(value.to_string())
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
            text_input("e.g., /media/movies, /mnt/storage/films", &form_data.paths)
                .on_input(Message::UpdateLibraryFormPaths)
                .padding(10)
                .size(16),
        ]
        .spacing(5),
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
                .on_input(Message::UpdateLibraryFormScanInterval)
                .padding(10)
                .size(16),
        ]
        .spacing(5),
    );

    // Enabled checkbox
    form_content = form_content.push(
        checkbox("Enable this library", form_data.enabled)
            .on_toggle(|_| Message::ToggleLibraryFormEnabled)
            .text_size(16),
    );

    content = content.push(
        container(form_content.padding(20))
            .style(theme::Container::Card.style())
            .width(Length::Fill),
    );

    // Action buttons
    content = content.push(
        row![
            Space::with_width(Length::Fill),
            button("Cancel")
                .on_press(Message::HideLibraryForm)
                .style(theme::Button::Secondary.style()),
            Space::with_width(Length::Fixed(10.0)),
            button(if form_data.editing {
                "Update Library"
            } else {
                "Create Library"
            })
            .on_press(Message::SubmitLibraryForm)
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
