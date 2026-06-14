use iced::Task;

use crate::{
    domains::library::messages::LibraryMessage,
    infra::api_types::DemoResetRequest, state::State,
};

pub fn handle_fetch_demo_status(state: &mut State) -> Task<LibraryMessage> {
    state.domains.library.state.demo_controls.is_loading = true;
    state.domains.library.state.demo_controls.error = None;

    if let Some(api_service) = state.domains.library.state.api_service.clone() {
        Task::perform(
            async move {
                api_service
                    .fetch_demo_status()
                    .await
                    .map_err(|e| e.to_string())
            },
            LibraryMessage::DemoStatusLoaded,
        )
    } else {
        state.domains.library.state.demo_controls.is_loading = false;
        state.domains.library.state.demo_controls.error =
            Some("Demo API unavailable".to_string());
        Task::done(LibraryMessage::DemoStatusLoaded(Err(
            "Demo API unavailable".into(),
        )))
    }
}

pub fn handle_apply_demo_sizing(
    state: &mut State,
    request: DemoResetRequest,
) -> Task<LibraryMessage> {
    state.domains.library.state.demo_controls.is_updating = true;
    state.domains.library.state.demo_controls.error = None;

    if let Some(api_service) = state.domains.library.state.api_service.clone() {
        Task::perform(
            async move {
                api_service
                    .resize_demo(request)
                    .await
                    .map_err(|e| e.to_string())
            },
            LibraryMessage::DemoSizingApplied,
        )
    } else {
        state.domains.library.state.demo_controls.is_updating = false;
        state.domains.library.state.demo_controls.error =
            Some("Demo API unavailable".to_string());
        Task::done(LibraryMessage::DemoSizingApplied(Err(
            "Demo API unavailable".into(),
        )))
    }
}
