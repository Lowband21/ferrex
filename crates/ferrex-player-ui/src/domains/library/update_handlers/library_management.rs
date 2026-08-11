use crate::domains::library::messages::LibraryMessage;
use crate::infra::api_types::Library;
use crate::state::State;
use chrono::Utc;
use ferrex_core::{
    api::types::{
        CreateLibraryRequest, ResetLibraryRequest, UpdateLibraryRequest,
    },
    types::{ids::LibraryId, library::LibraryType},
};
use ferrex_model::{DEFAULT_SCAN_INTERVAL_MINUTES, MovieReferenceBatchSize};
use iced::Task;
use serde_json::json;
use std::path::PathBuf;

fn create_library_request(
    library: &Library,
    start_scan: bool,
) -> CreateLibraryRequest {
    serde_json::from_value(json!({
        "name": library.name,
        "library_type": library.library_type,
        "paths": library
            .paths
            .iter()
            .filter_map(|p| p.to_str().map(|s| s.to_string()))
            .collect::<Vec<_>>(),
        "scan_interval_minutes": library.scan_interval_minutes,
        "enabled": library.enabled,
        "auto_scan": library.auto_scan,
        "watch_for_changes": library.watch_for_changes,
        "analyze_on_scan": library.analyze_on_scan,
        "max_retry_attempts": library.max_retry_attempts,
        "movie_ref_batch_size": library.movie_ref_batch_size.get(),
        "start_scan": start_scan,
    }))
    .expect("serialized library create request must match API schema")
}

fn update_library_request(library: &Library) -> UpdateLibraryRequest {
    serde_json::from_value(json!({
        "name": library.name,
        "paths": library
            .paths
            .iter()
            .filter_map(|p| p.to_str().map(|s| s.to_string()))
            .collect::<Vec<_>>(),
        "scan_interval_minutes": library.scan_interval_minutes,
        "enabled": library.enabled,
        "auto_scan": library.auto_scan,
        "watch_for_changes": library.watch_for_changes,
        "analyze_on_scan": library.analyze_on_scan,
        "max_retry_attempts": library.max_retry_attempts,
    }))
    .expect("serialized library update request must match API schema")
}

pub fn handle_create_library(
    state: &mut State,
    library: Library,
    start_scan: bool,
    _server_url: String,
) -> Task<LibraryMessage> {
    let req = create_library_request(&library, start_scan);

    let api = state.api_service.clone();
    Task::perform(
        async move { api.create_library(req).await.map_err(|e| e.to_string()) },
        |result| match result {
            Ok(_id) => LibraryMessage::LibraryCreated(Ok(library)),
            Err(e) => LibraryMessage::LibraryCreated(Err(e)),
        },
    )
}

pub fn handle_library_created(
    state: &mut State,
    result: Result<Library, String>,
) -> Task<LibraryMessage> {
    match result {
        Ok(library) => {
            log::info!("Created library successfully; refreshing libraries");
            state.domains.library.state.library_form_data = None; // Close form on success
            state.domains.library.state.library_form_errors.clear();
            state.domains.library.state.library_form_success = Some(format!(
                "Library \"{}\" created successfully",
                library.name
            ));
            Task::perform(
                super::library_loaded::fetch_libraries(
                    state.api_service.clone(),
                    state.disk_media_repo_cache.clone(),
                ),
                |res| {
                    LibraryMessage::LibrariesLoaded(
                        res.map_err(|e| e.to_string()),
                    )
                },
            )
        }
        Err(e) => {
            log::error!("Failed to create library: {}", e);
            state.domains.library.state.library_form_errors.clear();
            state.domains.library.state.library_form_success = None;
            state
                .domains
                .library
                .state
                .library_form_errors
                .push(format!("Failed to create library: {}", e));
            Task::none()
        }
    }
}

