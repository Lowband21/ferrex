//! Libraries section update handlers (Admin)

use super::{
    messages::{LibrariesMessage, ScanStatus},
    state::LibrarySummary,
};

use crate::{common::messages::DomainUpdateResult, state::State};

use ferrex_model::LibraryType;
use uuid::Uuid;

/// Main message handler for libraries section
pub fn handle_message(
    state: &mut State,
    message: LibrariesMessage,
) -> DomainUpdateResult {
    match message {
        // Library List
        LibrariesMessage::LoadLibraries => handle_load_libraries(state),
        LibrariesMessage::LibrariesLoaded(result) => {
            handle_libraries_loaded(state, result)
        }
        LibrariesMessage::SelectLibrary(id) => handle_select_library(state, id),
        LibrariesMessage::DeleteLibrary(id) => handle_delete_library(state, id),
        LibrariesMessage::DeleteResult(result) => {
            handle_delete_result(state, result)
        }

        // Scan Controls
        LibrariesMessage::StartScan(id) => handle_start_scan(state, id),
        LibrariesMessage::PauseScan(id) => handle_pause_scan(state, id),
        LibrariesMessage::CancelScan(id) => handle_cancel_scan(state, id),
        LibrariesMessage::ScanStatusUpdated(id, status) => {
            handle_scan_status_updated(state, id, status)
        }

        // Library Form
        LibrariesMessage::ShowAddForm => handle_show_add_form(state),
        LibrariesMessage::ShowEditForm(id) => handle_show_edit_form(state, id),
        LibrariesMessage::UpdateFormName(name) => {
            handle_update_form_name(state, name)
        }
        LibrariesMessage::UpdateFormPath(path) => {
            handle_update_form_path(state, path)
        }
        LibrariesMessage::UpdateFormType(lib_type) => {
            handle_update_form_type(state, lib_type)
        }
        LibrariesMessage::BrowseForPath => handle_browse_for_path(state),
        LibrariesMessage::PathSelected(path) => {
            handle_path_selected(state, path)
        }
        LibrariesMessage::SubmitForm => handle_submit_form(state),
        LibrariesMessage::FormResult(result) => {
            handle_form_result(state, result)
        }
        LibrariesMessage::CancelForm => handle_cancel_form(state),
    }
}

// Library List handlers
fn handle_load_libraries(state: &mut State) -> DomainUpdateResult {
    // TODO: Load libraries from API
    let _ = state;
    DomainUpdateResult::none()
}

fn handle_libraries_loaded(
    state: &mut State,
    result: Result<Vec<LibrarySummary>, String>,
) -> DomainUpdateResult {
    let _ = (state, result);
    DomainUpdateResult::none()
}

fn handle_select_library(state: &mut State, id: Uuid) -> DomainUpdateResult {
    let _ = (state, id);
    DomainUpdateResult::none()
}

fn handle_delete_library(state: &mut State, id: Uuid) -> DomainUpdateResult {
    let _ = (state, id);
    DomainUpdateResult::none()
}

fn handle_delete_result(
    state: &mut State,
    result: Result<Uuid, String>,
) -> DomainUpdateResult {
    let _ = (state, result);
    DomainUpdateResult::none()
}

// Scan Control handlers
fn handle_start_scan(state: &mut State, id: Uuid) -> DomainUpdateResult {
    let _ = (state, id);
    DomainUpdateResult::none()
}

fn handle_pause_scan(state: &mut State, id: Uuid) -> DomainUpdateResult {
    let _ = (state, id);
    DomainUpdateResult::none()
}

fn handle_cancel_scan(state: &mut State, id: Uuid) -> DomainUpdateResult {
    let _ = (state, id);
    DomainUpdateResult::none()
}

fn handle_scan_status_updated(
    state: &mut State,
    id: Uuid,
    status: ScanStatus,
) -> DomainUpdateResult {
    let _ = (state, id, status);
    DomainUpdateResult::none()
}

// Library Form handlers
fn handle_show_add_form(state: &mut State) -> DomainUpdateResult {
    let _ = state;
    DomainUpdateResult::none()
}

fn handle_show_edit_form(state: &mut State, id: Uuid) -> DomainUpdateResult {
    let _ = (state, id);
    DomainUpdateResult::none()
}

fn handle_update_form_name(
    state: &mut State,
    name: String,
) -> DomainUpdateResult {
    let _ = (state, name);
    DomainUpdateResult::none()
}

fn handle_update_form_path(
    state: &mut State,
    path: String,
) -> DomainUpdateResult {
    let _ = (state, path);
    DomainUpdateResult::none()
}

fn handle_update_form_type(
    state: &mut State,
    lib_type: LibraryType,
) -> DomainUpdateResult {
    let _ = (state, lib_type);
    DomainUpdateResult::none()
}

fn handle_browse_for_path(state: &mut State) -> DomainUpdateResult {
    let _ = state;
    DomainUpdateResult::none()
}

fn handle_path_selected(
    state: &mut State,
    path: Option<String>,
) -> DomainUpdateResult {
    let _ = (state, path);
    DomainUpdateResult::none()
}

fn handle_submit_form(state: &mut State) -> DomainUpdateResult {
    let _ = state;
    DomainUpdateResult::none()
}

fn handle_form_result(
    state: &mut State,
    result: Result<Uuid, String>,
) -> DomainUpdateResult {
    let _ = (state, result);
    DomainUpdateResult::none()
}

fn handle_cancel_form(state: &mut State) -> DomainUpdateResult {
    let _ = state;
    DomainUpdateResult::none()
}
