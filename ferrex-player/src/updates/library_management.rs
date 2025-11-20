use crate::{
    media_library::{self, Library},
    messages::library::Message,
    state::State,
};
use iced::Task;
use uuid::Uuid;

pub fn handle_create_library(state: &mut State, library: Library) -> Task<Message> {
    let server_url = state.server_url.clone();
    Task::perform(
        media_library::create_library(server_url, library),
        |result| match result {
            Ok(created_library) => Message::LibraryCreated(Ok(created_library)),
            Err(e) => Message::LibraryCreated(Err(e.to_string())),
        },
    )
}

pub fn handle_library_created(state: &mut State, result: Result<Library, String>) -> Task<Message> {
    match result {
        Ok(library) => {
            log::info!("Created library: {}", library.name);
            state.libraries.push(library);
            state.error_message = None;
            state.library_form_data = None; // Close form on success
            state.library_form_errors.clear();
        }
        Err(e) => {
            log::error!("Failed to create library: {}", e);
            state.library_form_errors.clear();
            state
                .library_form_errors
                .push(format!("Failed to create library: {}", e));
        }
    }
    Task::none()
}

pub fn handle_update_library(state: &mut State, library: Library) -> Task<Message> {
    let server_url = state.server_url.clone();
    Task::perform(
        media_library::update_library(server_url, library),
        |result| match result {
            Ok(updated_library) => Message::LibraryUpdated(Ok(updated_library)),
            Err(e) => Message::LibraryUpdated(Err(e.to_string())),
        },
    )
}

pub fn handle_library_updated(state: &mut State, result: Result<Library, String>) -> Task<Message> {
    match result {
        Ok(library) => {
            log::info!("Updated library: {}", library.name);
            if let Some(index) = state.libraries.iter().position(|l| l.id == library.id) {
                state.libraries[index] = library;
            }
            state.error_message = None;
            state.library_form_data = None; // Close form on success
            state.library_form_errors.clear();
        }
        Err(e) => {
            log::error!("Failed to update library: {}", e);
            state.library_form_errors.clear();
            state
                .library_form_errors
                .push(format!("Failed to update library: {}", e));
        }
    }
    Task::none()
}

pub fn handle_delete_library(state: &mut State, library_id: Uuid) -> Task<Message> {
    let server_url = state.server_url.clone();
    let id_for_response = library_id.clone();
    Task::perform(
        media_library::delete_library(server_url, library_id),
        move |result| match result {
            Ok(()) => Message::LibraryDeleted(Ok(id_for_response)),
            Err(e) => Message::LibraryDeleted(Err(e.to_string())),
        },
    )
}

pub fn handle_library_deleted(state: &mut State, result: Result<Uuid, String>) -> Task<Message> {
    match result {
        Ok(library_id) => {
            log::info!("Deleted library: {}", library_id);
            state.libraries.retain(|l| l.id != library_id);

            // If we deleted the current library, clear selection
            if state.current_library_id.as_ref() == Some(&library_id) {
                state.current_library_id = None;

                // Clear the library from MediaStore
                if let Ok(mut store) = state.media_store.write() {
                    store.clear_library(library_id);
                }

                // Update ViewModels to reflect no current library
                state.all_view_model.set_library_filter(None);
                state.movies_view_model.set_library_filter(None);
                state.tv_view_model.set_library_filter(None);
            }

            // Remove from cache
            state.library_media_cache.remove(&library_id);

            state.error_message = None;
        }
        Err(e) => {
            log::error!("Failed to delete library: {}", e);
            state.error_message = Some(format!("Failed to delete library: {}", e));
        }
    }
    Task::none()
}