pub fn handle_update_library(
    state: &mut State,
    library: Library,
    _server_url: String,
) -> Task<LibraryMessage> {
    let req = update_library_request(&library);

    let api = state.api_service.clone();
    let id = library.id;
    Task::perform(
        async move { api.update_library(id, req).await.map_err(|e| e.to_string()) },
        move |result| match result {
            Ok(()) => LibraryMessage::LibraryUpdated(Ok(library)),
            Err(e) => LibraryMessage::LibraryUpdated(Err(e)),
        },
    )
}

pub fn handle_library_updated(
    state: &mut State,
    result: Result<Library, String>,
) -> Task<LibraryMessage> {
    match result {
        Ok(library) => {
            log::info!(
                "Updated library: {} - refreshing libraries",
                library.name
            );
            state.domains.library.state.library_form_data = None; // Close form on success
            state.domains.library.state.library_form_errors.clear();
            // Trigger reload of libraries
            Task::perform(
                super::library_loaded::fetch_libraries(
                    state.api_service.clone(),
                    state.disk_media_repo_cache.clone(),
                ),
                |res| {
                    LibraryMessage::LibrariesLoaded(
                        res.map_err(|e| e.to_string()),
                    )
                },
            )
        }
        Err(e) => {
            log::error!("Failed to update library: {}", e);
            state.domains.library.state.library_form_errors.clear();
            state
                .domains
                .library
                .state
                .library_form_errors
                .push(format!("Failed to update library: {}", e));
            Task::none()
        }
    }
}

pub fn handle_delete_library(
    state: &mut State,
    library_id: LibraryId,
    _server_url: String,
) -> Task<LibraryMessage> {
    state.domains.ui.state.error_message = None;
    state.domains.library.state.library_form_errors.clear();
    state.domains.library.state.library_form_success = None;
    let api = state.api_service.clone();
    Task::perform(
        async move {
            api.delete_library(library_id)
                .await
                .map_err(|e| e.to_string())
        },
        move |result| match result {
            Ok(()) => LibraryMessage::LibraryDeleted(Ok(library_id)),
            Err(e) => LibraryMessage::LibraryDeleted(Err(e)),
        },
    )
}

pub fn handle_library_deleted(
    state: &mut State,
    result: Result<LibraryId, String>,
) -> Task<LibraryMessage> {
    state.domains.ui.state.library_maintenance_in_flight = None;
    match result {
        Ok(library_id) => {
            log::info!(
                "Deleted library: {} - refreshing libraries",
                library_id
            );
            if state.domains.ui.state.scope.lib_id() == Some(library_id) {
                state.domains.ui.state.scope =
                    crate::domains::ui::shell_ui::Scope::Home;
            }
            state.domains.library.state.library_form_errors.clear();
            state.domains.ui.state.error_message = None;
            state.domains.library.state.library_form_success =
                Some("Library deleted successfully".to_string());
            Task::batch([
                Task::perform(
                    super::library_loaded::fetch_libraries(
                        state.api_service.clone(),
                        state.disk_media_repo_cache.clone(),
                    ),
                    |res| {
                        LibraryMessage::LibrariesLoaded(
                            res.map_err(|e| e.to_string()),
                        )
                    },
                ),
                Task::done(LibraryMessage::FetchActiveScans),
            ])
        }
        Err(e) => {
            log::error!("Failed to delete library: {}", e);
            let message = format!("Failed to delete library: {}", e);
            state.domains.library.state.library_form_success = None;
            state.domains.library.state.library_form_errors.clear();
            state
                .domains
                .library
                .state
                .library_form_errors
                .push(message.clone());
            state.domains.ui.state.error_message = Some(message);
            Task::none()
        }
    }
}

