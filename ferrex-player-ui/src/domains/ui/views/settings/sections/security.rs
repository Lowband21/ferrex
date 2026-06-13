//! Security section view
//!
//! Renders the security settings section content including:
//! - PIN: Set/change/remove PIN for quick access
//! - Password: Change account password
//! - Remembered login state for this device

use iced::widget::{
    Space, button, checkbox, column, container, row, text, text_input,
};
use iced::{Alignment, Element, Length};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::settings_ui::SettingsUiMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::infra::design_tokens::FontTokens;
use crate::state::State;

/// Render the security settings section
pub fn view_security_section<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let has_pin = state.domains.settings.security.has_pin;
    let fonts = state.domains.ui.state.size_provider.font;

    let mut content = column![].spacing(24).padding(20).max_width(600);

    // Header
    content = content.push(
        text("Security")
            .size(fonts.title_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
    );

    // PIN subsection
    content = content.push(section_header("PIN", fonts));

    content = content.push(
        container(
            column![
                row![
                    column![
                        text("Quick Access PIN")
                            .size(fonts.body)
                            .color(MediaServerTheme::TEXT_PRIMARY),
                        text("Use a PIN for faster login on trusted devices")
                            .size(fonts.small)
                            .color(MediaServerTheme::TEXT_SUBDUED),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                    if has_pin {
                        text("Enabled")
                            .size(fonts.caption)
                            .color(MediaServerTheme::SUCCESS)
                    } else {
                        text("Not Set")
                            .size(fonts.caption)
                            .color(MediaServerTheme::TEXT_SUBDUED)
                    },
                ]
                .align_y(Alignment::Center)
                .spacing(16),
                Space::new().height(12),
                if has_pin {
                    button(text("Change or Remove PIN").size(fonts.caption))
                        .padding([10, 20])
                        .style(theme::Button::Secondary.style())
                        .on_press(SettingsUiMessage::ShowChangePin.into())
                } else {
                    button(text("Set PIN").size(fonts.caption))
                        .padding([10, 20])
                        .style(theme::Button::Primary.style())
                        .on_press(SettingsUiMessage::ShowSetPin.into())
                },
            ]
            .spacing(8),
        )
        .padding(16)
        .style(theme::Container::Card.style()),
    );

    if state.domains.settings.security.showing_pin_change {
        content = content.push(pin_change_form(state, fonts));
    }

    // Password subsection
    content = content.push(section_header("Password", fonts));

    content = content.push(
        container(
            column![
                text("Account Password")
                    .size(fonts.body)
                    .color(MediaServerTheme::TEXT_PRIMARY),
                text("Change your account password")
                    .size(fonts.small)
                    .color(MediaServerTheme::TEXT_SUBDUED),
                Space::new().height(12),
                button(text("Change Password").size(fonts.caption))
                    .padding([10, 20])
                    .style(theme::Button::Secondary.style())
                    .on_press(SettingsUiMessage::ShowChangePassword.into()),
            ]
            .spacing(4),
        )
        .padding(16)
        .style(theme::Container::Card.style()),
    );

    if state.domains.settings.security.showing_password_change {
        content = content.push(password_change_form(state, fonts));
    }

    content = content.push(section_header("Remembered Login", fonts));
    content = content.push(
        container(
            column![
                checkbox(state.domains.settings.preferences.auto_login_enabled)
                    .label("Remember this device / auto-login")
                    .on_toggle(|enabled| {
                        SettingsUiMessage::ToggleAutoLogin(enabled).into()
                    })
                    .style(theme::Checkbox::style())
                    .size(16)
                    .text_size(fonts.caption)
                    .spacing(8),
                text("Turning this off clears the remembered auth cache for this device; password sign-in remains available.")
                    .size(fonts.small)
                    .color(MediaServerTheme::TEXT_SUBDUED),
                button(text("Reset local auth state").size(fonts.caption))
                    .padding([10, 20])
                    .style(theme::Button::Destructive.style())
                    .on_press(SettingsUiMessage::ResetLocalAuthState.into()),
                text("Use reset when this device was revoked, trust expired, key material is missing, cached profiles are stale, or the server was reset.")
                    .size(fonts.small)
                    .color(MediaServerTheme::TEXT_SUBDUED),
            ]
            .spacing(8),
        )
        .padding(16)
        .style(theme::Container::Card.style()),
    );

    container(content)
        .width(Length::Fill)
        .style(theme::Container::Default.style())
        .into()
}

fn pin_change_form<'a>(
    state: &'a State,
    fonts: FontTokens,
) -> Element<'a, UiMessage> {
    let security = &state.domains.settings.security;
    let has_pin = security.has_pin;

    let mut form = column![].spacing(10);

    if has_pin {
        form = form.push(field_label("Current PIN", fonts));
        form = form.push(
            text_input("Current PIN", security.pin_current.as_str())
                .secure(true)
                .padding(10)
                .size(fonts.caption)
                .on_input(|value| {
                    SettingsUiMessage::UpdatePinCurrent(value).into()
                }),
        );
    }

    form =
        form.push(field_label(if has_pin { "New PIN" } else { "PIN" }, fonts));
    form = form.push(
        text_input("New PIN", security.pin_new.as_str())
            .secure(true)
            .padding(10)
            .size(fonts.caption)
            .on_input(|value| SettingsUiMessage::UpdatePinNew(value).into()),
    );

    form = form.push(field_label("Confirm PIN", fonts));
    form = form.push(
        text_input("Confirm PIN", security.pin_confirm.as_str())
            .secure(true)
            .padding(10)
            .size(fonts.caption)
            .on_input(|value| {
                SettingsUiMessage::UpdatePinConfirm(value).into()
            }),
    );

    if let Some(error) = &security.pin_error {
        form = form.push(error_box(error, fonts));
    }

    let primary_label = if security.pin_loading {
        "Saving..."
    } else if has_pin {
        "Change PIN"
    } else {
        "Set PIN"
    };

    let mut actions = row![
        button(text(primary_label).size(fonts.caption))
            .padding([10, 20])
            .style(theme::Button::Primary.style())
            .on_press_maybe(
                (!security.pin_loading)
                    .then_some(SettingsUiMessage::SubmitPinChange.into())
            ),
        button(text("Cancel").size(fonts.caption))
            .padding([10, 20])
            .style(theme::Button::Secondary.style())
            .on_press(SettingsUiMessage::CancelPinChange.into()),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    if has_pin {
        actions = actions.push(
            button(text("Remove PIN").size(fonts.caption))
                .padding([10, 20])
                .style(theme::Button::Destructive.style())
                .on_press_maybe(
                    (!security.pin_loading)
                        .then_some(SettingsUiMessage::SubmitPinRemoval.into()),
                ),
        );
    }

    form = form.push(actions);

    container(form)
        .padding(16)
        .style(theme::Container::Card.style())
        .into()
}

