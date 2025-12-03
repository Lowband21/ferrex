//! Authentication views module
//!
//! This module provides all authentication-related UI components in a modular way.
//! Each authentication flow state has its own view component.

mod components;
mod credential_entry;
mod loading_users;
mod pin_setup;
mod setup_wizard;
mod user_carousel;
mod user_selection;

use crate::common::messages::DomainMessage;
use crate::domains::auth::types::AuthenticationFlow;
use crate::domains::ui::views::auth::credential_entry::view_pre_auth_login;
use ferrex_core::player_prelude::UserPermissions;
use iced::Element;

pub use components::*;
pub use credential_entry::view_credential_entry;
pub use loading_users::view_loading_users;
pub use pin_setup::view_pin_setup;
pub use user_carousel::{
    UserCarouselMessage, UserCarouselState, view_user_carousel,
    view_user_selection_with_carousel,
};
pub use user_selection::view_user_selection;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_auth<'a>(
    state: &'a crate::state::State,
    auth_flow: &'a AuthenticationFlow,
    user_permissions: Option<&'a UserPermissions>,
) -> Element<'a, DomainMessage> {
    use AuthenticationFlow::*;

    match auth_flow {
        CheckingSetup => view_loading_users(state), // Show loading while checking

        CheckingAutoLogin => view_loading_users(state), // Show loading while checking auto-login

        PreAuthLogin {
            username,
            password,
            show_password,
            remember_device,
            error,
            loading,
        } => view_pre_auth_login(
            state,
            username,
            password,
            *show_password,
            *remember_device,
            error.as_deref(),
            *loading,
        ),

        FirstRunSetup {
            current_step,
            username,
            password,
            confirm_password,
            display_name,
            setup_token,
            show_password,
            claim_code,
            claim_status,
            claim_loading,
            pin,
            confirm_pin,
            error,
            loading,
            setup_token_required,
            transition_direction,
            transition_progress,
            ..
        } => setup_wizard::view_setup_wizard(
            state,
            current_step,
            username,
            password,
            confirm_password,
            display_name,
            setup_token,
            *show_password,
            claim_code.as_deref(),
            claim_status,
            *claim_loading,
            pin,
            confirm_pin,
            error.as_deref(),
            *loading,
            *setup_token_required,
            transition_direction,
            *transition_progress,
        ),

        LoadingUsers => view_loading_users(state),

        SelectingUser { users, error } => view_user_selection_with_carousel(
            state,
            users,
            error.as_deref(),
            user_permissions,
        ),

        CheckingDevice { user } => {
            view_loading_users(state) // Show loading while checking device
        }

        EnteringCredentials {
            user,
            input_type,
            input,
            show_password,
            remember_device,
            error,
            attempts_remaining,
            loading,
        } => view_credential_entry(
            state,
            user,
            input_type,
            input,
            *show_password,
            *remember_device,
            error.as_deref(),
            *attempts_remaining,
            *loading,
        ),

        SettingUpPin {
            user,
            pin,
            confirm_pin,
            error,
        } => view_pin_setup(state, user, pin, confirm_pin, error.as_deref()),

        Authenticated { user, mode } => {
            // This state should not render auth views - the app should show main content
            view_loading_users(state) // Fallback
        }
    }
}