pub fn handle_show_library_form(
    state: &mut State,
    library: Option<Library>,
) -> Task<LibraryMessage> {
    state.domains.library.state.library_form_errors.clear();
    state.domains.library.state.library_form_success = None;
    state.domains.ui.state.error_message = None;
    state.domains.library.state.library_form_data = Some(match library {
        Some(lib) => {
            // Editing existing library
            crate::domains::library::types::LibraryFormData {
                id: lib.id,
                name: lib.name,
                library_type: match lib.library_type {
                    LibraryType::Movies => "Movies".to_string(),
                    LibraryType::Series => "TvShows".to_string(),
                },
                paths: lib
                    .paths
                    .iter()
                    .map(|path| {
                        String::from(path.to_str().unwrap_or("Invalid Path"))
                    })
                    .collect(),
                scan_interval_minutes: lib.scan_interval_minutes.to_string(),
                enabled: lib.enabled,
                auto_scan: lib.auto_scan,
                watch_for_changes: lib.watch_for_changes,
                editing: true,
                start_scan: true,
            }
        }
        None => {
            // Creating new library
            crate::domains::library::types::LibraryFormData {
                id: LibraryId::new(),
                name: String::new(),
                library_type: "Movies".to_string(),
                paths: String::new(),
                scan_interval_minutes: DEFAULT_SCAN_INTERVAL_MINUTES
                    .to_string(),
                enabled: true,
                auto_scan: true,
                watch_for_changes: true,
                editing: false,
                start_scan: true,
            }
        }
    });
    Task::none()
}

pub fn handle_hide_library_form(state: &mut State) -> Task<LibraryMessage> {
    state.domains.library.state.library_form_data = None;
    state.domains.library.state.library_form_errors.clear();
    Task::none()
}

/// Ask the server to atomically clear the library while preserving its identity
/// and settings, then start a fresh bulk scan.
pub fn handle_reset_library(
    state: &mut State,
    library_id: LibraryId,
) -> Task<LibraryMessage> {
    state.domains.library.state.library_form_errors.clear();
    state.domains.library.state.library_form_success = None;
    state.domains.ui.state.error_message = None;
    let api = state.api_service.clone();

    Task::perform(
        async move {
            api.reset_library(library_id, ResetLibraryRequest::default())
                .await
                .map_err(|e| e.to_string())
        },
        |result| match result {
            Ok(reset) => LibraryMessage::ResetLibraryDone(Ok(reset)),
            Err(err) => LibraryMessage::ResetLibraryDone(Err(err)),
        },
    )
}

pub fn handle_update_libarary_form_name(
    state: &mut State,
    name: String,
) -> Task<LibraryMessage> {
    if let Some(ref mut form_data) =
        state.domains.library.state.library_form_data
    {
        form_data.name = name;
    }
    Task::none()
}

pub fn handle_update_library_form_type(
    state: &mut State,
    library_type: String,
) -> Task<LibraryMessage> {
    if let Some(ref mut form_data) =
        state.domains.library.state.library_form_data
    {
        form_data.library_type = library_type;
    }
    Task::none()
}

pub fn handle_update_library_form_paths(
    state: &mut State,
    paths: String,
) -> Task<LibraryMessage> {
    if let Some(ref mut form_data) =
        state.domains.library.state.library_form_data
    {
        form_data.paths = paths;
    }
    Task::none()
}

pub fn handle_update_library_form_scan_interval(
    state: &mut State,
    interval: String,
) -> Task<LibraryMessage> {
    if let Some(ref mut form_data) =
        state.domains.library.state.library_form_data
    {
        form_data.scan_interval_minutes = interval;
    }
    Task::none()
}

pub fn handle_toggle_library_form_enabled(
    state: &mut State,
) -> Task<LibraryMessage> {
    if let Some(ref mut form_data) =
        state.domains.library.state.library_form_data
    {
        form_data.enabled = !form_data.enabled;
    }
    Task::none()
}

pub fn handle_toggle_library_form_auto_scan(
    state: &mut State,
) -> Task<LibraryMessage> {
    if let Some(ref mut form_data) =
        state.domains.library.state.library_form_data
    {
        form_data.auto_scan = !form_data.auto_scan;
    }
    Task::none()
}