// Library form management
pub fn handle_show_library_form(state: &mut State, library: Option<Library>) -> Task<Message> {
    state.library_form_errors.clear();
    state.library_form_data = Some(match library {
        Some(lib) => {
            // Editing existing library
            crate::state::LibraryFormData {
                id: lib.id,
                name: lib.name,
                library_type: match lib.library_type {
                    crate::api_types::LibraryType::Movies => "Movies".to_string(),
                    crate::api_types::LibraryType::TvShows => "TvShows".to_string(),
                },
                paths: lib.paths.join(", "),
                scan_interval_minutes: lib.scan_interval_minutes.to_string(),
                enabled: lib.enabled,
                editing: true,
            }
        }
        None => {
            // Creating new library
            crate::state::LibraryFormData {
                id: Uuid::now_v7(),
                name: String::new(),
                library_type: "Movies".to_string(),
                paths: String::new(),
                scan_interval_minutes: "60".to_string(),
                enabled: true,
                editing: false,
            }
        }
    });
    Task::none()
}

pub fn handle_hide_library_form(state: &mut State) -> Task<Message> {
    state.library_form_data = None;
    state.library_form_errors.clear();
    Task::none()
}

pub fn handle_update_libarary_form_name(state: &mut State, name: String) -> Task<Message> {
    if let Some(ref mut form_data) = state.library_form_data {
        form_data.name = name;
    }
    Task::none()
}

pub fn handle_update_library_form_type(state: &mut State, library_type: String) -> Task<Message> {
    if let Some(ref mut form_data) = state.library_form_data {
        form_data.library_type = library_type;
    }
    Task::none()
}

pub fn handle_update_library_form_paths(state: &mut State, paths: String) -> Task<Message> {
    if let Some(ref mut form_data) = state.library_form_data {
        form_data.paths = paths;
    }
    Task::none()
}

pub fn handle_update_library_form_scan_interval(
    state: &mut State,
    interval: String,
) -> Task<Message> {
    if let Some(ref mut form_data) = state.library_form_data {
        form_data.scan_interval_minutes = interval;
    }
    Task::none()
}

pub fn handle_toggle_library_form_enabled(state: &mut State) -> Task<Message> {
    if let Some(ref mut form_data) = state.library_form_data {
        form_data.enabled = !form_data.enabled;
    }
    Task::none()
}

pub fn handle_submit_library_form(state: &mut State) -> Task<Message> {
    if let Some(ref form_data) = state.library_form_data {
        // Validate form
        state.library_form_errors.clear();

        if form_data.name.trim().is_empty() {
            state
                .library_form_errors
                .push("Library name is required".to_string());
        }

        if form_data.paths.trim().is_empty() {
            state
                .library_form_errors
                .push("At least one path is required".to_string());
        }

        if let Err(_) = form_data.scan_interval_minutes.parse::<u32>() {
            state
                .library_form_errors
                .push("Scan interval must be a valid number".to_string());
        }

        if !state.library_form_errors.is_empty() {
            return Task::none();
        }

        // Create library object from form data
        // Convert string library type to enum
        let library_type = match form_data.library_type.as_str() {
            "Movies" => crate::api_types::LibraryType::Movies,
            "TvShows" => crate::api_types::LibraryType::TvShows,
            _ => {
                state
                    .library_form_errors
                    .push("Invalid library type".to_string());
                return Task::none();
            }
        };

        let library = media_library::Library {
            id: if form_data.editing {
                form_data.id.clone()
            } else {
                Uuid::now_v7()
            },
            name: form_data.name.trim().to_string(),
            library_type,
            paths: form_data
                .paths
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            scan_interval_minutes: form_data.scan_interval_minutes.parse().unwrap_or(60),
            last_scan: None,
            enabled: form_data.enabled,
            media: Vec::new(), // Empty initially
        };

        if form_data.editing {
            // Update existing library
            let server_url = state.server_url.clone();
            Task::perform(
                media_library::update_library(server_url, library),
                |result| match result {
                    Ok(updated_library) => Message::LibraryUpdated(Ok(updated_library)),
                    Err(e) => Message::LibraryUpdated(Err(e.to_string())),
                },
            )
        } else {
            // Create new library
            let server_url = state.server_url.clone();
            Task::perform(
                media_library::create_library(server_url, library),
                |result| match result {
                    Ok(created_library) => Message::LibraryCreated(Ok(created_library)),
                    Err(e) => Message::LibraryCreated(Err(e.to_string())),
                },
            )
        }
    } else {
        Task::none()
    }
}
