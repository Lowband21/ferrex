//! PIN setup view

use super::components::{
    auth_card, auth_container, error_message, primary_button, secondary_button,
    spacing, title,
};
use crate::common::focus::{FocusMessage, ids};
use crate::common::messages::DomainMessage;
use crate::domains::auth::messages as auth;
use crate::domains::auth::pin_policy::{
    PinPolicyRules, pin_pair_satisfies_policy, policy_label_for,
};
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::auth::types::PinEntryTarget;
use crate::domains::ui::theme;
use crate::state::State;
use ferrex_core::player_prelude::User;
use iced::{
    Alignment, Element, Length, Theme,
    widget::{Space, column, text, text_input},
};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_pin_setup<'a>(
    state: &'a State,
    user: &'a User,
    pin: &'a SecureCredential,
    confirm_pin: &'a SecureCredential,
    _pin_entry_target: PinEntryTarget,
    error: Option<&'a str>,
) -> Element<'a, DomainMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let policy: PinPolicyRules = (&state.domains.auth.state.pin_policy).into();

    let mut content = column![
        title("Set Up PIN", fonts.title_lg),
        text(format!("Create a secure PIN for {}", user.display_name))
            .size(fonts.body)
            .style(|theme: &Theme| {
                text::Style {
                    color: Some(
                        theme.extended_palette().background.strong.text,
                    ),
                }
            })
            .align_x(iced::alignment::Horizontal::Center),
        text(policy_label_for(policy))
            .size(fonts.caption)
            .style(|theme: &Theme| {
                text::Style {
                    color: Some(
                        theme.extended_palette().background.strong.text,
                    ),
                }
            })
            .align_x(iced::alignment::Horizontal::Center),
        spacing(),
    ];

    // Error message
    if let Some(err) = error {
        content = content.push(error_message(err, fonts.caption));
        content = content.push(spacing());
    }

    // PIN entry
    content = content.push(text("Enter PIN").size(fonts.caption).style(
        |theme: &Theme| text::Style {
            color: Some(theme.extended_palette().background.strong.text),
        },
    ));

    content = content.push(Space::new().height(Length::Fixed(8.0)));

    content = content.push(
        text_input("Enter PIN", pin.as_str())
            .on_input(|value| {
                DomainMessage::Auth(auth::AuthMessage::UpdatePin(value))
            })
            .on_submit(DomainMessage::Focus(FocusMessage::Traverse {
                backwards: false,
            }))
            .secure(true)
            .id(ids::auth_pin_setup_pin())
            .padding(12)
            .size(fonts.body)
            .width(Length::Fill)
            .style(theme::TextInput::style()),
    );

    content = content.push(spacing());

    // Confirm PIN entry
    content = content.push(text("Confirm PIN").size(fonts.caption).style(
        |theme: &Theme| text::Style {
            color: Some(theme.extended_palette().background.strong.text),
        },
    ));

    content = content.push(Space::new().height(Length::Fixed(8.0)));

    content = content.push(
        text_input("Confirm PIN", confirm_pin.as_str())
            .on_input(|value| {
                DomainMessage::Auth(auth::AuthMessage::UpdateConfirmPin(value))
            })
            .on_submit(DomainMessage::Auth(auth::AuthMessage::SubmitPin))
            .secure(true)
            .id(ids::auth_pin_setup_confirm_pin())
            .padding(12)
            .size(fonts.body)
            .width(Length::Fill)
            .style(theme::TextInput::style()),
    );

    content = content.push(spacing());

    // Submit button
    let can_submit =
        pin_pair_satisfies_policy(pin.as_str(), confirm_pin.as_str(), policy);
    let submit_button = if can_submit {
        primary_button("Set PIN", fonts.body)
            .on_press(DomainMessage::Auth(auth::AuthMessage::SubmitPin))
    } else {
        primary_button("Set PIN", fonts.body)
    };

    content = content.push(submit_button);

    content = content.push(Space::new().height(Length::Fixed(12.0)));

    // Skip button
    content = content.push(
        secondary_button("Skip for now", fonts.body)
            .on_press(DomainMessage::Auth(auth::AuthMessage::Back)),
    );

    let card = auth_card(content.align_x(Alignment::Center));
    auth_container(card).into()
}
