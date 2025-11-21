use crate::domains::library::messages::LibraryMessage;
use crate::infra::api_types::Library;
use crate::state::State;

use chrono::Utc;
use ferrex_core::{
    api::types::{CreateLibraryRequest, UpdateLibraryRequest},
    types::{ids::LibraryID, library::LibraryType},
};
use iced::Task;
use std::path::PathBuf;

pub fn handle_create_library(
    state: &mut State,
    library: Library,
    start_scan: bool,
    _server_url: String,
) -> Task<LibraryMessage> {
    let req = CreateLibraryRequest {
        name: library.name.clone(),
        library_type: library.library_type,
        paths: library
            .paths
            .iter()
            .filter_map(|p| p.to_str().map(|s| s.to_string()))
            .collect(),
        scan_interval_minutes: library.scan_interval_minutes,
        enabled: library.enabled,
        start_scan,
    };

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
    let req = UpdateLibraryRequest {
        name: Some(library.name.clone()),
        paths: Some(
            library
                .paths
                .iter()
                .filter_map(|p| p.to_str().map(|s| s.to_string()))
                .collect(),
        ),
        scan_interval_minutes: Some(library.scan_interval_minutes),
        enabled: Some(library.enabled),
    };

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
    library_id: LibraryID,
    _server_url: String,
) -> Task<LibraryMessage> {
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
    result: Result<LibraryID, String>,
) -> Task<LibraryMessage> {
    match result {
        Ok(library_id) => {
            log::info!(
                "Deleted library: {} - refreshing libraries",
                library_id
            );
            if state.domains.library.state.current_library_id.as_ref()
                == Some(&library_id)
            {
                state.domains.library.state.current_library_id = None;
            }
            Task::perform(
                super::library_loaded::fetch_libraries(
                    state.api_service.clone(),
                ),
                |res| {
                    LibraryMessage::LibrariesLoaded(
                        res.map_err(|e| e.to_string()),
                    )
                },
            )
        }
        Err(e) => {
            log::error!("Failed to delete library: {}", e);
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
                editing: true,
                start_scan: true,
            }
        }
        None => {
            // Creating new library
            crate::domains::library::types::LibraryFormData {
                id: LibraryID::new(),
                name: String::new(),
                library_type: "Movies".to_string(),
                paths: String::new(),
                scan_interval_minutes: "60".to_string(),
                enabled: true,
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

/// Delete a library and recreate it with the same properties, triggering a fresh bulk scan.
pub fn handle_reset_library(
    state: &mut State,
    library_id: LibraryID,
) -> Task<LibraryMessage> {
    use rkyv::{deserialize, rancor::Error};

    // Read current library from repo (archived)
    let archived = state
        .domains
        .library
        .state
        .repo_accessor
        .get_archived_library_yoke(library_id.as_uuid());

    let Ok(Some(yoke)) = archived else {
        return Task::done(LibraryMessage::ResetLibraryDone(Err(
            "library_not_found".to_string(),
        )));
    };

    let lib = match deserialize::<crate::infra::api_types::Library, Error>(
        *yoke.get(),
    ) {
        Ok(v) => v,
        Err(_) => {
            return Task::done(LibraryMessage::ResetLibraryDone(Err(
                "deserialize_failed".to_string(),
            )));
        }
    };

    let delete_api = state.api_service.clone();
    let create_api = state.api_service.clone();

    Task::perform(
        async move {
            // Delete existing library
            delete_api
                .delete_library(library_id)
                .await
                .map_err(|e| e.to_string())?;

            // Recreate with same properties and start_scan=true
            let req = crate::infra::api_types::CreateLibraryRequest {
                name: lib.name.clone(),
                library_type: lib.library_type,
                paths: lib
                    .paths
                    .iter()
                    .filter_map(|p| p.to_str().map(|s| s.to_string()))
                    .collect(),
                scan_interval_minutes: lib.scan_interval_minutes,
                enabled: lib.enabled,
                start_scan: true,
            };
            let _id = create_api
                .create_library(req)
                .await
                .map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        },
        |result| match result {
            Ok(()) => LibraryMessage::ResetLibraryDone(Ok(())),
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

        if form_data.scan_interval_minutes.parse::<u32>().is_err() {
            state
                .domains
                .library
                .state
                .library_form_errors
                .push("Scan interval must be a valid number".to_string());
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
                LibraryID::new()
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
                .unwrap_or(60),
            last_scan: None,
            enabled: form_data.enabled,
            media: None,
            auto_scan: true,
            watch_for_changes: true,
            analyze_on_scan: true,
            max_retry_attempts: 3,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        if form_data.editing {
            // Update existing library
            let api = state.api_service.clone();
            Task::perform(
                async move {
                    let req = crate::infra::api_types::UpdateLibraryRequest {
                        name: Some(library.name.clone()),
                        paths: Some(
                            library
                                .paths
                                .iter()
                                .filter_map(|p| {
                                    p.to_str().map(|s| s.to_string())
                                })
                                .collect(),
                        ),
                        scan_interval_minutes: Some(
                            library.scan_interval_minutes,
                        ),
                        enabled: Some(library.enabled),
                    };
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
                    let req = crate::infra::api_types::CreateLibraryRequest {
                        name: library.name.clone(),
                        library_type: library.library_type,
                        paths: library
                            .paths
                            .iter()
                            .filter_map(|p| p.to_str().map(|s| s.to_string()))
                            .collect(),
                        scan_interval_minutes: library.scan_interval_minutes,
                        enabled: library.enabled,
                        start_scan,
                    };
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