fn password_change_form<'a>(
    state: &'a State,
    fonts: FontTokens,
) -> Element<'a, UiMessage> {
    let security = &state.domains.settings.security;
    let mut form = column![].spacing(10);

    form = form.push(field_label("Current Password", fonts));
    form = form.push(
        text_input("Current password", security.password_current.as_str())
            .secure(!security.password_show)
            .padding(10)
            .size(fonts.caption)
            .on_input(|value| {
                SettingsUiMessage::UpdatePasswordCurrent(value).into()
            }),
    );

    form = form.push(field_label("New Password", fonts));
    form = form.push(
        text_input("New password", security.password_new.as_str())
            .secure(!security.password_show)
            .padding(10)
            .size(fonts.caption)
            .on_input(|value| {
                SettingsUiMessage::UpdatePasswordNew(value).into()
            }),
    );

    form = form.push(field_label("Confirm Password", fonts));
    form = form.push(
        text_input("Confirm password", security.password_confirm.as_str())
            .secure(!security.password_show)
            .padding(10)
            .size(fonts.caption)
            .on_input(|value| {
                SettingsUiMessage::UpdatePasswordConfirm(value).into()
            }),
    );

    form = form.push(
        checkbox(security.password_show)
            .label("Show passwords")
            .on_toggle(|_| SettingsUiMessage::TogglePasswordVisibility.into())
            .style(theme::Checkbox::style())
            .size(16)
            .text_size(fonts.caption)
            .spacing(8),
    );

    if let Some(error) = &security.password_error {
        form = form.push(error_box(error, fonts));
    }

    let label = if security.password_loading {
        "Saving..."
    } else {
        "Change Password"
    };

    form =
        form.push(
            row![
                button(text(label).size(fonts.caption))
                    .padding([10, 20])
                    .style(theme::Button::Primary.style())
                    .on_press_maybe((!security.password_loading).then_some(
                        SettingsUiMessage::SubmitPasswordChange.into()
                    )),
                button(text("Cancel").size(fonts.caption))
                    .padding([10, 20])
                    .style(theme::Button::Secondary.style())
                    .on_press(SettingsUiMessage::CancelPasswordChange.into()),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );

    container(form)
        .padding(16)
        .style(theme::Container::Card.style())
        .into()
}

/// Create a section header with divider
fn section_header(title: &str, fonts: FontTokens) -> Element<'_, UiMessage> {
    column![
        text(title)
            .size(fonts.body_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
        container(Space::new().height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(
                    MediaServerTheme::BORDER_COLOR
                )),
                ..Default::default()
            }),
    ]
    .spacing(8)
    .into()
}

fn field_label(title: &str, fonts: FontTokens) -> Element<'_, UiMessage> {
    text(title)
        .size(fonts.small)
        .color(MediaServerTheme::TEXT_SECONDARY)
        .into()
}

fn error_box<'a>(
    message: &'a str,
    fonts: FontTokens,
) -> Element<'a, UiMessage> {
    container(
        text(message)
            .size(fonts.caption)
            .color(MediaServerTheme::ERROR),
    )
    .padding([8, 12])
    .style(theme::Container::ErrorBox.style())
    .into()
}
