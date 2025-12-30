//! Setup wizard for first-run admin creation
//!
//! A card carousel-style wizard with horizontal sliding transitions.

mod navigation;
mod steps;

use crate::common::messages::DomainMessage;
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::auth::types::{
    SetupClaimStatus, SetupStep, TransitionDirection,
};
use crate::domains::ui::views::auth::components::{auth_card, auth_container};
use crate::state::State;
use iced::widget::{Space, column, container, scrollable};
use iced::{Alignment, Element, Length};

/// Main setup wizard view
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_setup_wizard<'a>(
    state: &'a State,
    current_step: &SetupStep,
    username: &'a str,
    password: &'a SecureCredential,
    confirm_password: &'a SecureCredential,
    display_name: &'a str,
    setup_token: &'a str,
    show_password: bool,
    claim_code: Option<&'a str>,
    claim_status: &'a SetupClaimStatus,
    claim_loading: bool,
    pin: &'a SecureCredential,
    confirm_pin: &'a SecureCredential,
    error: Option<&'a str>,
    loading: bool,
    setup_token_required: bool,
    _transition_direction: &TransitionDirection,
    _transition_progress: f32,
) -> Element<'a, DomainMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let spacing_tokens = &state.domains.ui.state.size_provider.spacing;

    // Build progress indicator
    let progress = navigation::view_progress_indicator(
        current_step,
        setup_token_required,
        fonts,
    );

    // Build step content
    let step_content: Element<'a, DomainMessage> = match current_step {
        SetupStep::Welcome => steps::view_welcome_step(fonts),
        SetupStep::Account => steps::view_account_step(
            username,
            display_name,
            password,
            confirm_password,
            show_password,
            error,
            fonts,
        ),
        SetupStep::SetupToken => {
            steps::view_setup_token_step(setup_token, error, fonts)
        }
        SetupStep::DeviceClaim => steps::view_device_claim_step(
            claim_code,
            claim_status,
            claim_loading,
            error,
            fonts,
        ),
        SetupStep::Pin => steps::view_pin_step(state, pin, confirm_pin, error),
        SetupStep::Complete => steps::view_complete_step(fonts),
    };

    // Make step content scrollable for overflow handling
    let scrollable_content = scrollable(step_content)
        .width(Length::Fill)
        .height(Length::FillPortion(1));

    // Build navigation buttons
    let nav_buttons = navigation::view_navigation_buttons(
        current_step,
        setup_token_required,
        loading,
        fonts,
    );

    // Compose the wizard layout
    let content = column![
        progress,
        Space::new().height(spacing_tokens.lg),
        scrollable_content,
        Space::new().height(spacing_tokens.md),
        nav_buttons,
    ]
    .spacing(0)
    .align_x(Alignment::Center)
    .width(Length::Fill)
    .height(Length::FillPortion(2));

    // Wrap in auth container (centered on screen)
    let card = auth_card(
        container(content)
            .padding(24)
            .width(Length::Fill)
            .height(Length::FillPortion(2)),
    );

    auth_container(card).into()
}
