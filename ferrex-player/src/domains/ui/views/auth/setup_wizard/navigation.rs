//! Navigation components for the setup wizard

use crate::common::messages::DomainMessage;
use crate::domains::auth::messages as auth;
use crate::domains::auth::types::SetupStep;
use crate::domains::ui::views::auth::components::{
    primary_button, secondary_button,
};
use crate::infra::design_tokens::FontTokens;
use iced::widget::{Space, container, row};
use iced::{Alignment, Element, Length, Theme};

/// Progress indicator showing current step
pub fn view_progress_indicator<'a>(
    current_step: &SetupStep,
    setup_token_required: bool,
    _fonts: &FontTokens,
) -> Element<'a, DomainMessage> {
    let total = SetupStep::total_steps(setup_token_required);
    let current_idx = current_step.index(setup_token_required);

    let mut dots = row![].spacing(12).align_y(Alignment::Center);

    for i in 0..total {
        let is_current = i == current_idx;
        let is_done = i < current_idx;

        // Create dot with connecting line
        if i > 0 {
            // Add connecting line before each dot (except first)
            dots =
                dots.push(container(Space::new().width(20).height(2)).style(
                    move |theme: &Theme| {
                        let palette = theme.extended_palette();
                        container::Style {
                            background: Some(
                                if is_done || is_current {
                                    palette.primary.weak.color
                                } else {
                                    palette.background.strong.color
                                }
                                .into(),
                            ),
                            ..Default::default()
                        }
                    },
                ));
        }

        // Create the dot
        let dot = container(Space::new().width(10).height(10)).style(
            move |theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(
                        if is_current {
                            palette.primary.base.color
                        } else if is_done {
                            palette.primary.weak.color
                        } else {
                            palette.background.strong.color
                        }
                        .into(),
                    ),
                    border: iced::Border {
                        radius: 5.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            },
        );

        dots = dots.push(dot);
    }

    container(dots)
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .into()
}

/// Navigation buttons (Back, Next/Submit/Skip)
pub fn view_navigation_buttons<'a>(
    current_step: &SetupStep,
    setup_token_required: bool,
    loading: bool,
    fonts: &FontTokens,
) -> Element<'a, DomainMessage> {
    let mut nav = row![].spacing(16).width(Length::Fill);

    // Back button (not shown on Welcome or Complete steps)
    if current_step.previous(setup_token_required).is_some() {
        nav = nav.push(
            secondary_button("Back", fonts.body)
                .on_press(DomainMessage::Auth(
                    auth::AuthMessage::SetupPreviousStep,
                ))
                .width(Length::FillPortion(1)),
        );
    } else {
        nav = nav.push(Space::new().width(Length::FillPortion(1)));
    }

    // Primary action button
    match current_step {
        SetupStep::Welcome => {
            nav = nav.push(
                primary_button("Get Started", fonts.body)
                    .on_press(DomainMessage::Auth(
                        auth::AuthMessage::SetupNextStep,
                    ))
                    .width(Length::FillPortion(1)),
            );
        }
        SetupStep::Account | SetupStep::SetupToken => {
            nav = nav.push(
                primary_button("Continue", fonts.body)
                    .on_press(DomainMessage::Auth(
                        auth::AuthMessage::SetupNextStep,
                    ))
                    .width(Length::FillPortion(1)),
            );
        }
        SetupStep::DeviceClaim => {
            // Continue button - validation happens in update handler
            nav = nav.push(
                primary_button("Continue", fonts.body)
                    .on_press(DomainMessage::Auth(
                        auth::AuthMessage::SetupNextStep,
                    ))
                    .width(Length::FillPortion(1)),
            );
        }
        SetupStep::Pin => {
            // Two buttons: Skip and Set PIN
            nav = nav.push(
                secondary_button("Skip", fonts.body)
                    .on_press(DomainMessage::Auth(
                        auth::AuthMessage::SkipPinSetup,
                    ))
                    .width(Length::FillPortion(1)),
            );
            nav = nav.push(
                primary_button("Set PIN", fonts.body)
                    .on_press(DomainMessage::Auth(
                        auth::AuthMessage::SetupNextStep,
                    ))
                    .width(Length::FillPortion(1)),
            );
        }
        SetupStep::Complete => {
            let label = if loading {
                "Creating..."
            } else {
                "Get Started"
            };
            let mut btn =
                primary_button(label, fonts.body).width(Length::FillPortion(1));
            if !loading {
                btn = btn.on_press(DomainMessage::Auth(
                    auth::AuthMessage::SubmitSetup,
                ));
            }
            nav = nav.push(btn);
        }
    }

    container(nav)
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .into()
}