pub fn handle_toggle_library_form_watch_for_changes(
    state: &mut State,
) -> Task<LibraryMessage> {
    if let Some(ref mut form_data) =
        state.domains.library.state.library_form_data
    {
        form_data.watch_for_changes = !form_data.watch_for_changes;
    }
    Task::none()
}

pub fn handle_toggle_library_form_start_scan(
    state: &mut State,
) -> Task<LibraryMessage> {
    if let Some(ref mut form_data) =
        state.domains.library.state.library_form_data
    {
        form_data.start_scan = !form_data.start_scan;
    }
    Task::none()
}

pub fn handle_submit_library_form(state: &mut State) -> Task<LibraryMessage> {
    if let Some(ref form_data) = state.domains.library.state.library_form_data {
        // Validate form
        state.domains.library.state.library_form_errors.clear();
        state.domains.library.state.library_form_success = None;

        if form_data.name.trim().is_empty() {
            state
                .domains
                .library
                .state
                .library_form_errors
                .push("Library name is required".to_string());
        }

        if form_data.paths.trim().is_empty() {
            state
                .domains
                .library
                .state
                .library_form_errors
                .push("At least one path is required".to_string());
        }

        match form_data.scan_interval_minutes.parse::<u32>() {
            Ok(0) => state
                .domains
                .library
                .state
                .library_form_errors
                .push("Scan interval must be at least 1 minute".to_string()),
            Ok(_) => {}
            Err(_) => state
                .domains
                .library
                .state
                .library_form_errors
                .push("Scan interval must be a valid number".to_string()),
        }

        if !state.domains.library.state.library_form_errors.is_empty() {
            return Task::none();
        }

        // Create library object from form data
        // Convert string library type to enum
        let library_type = match form_data.library_type.as_str() {
            "Movies" => crate::infra::api_types::LibraryType::Movies,
            "TvShows" => crate::infra::api_types::LibraryType::Series,
            _ => {
                state
                    .domains
                    .library
                    .state
                    .library_form_errors
                    .push("Invalid library type".to_string());
                return Task::none();
            }
        };

        let library = Library {
            id: if form_data.editing {
                form_data.id
            } else {
                LibraryId::new()
            },
            name: form_data.name.trim().to_string(),
            library_type,
            paths: form_data
                .paths
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect(),
            scan_interval_minutes: form_data
                .scan_interval_minutes
                .parse()
                .unwrap_or(DEFAULT_SCAN_INTERVAL_MINUTES),
            last_scan: None,
            enabled: form_data.enabled,
            media: None,
            auto_scan: form_data.auto_scan,
            watch_for_changes: form_data.watch_for_changes,
            analyze_on_scan: true,
            max_retry_attempts: 3,
            movie_ref_batch_size: MovieReferenceBatchSize::default(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        if form_data.editing {
            // Update existing library
            let api = state.api_service.clone();
            Task::perform(
                async move {
                    let req = update_library_request(&library);
                    api.update_library(library.id, req).await.map(|_| library)
                },
                |result| match result {
                    Ok(updated_library) => {
                        LibraryMessage::LibraryUpdated(Ok(updated_library))
                    }
                    Err(e) => {
                        LibraryMessage::LibraryUpdated(Err(e.to_string()))
                    }
                },
            )
        } else {
            // Create new library
            let api = state.api_service.clone();
            let start_scan = form_data.start_scan;
            Task::perform(
                async move {
                    let req = create_library_request(&library, start_scan);
                    api.create_library(req).await.map(|_| library)
                },
                |result| match result {
                    Ok(created_library) => {
                        LibraryMessage::LibraryCreated(Ok(created_library))
                    }
                    Err(e) => {
                        LibraryMessage::LibraryCreated(Err(e.to_string()))
                    }
                },
            )
        }
    } else {
        Task::none()
    }
}
