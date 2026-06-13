use crate::{
    common::messages::DomainUpdateResult,
    domains::ui::{
        shell_ui::Scope,
        tabs::TabId,
        update_handlers::{
            home_tab, home_tab::emit_initial_all_tab_snapshots_combined,
        },
        utils::bump_keep_alive,
        view_model_ui::ViewModelMessage,
    },
    state::State,
};

use iced::Task;

pub fn update_view_model_ui(
    state: &mut State,
    message: ViewModelMessage,
) -> DomainUpdateResult {
    match message {
        ViewModelMessage::RefreshViewModels => {
            bump_keep_alive(state);
            // Update library filters based on current display mode
            let library_filter = match state.domains.ui.state.scope {
                Scope::Home => None, // Show all libraries
                Scope::Library(id) => Some(id),
            };

            // Sync UI domain's library ID with the determined filter
            // This ensures UI domain state matches what ViewModels will use
            // TODO: This should not be necessary once we properly handle current library ID
            if matches!(state.domains.ui.state.scope, Scope::Library(_))
                && library_filter != state.domains.ui.state.current_library_id
            {
                log::warn!(
                    "UI: Syncing UI domain library ID from {:?} to {:?}",
                    state.domains.ui.state.current_library_id,
                    library_filter
                );
                state.domains.ui.state.current_library_id = library_filter;
            }

            state
                .tab_manager
                .mark_tab_needs_refresh(state.tab_manager.active_tab_id());
            state.tab_manager.refresh_active_tab();

            // After view models refresh, if we're in Home,
            // (re)initialize carousels and emit initial snapshots so images load immediately.
            if matches!(state.domains.ui.state.scope, Scope::Home)
                && matches!(state.tab_manager.active_tab_id(), TabId::Home)
            {
                home_tab::init_all_tab_view(state);
                emit_initial_all_tab_snapshots_combined(state);
            }

            DomainUpdateResult::task(Task::none())
        }
        ViewModelMessage::UpdateViewModelFilters => {
            bump_keep_alive(state);
            let library_filter = match state.domains.ui.state.scope {
                Scope::Library(id) => Some(id),
                Scope::Home => None,
            };

            log::debug!(
                "UI: UpdateViewModelFilters called - library_filter = {:?}, scope = {:?}, ui.current_library_id = {:?}",
                library_filter,
                state.domains.ui.state.scope,
                state.domains.ui.state.current_library_id
            );

            DomainUpdateResult::task(Task::none()) // View will update on next frame
        }
    }
}
