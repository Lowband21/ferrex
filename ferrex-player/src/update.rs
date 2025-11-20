use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use iced::{
    widget::scrollable,
    Task,
};

use crate::{
    carousel::{CarouselMessage, CarouselState}, close_video, fetch_metadata_for_media, image_cache::{self, ImageSource}, load_video, media_library, message::Message, models::{MediaOrganizer, TvShow}, performance_config::posters, player::PlayerMessage, poster_cache::{self, PosterState}, poster_monitor::PosterMonitor, profiling::PROFILER, start_library_scan, start_media_scan, state::{ScanStatus, SortBy, SortOrder, State, ViewMode, ViewState}, util::{sort_media, trigger_metadata_fetch}, virtual_list::VirtualGridState, MediaEvent
};

// Scrolling performance constants - tune these based on profiling
// Lower FAST_SCROLL_THRESHOLD for more aggressive fast mode activation
// Higher values keep normal rendering longer but may cause stuttering
const FAST_SCROLL_THRESHOLD: f32 = 5000.0; // pixels per second - when to switch to fast mode (lowered for better performance)

// Lower SCROLL_STOP_DEBOUNCE_MS for quicker poster loading after scroll
// Higher values reduce unnecessary loads during small scroll adjustments  
const SCROLL_STOP_DEBOUNCE_MS: u64 = 10; // milliseconds to wait before considering scroll stopped

// Monitor ui_performance.log for frame times exceeding 16ms during scrolling
// Adjust thresholds if you see consistent patterns of slow frames

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    let message_name = message.name();
    PROFILER.start(&format!("update::{}", message_name));

    let result = match message {
        Message::LibraryLoaded(result) => {
            state.loading = false;
            match result {
                Ok(files) => {
                    let start_time = std::time::Instant::now();
                    log::info!("Loaded {} media files", files.len());

                    // Quick sanity check: Just log sample IDs without parsing
                    if log::log_enabled!(log::Level::Info) {
                        for (i, file) in files.iter().take(3).enumerate() {
                            log::info!(
                                "Sample media[{}]: id='{}', filename='{}'",
                                i,
                                file.id,
                                file.filename
                            );
                        }
                    }

                    // Count items without posters and collect their IDs
                    let count_start = std::time::Instant::now();
                    let missing_poster_ids: Vec<String> = files
                        .iter()
                        .filter(|f| {
                            f.metadata
                                .as_ref()
                                .and_then(|m| m.external_info.as_ref())
                                .and_then(|e| e.poster_url.as_ref())
                                .is_none()
                        })
                        .map(|f| f.id.clone())
                        .collect();
                    let missing_posters = missing_poster_ids.len();
                    log::info!("Found {} items missing posters in {:?}", missing_posters, count_start.elapsed());

                    // Check for ALL files missing color metadata (needed for HDR detection)
                    let missing_color_metadata: Vec<String> = files
                        .iter()
                        .filter(|f| {
                            // Check if color metadata is missing
                            if let Some(metadata) = &f.metadata {
                                // If any of these fields are missing, we need to fetch metadata
                                metadata.color_transfer.is_none() 
                                    || metadata.color_space.is_none() 
                                    || metadata.color_primaries.is_none()
                                    || metadata.bit_depth.is_none()
                            } else {
                                true // No metadata at all
                            }
                        })
                        .map(|f| f.id.clone())
                        .collect();
                    
                    // Always set library files
                    let set_files_start = std::time::Instant::now();
                    state.library.set_server_url(state.server_url.clone());
                    state.library.set_files(files);
                    state.error_message = None;
                    log::info!("Set library files in {:?}", set_files_start.elapsed());

                    // Initialize poster monitor for background checking
                    state.poster_monitor = Some(PosterMonitor::new());

                    // Move media organization to background task with optimizations
                    let files_for_organize = state.library.files.clone();
                    let sort_by = state.sort_by;
                    let sort_order = state.sort_order;
                    let organize_task = Task::perform(
                        async move {
                            // Spawn on tokio runtime for true parallelism
                            tokio::spawn(async move {
                                // Run organization in chunks to avoid blocking
                                let chunk_size = 100;
                                let mut all_movies = Vec::new();
                                let mut all_tv_shows = HashMap::new();
                                
                                for chunk in files_for_organize.chunks(chunk_size) {
                                    let (movies, tv_shows) = MediaOrganizer::organize_media(chunk);
                                    all_movies.extend(movies);
                                    
                                    // Merge TV shows
                                    for (show_name, mut show) in tv_shows {
                                        match all_tv_shows.entry(show_name.clone()) {
                                            std::collections::hash_map::Entry::Occupied(mut entry) => {
                                                let existing: &mut TvShow = entry.get_mut();
                                                // Move seasons from new show to existing
                                                for (season_num, season) in show.seasons.drain() {
                                                    match existing.seasons.entry(season_num) {
                                                        std::collections::hash_map::Entry::Occupied(mut season_entry) => {
                                                            let existing_season = season_entry.get_mut();
                                                            // Merge episodes
                                                            for (ep_num, episode) in season.episodes {
                                                                existing_season.episodes.insert(ep_num, episode);
                                                            }
                                                            existing_season.episode_count = existing_season.episodes.len();
                                                        }
                                                        std::collections::hash_map::Entry::Vacant(season_entry) => {
                                                            season_entry.insert(season);
                                                        }
                                                    }
                                                }
                                                existing.total_episodes = existing.seasons.values()
                                                    .map(|s| s.episodes.len())
                                                    .sum();
                                            }
                                            std::collections::hash_map::Entry::Vacant(entry) => {
                                                entry.insert(show);
                                            }
                                        }
                                    }
                                    
                                    // Yield to prevent blocking
                                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                                }
                                
                                // Sort movies using parallel sort if available
                                all_movies.sort_by(|a, b| {
                                    let cmp = match sort_by {
                                        SortBy::DateAdded => a.created_at.cmp(&b.created_at),
                                        SortBy::Title => {
                                            let title_a = a.metadata.as_ref()
                                                .and_then(|m| m.parsed_info.as_ref())
                                                .map(|p| &p.title)
                                                .unwrap_or(&a.filename);
                                            let title_b = b.metadata.as_ref()
                                                .and_then(|m| m.parsed_info.as_ref())
                                                .map(|p| &p.title)
                                                .unwrap_or(&b.filename);
                                            title_a.cmp(title_b)
                                        }
                                        SortBy::Year => {
                                            let year_a = a.metadata.as_ref()
                                                .and_then(|m| m.parsed_info.as_ref())
                                                .and_then(|p| p.year);
                                            let year_b = b.metadata.as_ref()
                                                .and_then(|m| m.parsed_info.as_ref())
                                                .and_then(|p| p.year);
                                            year_a.cmp(&year_b)
                                        }
                                        SortBy::Rating => {
                                            let rating_a = a.metadata.as_ref()
                                                .and_then(|m| m.external_info.as_ref())
                                                .and_then(|e| e.rating);
                                            let rating_b = b.metadata.as_ref()
                                                .and_then(|m| m.external_info.as_ref())
                                                .and_then(|e| e.rating);
                                            rating_a.partial_cmp(&rating_b).unwrap_or(std::cmp::Ordering::Equal)
                                        }
                                    };
                                    
                                    match sort_order {
                                        SortOrder::Ascending => cmp,
                                        SortOrder::Descending => cmp.reverse(),
                                    }
                                });
                                
                                log::info!(
                                    "Optimized organization complete: {} movies, {} TV shows",
                                    all_movies.len(),
                                    all_tv_shows.len()
                                );
                                (all_movies, all_tv_shows)
                            }).await.unwrap_or_else(|e| {
                                log::error!("Organization task failed: {}", e);
                                (Vec::new(), HashMap::new())
                            })
                        },
                        |(movies, tv_shows)| Message::MediaOrganized(movies, tv_shows),
                    );

                    // Clear existing data while waiting for organization
                    state.movies.clear();
                    state.tv_shows.clear();

                    // Update carousel states with empty data for now
                    state.movies_carousel = CarouselState::new(0);
                    state.tv_shows_carousel = CarouselState::new(0);

                    // Start loading posters in background while organizing
                    let mut tasks = vec![organize_task];

                    // Check if we need to fetch missing posters
                    if missing_posters > 0 {
                        log::warn!(
                            "{} items missing posters! Manual metadata refresh may be needed.",
                            missing_posters
                        );
                    }
                    
                    /*
                    if !missing_color_metadata.is_empty() {
                        log::info!(
                            "Found {} files missing color metadata. Queueing metadata fetch for HDR detection.",
                            missing_color_metadata.len()
                        );
                        
                        // Batch the metadata fetches to avoid overwhelming the server
                        const BATCH_SIZE: usize = 50;
                        for chunk in missing_color_metadata.chunks(BATCH_SIZE) {
                            let server_url_clone = state.server_url.clone();
                            let chunk_ids = chunk.to_vec();
                            tasks.push(Task::perform(
                                async move {
                                    log::info!("Fetching metadata batch of {} items", chunk_ids.len());
                                    if let Err(e) = trigger_metadata_fetch(server_url_clone, chunk_ids.clone()).await {
                                        log::error!("Failed to trigger metadata refresh: {}", e);
                                    } else {
                                        log::info!("Successfully triggered metadata refresh for batch");
                                    }
                                },
                                |_| Message::NoOp,
                            ));
                        }
                    } else {
                        log::info!("All files have color metadata - HDR detection ready");
                    }
                    */

                    for file in state.library.files.iter().take(3) {
                        if state.poster_cache.get(&file.id).is_none() {
                            state.poster_cache.set_loading(file.id.clone());
                            let server_url = state.server_url.clone();
                            let media_id = file.id.clone();
                            tasks.push(Task::perform(
                                poster_cache::fetch_poster_with_id(server_url, media_id),
                                |(id, result)| Message::PosterLoaded(id, result),
                            ));
                        }
                    }

                    // PosterMonitorTick will handle poster loading

                    log::info!(
                        "LibraryLoaded processing took {:?} total",
                        start_time.elapsed()
                    );
                    Task::batch(tasks)
                }
                Err(e) => {
                    log::error!("Failed to load library: {}", e);
                    state.error_message = Some(format!("Failed to load library: {}", e));
                    Task::none()
                }
            }
        }

        Message::RefreshLibrary => {
            state.loading = true;

            // Clear failed poster states to allow retry
            let failed_ids = state.poster_cache.get_failed_ids();

            for id in failed_ids {
                state.poster_cache.remove(&id);
            }

            // Also clear loading posters set
            state.loading_posters.clear();

            let server_url = state.server_url.clone();
            Task::perform(
                media_library::fetch_library(server_url),
                |result| match result {
                    Ok(files) => Message::LibraryLoaded(Ok(files)),
                    Err(e) => Message::LibraryLoaded(Err(e.to_string())),
                },
            )
        }
        
        // Library Management Messages
        Message::LibrariesLoaded(result) => {
            let mut tasks: Vec<Task<Message>> = vec![];
            match result {
                Ok(libraries) => {
                    log::info!("Loaded {} libraries", libraries.len());
                    state.libraries = libraries;
                    
                    if state.libraries.is_empty() {
                        // No libraries configured - don't try to load media
                        log::info!("No libraries configured, skipping media loading");
                        state.loading = false;
                        return Task::none();
                    } else if state.current_library_id.is_none() {
                        // Libraries exist but none selected - select the first enabled one

                       for library in state.libraries.clone().into_iter() {
                            let library_id = library.id.clone();
                            let server_url = state.server_url.clone();
                            tasks.push(Task::perform(
                                media_library::fetch_library_media(server_url, library_id.clone()),
                                move |result| match result {
                                    Ok(files) => Message::LibrarySelected(library_id.clone(), Ok(files)),
                                    Err(e) => Message::LibrarySelected(library_id.clone(), Err(e.to_string())),
                                },
                            ));
                       } 
                    }
                    
                    state.error_message = None;
                }
                Err(e) => {
                    log::error!("Failed to load libraries: {}", e);
                    state.error_message = Some(format!("Failed to load libraries: {}", e));
                    
                    // Don't fall back to legacy loading - just show the error
                    state.loading = false;
                    return Task::none();
                }
            }
            Task::batch(tasks)
        }
        
        Message::LoadLibraries => {
            let server_url = state.server_url.clone();
            Task::perform(
                media_library::fetch_libraries(server_url),
                |result| match result {
                    Ok(libraries) => Message::LibrariesLoaded(Ok(libraries)),
                    Err(e) => Message::LibrariesLoaded(Err(e.to_string())),
                },
            )
        }
        
        Message::CreateLibrary(library) => {
            let server_url = state.server_url.clone();
            Task::perform(
                media_library::create_library(server_url, library),
                |result| match result {
                    Ok(created_library) => Message::LibraryCreated(Ok(created_library)),
                    Err(e) => Message::LibraryCreated(Err(e.to_string())),
                },
            )
        }
        
        Message::LibraryCreated(result) => {
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
                    state.library_form_errors.push(format!("Failed to create library: {}", e));
                }
            }
            Task::none()
        }
        
        Message::UpdateLibrary(library) => {
            let server_url = state.server_url.clone();
            Task::perform(
                media_library::update_library(server_url, library),
                |result| match result {
                    Ok(updated_library) => Message::LibraryUpdated(Ok(updated_library)),
                    Err(e) => Message::LibraryUpdated(Err(e.to_string())),
                },
            )
        }
        
        Message::LibraryUpdated(result) => {
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
                    state.library_form_errors.push(format!("Failed to update library: {}", e));
                }
            }
            Task::none()
        }
        
        Message::DeleteLibrary(library_id) => {
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
        
        Message::LibraryDeleted(result) => {
            match result {
                Ok(library_id) => {
                    log::info!("Deleted library: {}", library_id);
                    state.libraries.retain(|l| l.id != library_id);
                    
                    // If we deleted the current library, clear selection
                    if state.current_library_id.as_ref() == Some(&library_id) {
                        state.current_library_id = None;
                        state.movies.clear();
                        state.tv_shows.clear();
                        state.tv_shows_sorted.clear();
                    }
                    
                    state.error_message = None;
                }
                Err(e) => {
                    log::error!("Failed to delete library: {}", e);
                    state.error_message = Some(format!("Failed to delete library: {}", e));
                }
            }
            Task::none()
        }
        
        Message::SelectLibrary(library_id) => {
            log::info!("Selecting library: {}", library_id);
            
            let server_url = state.server_url.clone();
            
            if library_id == "all" {
                // Special case: show all media files from all libraries
                state.current_library_id = None;
                // Switch to All view mode to show carousel
                state.view_mode = ViewMode::All;
                
                // Check if we already have media loaded
                if !state.movies.is_empty() || !state.tv_shows.is_empty() {
                    log::info!("Media already loaded, just switching view");
                    return Task::none();
                }
                
                // Only load if we don't have any media yet
                state.loading = true;
                Task::perform(
                    media_library::fetch_library(server_url), // Legacy function loads all media
                    move |result| match result {
                        Ok(files) => Message::LibrarySelected("all".to_string(), Ok(files)),
                        Err(e) => Message::LibrarySelected("all".to_string(), Err(e.to_string())),
                    },
                )
            } else {
                // Select specific library
                state.current_library_id = Some(library_id.clone());
                
                // Check library type and set appropriate view mode for grid view
                if let Some(library) = state.libraries.iter().find(|l| l.id == library_id) {
                    match library.library_type.as_str() {
                        "Movies" => state.view_mode = ViewMode::Movies,
                        "TV Shows" | "TvShows" => state.view_mode = ViewMode::TvShows,
                        _ => {
                            // Default to All view if library type is unknown
                            log::warn!("Unknown library type: {}", library.library_type);
                            state.view_mode = ViewMode::All;
                        }
                    }
                }
                
                // Check if we already have media loaded
                if !state.movies.is_empty() || !state.tv_shows.is_empty() {
                    log::info!("Media already loaded for library {}, just switching view", library_id);
                    return Task::none();
                }
                
                // Only load if we don't have any media yet
                state.loading = true;
                Task::perform(
                    media_library::fetch_library_media(server_url, library_id.clone()),
                    move |result| match result {
                        Ok(files) => Message::LibrarySelected(library_id.clone(), Ok(files)),
                        Err(e) => Message::LibrarySelected(library_id.clone(), Err(e.to_string())),
                    },
                )
            }
        }
        
        Message::LibrarySelected(library_id, result) => {
            state.loading = false;
            match result {
                Ok(files) => {
                    log::info!("Selected library {} with {} files", library_id, files.len());
                    
                    // Update the current library ID
                    if library_id == "all" {
                        state.current_library_id = None;
                    } else {
                        state.current_library_id = Some(library_id.clone());
                    }
                    
                    // Update the library with new files
                    state.library.set_files(files.clone());
                    state.error_message = None;
                    
                    // Organize media similar to LibraryLoaded
                    let _sort_by = state.sort_by;
                    let _sort_order = state.sort_order;
                    Task::perform(
                        async move {
                            tokio::spawn(async move {
                                // MediaOrganizer is a unit struct, no need to instantiate
                                MediaOrganizer::organize_media(&files)
                            }).await.unwrap_or_else(|e| {
                                log::error!("Organization task failed: {}", e);
                                (Vec::new(), HashMap::new())
                            })
                        },
                        |(movies, tv_shows)| Message::MediaOrganized(movies, tv_shows),
                    )
                }
                Err(e) => {
                    log::error!("Failed to load library media: {}", e);
                    state.error_message = Some(format!("Failed to load library: {}", e));
                    Task::none()
                }
            }
        }
        
        Message::ScanLibrary_(library_id) => {
            log::info!("Starting scan for library: {}", library_id);
            state.scanning = true;
            state.error_message = None;
            state.scan_progress = None;
            
            let server_url = state.server_url.clone();
            Task::perform(
                start_library_scan(server_url, library_id, true), // Enable streaming
                |result| match result {
                    Ok(scan_id) => Message::ScanStarted(Ok(scan_id)),
                    Err(e) => Message::ScanStarted(Err(e.to_string())),
                },
            )
        }
        
        Message::ShowLibraryManagement => {
            state.view = ViewState::LibraryManagement;
            state.show_library_management = true;
            
            // Load libraries if not already loaded
            if state.libraries.is_empty() {
                let server_url = state.server_url.clone();
                Task::perform(
                    media_library::fetch_libraries(server_url),
                    |result| match result {
                        Ok(libraries) => Message::LibrariesLoaded(Ok(libraries)),
                        Err(e) => Message::LibrariesLoaded(Err(e.to_string())),
                    },
                )
            } else {
                Task::none()
            }
        }
        
        Message::HideLibraryManagement => {
            state.view = ViewState::Library;
            state.show_library_management = false;
            state.library_form_data = None; // Clear form when leaving management view
            Task::none()
        }
        
        Message::ShowAdminDashboard => {
            state.view = ViewState::AdminDashboard;
            Task::none()
        }
        
        Message::HideAdminDashboard => {
            state.view = ViewState::Library;
            Task::none()
        }
        
        Message::ShowClearDatabaseConfirm => {
            state.show_clear_database_confirm = true;
            Task::none()
        }
        
        Message::HideClearDatabaseConfirm => {
            state.show_clear_database_confirm = false;
            Task::none()
        }
        
        Message::ClearDatabase => {
            log::info!("Clearing all database contents");
            state.show_clear_database_confirm = false; // Hide confirmation dialog
            let server_url = state.server_url.clone();
            Task::perform(
                async move {
                    let client = reqwest::Client::new();
                    let url = format!("{}/maintenance/clear-database", server_url);
                    
                    match client.post(&url).send().await {
                        Ok(response) => {
                            if response.status().is_success() {
                                Ok(())
                            } else {
                                Err(format!("Server error: {}", response.status()))
                            }
                        }
                        Err(e) => Err(format!("Request failed: {}", e))
                    }
                },
                Message::DatabaseCleared
            )
        }
        
        Message::DatabaseCleared(result) => {
            match result {
                Ok(()) => {
                    log::info!("Database cleared successfully");
                    
                    // Clear all media data
                    state.library.files.clear();
                    state.movies.clear();
                    state.tv_shows.clear();
                    state.tv_shows_sorted.clear();
                    
                    // Clear all caches
                    state.poster_cache.clear();
                    state.image_cache.clear();
                    // Note: metadata_cache.clear() is async, so we'll spawn a task for it
                    let metadata_cache = state.metadata_cache.clone();
                    tokio::spawn(async move {
                        metadata_cache.clear().await;
                    });
                    
                    // Clear library data
                    state.libraries.clear();
                    state.current_library_id = None;
                    state.library_form_data = None;
                    state.library_form_errors.clear();
                    
                    // Reset scan state
                    state.scanning = false;
                    state.loading = false;
                    state.active_scan_id = None;
                    state.scan_progress = None;
                    state.show_scan_progress = false;
                    
                    // Clear poster loading state
                    state.posters_to_load.clear();
                    state.loading_posters.clear();
                    state.poster_mark_progress = 0;
                    state.poster_animation_states.clear();
                    state.poster_animation_types.clear();
                    state.poster_monitor = None;
                    
                    // Clear detail view data
                    state.current_show_details = None;
                    state.current_season_details = None;
                    state.expanded_shows.clear();
                    state.show_seasons_carousel = None;
                    state.season_episodes_carousel = None;
                    
                    // Clear UI state
                    state.hovered_media_id = None;
                    state.error_message = None;
                    
                    // Reset scroll positions
                    state.movies_scroll_position = None;
                    state.tv_shows_scroll_position = None;
                    state.last_scroll_position = 0.0;
                    state.scroll_velocity = 0.0;
                    state.fast_scrolling = false;
                    state.scroll_stopped_time = None;
                    state.scroll_samples.clear();
                    
                    // Update carousel counts
                    state.movies_carousel.set_total_items(0);
                    state.tv_shows_carousel.set_total_items(0);
                    
                    // Reset grid states
                    state.movies_grid_state.total_items = 0;
                    state.tv_shows_grid_state.total_items = 0;
                    
                    // Reset view to library (in case user was in detail view)
                    state.view = ViewState::Library;
                    state.view_mode = ViewMode::All;
                    
                    log::info!("All local state cleared and reset");
                    
                    // Refresh libraries list to get fresh data from server
                    let server_url = state.server_url.clone();
                    Task::perform(
                        media_library::fetch_libraries(server_url),
                        |result| match result {
                            Ok(libraries) => Message::LibrariesLoaded(Ok(libraries)),
                            Err(e) => Message::LibrariesLoaded(Err(e.to_string())),
                        },
                    )
                }
                Err(e) => {
                    log::error!("Failed to clear database: {}", e);
                    state.error_message = Some(format!("Failed to clear database: {}", e));
                    Task::none()
                }
            }
        }
        
        // Library form management
        Message::ShowLibraryForm(library) => {
            state.library_form_errors.clear();
            state.library_form_data = Some(match library {
                Some(lib) => {
                    // Editing existing library
                    crate::state::LibraryFormData {
                        id: lib.id,
                        name: lib.name,
                        library_type: lib.library_type,
                        paths: lib.paths.join(", "),
                        scan_interval_minutes: lib.scan_interval_minutes.to_string(),
                        enabled: lib.enabled,
                        editing: true,
                    }
                }
                None => {
                    // Creating new library
                    crate::state::LibraryFormData {
                        id: String::new(),
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
        
        Message::HideLibraryForm => {
            state.library_form_data = None;
            state.library_form_errors.clear();
            Task::none()
        }
        
        Message::UpdateLibraryFormName(name) => {
            if let Some(ref mut form_data) = state.library_form_data {
                form_data.name = name;
            }
            Task::none()
        }
        
        Message::UpdateLibraryFormType(library_type) => {
            if let Some(ref mut form_data) = state.library_form_data {
                form_data.library_type = library_type;
            }
            Task::none()
        }
        
        Message::UpdateLibraryFormPaths(paths) => {
            if let Some(ref mut form_data) = state.library_form_data {
                form_data.paths = paths;
            }
            Task::none()
        }
        
        Message::UpdateLibraryFormScanInterval(interval) => {
            if let Some(ref mut form_data) = state.library_form_data {
                form_data.scan_interval_minutes = interval;
            }
            Task::none()
        }
        
        Message::ToggleLibraryFormEnabled => {
            if let Some(ref mut form_data) = state.library_form_data {
                form_data.enabled = !form_data.enabled;
            }
            Task::none()
        }
        
        Message::SubmitLibraryForm => {
            if let Some(ref form_data) = state.library_form_data {
                // Validate form
                state.library_form_errors.clear();
                
                if form_data.name.trim().is_empty() {
                    state.library_form_errors.push("Library name is required".to_string());
                }
                
                if form_data.paths.trim().is_empty() {
                    state.library_form_errors.push("At least one path is required".to_string());
                }
                
                if let Err(_) = form_data.scan_interval_minutes.parse::<u32>() {
                    state.library_form_errors.push("Scan interval must be a valid number".to_string());
                }
                
                if !state.library_form_errors.is_empty() {
                    return Task::none();
                }
                
                // Create library object from form data
                let library = media_library::Library {
                    id: if form_data.editing { form_data.id.clone() } else { String::new() },
                    name: form_data.name.trim().to_string(),
                    library_type: form_data.library_type.clone(),
                    paths: form_data.paths
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                    scan_interval_minutes: form_data.scan_interval_minutes.parse().unwrap_or(60),
                    last_scan: None,
                    enabled: form_data.enabled,
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

        Message::ScanLibrary => {
            state.scanning = true;
            state.error_message = None;
            state.scan_progress = None;
            let server_url = state.server_url.clone();

            Task::perform(start_media_scan(server_url, false, true), |result| match result {
                Ok(scan_id) => Message::ScanStarted(Ok(scan_id)),
                Err(e) => Message::ScanStarted(Err(e.to_string())),
            })
        }

        Message::ForceRescan => {
            state.scanning = true;
            state.error_message = None;
            state.scan_progress = None;
            let server_url = state.server_url.clone();

            Task::perform(start_media_scan(server_url, true, true), |result| match result {
                Ok(scan_id) => Message::ScanStarted(Ok(scan_id)),
                Err(e) => Message::ScanStarted(Err(e.to_string())),
            })
        }

        Message::ScanStarted(result) => {
            match result {
                Ok(scan_id) => {
                    log::info!("Scan started with ID: {}", scan_id);
                    state.active_scan_id = Some(scan_id);
                    state.show_scan_progress = true; // Auto-show progress overlay
                                                     // Will start receiving progress updates via subscription
                    Task::none()
                }
                Err(e) => {
                    log::error!("Failed to start scan: {}", e);
                    state.scanning = false;
                    state.error_message = Some(format!("Failed to start scan: {}", e));
                    Task::none()
                }
            }
        }

        Message::ScanProgressUpdate(progress) => {
            log::info!(
                "Received scan progress update: {} files scanned, {} stored, {} metadata fetched",
                progress.scanned_files,
                progress.stored_files,
                progress.metadata_fetched
            );
            log::info!(
                "Scan progress state - show_scan_progress: {}, active_scan_id: {:?}",
                state.show_scan_progress,
                state.active_scan_id
            );

            let previous_stored = state
                .scan_progress
                .as_ref()
                .map(|p| p.stored_files)
                .unwrap_or(0);

            state.scan_progress = Some(progress.clone());
            log::info!("Set scan_progress to Some - overlay should be visible if show_scan_progress is true");

            // Check if scan is completed
            if progress.status == ScanStatus::Completed
                || progress.status == ScanStatus::Failed
                || progress.status == ScanStatus::Cancelled
            {
                state.scanning = false;
                // Don't clear active_scan_id yet - keep it until we clear scan_progress

                if progress.status == ScanStatus::Completed {
                    // Refresh library after successful scan
                    log::info!("Scan completed successfully, refreshing library");
                    // Clear scan progress after a short delay
                    Task::batch([
                        update(state, Message::RefreshLibrary),
                        Task::perform(
                            async {
                                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                            },
                            |_| Message::ClearScanProgress,
                        ),
                    ])
                } else if progress.status == ScanStatus::Failed {
                    state.error_message = Some(format!("Scan failed: {:?}", progress.errors));
                    // Clear scan progress after a delay
                    Task::perform(
                        async {
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        },
                        |_| Message::ClearScanProgress,
                    )
                } else {
                    // Cancelled - clear immediately
                    state.scan_progress = None;
                    state.active_scan_id = None;
                    Task::none()
                }
            } else {
                // If new files were stored, trigger an incremental update
                if progress.stored_files > previous_stored {
                    // No longer triggering incremental updates - using SSE events instead
                    Task::none()
                } else {
                    Task::none()
                }
            }
        }

        Message::ClearScanProgress => {
            state.scan_progress = None;
            state.active_scan_id = None; // Clear active_scan_id when we clear the progress
            state.show_scan_progress = false;
            Task::none()
        }

        Message::ToggleScanProgress => {
            state.show_scan_progress = !state.show_scan_progress;
            log::info!(
                "Toggled scan progress overlay to: {}, scan_progress exists: {}",
                state.show_scan_progress,
                state.scan_progress.is_some()
            );
            Task::none()
        }

        Message::ActiveScansChecked(scans) => {
            if let Some(active_scan) = scans
                .into_iter()
                .find(|s| s.status == ScanStatus::Scanning || s.status == ScanStatus::Processing)
            {
                log::info!("Found active scan {}, reconnecting...", active_scan.scan_id);
                state.active_scan_id = Some(active_scan.scan_id.clone());
                state.scan_progress = Some(active_scan);
                state.scanning = true;
                //state.show_scan_progress = true;
            }
            Task::none()
        }

        Message::MediaEventReceived(event) => {
            match event {
                MediaEvent::MediaAdded { media } => {
                    log::info!("Media added: {}, has_poster: {}", media.filename, media.has_poster());
                    log::info!("Media type check - is_tv_episode: {}", media.is_tv_episode());
                    
                    if let Some(metadata) = &media.metadata {
                        if let Some(parsed) = &metadata.parsed_info {
                            log::info!("Media type: '{}', show_name: {:?}", 
                                parsed.media_type, parsed.show_name);
                        } else {
                            log::warn!("No parsed_info in metadata for: {}", media.filename);
                        }
                    } else {
                        log::warn!("No metadata for: {}", media.filename);
                    }

                    // Add to library
                    state.library.files.push(media.clone());

                    // Update organized collections directly instead of full reorganization
                    if media.is_tv_episode() {
                        log::info!("Processing TV episode: {}", media.filename);
                        
                        // Try to get show name from metadata or filename
                        if let Some(show_name) = media.get_show_name() {
                            log::info!("Adding episode to show: {}", show_name);

                            // Add to existing show or create new one
                            if let Some(show) = state.tv_shows.get_mut(&show_name) {
                                show.add_episode(media.clone());
                                log::info!("Added episode to existing show: {}", show_name);
                            } else if let Some(new_show) = TvShow::from_episode(&media) {
                                state.tv_shows.insert(show_name.clone(), new_show);
                                log::info!("Created new show and added episode: {}", show_name);
                            }
                        } else {
                            log::warn!("TV episode missing show name: {}", media.filename);
                            // Still treat as movie if we can't extract show name
                            state.movies.push(media.clone());
                            log::info!("Added TV episode without show name as movie, total movies: {}", state.movies.len());
                        }
                    } else {
                        log::info!("Processing movie: {}", media.filename);
                        // Add movie without re-sorting (sorting happens periodically)
                        state.movies.push(media.clone());
                        log::info!("Added movie to state.movies, total movies: {}", state.movies.len());
                    }

                    // Update carousel item counts
                    state.movies_carousel.set_total_items(state.movies.len());
                    state
                        .tv_shows_carousel
                        .set_total_items(state.tv_shows.len());
                    
                    log::info!("Updated carousel counts - Movies: {}, TV Shows: {}", 
                        state.movies.len(), state.tv_shows.len());

                    // Load poster for new media and schedule batch sort
                    let mut tasks = Vec::new();

                    // Initialize poster cache for all new media
                    if state.poster_cache.get(&media.id).is_none() {
                        if media.has_poster() {
                            // Media has poster metadata, attempt to load it
                            state.poster_cache.set_loading(media.id.clone());
                            let server_url = state.server_url.clone();
                            let media_id = media.id.clone();
                            tasks.push(Task::perform(
                                poster_cache::fetch_poster_with_id(server_url, media_id),
                                |(id, result)| Message::PosterLoaded(id, result),
                            ));
                        } else {
                            // No poster available, mark as failed so default poster is shown
                            log::info!("Media {} has no poster, marking as failed in cache", media.filename);
                            state.poster_cache.set_failed(media.id.clone());
                        }
                    }

                    // Schedule batch sort after a delay
                    if state.sort_by != SortBy::DateAdded {
                        tasks.push(Task::perform(
                            async {
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            },
                            |_| Message::BatchSort,
                        ));
                    }

                    // Update grid states with new item counts
                    state.movies_grid_state.total_items = state.movies.len();
                    state.tv_shows_grid_state.total_items = state.tv_shows_sorted.len();

                    if !tasks.is_empty() {
                        Task::batch(tasks)
                    } else {
                        Task::none()
                    }
                }
                MediaEvent::MediaUpdated { media} => {
                    log::info!("Media updated: {}, has_poster: {}", media.filename, media.has_poster());

                    // Update in library
                    if let Some(index) = state.library.files.iter().position(|m| m.id == media.id) {
                        state.library.files[index] = media.clone();

                        // Update in organized collections
                        if media.is_tv_episode() {
                            // Find and update in TV shows
                            for (_, show) in state.tv_shows.iter_mut() {
                                for season in show.seasons.values_mut() {
                                    if let Some(ep_num) = media
                                        .metadata
                                        .as_ref()
                                        .and_then(|m| m.parsed_info.as_ref())
                                        .and_then(|p| p.episode)
                                    {
                                        if season.episodes.contains_key(&ep_num) {
                                            season.episodes.insert(ep_num, media.clone());
                                            break;
                                        }
                                    }
                                }
                            }
                        } else {
                            // Update movie
                            if let Some(movie_index) =
                                state.movies.iter().position(|m| m.id == media.id)
                            {
                                state.movies[movie_index] = media.clone();
                            }
                        }
                        
                        // Load poster if it's newly available or metadata was updated
                        if media.has_poster() {
                            match state.poster_cache.get(&media.id) {
                                None | Some(PosterState::Failed) | Some(PosterState::Loading) => {
                                    log::info!("Starting poster fetch for updated media: {}", media.filename);
                                    // Remove any existing state and try loading with updated metadata
                                    state.poster_cache.remove(&media.id);
                                    state.poster_cache.set_loading(media.id.clone());
                                    state.posters_to_load.push_front(media.id.clone());
                                    let server_url = state.server_url.clone();
                                    let media_id = media.id.clone();
                                    return Task::perform(
                                        poster_cache::fetch_poster_with_id_retry(server_url, media_id),
                                        |(id, result)| Message::PosterLoaded(id, result),
                                    );
                                }
                                Some(PosterState::Loaded { .. }) => {
                                    log::info!("Poster already loaded for: {}", media.filename);
                                } // Already successfully loaded
                            }
                        } else {
                            log::info!("No poster available for updated media: {}", media.filename);
                        }
                    }
                    
                    // Update grid states with new item counts
                    state.movies_grid_state.total_items = state.movies.len();
                    state.tv_shows_grid_state.total_items = state.tv_shows_sorted.len();

                    Task::none()
                }
                MediaEvent::MediaDeleted { id } => {
                    log::info!("Media deleted: {}", id);
                    let media_id = id;

                    // Remove from library
                    state.library.files.retain(|m| m.id != media_id);

                    // Remove from organized collections
                    // First check movies
                    state.movies.retain(|m| m.id != media_id);

                    // Then check TV shows
                    let mut empty_shows = Vec::new();
                    for (show_name, show) in state.tv_shows.iter_mut() {
                        for season in show.seasons.values_mut() {
                            season.episodes.retain(|_, ep| ep.id != media_id);
                            season.episode_count = season.episodes.len();
                        }
                        // Remove empty seasons
                        show.seasons.retain(|_, s| !s.episodes.is_empty());
                        // Update total episodes
                        show.total_episodes = show.seasons.values().map(|s| s.episodes.len()).sum();

                        if show.seasons.is_empty() {
                            empty_shows.push(show_name.clone());
                        }
                    }

                    // Remove empty shows
                    for show_name in empty_shows {
                        state.tv_shows.remove(&show_name);
                    }

                    // Update carousel item counts
                    state.movies_carousel.set_total_items(state.movies.len());
                    state
                        .tv_shows_carousel
                        .set_total_items(state.tv_shows.len());

                    // Remove from poster cache
                    state.poster_cache.remove(&media_id);
                    
                    // Update grid states with new item counts
                    state.movies_grid_state.total_items = state.movies.len();
                    state.tv_shows_grid_state.total_items = state.tv_shows_sorted.len();

                    Task::none()
                }
                MediaEvent::MetadataUpdated { id } => {
                    log::info!("Metadata updated for media: {}", id);
                    // Could trigger a library refresh or specific media update
                    Task::none()
                }
                MediaEvent::ScanStarted { scan_id } => {
                    log::info!("Scan started event received: {}", scan_id);
                    // This is handled by the scan start message
                    Task::none()
                }
                MediaEvent::ScanCompleted { scan_id } => {
                    log::info!("Scan completed event received: {}", scan_id);
                    // This is handled by the scan progress subscription
                    Task::none()
                }
            }
        }

        Message::MediaEventsError(error) => {
            log::error!("Media events SSE error: {}", error);
            // TODO: Implement retry logic
            Task::none()
        }

        Message::MediaOrganized(movies, tv_shows) => {
            log::info!(
                "Media organization complete: {} movies, {} TV shows",
                movies.len(),
                tv_shows.len()
            );
            
            // Log sample media for debugging
            if !movies.is_empty() {
                log::info!("Sample movie: {}", movies[0].display_title());
                // Log first few movies with their media types
                for (i, movie) in movies.iter().take(3).enumerate() {
                    log::debug!("Movie[{}]: {} - type: {}", i, movie.filename,
                        movie.metadata.as_ref()
                            .and_then(|m| m.parsed_info.as_ref())
                            .map(|p| &p.media_type)
                            .unwrap_or(&"no type".to_string()));
                }
            } else {
                log::warn!("No movies found after organization!");
            }
            
            if !tv_shows.is_empty() {
                let first_show = tv_shows.values().next().unwrap();
                log::info!("Sample TV show: {}", first_show.name);
            } else {
                log::warn!("No TV shows found after organization!");
            }

            // Merge new media with existing media instead of replacing
            // This preserves media already loaded while adding new ones from scans
            for movie in movies {
                // Check if movie already exists by ID
                if !state.movies.iter().any(|m| m.id == movie.id) {
                    state.movies.push(movie);
                }
            }
            
            // Merge TV shows
            for (show_name, new_show) in tv_shows {
                match state.tv_shows.get_mut(&show_name) {
                    Some(existing_show) => {
                        // Merge episodes from new show into existing show
                        for (season_num, new_season) in new_show.seasons {
                            match existing_show.seasons.get_mut(&season_num) {
                                Some(existing_season) => {
                                    // Merge episodes
                                    for (ep_num, episode) in new_season.episodes {
                                        existing_season.episodes.insert(ep_num, episode);
                                    }
                                }
                                None => {
                                    // Add new season
                                    existing_show.seasons.insert(season_num, new_season);
                                }
                            }
                        }
                        // Update metadata if newer
                        if new_show.poster_url.is_some() && existing_show.poster_url.is_none() {
                            existing_show.poster_url = new_show.poster_url;
                        }
                        // Update other metadata fields if they exist
                        if new_show.description.is_some() && existing_show.description.is_none() {
                            existing_show.description = new_show.description;
                        }
                        if new_show.tmdb_id.is_some() && existing_show.tmdb_id.is_none() {
                            existing_show.tmdb_id = new_show.tmdb_id;
                        }
                    }
                    None => {
                        // Add new show
                        state.tv_shows.insert(show_name, new_show);
                    }
                }
            }
            
            // Sort movies by title for consistent ordering
            state.movies.sort_by(|a, b| {
                let title_a = a.metadata.as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                    .map(|p| &p.title)
                    .unwrap_or(&a.filename);
                let title_b = b.metadata.as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                    .map(|p| &p.title)
                    .unwrap_or(&b.filename);
                title_a.cmp(title_b)
            });
            
            // Create sorted TV shows vector for grid view
            let mut tv_shows_sorted: Vec<_> = state.tv_shows.values().cloned().collect();
            tv_shows_sorted.sort_by(|a, b| a.name.cmp(&b.name));
            state.tv_shows_sorted = tv_shows_sorted;

            // Update carousel states
            state.movies_carousel = CarouselState::new(state.movies.len());
            state.tv_shows_carousel = CarouselState::new(state.tv_shows.len());

            // Update items per page based on current window size
            let available_width = state.window_size.width - 80.0;
            state.movies_carousel.update_items_per_page(available_width);
            state
                .tv_shows_carousel
                .update_items_per_page(available_width);

            // Update virtual grid states
            state.movies_grid_state = VirtualGridState::new(state.movies.len(), 5, 380.0);
            state.tv_shows_grid_state = VirtualGridState::new(state.tv_shows_sorted.len(), 5, 380.0);
            state
                .movies_grid_state
                .update_columns(state.window_size.width);
            state
                .tv_shows_grid_state
                .update_columns(state.window_size.width);

            // Mark visible items for loading
            let marked_ids = state.mark_visible_posters_for_loading();

            // Restore scroll positions if we have them
            let restore_task = match state.view_mode {
                ViewMode::Movies => {
                    if let Some(position) = state.movies_scroll_position {
                        log::debug!("Restoring movies scroll position after organization: {}", position);
                        scrollable::scroll_to(
                            state.movies_grid_state.scrollable_id.clone(),
                            scrollable::AbsoluteOffset { x: 0.0, y: position },
                        )
                    } else {
                        Task::none()
                    }
                }
                ViewMode::TvShows => {
                    if let Some(position) = state.tv_shows_scroll_position {
                        log::debug!("Restoring TV shows scroll position after organization: {}", position);
                        scrollable::scroll_to(
                            state.tv_shows_grid_state.scrollable_id.clone(),
                            scrollable::AbsoluteOffset { x: 0.0, y: position },
                        )
                    } else {
                        Task::none()
                    }
                }
                ViewMode::All => {
                    // In All mode, restore movies scroll position
                    if let Some(position) = state.movies_scroll_position {
                        scrollable::scroll_to(
                            state.movies_grid_state.scrollable_id.clone(),
                            scrollable::AbsoluteOffset { x: 0.0, y: position },
                        )
                    } else {
                        Task::none()
                    }
                }
            };

            // If we marked any items, trigger poster loading
            let poster_task = if !marked_ids.is_empty() {
                Task::perform(async {}, |_| Message::PosterMonitorTick)
            } else {
                log::info!("No items marked for loading");
                Task::none()
            };

            // Batch the tasks
            Task::batch([restore_task, poster_task])
        }

        Message::BatchSort => {
            // Sort media collections
            sort_media(
                &mut state.movies,
                &mut state.tv_shows,
                state.sort_by,
                state.sort_order,
            );
            Task::none()
        }

        Message::MoviesGridScrolled(viewport) => {
            // Update scroll position
            state.movies_grid_state.update_scroll(viewport);
            
            // Calculate scroll velocity
            let current_position = viewport.absolute_offset().y;
            let now = Instant::now();
            
            // Add current sample to the queue
            state.scroll_samples.push_back((now, current_position));
            
            // Keep only the last 5 samples
            while state.scroll_samples.len() > 5 {
                state.scroll_samples.pop_front();
            }
            
            // Calculate velocity if we have at least 2 samples
            if state.scroll_samples.len() >= 2 {
                let oldest = state.scroll_samples.front().unwrap();
                let newest = state.scroll_samples.back().unwrap();
                
                let time_delta = newest.0.duration_since(oldest.0).as_secs_f32();
                if time_delta > 0.0 {
                    let position_delta = newest.1 - oldest.1;
                    state.scroll_velocity = (position_delta / time_delta).abs();
                    
                    // Determine if we're fast scrolling
                    let was_fast_scrolling = state.fast_scrolling;
                    state.fast_scrolling = state.scroll_velocity > FAST_SCROLL_THRESHOLD;
                    
                    // Log state changes
                    if was_fast_scrolling != state.fast_scrolling {
                        log::info!(
                            "Scroll mode changed: {} (velocity: {:.0} px/s)",
                            if state.fast_scrolling { "FAST" } else { "NORMAL" },
                            state.scroll_velocity
                        );
                    }
                    
                    // Reset scroll stopped time when actively scrolling
                    state.scroll_stopped_time = None;
                }
            }
            
            state.last_scroll_position = current_position;
            state.last_scroll_time = Some(now);
            
            // Schedule a check for scroll stop
            Task::perform(
                async move {
                    tokio::time::sleep(Duration::from_millis(SCROLL_STOP_DEBOUNCE_MS)).await;
                },
                |_| Message::CheckScrollStopped,
            )
        }

        Message::TvShowsGridScrolled(viewport) => {
            // Update scroll position
            state.tv_shows_grid_state.update_scroll(viewport);
            
            // Use same velocity tracking as movies grid
            let current_position = viewport.absolute_offset().y;
            let now = Instant::now();
            
            state.scroll_samples.push_back((now, current_position));
            while state.scroll_samples.len() > 5 {
                state.scroll_samples.pop_front();
            }
            
            if state.scroll_samples.len() >= 2 {
                let oldest = state.scroll_samples.front().unwrap();
                let newest = state.scroll_samples.back().unwrap();
                
                let time_delta = newest.0.duration_since(oldest.0).as_secs_f32();
                if time_delta > 0.0 {
                    let position_delta = newest.1 - oldest.1;
                    state.scroll_velocity = (position_delta / time_delta).abs();
                    
                    let was_fast_scrolling = state.fast_scrolling;
                    state.fast_scrolling = state.scroll_velocity > FAST_SCROLL_THRESHOLD;
                    
                    if was_fast_scrolling != state.fast_scrolling {
                        log::info!(
                            "TV scroll mode changed: {} (velocity: {:.0} px/s)",
                            if state.fast_scrolling { "FAST" } else { "NORMAL" },
                            state.scroll_velocity
                        );
                    }
                    
                    state.scroll_stopped_time = None;
                }
            }
            
            state.last_scroll_position = current_position;
            state.last_scroll_time = Some(now);
            
            Task::perform(
                async move {
                    tokio::time::sleep(Duration::from_millis(SCROLL_STOP_DEBOUNCE_MS)).await;
                },
                |_| Message::CheckScrollStopped,
            )
        }

        Message::PostersBatchChecked(results) => {
            // Process batch poster check results
            log::info!("PostersBatchChecked: Processing {} results", results.len());
            let mut missing_metadata_ids = Vec::new();
            let mut posters_to_load = Vec::new();

            for (media_id, poster_url) in results {
                if poster_url.is_some() {
                    // Poster exists, queue it for loading if not already loading
                    if !state.loading_posters.contains(&media_id) {
                        posters_to_load.push(media_id);
                    }
                } else {
                    // No poster available, mark as failed and queue for metadata fetch
                    state.poster_cache.set_failed(media_id.clone());
                    missing_metadata_ids.push(media_id);
                }
            }

            log::info!(
                "PostersBatchChecked: {} posters to load, {} missing metadata",
                posters_to_load.len(),
                missing_metadata_ids.len()
            );

            let mut tasks = Vec::new();

            // Only load posters if we have capacity (limit concurrent loads)
            let current_loading = state.loading_posters.len();
            let max_concurrent = posters::MAX_CONCURRENT_LOADS;
            let available_slots = max_concurrent.saturating_sub(current_loading);

            if available_slots == 0 {
                log::debug!(
                    "PostersBatchChecked: Already {} posters loading, will queue for later",
                    current_loading
                );
                // Mark as loading in cache for UI display, but don't add to loading_posters
                // This way PosterMonitorTick can still pick them up when slots are available
                for media_id in posters_to_load {
                    state.poster_cache.set_loading(media_id);
                }
            } else {
                // Load only as many as we have slots available
                let posters_to_load_now = posters_to_load
                    .into_iter()
                    .take(available_slots)
                    .collect::<Vec<_>>();
                log::debug!(
                    "PostersBatchChecked: Loading {} posters (slots available: {})",
                    posters_to_load_now.len(),
                    available_slots
                );

                for media_id in posters_to_load_now {
                    // Only mark as loading when we actually spawn the thread
                    state.loading_posters.insert(media_id.clone());
                    state.poster_cache.set_loading(media_id.clone());

                    let server_url = state.server_url.clone();
                    let semaphore = state.poster_load_semaphore.clone();

                    // Fire off poster load task
                    tasks.push(Task::perform(
                        async move {
                            let media_id_log = media_id.clone();
                            // Spawn on tokio runtime for true parallelism
                            tokio::spawn(async move {
                                // Semaphore controls concurrency in background thread
                                log::debug!("Acquiring semaphore for poster {}", media_id_log);
                                let _permit = semaphore.acquire().await.unwrap();
                                log::debug!("Loading poster {}", media_id_log);
                                let result =
                                    poster_cache::fetch_poster_with_id(server_url, media_id).await;
                                drop(_permit); // Release permit immediately
                                log::debug!("Finished loading poster {}", media_id_log);
                                result
                            })
                            .await
                            .unwrap_or_else(|e| {
                                let id = format!("error");
                                (id, Err(format!("Task panicked: {}", e)))
                            })
                        },
                        |(id, result)| Message::PosterLoaded(id, result),
                    ));
                }
            }

            // Queue missing metadata to be fetched by server in background
            if !missing_metadata_ids.is_empty() {
                let server_url = state.server_url.clone();
                tasks.push(Task::perform(
                    async move {
                        if let Err(e) =
                            media_library::queue_missing_metadata(&server_url, missing_metadata_ids)
                                .await
                        {
                            log::warn!("Failed to queue missing metadata: {}", e);
                        }
                    },
                    |_| Message::NoOp,
                ));
            }

            if tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }

        Message::LoadPoster(media_id) => {
            // Start loading poster if not already loading
            if !state.loading_posters.contains(&media_id)
                && state.poster_cache.get(&media_id).is_none()
            {
                state.loading_posters.insert(media_id.clone());
                state.poster_cache.set_loading(media_id.clone());
                let server_url = state.server_url.clone();
                Task::perform(
                    poster_cache::fetch_poster_with_id(server_url, media_id.clone()),
                    |(id, result)| Message::PosterLoaded(id, result),
                )
            } else {
                Task::none()
            }
        }

        Message::PosterLoaded(media_id, result) => {
            // Remove from loading set
            state.loading_posters.remove(&media_id);

            let mut tasks = Vec::new();

            // Handle result with fade-in animation
            match result {
                Ok(bytes) => {
                    log::info!("Successfully loaded poster for media_id: {} ({} bytes)", media_id, bytes.len());
                    
                    // Process the poster to create both thumbnail and full-size versions
                    match poster_cache::process_poster_bytes(bytes) {
                        Ok((thumbnail_handle, full_size_handle)) => {
                            // Check if poster is visible before animating
                            if state.is_media_visible(&media_id) {
                                log::debug!("Poster {} is visible, starting flip animation", media_id);
                                // Start at 0 opacity for animation
                                state.poster_cache.set_loaded(media_id.clone(), thumbnail_handle, full_size_handle);
                                state.poster_cache.update_opacity(&media_id, 0.0); // Start at 0 opacity
                                state.poster_animation_states.insert(media_id.clone(), 0.0);
                                
                                // Set flip animation type
                                state.poster_animation_types.insert(
                                    media_id.clone(), 
                                    (crate::widgets::AnimationType::Flip { duration: Duration::from_millis(600) }, Instant::now())
                                );

                                // Start animation
                                tasks.push(Task::perform(
                                    async move { media_id },
                                    Message::AnimatePoster,
                                ));
                            } else {
                                log::debug!("Poster {} is not visible, setting to full opacity immediately", media_id);
                                // Not visible, set to full opacity immediately
                                state.poster_cache.set_loaded(media_id.clone(), thumbnail_handle, full_size_handle);
                                state.poster_cache.update_opacity(&media_id, 1.0);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to process poster for {}: {}", media_id, e);
                            state.poster_cache.set_failed(media_id);
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to load poster for {}: {}", media_id, e);
                    state.poster_cache.set_failed(media_id);
                }
            }

            // Since we finished loading one poster, immediately check if we should load more
            // This ensures continuous loading without waiting for the next tick
            tasks.push(Task::perform(async {}, |_| Message::PosterMonitorTick));

            if tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }

        Message::PostersBatchLoaded(_results) => {
            // This message is no longer used - posters load individually
            Task::none()
        }

        Message::ProcessPosterQueue => {
            // This message is no longer used - posters load independently
            Task::none()
        }

        Message::PosterProcessed(media_id, result) => {
            let mut tasks = Vec::new();
            
            match result {
                Ok((thumbnail_handle, full_size_handle, was_visible)) => {
                    // Check if poster is still visible (may have scrolled out of view during processing)
                    let is_still_visible = state.is_media_visible(&media_id);
                    
                    if was_visible && is_still_visible {
                        // Limit concurrent animations for performance
                        let active_animations = state.poster_animation_types.len();
                        if active_animations < 5 {
                            log::debug!("Poster {} is still visible, starting flip animation", media_id);
                            // Start at 0 opacity for animation
                            state.poster_cache.set_loaded(media_id.clone(), thumbnail_handle, full_size_handle);
                            state.poster_cache.update_opacity(&media_id, 0.0); // Start at 0 opacity
                            state.poster_animation_states.insert(media_id.clone(), 0.0);
                            
                            // Set flip animation type with slightly longer duration
                            state.poster_animation_types.insert(
                                media_id.clone(), 
                                (crate::widgets::AnimationType::Flip { duration: Duration::from_millis(800) }, Instant::now())
                            );

                            // Start animation
                            tasks.push(Task::perform(
                                async move { media_id },
                                Message::AnimatePoster,
                            ));
                        } else {
                            log::debug!("Too many animations active ({}), skipping animation for {}", active_animations, media_id);
                            // Too many animations, just show immediately
                            state.poster_cache.set_loaded(media_id.clone(), thumbnail_handle, full_size_handle);
                            state.poster_cache.update_opacity(&media_id, 1.0);
                        }
                    } else {
                        log::debug!("Poster {} is not visible, setting to full opacity immediately", media_id);
                        // Not visible, set to full opacity immediately
                        state.poster_cache.set_loaded(media_id.clone(), thumbnail_handle, full_size_handle);
                        state.poster_cache.update_opacity(&media_id, 1.0);
                    }
                }
                Err(e) => {
                    log::error!("Failed to process poster for {}: {}", media_id, e);
                    state.poster_cache.set_failed(media_id);
                }
            }
            
            // Since we finished processing one poster, check if we should load more
            tasks.push(Task::perform(async {}, |_| Message::PosterMonitorTick));
            
            if tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }
        
        Message::AnimatePoster(media_id) => {
            // Animate poster only if still visible
            if !state.is_media_visible(&media_id) {
                // Item scrolled out of view, complete animation immediately
                state.poster_cache.update_opacity(&media_id, 1.0);
                state.poster_animation_states.remove(&media_id);
                state.poster_animation_types.remove(&media_id);
                return Task::none();
            }

            // Get animation type and calculate progress
            if let Some((animation_type, start_time)) = state.poster_animation_types.get(&media_id) {
                let elapsed = start_time.elapsed().as_secs_f32();
                
                let (is_complete, new_opacity) = match animation_type {
                    crate::widgets::AnimationType::Fade { duration } => {
                        let progress = (elapsed / duration.as_secs_f32()).min(1.0);
                        (progress >= 1.0, progress)
                    }
                    crate::widgets::AnimationType::Flip { duration } => {
                        let progress = (elapsed / duration.as_secs_f32()).min(1.0);
                        // For flip, opacity stays at 0 for first half, then fades in
                        let opacity = if progress < 0.5 { 
                            0.0 
                        } else { 
                            (progress - 0.5) * 2.0 
                        };
                        (progress >= 1.0, opacity)
                    }
                    crate::widgets::AnimationType::None => (true, 1.0),
                };
                
                // Update opacity
                state.poster_cache.update_opacity(&media_id, new_opacity);
                if let Some(opacity) = state.poster_animation_states.get_mut(&media_id) {
                    *opacity = new_opacity;
                }
                
                if !is_complete {
                    // Continue animation
                    Task::perform(
                        async move {
                            tokio::time::sleep(std::time::Duration::from_millis(25)).await; // ~40fps for smoother performance
                            media_id
                        },
                        Message::AnimatePoster,
                    )
                } else {
                    // Animation complete, cleanup
                    state.poster_animation_states.remove(&media_id);
                    state.poster_animation_types.remove(&media_id);
                    Task::none()
                }
            } else {
                // No animation type set, use default fade
                if let Some(opacity) = state.poster_animation_states.get_mut(&media_id) {
                    if *opacity < 1.0 {
                        let increment = 0.02;
                        *opacity = (*opacity + increment).min(1.0);
                        state.poster_cache.update_opacity(&media_id, *opacity);
                        
                        if *opacity < 1.0 {
                            Task::perform(
                                async move {
                                    tokio::time::sleep(std::time::Duration::from_millis(25)).await; // ~40fps
                                    media_id
                                },
                                Message::AnimatePoster,
                            )
                        } else {
                            Task::none()
                        }
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
        }

        Message::MarkPostersForLoading(media_ids, new_progress) => {
            // Mark posters for loading (from background thread)
            log::debug!(
                "Marking {} posters for loading, progress: {}",
                media_ids.len(),
                new_progress
            );

            // Update progress position
            state.poster_mark_progress = new_progress;

            // Mark each poster as loading in the cache
            for media_id in media_ids {
                if state.poster_cache.get(&media_id).is_none() {
                    state.poster_cache.set_loading(media_id.clone());
                    state.posters_to_load.push_back(media_id);
                }
            }

            Task::none()
        }

        Message::CheckPosterUpdates => {
            // Re-fetch library data to get updated poster URLs
            let server_url = state.server_url.clone();
            Task::perform(
                media_library::fetch_library(server_url),
                |result| match result {
                    Ok(files) => Message::LibraryLoaded(Ok(files)),
                    Err(e) => Message::LibraryLoaded(Err(e.to_string())),
                },
            )
        }

        Message::PosterMonitorTick => {
            let mut tasks = Vec::new();
            
            // Skip poster loading during any meaningful scrolling
            if state.fast_scrolling || state.scroll_velocity > 1000.0 {
                log::debug!("Skipping poster loading during scrolling (velocity: {:.0} px/s)", state.scroll_velocity);
                return Task::none();
            }

            // Cleanup: Check for stuck Loading states
            // Items that are in Loading state but not in loading_posters or posters_to_load
            let loading_ids = state.poster_cache.get_loading_ids();
            for id in loading_ids {
                if !state.loading_posters.contains(&id) && !state.posters_to_load.contains(&id) {
                    log::warn!("Found stuck poster in Loading state: {}, re-queuing", id);
                    state.posters_to_load.push_back(id);
                }
            }

            // Phase 1: Load any items already marked as loading (UI thread)
            PROFILER.start("PosterMonitorTick::phase1_check_loading");
            let current_loading = state.loading_posters.len();
            let max_concurrent = 3usize;

            //log::debug!(
            //    "PosterMonitorTick: {} posters currently loading",
            //    current_loading
            //);

            if current_loading < max_concurrent && !state.posters_to_load.is_empty() {
                // Find ONE poster marked as loading that isn't actively being loaded
                if let Some(media_id) = state.posters_to_load.pop_front() {
                    state.loading_posters.insert(media_id.clone());
                    state.poster_cache.set_loading(media_id.clone());

                    /*
                    // Check movies first
                    for media in &state.movies {
                        if let Some(PosterState::Loading) = state.poster_cache.get(&media.id) {
                            if !state.loading_posters.contains(&media.id) {
                                found_poster = Some((media.id.clone(), state.server_url.clone()));
                                state.loading_posters.insert(media.id.clone());
                                break;
                            }
                        }
                    }

                    // If not found in movies, check TV shows
                    if found_poster.is_none() {
                        for (_, show) in state.tv_shows.iter() {
                            if let Some(poster_id) = show.get_poster_id() {
                                if let Some(PosterState::Loading) = state.poster_cache.get(&poster_id) {
                                    if !state.loading_posters.contains(&poster_id) {
                                        found_poster = Some((poster_id.clone(), state.server_url.clone()));
                                        state.loading_posters.insert(poster_id.clone());
                                        break;
                                    }
                                }
                            }
                        }
                    } */

                    // If we found a poster to load, create the task
                    //if let Some((media_id, server_url)) = found_poster {
                    log::debug!("Loading queued poster: {}", media_id);
                    tasks.push(Task::perform(
                        poster_cache::fetch_poster_with_id(state.server_url.clone(), media_id),
                        |(id, result)| Message::PosterLoaded(id, result),
                    ));
                    //}
                }
            }

            PROFILER.end("PosterMonitorTick::phase1_check_loading");
            PROFILER.start("PosterMonitorTick::phase2_mark_for_loading");

            // Phase 2: Mark new items for loading
            // Optimized to do minimal work on UI thread
            const MAX_TO_MARK: usize = 4;
            const BUFFER_ROWS: usize = 2;
            let mut items_marked = 0;

            // Priority 1: Visible items
            match state.view_mode {
                ViewMode::All | ViewMode::Movies => {
                    // Check visible movies
                    for idx in state.movies_grid_state.visible_range.clone() {
                        if items_marked >= MAX_TO_MARK {
                            break;
                        }
                        if let Some(movie) = state.movies.get(idx) {
                            // Check if item needs poster
                            if state.poster_cache.get(&movie.id).is_none()
                                && !state.loading_posters.contains(&movie.id)
                                && !state.posters_to_load.contains(&movie.id)
                            {
                                state.posters_to_load.push_back(movie.id.clone());
                                items_marked += 1;
                            }
                        }
                    }
                }
                _ => {}
            }

            match state.view_mode {
                ViewMode::All | ViewMode::TvShows => {
                    // Check visible TV shows
                    for idx in state.tv_shows_grid_state.visible_range.clone() {
                        if items_marked >= MAX_TO_MARK {
                            break;
                        }
                        if let Some(show) = state.tv_shows_sorted.get(idx) {
                            if let Some(poster_id) = show.get_poster_id() {
                                // Check if item needs poster
                                if state.poster_cache.get(&poster_id).is_none()
                                    && !state.loading_posters.contains(&poster_id)
                                    && !state.posters_to_load.contains(&poster_id)
                                {
                                    state.posters_to_load.push_back(poster_id);
                                    items_marked += 1;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }

            // Priority 2: Near-visible items (preload ahead)
            if items_marked < MAX_TO_MARK {
                let items_per_row = state.movies_grid_state.columns;

                match state.view_mode {
                    ViewMode::All | ViewMode::Movies => {
                        let preload_start = state.movies_grid_state.visible_range.end;
                        let preload_end =
                            (preload_start + BUFFER_ROWS * items_per_row).min(state.movies.len());

                        for idx in preload_start..preload_end {
                            if items_marked >= MAX_TO_MARK {
                                break;
                            }
                            if let Some(movie) = state.movies.get(idx) {
                                // Check if item needs poster
                                if state.poster_cache.get(&movie.id).is_none()
                                    && !state.loading_posters.contains(&movie.id)
                                    && !state.posters_to_load.contains(&movie.id)
                                {
                                    state.posters_to_load.push_back(movie.id.clone());
                                    items_marked += 1;
                                }
                            }
                        }
                    }
                    _ => {}
                }

                match state.view_mode {
                    ViewMode::All | ViewMode::TvShows => {
                            let preload_start = state.tv_shows_grid_state.visible_range.end;
                        let preload_end =
                            (preload_start + BUFFER_ROWS * items_per_row).min(state.tv_shows_sorted.len());

                        for idx in preload_start..preload_end {
                            if items_marked >= MAX_TO_MARK {
                                break;
                            }
                            if let Some(show) = state.tv_shows_sorted.get(idx) {
                                if let Some(poster_id) = show.get_poster_id() {
                                    // Check if item needs poster
                                    if state.poster_cache.get(&poster_id).is_none()
                                        && !state.loading_posters.contains(&poster_id)
                                        && !state.posters_to_load.contains(&poster_id)
                                    {
                                        state.posters_to_load.push_back(poster_id);
                                        items_marked += 1;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            PROFILER.end("PosterMonitorTick::phase2_mark_for_loading");

            if tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }

        Message::ScanCompleted(result) => {
            state.scanning = false;
            match result {
                Ok(msg) => {
                    log::info!("Scan completed: {}", msg);
                    // Refresh library after successful scan
                    update(state, Message::RefreshLibrary)
                }
                Err(e) => {
                    log::error!("Scan failed: {}", e);
                    state.error_message = Some(format!("Scan failed: {}", e));
                    Task::none()
                }
            }
        }

        Message::PlayMedia(media) => {
            log::info!(
                "Playing media: {} (id: {})",
                media.display_title(),
                media.id
            );
            log::info!("Media path: {:?}", media.path);
            log::info!("Server URL: {}", state.server_url);
            
            // Save current scroll position before playing media
            state.save_scroll_position();
            
            state.player.current_media = Some(media.clone());

            // Check if this is HDR content
            let is_hdr_content = if let Some(metadata) = &media.metadata {
                // Check bit depth
                if let Some(bit_depth) = metadata.bit_depth {
                    if bit_depth > 8 {
                        log::info!("HDR detected: bit depth = {}", bit_depth);
                        true
                    } else {
                        false
                    }
                } else if let Some(color_transfer) = &metadata.color_transfer {
                    // Check color transfer characteristics
                    let hdr_transfers = ["smpte2084", "arib-std-b67", "smpte2086"];
                    let is_hdr = hdr_transfers.iter().any(|&t| color_transfer.contains(t));
                    if is_hdr {
                        log::info!("HDR detected: color transfer = {}", color_transfer);
                    }
                    is_hdr
                } else if let Some(color_primaries) = &metadata.color_primaries {
                    // Check color primaries
                    let is_hdr = color_primaries.contains("bt2020");
                    if is_hdr {
                        log::info!("HDR detected: color primaries = {}", color_primaries);
                    }
                    is_hdr
                } else {
                    false
                }
            } else {
                // Fallback to filename detection if no metadata
                let filename_suggests_hdr = media.filename.contains("2160p") || 
                                          media.filename.contains("UHD") ||
                                          media.filename.contains("HDR") ||
                                          media.filename.contains("DV");
                if filename_suggests_hdr {
                    log::info!("HDR suggested by filename: {}", media.filename);
                }
                filename_suggests_hdr
            };
            
            // Determine if we should use adaptive streaming
            // Use HLS for HDR content that requires transcoding
            let use_adaptive_streaming = false; // Only use adaptive for HDR content
            
            // Initialize HLS client if using adaptive streaming
            if use_adaptive_streaming {
                state.player.hls_client = Some(crate::hls::HlsClient::new(state.server_url.clone()));
                state.player.using_hls = true;
            }
            
            let (video_url, start_transcoding_task) = if media.path.starts_with("http") {
                (media.path.clone(), None)
            } else if use_adaptive_streaming && is_hdr_content {
                // Use adaptive streaming for all content
                log::info!("Using adaptive streaming for media: {}", media.id);
                
                // Store transcoding state
                state.player.is_hdr_content = is_hdr_content;
                state.player.using_hls = true;
                state.player.transcoding_status = Some(crate::player::state::TranscodingStatus::Pending);
                
                // Create HLS client
                let hls_client = crate::hls::HlsClient::new(state.server_url.clone());
                state.player.hls_client = Some(hls_client);
                
                // Use master playlist URL for HLS playback
                log::debug!("Building master URL - server: {}, media.id: {}", state.server_url, media.id);
                log::debug!("Media ID bytes: {:?}", media.id.as_bytes());
                // Percent-encode the media ID to handle special characters
                let encoded_media_id = urlencoding::encode(&media.id);
                let master_url = format!("{}/transcode/{}/master.m3u8", state.server_url, encoded_media_id);
                log::debug!("Encoded media ID: {}", encoded_media_id);
                log::debug!("Constructed master URL: {}", master_url);
                log::debug!("Master URL bytes: {:?}", master_url.as_bytes());
                
                // Create task to start transcoding only if we don't already have a job
                let start_task = if state.player.transcoding_job_id.is_none() {
                    let server_url = state.server_url.clone();
                    let media_id = media.id.clone();
                    
                    log::info!("Starting new adaptive transcoding for media: {}", media_id);
                    log::info!("Current transcoding status: {:?}", state.player.transcoding_status);
                    
                    Some(Task::perform(
                        async move {
                            let client = crate::hls::HlsClient::new(server_url);
                            // Use retry logic with 3 retries
                            match client.start_adaptive_transcoding_with_retry(&media_id, 3).await {
                                Ok(job_id) => {
                                    log::info!("Adaptive transcoding started successfully with master job ID: {}", job_id);
                                    Ok(job_id)
                                }
                                Err(e) => {
                                    log::error!("Failed to start adaptive transcoding: {}", e);
                                    Err(e)
                                }
                            }
                        },
                        Message::TranscodingStarted
                    ))
                } else {
                    log::warn!("Transcoding job already exists: {:?}, skipping duplicate start request", state.player.transcoding_job_id);
                    log::warn!("Current transcoding status: {:?}", state.player.transcoding_status);
                    None
                };
                
                (master_url, start_task)
            } else {
                // Fallback to direct streaming (old behavior)
                let video_url = if is_hdr_content {
                    let profile = if let Some(metadata) = &media.metadata {
                        if let Some(height) = metadata.height {
                            if height >= 2160 { "hdr_to_sdr_4k" } else { "hdr_to_sdr_1080p" }
                        } else {
                            "hdr_to_sdr_1080p"
                        }
                    } else {
                        "hdr_to_sdr_1080p"
                    };
                    
                    let encoded_media_id = urlencoding::encode(&media.id);
                    let transcode_url = format!("{}/stream/{}", state.server_url, encoded_media_id);
                    log::info!("Using direct transcode stream: {}", transcode_url);
                    
                    state.player.is_hdr_content = true;
                    state.player.using_hls = false;
                    state.player.transcoding_status = Some(crate::player::state::TranscodingStatus::Processing { progress: 0.0 });
                    
                    transcode_url
                } else {
                    let encoded_media_id = urlencoding::encode(&media.id);
                    let stream_url = format!("{}/stream/{}", state.server_url, encoded_media_id);
                    log::info!("Using direct stream: {}", stream_url);
                    
                    state.player.is_hdr_content = false;
                    state.player.using_hls = false;
                    state.player.transcoding_status = None;
                    
                    stream_url
                };
                
                // For direct playback, we'll get duration from the video object itself
                // This ensures we have the actual playable duration, not just metadata
                
                (video_url, None)
            };

            log::info!("Final video URL: {}", video_url);
            
            // Check for UTF-8 validity before parsing
            if let Err(e) = std::str::from_utf8(video_url.as_bytes()) {
                log::error!("Video URL contains invalid UTF-8: {:?}", e);
                log::error!("URL bytes: {:?}", video_url.as_bytes());
            }

            // Parse URL and load video
            match url::Url::parse(&video_url) {
                Ok(url) => {
                    state.player.current_url = Some(url);
                    // Set loading state
                    state.view = ViewState::LoadingVideo {
                        url: video_url.clone(),
                    };
                    state.error_message = None;
                    
                    // If we're using adaptive streaming, don't load video yet
                    // Wait for transcoding to be ready first
                    if use_adaptive_streaming && is_hdr_content {
                        match start_transcoding_task {
                            Some(transcode_task) => transcode_task,
                            None => {
                                log::error!("No transcoding task for adaptive streaming!");
                                Task::none()
                            }
                        }
                    } else {
                        // For direct streaming, load video immediately
                        load_video(state)
                    }
                }
                Err(e) => {
                    state.error_message = Some(format!("Invalid URL: {}", e));
                    state.view = ViewState::VideoError {
                        message: format!("Invalid URL: {}", e),
                    };
                    Task::none()
                }
            }
        }

        Message::MediaUnavailable(reason, message) => {
            log::error!("Media unavailable: {} - {}", reason, message);

            let error_msg = match reason.as_str() {
                "library_offline" => {
                    "Media Library Offline\n\nThe media library storage is currently unavailable. Please ensure the storage device is connected and mounted properly.".to_string()
                }
                "file_missing" => {
                    "Media File Not Found\n\nThis media file has been moved or deleted from the library. You may need to rescan the library to update the database.".to_string()
                }
                _ => message.clone()
            };

            state.error_message = Some(error_msg.clone());
            state.view = ViewState::VideoError { message: error_msg };
            Task::none()
        }

        Message::ViewDetails(media) => {
            log::info!("Viewing details for: {}", media.display_title());
            
            // Save current scroll position before navigating away
            state.save_scroll_position();
            
            // Determine if it's a movie or TV episode
            if media.is_tv_episode() {
                state.view = ViewState::EpisodeDetail { media };
            } else {
                state.view = ViewState::MovieDetail { media };
            }
            Task::none()
        }

        Message::ViewTvShow(show_name) => {
            log::info!("Viewing TV show: {}", show_name);
            
            // Save current scroll position before navigating away
            state.save_scroll_position();
            
            state.view = ViewState::TvShowDetail {
                show_name: show_name.clone(),
            };
            // Load show details
            let server_url = state.server_url.clone();
            let show_name_clone = show_name.clone();
            Task::perform(
                media_library::fetch_tv_show_details(server_url, show_name),
                move |result| match result {
                    Ok(details) => Message::TvShowLoaded(show_name_clone.clone(), Ok(details)),
                    Err(e) => Message::TvShowLoaded(show_name_clone.clone(), Err(e.to_string())),
                },
            )
        }

        Message::ViewSeason(show_name, season_num) => {
            log::info!("Viewing season {} of {}", season_num, show_name);
            
            // Save current scroll position if navigating from library view
            if matches!(state.view, ViewState::Library) {
                state.save_scroll_position();
            }
            
            state.view = ViewState::SeasonDetail {
                show_name: show_name.clone(),
                season_num,
            };
            // Load season details
            let server_url = state.server_url.clone();
            let show_name_clone = show_name.clone();
            Task::perform(
                media_library::fetch_season_details(server_url, show_name, season_num),
                move |result| match result {
                    Ok(details) => {
                        Message::SeasonLoaded(show_name_clone.clone(), season_num, Ok(details))
                    }
                    Err(e) => Message::SeasonLoaded(
                        show_name_clone.clone(),
                        season_num,
                        Err(e.to_string()),
                    ),
                },
            )
        }

        Message::ViewEpisode(media) => {
            log::info!("Viewing episode: {}", media.display_title());
            
            // Save current scroll position if navigating from library view
            if matches!(state.view, ViewState::Library) {
                state.save_scroll_position();
            }
            
            state.view = ViewState::EpisodeDetail { media };
            Task::none()
        }

        Message::SetViewMode(mode) => {
            log::info!("Setting view mode to: {:?}", mode);
            log::info!("Current state: {} movies, {} TV shows, {} libraries", 
                state.movies.len(), 
                state.tv_shows.len(), 
                state.libraries.len()
            );
            
            // Save current scroll position for the old view mode
            state.save_scroll_position();
            
            state.view_mode = mode;
            
            // Restore scroll position for the new view mode
            let restore_task = match mode {
                ViewMode::Movies => {
                    if let Some(position) = state.movies_scroll_position {
                        log::debug!("Restoring movies scroll position: {}", position);
                        scrollable::scroll_to(
                            state.movies_grid_state.scrollable_id.clone(),
                            scrollable::AbsoluteOffset { x: 0.0, y: position },
                        )
                    } else {
                        Task::none()
                    }
                }
                ViewMode::TvShows => {
                    if let Some(position) = state.tv_shows_scroll_position {
                        log::debug!("Restoring TV shows scroll position: {}", position);
                        scrollable::scroll_to(
                            state.tv_shows_grid_state.scrollable_id.clone(),
                            scrollable::AbsoluteOffset { x: 0.0, y: position },
                        )
                    } else {
                        Task::none()
                    }
                }
                ViewMode::All => {
                    // In All mode, restore movies scroll position
                    if let Some(position) = state.movies_scroll_position {
                        scrollable::scroll_to(
                            state.movies_grid_state.scrollable_id.clone(),
                            scrollable::AbsoluteOffset { x: 0.0, y: position },
                        )
                    } else {
                        Task::none()
                    }
                }
            };
            
            // Mark visible items for loading in the new view
            state.mark_visible_posters_for_loading();
            
            restore_task
        }

        Message::SetSortBy(sort_by) => {
            log::info!("Setting sort by: {:?}", sort_by);
            state.sort_by = sort_by;
            // Sort the media collections
            sort_media(
                &mut state.movies,
                &mut state.tv_shows,
                sort_by,
                state.sort_order,
            );
            
            // Maintain current scroll position after sorting
            // No need to explicitly restore as the scrollable will maintain its position
            Task::none()
        }

        Message::ToggleSortOrder => {
            state.sort_order = match state.sort_order {
                SortOrder::Ascending => SortOrder::Descending,
                SortOrder::Descending => SortOrder::Ascending,
            };
            log::info!("Toggled sort order to: {:?}", state.sort_order);
            // Re-sort with new order
            sort_media(
                &mut state.movies,
                &mut state.tv_shows,
                state.sort_by,
                state.sort_order,
            );
            
            // Maintain current scroll position after sorting
            // No need to explicitly restore as the scrollable will maintain its position
            Task::none()
        }

        Message::TvShowLoaded(show_name, result) => {
            match result {
                Ok(details) => {
                    log::info!("TV show details loaded for: {}", show_name);

                    // Check if we need to load the poster
                    let mut tasks = Vec::new();
                    if let Some(poster_url) = &details.poster_url {
                        if state.image_cache.get(poster_url).is_none() {
                            state.image_cache.set_loading(poster_url.clone());
                            let source = ImageSource::Url(poster_url.clone());
                            tasks.push(Task::perform(
                                image_cache::fetch_image_with_key(source),
                                |(key, result)| Message::ImageLoaded(key, result),
                            ));
                        }
                    }

                    // Also load season posters
                    for season in &details.seasons {
                        if let Some(season_poster_url) = &season.poster_url {
                            // Convert relative paths to full URLs
                            let full_url = if season_poster_url.starts_with("/") {
                                format!("{}{}", state.server_url, season_poster_url)
                            } else {
                                season_poster_url.clone()
                            };

                            if state.image_cache.get(&full_url).is_none() {
                                state.image_cache.set_loading(full_url.clone());
                                let source = ImageSource::Url(full_url);
                                tasks.push(Task::perform(
                                    image_cache::fetch_image_with_key(source),
                                    |(key, result)| Message::ImageLoaded(key, result),
                                ));
                            }
                        }
                    }

                    state.current_show_details = Some(details.clone());
                    // Create carousel state for seasons
                    state.show_seasons_carousel = Some(CarouselState::new(details.seasons.len()));
                    if let Some(carousel) = &mut state.show_seasons_carousel {
                        let available_width = state.window_size.width - 80.0;
                        carousel.update_items_per_page(available_width);
                    }

                    if !tasks.is_empty() {
                        Task::batch(tasks)
                    } else {
                        Task::none()
                    }
                }
                Err(e) => {
                    log::error!("Failed to load TV show details: {}", e);
                    state.error_message = Some(format!("Failed to load show details: {}", e));
                    Task::none()
                }
            }
        }

        Message::SeasonLoaded(show_name, season_num, result) => {
            match result {
                Ok(details) => {
                    log::info!("Season {} details loaded for: {}", season_num, show_name);

                    // Check if we need to load the poster
                    let mut tasks = Vec::new();
                    if let Some(poster_url) = &details.poster_url {
                        // Convert relative paths to full URLs
                        let full_url = if poster_url.starts_with("/") {
                            format!("{}{}", state.server_url, poster_url)
                        } else {
                            poster_url.clone()
                        };

                        // Use full URL as cache key
                        if state.image_cache.get(&full_url).is_none() {
                            state.image_cache.set_loading(full_url.clone());
                            let source = ImageSource::Url(full_url);
                            tasks.push(Task::perform(
                                image_cache::fetch_image_with_key(source),
                                |(key, result)| Message::ImageLoaded(key, result),
                            ));
                        }
                    }

                    // Also load thumbnails for episodes
                    for episode in &details.episodes {
                        // Episodes will use server thumbnails
                        let thumbnail_key = format!("thumbnail:{}", episode.id);
                        if state.image_cache.get(&thumbnail_key).is_none() {
                            state.image_cache.set_loading(thumbnail_key.clone());
                            let source = ImageSource::ServerThumbnail {
                                server_url: state.server_url.clone(),
                                media_id: episode.id.clone(),
                            };
                            tasks.push(Task::perform(
                                image_cache::fetch_image_with_key(source),
                                |(key, result)| Message::ImageLoaded(key, result),
                            ));
                        }
                    }

                    state.current_season_details = Some(details.clone());
                    // Create carousel state for episodes
                    state.season_episodes_carousel =
                        Some(CarouselState::new(details.episodes.len()));
                    if let Some(carousel) = &mut state.season_episodes_carousel {
                        let available_width = state.window_size.width - 80.0;
                        carousel.update_items_per_page(available_width);
                    }

                    if !tasks.is_empty() {
                        Task::batch(tasks)
                    } else {
                        Task::none()
                    }
                }
                Err(e) => {
                    log::error!("Failed to load season details: {}", e);
                    state.error_message = Some(format!("Failed to load season details: {}", e));
                    Task::none()
                }
            }
        }

        // Delegate player messages to the player module
        msg if PlayerMessage::is_player_message(&msg) => {
            if let Some(player_msg) = PlayerMessage::from_main_message(msg) {
                // Handle special cases that need access to main state
                match &player_msg {
                    PlayerMessage::BackToLibrary => {
                        close_video(state);
                        state.view = ViewState::Library;
                        
                        // Restore scroll position when returning to library
                        let mut restore_task = Task::none();
                        match state.view_mode {
                            ViewMode::Movies => {
                                if let Some(position) = state.movies_scroll_position {
                                    log::debug!("Restoring movies scroll position: {}", position);
                                    restore_task = scrollable::scroll_to(
                                        state.movies_grid_state.scrollable_id.clone(),
                                        scrollable::AbsoluteOffset { x: 0.0, y: position },
                                    );
                                }
                            }
                            ViewMode::TvShows => {
                                if let Some(position) = state.tv_shows_scroll_position {
                                    log::debug!("Restoring TV shows scroll position: {}", position);
                                    restore_task = scrollable::scroll_to(
                                        state.tv_shows_grid_state.scrollable_id.clone(),
                                        scrollable::AbsoluteOffset { x: 0.0, y: position },
                                    );
                                }
                            }
                            ViewMode::All => {
                                // In All mode, could restore both but let's just restore movies for simplicity
                                if let Some(position) = state.movies_scroll_position {
                                    restore_task = scrollable::scroll_to(
                                        state.movies_grid_state.scrollable_id.clone(),
                                        scrollable::AbsoluteOffset { x: 0.0, y: position },
                                    );
                                }
                            }
                        }
                        return restore_task;
                    }
                    PlayerMessage::Stop => {
                        // Stop playback and return to library
                        close_video(state);
                        state.view = ViewState::Library;
                        
                        // Restore scroll position when returning to library
                        let mut restore_task = Task::none();
                        match state.view_mode {
                            ViewMode::Movies => {
                                if let Some(position) = state.movies_scroll_position {
                                    log::debug!("Restoring movies scroll position after stop: {}", position);
                                    restore_task = scrollable::scroll_to(
                                        state.movies_grid_state.scrollable_id.clone(),
                                        scrollable::AbsoluteOffset { x: 0.0, y: position },
                                    );
                                }
                            }
                            ViewMode::TvShows => {
                                if let Some(position) = state.tv_shows_scroll_position {
                                    log::debug!("Restoring TV shows scroll position after stop: {}", position);
                                    restore_task = scrollable::scroll_to(
                                        state.tv_shows_grid_state.scrollable_id.clone(),
                                        scrollable::AbsoluteOffset { x: 0.0, y: position },
                                    );
                                }
                            }
                            ViewMode::All => {
                                // In All mode, could restore both but let's just restore movies for simplicity
                                if let Some(position) = state.movies_scroll_position {
                                    restore_task = scrollable::scroll_to(
                                        state.movies_grid_state.scrollable_id.clone(),
                                        scrollable::AbsoluteOffset { x: 0.0, y: position },
                                    );
                                }
                            }
                        }
                        return restore_task;
                    }
                    PlayerMessage::Reload => {
                        return load_video(state);
                    }
                    _ => {}
                }

                // Delegate to player module
                let task = state.player.update(player_msg);

                // Update controls tracking if needed
                if state.player.controls
                    && state.player.controls_time.elapsed() > Duration::from_secs(3)
                {
                    state.player.controls = false;
                }

                task
            } else {
                Task::none()
            }
        }

        Message::Tick => {
            // Check if controls should be hidden
            if state.player.controls
                && state.player.controls_time.elapsed() > Duration::from_secs(3)
            {
                state.player.controls = false;
            }

            // Handle pending seeks (for throttled seeking)
            if state.player.dragging && state.player.pending_seek_position.is_some() {
                if let Some(pending_position) = state.player.pending_seek_position {
                    let should_seek = match state.player.last_seek_time {
                        Some(last_time) => last_time.elapsed() > Duration::from_millis(100),
                        None => true,
                    };

                    if should_seek {
                        if let Some(video) = &mut state.player.video_opt {
                            let duration =
                                Duration::try_from_secs_f64(pending_position).unwrap_or_default();
                            if let Err(e) = video.seek(duration, false) {
                                log::error!("Pending seek failed: {:?}", e);
                            } else {
                                state.player.last_seek_time = Some(Instant::now());
                                state.player.pending_seek_position = None;
                            }
                        }
                    }
                }
            }

            // Clear seeking flag if we're able to get position again
            if state.player.seeking {
                if let Some(video) = &state.player.video_opt {
                    let pos = video.position().as_secs_f64();
                    if pos > 0.0 {
                        // We got a valid position, seek is complete
                        state.player.seeking = false;
                        state.player.position = pos;
                        log::debug!("Seek completed, position now: {}", pos);
                    }
                }
            }

            // Update position and duration from video if not dragging
            if let Some(video) = &state.player.video_opt {
                // For direct streams, always get duration from video object
                // For HLS, only update if we don't have duration yet
                if !state.player.using_hls || state.player.duration <= 0.0 {
                    let new_duration = video.duration().as_secs_f64();
                    if new_duration > 0.0 && (state.player.duration <= 0.0 || !state.player.using_hls) {
                        if (new_duration - state.player.duration).abs() > 0.1 {
                            log::info!("Duration updated: {} -> {} seconds (using_hls: {})", 
                                state.player.duration, new_duration, state.player.using_hls);
                        }
                        state.player.duration = new_duration;
                        
                        // Also update source_duration if not set
                        if state.player.source_duration.is_none() {
                            state.player.source_duration = Some(new_duration);
                        }
                    }
                }

                // Always update position when not dragging/seeking
                if !state.player.dragging && !state.player.seeking {
                    let new_position = video.position().as_secs_f64();
                    
                    // Log position changes for debugging
                    if (new_position - state.player.position).abs() > 0.1 {
                        log::debug!("Position update: {:.1}s -> {:.1}s (duration: {:.1}s, using_hls: {})",
                            state.player.position, new_position, state.player.duration, state.player.using_hls);
                    }
                    
                    // Update position for smooth seek bar movement
                    state.player.position = new_position;
                }
            }

            // Update track notification timeout
            state.player.update_track_notification();

            Task::none()
        }

        Message::VideoLoaded(success) => {
            if success {
                log::info!("Video loaded successfully - using_hls: {}, duration: {}, source_duration: {:?}", 
                    state.player.using_hls, state.player.duration, state.player.source_duration);
                
                // For HLS streams, fetch the master playlist to get available quality options
                // Note: We should NOT call load_video here as it's already been loaded
                if state.player.using_hls && state.player.video_opt.is_some() {
                    // Fetch the master playlist to populate quality options
                    if let Some(ref media) = state.player.current_media {
                        if let Some(ref client) = state.player.hls_client {
                            let client = client.clone();
                            let media_id = media.id.clone();
                            
                            Task::perform(
                                async move {
                                    match client.fetch_master_playlist(&media_id).await {
                                        Ok(playlist) => {
                                            log::info!("Master playlist fetched with {} variants", playlist.variants.len());
                                            Some(playlist)
                                        }
                                        Err(e) => {
                                            log::error!("Failed to fetch master playlist: {}", e);
                                            None
                                        }
                                    }
                                },
                                |playlist| Message::MasterPlaylistLoaded(playlist)
                            )
                        } else {
                            Task::none()
                        }
                    } else {
                        Task::none()
                    }
                } else if state.player.using_hls && state.player.transcoding_job_id.is_some() {
                    // Continue checking transcoding status if still needed
                    Task::perform(
                        async {
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        },
                        |_| Message::CheckTranscodingStatus
                    )
                } else {
                    Task::none()
                }
            } else {
                log::error!("Video loading failed");
                
                let error_msg = "Failed to load video. Please check the server connection and try again.".to_string();
                state.error_message = Some(error_msg.clone());
                state.view = ViewState::VideoError { message: error_msg };
                
                Task::none()
            }
        }
        
        Message::VideoCreated(result) => {
            match result {
                Ok(()) => {
                    // Retrieve the video from global storage
                    let video_opt = crate::TEMP_VIDEO_STORAGE.lock().unwrap().take();
                    
                    if let Some(mut video) = video_opt {
                        log::info!("Video object created successfully");
                        
                        // Get duration - use transcoding duration as fallback for HLS streams
                        let video_duration = video.duration().as_secs_f64();
                        log::info!("Initial video duration from GStreamer: {} seconds", video_duration);
                        
                        // Validate and determine the best duration source
                        let duration = match (video_duration, state.player.transcoding_duration) {
                            // Video has valid duration - use it
                            (vd, _) if vd > 0.0 && vd.is_finite() => {
                                log::info!("Using video-reported duration: {} seconds", vd);
                                vd
                            }
                            // Video duration invalid but we have transcoding duration
                            (vd, Some(td)) if td > 0.0 && td.is_finite() => {
                                log::info!("Using transcoding duration: {} seconds (video reported {})", td, vd);
                                td
                            }
                            // Neither source has valid duration
                            _ => {
                                log::warn!("No valid duration available (video: {}, transcoding: {:?})", 
                                    video_duration, state.player.transcoding_duration);
                                // For HLS streams, duration might become available later
                                if state.player.using_hls {
                                    log::info!("HLS stream - duration may update during playback");
                                }
                                0.0
                            }
                        };
                        
                        state.player.duration = duration;
                        if duration > 0.0 {
                            log::info!("Playback duration set to: {} seconds ({:.1} minutes)", 
                                duration, duration / 60.0);
                            
                            // Store source duration if not already set
                            if state.player.source_duration.is_none() {
                                state.player.source_duration = Some(duration);
                                log::info!("Source duration initialized from video/transcoding metadata");
                            }
                        }
                        
                        // Reset seeking state
                        state.player.position = 0.0;
                        state.player.dragging = false;
                        
                        // Start playing immediately
                        video.set_paused(false);
                        
                        // Initialize volume and mute state
                        video.set_volume(state.player.volume);
                        video.set_muted(state.player.is_muted);
                        
                        state.player.video_opt = Some(video);
                        state.player.is_loading_video = false; // Clear loading flag
                        state.error_message = None;
                        
                        log::info!("Video loaded - duration: {}, source_duration: {:?}, using_hls: {}", 
                            state.player.duration, state.player.source_duration, state.player.using_hls);
                        
                        // Query available tracks after loading
                        state.player.update_available_tracks();
                        
                        state.player.update_controls(true);
                        
                        // Send VideoLoaded message to trigger further processing (like fetching HLS playlists)
                        Task::done(Message::VideoLoaded(true))
                    } else {
                        log::error!("Video creation succeeded but video object not found in storage");
                        state.player.is_loading_video = false;
                        state.error_message = Some("Failed to retrieve video object".to_string());
                        state.view = ViewState::VideoError { message: "Failed to retrieve video object".to_string() };
                        Task::none()
                    }
                }
                Err(e) => {
                    log::error!("=== VIDEO LOADING FAILED ===");
                    log::error!("Error: {}", e);
                    
                    // Provide more helpful error message
                    let error_msg = if e.contains("StateChange") {
                        "Failed to start video pipeline. This usually means:\n\n• The media format is not supported\n• Required GStreamer plugins are missing\n• The server is not responding correctly\n\nTry checking the server logs for more details.".to_string()
                    } else {
                        format!("Video loading error: {}", e)
                    };
                    
                    state.player.is_loading_video = false; // Clear loading flag on error
                    state.error_message = Some(error_msg.clone());
                    state.view = ViewState::VideoError { message: error_msg };
                    
                    Task::none()
                }
            }
        }
        
        Message::TranscodingStarted(result) => {
            match result {
                Ok(job_id) => {
                    log::info!("Transcoding started successfully with job ID: {}", job_id);
                    
                    // Check if this is a cached response
                    if job_id.starts_with("cached_") {
                        log::info!("Media is already cached, marking as ready immediately");
                        state.player.transcoding_job_id = None; // No job to track
                        state.player.transcoding_status = Some(crate::player::state::TranscodingStatus::Completed);
                        
                        // Load video immediately
                        if state.player.video_opt.is_none() && state.player.using_hls {
                            return load_video(state);
                        } else {
                            return Task::none();
                        }
                    }
                    
                    // Normal transcoding job
                    state.player.transcoding_job_id = Some(job_id);
                    state.player.transcoding_status = Some(crate::player::state::TranscodingStatus::Processing { progress: 0.0 });
                    state.player.transcoding_check_count = 0; // Reset check count
                    
                    // Start checking status immediately
                    Task::perform(
                        async {},
                        |_| Message::CheckTranscodingStatus
                    )
                }
                Err(e) => {
                    log::error!("Failed to start transcoding: {}", e);
                    state.player.transcoding_status = Some(crate::player::state::TranscodingStatus::Failed { error: e.clone() });
                    
                    // Show error to user
                    state.error_message = Some(format!("Transcoding failed: {}", e));
                    
                    Task::none()
                }
            }
        }
        
        Message::CheckTranscodingStatus => {
            if let Some(ref job_id) = state.player.transcoding_job_id {
                if let Some(ref client) = state.player.hls_client {
                    // Increment check count
                    state.player.transcoding_check_count += 1;
                    
                    // If we've checked too many times (30 checks = ~1 minute), give up and load video
                    if state.player.transcoding_check_count > 30 {
                        log::warn!("Transcoding status checks exceeded limit - loading video anyway");
                        state.player.transcoding_status = Some(crate::player::state::TranscodingStatus::Completed);
                        state.player.transcoding_job_id = None;
                        
                        if state.player.video_opt.is_none() && state.player.using_hls {
                            return load_video(state);
                        } else {
                            return Task::none();
                        }
                    }
                    
                    let client = client.clone();
                    let job_id = job_id.clone();
                    
                    Task::perform(
                        async move {
                            match client.check_transcoding_status(&job_id).await {
                                Ok(job) => {
                                    // Status is already deserialized from the shared enum
                                    let status = job.status.clone();
                                    
                                    // Use duration from job if available
                                    let duration = job.duration;
                                    
                                    // Log job details for debugging
                                    log::info!("Transcoding job details: id={}, media_id={}, playlist_path={:?}", 
                                        job.id, job.media_id, job.playlist_path);
                                    
                                    // Log progress details if processing
                                    let playlist_path = match &status {
                                        ferrex_core::TranscodingStatus::Processing { progress } => {
                                            if let Some(details) = &job.progress_details {
                                                log::info!("Transcoding progress: {:.1}%, FPS: {:.0}, ETA: {:.0}s",
                                                    details.percentage,
                                                    details.current_fps.unwrap_or(0.0),
                                                    details.estimated_time_remaining.unwrap_or(0.0)
                                                );
                                            } else {
                                                log::info!("Transcoding progress: {:.5}%", progress * 100.0);
                                                log::info!("Raw transcoding progress: {}%", progress);
                                            }
                                            None
                                        }
                                        ferrex_core::TranscodingStatus::Failed { error } => {
                                            log::error!("Transcoding failed: {}", error);
                                            None
                                        }
                                        ferrex_core::TranscodingStatus::Pending => {
                                            log::info!("Transcoding is pending");
                                            None
                                        }
                                        ferrex_core::TranscodingStatus::Queued => {
                                            log::info!("Transcoding is queued");
                                            None
                                        }
                                        ferrex_core::TranscodingStatus::Cancelled => {
                                            log::warn!("Transcoding was cancelled");
                                            None
                                        }
                                        ferrex_core::TranscodingStatus::Completed => {
                                            if let Some(ref path) = job.playlist_path {
                                                Some(path.clone())
                                            } else {
                                                None
                                            }
                                        }
                                    };
                                    
                                    Ok((status, duration, playlist_path))
                                }
                                Err(e) => Err(e),
                            }
                        },
                        Message::TranscodingStatusUpdate
                    )
                } else {
                    Task::none()
                }
            } else {
                Task::none()
            }
        }
        
        Message::TranscodingStatusUpdate(result) => {
            match result {
                Ok((status, duration, playlist_path)) => {
                    let should_continue_checking = match &status {
                        crate::player::state::TranscodingStatus::Pending |
                        crate::player::state::TranscodingStatus::Queued => true,
                        crate::player::state::TranscodingStatus::Processing { progress } => {
                            // For HLS, we can start playback once we have enough segments
                            // Continue checking if video not loaded yet or progress < 100%
                            state.player.video_opt.is_none() || *progress < 1.0
                        },
                        _ => false,
                    };
                    
                    state.player.transcoding_status = Some(status.clone());
                    
                    // Store duration from transcoding job if available and valid
                    if let Some(dur) = duration {
                        if dur > 0.0 && dur.is_finite() {
                            state.player.transcoding_duration = Some(dur);
                            
                            // Store source duration separately - this is the full media duration
                            if state.player.source_duration.is_none() {
                                state.player.source_duration = Some(dur);
                                log::info!("Stored source duration: {} seconds ({:.1} minutes)", 
                                    dur, dur / 60.0);
                            }
                            
                            // Update player duration if video is already loaded but had no duration
                            if state.player.duration <= 0.0 && state.player.video_opt.is_some() {
                                state.player.duration = dur;
                                log::info!("Updated player duration from transcoding job");
                            }
                        } else {
                            log::warn!("Invalid duration from transcoding job: {}", dur);
                        }
                    }
                    
                    // Update playlist URL if provided (when transcoding is ready)
                    if let Some(playlist_path) = playlist_path {
                        let playlist_url = if playlist_path.starts_with("http") {
                            playlist_path
                        } else {
                            format!("{}{}", state.server_url, playlist_path)
                        };
                        log::info!("Updating playlist URL from job: {}", playlist_url);
                        
                        // Update the URL to the actual playlist path
                        if let Ok(url) = url::Url::parse(&playlist_url) {
                            state.player.current_url = Some(url);
                        }
                    }
                    
                    // For HLS streaming, try to start playback during processing if we have segments
                    let should_try_playback = match &status {
                        crate::player::state::TranscodingStatus::Processing { progress } => {
                            // Start playback when we have at least 1% transcoded (ensures initial segments exist)
                            // With 4-second segments, 2 segments = 8 seconds, which is <1% of most videos
                            *progress >= 0.01 && state.player.video_opt.is_none() && state.player.using_hls
                        },
                        crate::player::state::TranscodingStatus::Completed => {
                            // Also try when completed if not already playing
                            state.player.video_opt.is_none() && state.player.using_hls
                        },
                        _ => false,
                    };
                    
                    let mut tasks = Vec::new();
                    
                    if should_continue_checking {
                        // Continue checking every 2 seconds
                        tasks.push(Task::perform(
                            async {
                                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                            },
                            |_| Message::CheckTranscodingStatus
                        ));
                    }
                    
                    if should_try_playback {
                        log::info!("Attempting to start HLS playback (status: {:?})...", status);
                        // Load video now that we have segments ready
                        if state.player.video_opt.is_none() && state.player.using_hls {
                            // First check if master playlist exists before trying to load
                            let check_playlist_task = if let Some(ref media) = state.player.current_media {
                            if let Some(ref client) = state.player.hls_client {
                                let client = client.clone();
                                let media_id = media.id.clone();
                                            
                                            Some(Task::perform(
                                                async move {
                                                    // Small delay to ensure playlist files are written
                                                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                                    
                                                    match client.fetch_master_playlist(&media_id).await {
                                                        Ok(playlist) => {
                                                            log::info!("Master playlist fetched with {} variants", playlist.variants.len());
                                                            Some(playlist)
                                                        }
                                                        Err(e) => {
                                                            log::error!("Failed to fetch master playlist: {}", e);
                                                            None
                                                        }
                                                    }
                                                },
                                                |playlist| Message::MasterPlaylistReady(playlist)
                                            ))
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    };
                                    
                                    // Only check for playlist first, don't load video yet
                                    if let Some(playlist_task) = check_playlist_task {
                                        tasks.push(playlist_task);
                                    } else {
                                        // Fallback to direct loading if no HLS client
                                        tasks.push(load_video(state));
                                    }
                        }
                    }
                    
                    // Handle transcoding failures
                    match &status {
                        crate::player::state::TranscodingStatus::Failed { error } => {
                            log::error!("Transcoding failed: {}", error);
                            state.error_message = Some(format!("Transcoding failed: {}", error));
                            state.view = ViewState::VideoError {
                                message: format!("Transcoding failed: {}", error),
                            };
                        }
                        crate::player::state::TranscodingStatus::Cancelled => {
                            log::warn!("Transcoding was cancelled");
                            state.error_message = Some("Transcoding was cancelled".to_string());
                        }
                        _ => {}
                    }
                    
                    // Return appropriate task
                    if tasks.is_empty() {
                        Task::none()
                    } else if tasks.len() == 1 {
                        tasks.into_iter().next().unwrap()
                    } else {
                        Task::batch(tasks)
                    }
                }
                Err(e) => {
                    log::warn!("Failed to check transcoding status: {}", e);
                    
                    // Special handling for "Job not found" - the job might have completed or expired
                    if e.contains("Job not found") || e.contains("not found") {
                        log::info!("Transcoding job not found - this could mean the job completed or the master playlist is ready");
                        
                        // For adaptive streaming, check if the master playlist exists
                        if state.player.using_hls && state.player.video_opt.is_none() {
                            if let Some(ref media) = state.player.current_media {
                                log::info!("Checking if master playlist is available for media {}", media.id);
                                
                                // Try to load the video directly - if the playlist exists, it will work
                                state.player.transcoding_status = Some(crate::player::state::TranscodingStatus::Completed);
                                state.player.transcoding_job_id = None;
                                
                                // Load video and fetch master playlist
                                let fetch_playlist_task = if let Some(ref client) = state.player.hls_client {
                                    let client = client.clone();
                                    let media_id = media.id.clone();
                                    
                                    Some(Task::perform(
                                        async move {
                                            match client.fetch_master_playlist(&media_id).await {
                                                Ok(playlist) => {
                                                    log::info!("Master playlist fetched with {} variants", playlist.variants.len());
                                                    Some(playlist)
                                                }
                                                Err(e) => {
                                                    log::error!("Failed to fetch master playlist: {}", e);
                                                    None
                                                }
                                            }
                                        },
                                        |playlist| Message::MasterPlaylistLoaded(playlist)
                                    ))
                                } else {
                                    None
                                };
                                
                                if let Some(playlist_task) = fetch_playlist_task {
                                    Task::batch([load_video(state), playlist_task])
                                } else {
                                    load_video(state)
                                }
                            } else {
                                Task::none()
                            }
                        } else {
                            Task::none()
                        }
                    } else {
                        // Other errors - retry after 5 seconds
                        Task::perform(
                            async {
                                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            },
                            |_| Message::CheckTranscodingStatus
                        )
                    }
                }
            }
        }

        Message::StartSegmentPrefetch(_segment_index) => {
            // TODO: Implement segment prefetching when needed
            // For now, GStreamer handles buffering internally
            Task::none()
        }
        
        Message::SegmentPrefetched(_index, _result) => {
            // TODO: Handle prefetched segment data
            Task::none()
        }
        
        Message::QualityVariantSelected(profile) => {
            // Close the quality menu
            state.player.show_quality_menu = false;
            
            if let Some(ref _hls_client) = state.player.hls_client {
                if let Some(ref master_playlist) = state.player.master_playlist {
                    // Empty string means "Auto" mode
                    if profile.is_empty() {
                        state.player.current_quality_profile = None;
                        log::info!("Switched to automatic quality selection");
                    } else {
                        // Find the selected variant
                        if let Some(variant) = master_playlist.variants.iter().find(|v| v.profile == profile) {
                            state.player.current_quality_profile = Some(profile.clone());
                            log::info!("Selected quality profile: {} ({}p, {:.1} Mbps)", 
                                profile, 
                                variant.resolution.map(|(_, h)| h).unwrap_or(0),
                                variant.bandwidth as f64 / 1_000_000.0
                            );
                            
                            // TODO: Implement actual variant switching in the HLS client
                            // For now, we just update the UI state
                            // In a full implementation, this would:
                            // 1. Stop fetching current variant segments
                            // 2. Switch to the new variant playlist
                            // 3. Calculate the appropriate segment to continue from
                            // 4. Start fetching from the new variant
                        }
                    }
                    
                    // Update quality switch count for metrics
                    state.player.quality_switch_count += 1;
                }
            }
            
            Task::none()
        }
        
        Message::BandwidthMeasured(bandwidth) => {
            log::debug!("Bandwidth measured: {} bps", bandwidth);
            state.player.last_bandwidth_measurement = Some(bandwidth);
            
            // Check if we should switch quality based on bandwidth
            if let Some(ref mut hls_client) = state.player.hls_client {
                if let Some(ref master_playlist) = state.player.master_playlist {
                    if let Some(new_variant) = hls_client.should_switch_variant(master_playlist) {
                        log::info!("Switching to quality variant: {}", new_variant.profile);
                        // TODO: Implement automatic quality switching
                    }
                }
            }
            
            Task::none()
        }

        Message::ImageLoaded(cache_key, result) => {
            match result {
                Ok(bytes) => {
                    log::info!("Image loaded for key {}", cache_key);
                    let handle = iced::widget::image::Handle::from_bytes(bytes);
                    state.image_cache.set_loaded(cache_key, handle);
                }
                Err(e) => {
                    log::warn!("Failed to load image for {}: {}", cache_key, e);
                    state.image_cache.set_failed(cache_key);
                }
            }
            Task::none()
        }

        Message::FetchMetadata(media_id) => {
            let server_url = state.server_url.clone();
            Task::perform(
                async move {
                    let id = media_id.clone();
                    match fetch_metadata_for_media(server_url, media_id).await {
                        Ok(_) => (id, Ok(())),
                        Err(e) => (id, Err(e.to_string())),
                    }
                },
                |(id, result)| Message::MetadataFetched(id, result),
            )
        }

        Message::MetadataFetched(media_id, result) => {
            match result {
                Ok(_) => {
                    log::info!("Metadata fetched successfully for {}", media_id);
                    // Try to load the poster now that metadata is available
                    state.poster_cache.set_loading(media_id.clone());
                    let server_url = state.server_url.clone();
                    Task::perform(
                        poster_cache::fetch_poster_with_id(server_url, media_id),
                        |(id, result)| Message::PosterLoaded(id, result),
                    )
                }
                Err(e) => {
                    log::warn!("Failed to fetch metadata for {}: {}", media_id, e);
                    Task::none()
                }
            }
        }

        Message::RefreshShowMetadata(show_name) => {
            log::info!("Refreshing metadata for show: {}", show_name);

            // Find all episodes for this show
            let episode_ids: Vec<String> = state
                .library
                .files
                .iter()
                .filter(|media| {
                    media
                        .metadata
                        .as_ref()
                        .and_then(|m| m.parsed_info.as_ref())
                        .and_then(|p| p.show_name.as_ref())
                        .map(|name| name == &show_name)
                        .unwrap_or(false)
                })
                .map(|media| media.id.clone())
                .collect();

            log::info!(
                "Found {} episodes to refresh for show {}",
                episode_ids.len(),
                show_name
            );

            // Batch refresh all episodes
            if !episode_ids.is_empty() {
                let server_url = state.server_url.clone();
                Task::perform(trigger_metadata_fetch(server_url, episode_ids), |_| {
                    Message::NoOp
                })
            } else {
                Task::none()
            }
        }

        Message::RefreshSeasonMetadata(show_name, season) => {
            log::info!(
                "Refreshing metadata for show {} season {}",
                show_name,
                season
            );

            // Find all episodes for this show and season
            let episode_ids: Vec<String> = state
                .library
                .files
                .iter()
                .filter(|media| {
                    media
                        .metadata
                        .as_ref()
                        .and_then(|m| m.parsed_info.as_ref())
                        .and_then(|p| {
                            if p.show_name.as_ref() == Some(&show_name) {
                                p.season.map(|s| s == season)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(false)
                })
                .map(|media| media.id.clone())
                .collect();

            log::info!(
                "Found {} episodes to refresh for season {}",
                episode_ids.len(),
                season
            );

            // Batch refresh season episodes
            if !episode_ids.is_empty() {
                let server_url = state.server_url.clone();
                Task::perform(trigger_metadata_fetch(server_url, episode_ids), |_| {
                    Message::NoOp
                })
            } else {
                Task::none()
            }
        }

        Message::RefreshEpisodeMetadata(media_id) => {
            // Just reuse the existing FetchMetadata logic
            update(state, Message::FetchMetadata(media_id))
        }

        Message::CarouselNavigation(carousel_msg) => {
            match carousel_msg {
                CarouselMessage::Previous(section_id) => {
                    if section_id == "movies" {
                        state.movies_carousel.go_left();
                        // Scroll programmatically
                        return scrollable::scroll_to(
                            state.movies_carousel.scrollable_id.clone(),
                            state.movies_carousel.get_scroll_offset(),
                        );
                    } else if section_id == "tv_shows" {
                        state.tv_shows_carousel.go_left();
                        return scrollable::scroll_to(
                            state.tv_shows_carousel.scrollable_id.clone(),
                            state.tv_shows_carousel.get_scroll_offset(),
                        );
                    } else if section_id == "show_seasons" {
                        if let Some(carousel) = &mut state.show_seasons_carousel {
                            carousel.go_left();
                            return scrollable::scroll_to(
                                carousel.scrollable_id.clone(),
                                carousel.get_scroll_offset(),
                            );
                        }
                    } else if section_id == "season_episodes" {
                        if let Some(carousel) = &mut state.season_episodes_carousel {
                            carousel.go_left();
                            return scrollable::scroll_to(
                                carousel.scrollable_id.clone(),
                                carousel.get_scroll_offset(),
                            );
                        }
                    }
                }
                CarouselMessage::Next(section_id) => {
                    if section_id == "movies" {
                        state.movies_carousel.go_right();
                        return scrollable::scroll_to(
                            state.movies_carousel.scrollable_id.clone(),
                            state.movies_carousel.get_scroll_offset(),
                        );
                    } else if section_id == "tv_shows" {
                        state.tv_shows_carousel.go_right();
                        return scrollable::scroll_to(
                            state.tv_shows_carousel.scrollable_id.clone(),
                            state.tv_shows_carousel.get_scroll_offset(),
                        );
                    } else if section_id == "show_seasons" {
                        if let Some(carousel) = &mut state.show_seasons_carousel {
                            carousel.go_right();
                            return scrollable::scroll_to(
                                carousel.scrollable_id.clone(),
                                carousel.get_scroll_offset(),
                            );
                        }
                    } else if section_id == "season_episodes" {
                        if let Some(carousel) = &mut state.season_episodes_carousel {
                            carousel.go_right();
                            return scrollable::scroll_to(
                                carousel.scrollable_id.clone(),
                                carousel.get_scroll_offset(),
                            );
                        }
                    }
                }
                CarouselMessage::Scrolled(section_id, viewport) => {
                    // Update scroll position and max scroll based on viewport
                    if section_id == "movies" {
                        state.movies_carousel.scroll_position = viewport.absolute_offset().x;
                        let content_width = viewport.content_bounds().width;
                        let viewport_width = viewport.bounds().width;
                        state.movies_carousel.max_scroll =
                            (content_width - viewport_width).max(0.0);
                    } else if section_id == "tv_shows" {
                        state.tv_shows_carousel.scroll_position = viewport.absolute_offset().x;
                        let content_width = viewport.content_bounds().width;
                        let viewport_width = viewport.bounds().width;
                        state.tv_shows_carousel.max_scroll =
                            (content_width - viewport_width).max(0.0);
                    } else if section_id == "show_seasons" {
                        if let Some(carousel) = &mut state.show_seasons_carousel {
                            carousel.scroll_position = viewport.absolute_offset().x;
                            let content_width = viewport.content_bounds().width;
                            let viewport_width = viewport.bounds().width;
                            carousel.max_scroll = (content_width - viewport_width).max(0.0);
                        }
                    } else if section_id == "season_episodes" {
                        if let Some(carousel) = &mut state.season_episodes_carousel {
                            carousel.scroll_position = viewport.absolute_offset().x;
                            let content_width = viewport.content_bounds().width;
                            let viewport_width = viewport.bounds().width;
                            carousel.max_scroll = (content_width - viewport_width).max(0.0);
                        }
                    }
                }
            }
            Task::none()
        }
        Message::WindowResized(size) => {
            state.window_size = size;

            // Update carousel items per page based on new window width
            // Account for padding and scrollbar space
            let available_width = size.width - 80.0; // 40px padding on each side
            state.movies_carousel.update_items_per_page(available_width);
            state
                .tv_shows_carousel
                .update_items_per_page(available_width);

            // Update virtual grid columns
            state.movies_grid_state.update_columns(size.width);
            state.tv_shows_grid_state.update_columns(size.width);

            // Update grid states with new item counts
            state.movies_grid_state.total_items = state.movies.len();
            state.tv_shows_grid_state.total_items = state.tv_shows_sorted.len();

            Task::none()
        }
        Message::SeekBarMoved(point) => {
            // Calculate seek position based on window width
            // Assume seek bar spans full window width
            let percentage = (point.x / state.window_size.width).clamp(0.0, 1.0) as f64;
            
            // Use source duration if available (for HLS this is the full media duration)
            let duration = state.player.source_duration.unwrap_or(state.player.duration);
            let seek_position = percentage * duration;

            // Always store the position for potential clicks
            state.player.last_seek_position = Some(seek_position);

            // If dragging, throttle seeks to prevent overwhelming the network
            if state.player.dragging {
                // Update visual position immediately for responsive UI
                state.player.position = seek_position;
                state.player.update_controls(true);

                // Check if we should perform actual seek (throttle to ~100ms intervals)
                let should_seek = match state.player.last_seek_time {
                    Some(last_time) => last_time.elapsed() > Duration::from_millis(100),
                    None => true,
                };

                if should_seek {
                    // Perform the actual seek
                    if let Some(video) = state.player.video_opt.as_mut() {
                        let duration =
                            Duration::try_from_secs_f64(seek_position).unwrap_or_default();
                        if let Err(e) = video.seek(duration, false) {
                            log::error!("Seek failed: {:?}", e);
                        } else {
                            state.player.last_seek_time = Some(Instant::now());
                            // Clear pending seek since we just performed it
                            state.player.pending_seek_position = None;
                        }
                    }
                } else {
                    // Store pending seek position to be executed later
                    state.player.pending_seek_position = Some(seek_position);
                }
            }

            Task::none()
        }

        // Handle all player messages
        msg if PlayerMessage::is_player_message(&msg) => {
            // Special handling for SeekBarMoved - calculate position using window width
            if let Some(player_msg) = PlayerMessage::from_main_message(msg) {
                state.player.update(player_msg)
            } else {
                Task::none()
            }
        }

        Message::CheckScrollStopped => {
            // Check if scrolling has actually stopped
            if let Some(last_time) = state.last_scroll_time {
                let elapsed = Instant::now().duration_since(last_time);
                if elapsed >= Duration::from_millis(SCROLL_STOP_DEBOUNCE_MS) {
                    // Scrolling has stopped
                    if state.scroll_velocity > 0.0 {
                        log::info!("Scrolling stopped (was at {:.0} px/s)", state.scroll_velocity);
                        state.scroll_velocity = 0.0;
                        state.fast_scrolling = false;
                        state.scroll_stopped_time = Some(Instant::now());
                        
                        // Clear scroll samples
                        state.scroll_samples.clear();
                        
                        // Trigger poster loading for visible items
                        state.mark_visible_posters_for_loading();
                        
                        // Start loading posters
                        return Task::perform(async {}, |_| Message::PosterMonitorTick);
                    }
                }
            }
            Task::none()
        }

        Message::BackToLibrary => {
            // Return to library from detail views
            state.view = ViewState::Library;
            
            // Restore scroll position when returning to library
            match state.view_mode {
                ViewMode::Movies => {
                    if let Some(position) = state.movies_scroll_position {
                        log::debug!("Restoring movies scroll position: {}", position);
                        scrollable::scroll_to(
                            state.movies_grid_state.scrollable_id.clone(),
                            scrollable::AbsoluteOffset { x: 0.0, y: position },
                        )
                    } else {
                        Task::none()
                    }
                }
                ViewMode::TvShows => {
                    if let Some(position) = state.tv_shows_scroll_position {
                        log::debug!("Restoring TV shows scroll position: {}", position);
                        scrollable::scroll_to(
                            state.tv_shows_grid_state.scrollable_id.clone(),
                            scrollable::AbsoluteOffset { x: 0.0, y: position },
                        )
                    } else {
                        Task::none()
                    }
                }
                ViewMode::All => {
                    // In All mode, restore movies scroll position
                    if let Some(position) = state.movies_scroll_position {
                        scrollable::scroll_to(
                            state.movies_grid_state.scrollable_id.clone(),
                            scrollable::AbsoluteOffset { x: 0.0, y: position },
                        )
                    } else {
                        Task::none()
                    }
                }
            }
        }
        
        Message::NoOp => {
            // No operation needed
            Task::none()
        }

        Message::ClearError => {
            state.error_message = None;
            Task::none()
        }
        
        Message::MediaHovered(media_id) => {
            state.hovered_media_id = Some(media_id);
            Task::none()
        }
        
        Message::MediaUnhovered => {
            state.hovered_media_id = None;
            Task::none()
        }
        
        Message::ToggleQualityMenu => {
            state.player.show_quality_menu = !state.player.show_quality_menu;
            // Close other menus if opening quality menu
            if state.player.show_quality_menu {
                state.player.show_settings = false;
                state.player.show_subtitle_menu = false;
            }
            Task::none()
        }
        
        Message::MasterPlaylistLoaded(playlist_opt) => {
            if let Some(playlist) = playlist_opt {
                log::info!("Master playlist loaded with {} quality variants", playlist.variants.len());
                for variant in &playlist.variants {
                    log::info!("  - {} ({}p, {:.1} Mbps)", 
                        variant.profile,
                        variant.resolution.map(|(_, h)| h).unwrap_or(0),
                        variant.bandwidth as f64 / 1_000_000.0
                    );
                }
                state.player.master_playlist = Some(playlist);
            }
            Task::none()
        }
        
        Message::MasterPlaylistReady(playlist_opt) => {
            if let Some(playlist) = playlist_opt {
                log::info!("Master playlist is ready - loading video with {} quality variants", playlist.variants.len());
                state.player.master_playlist = Some(playlist);
                
                // Now that we confirmed the playlist exists, load the video
                load_video(state)
            } else {
                log::error!("Master playlist check failed - retrying in 2 seconds");
                // Retry checking after a delay
                Task::perform(
                    async {
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    },
                    |_| Message::CheckTranscodingStatus
                )
            }
        }

        Message::ExitFullscreen => {
            // Only exit fullscreen if we're actually in fullscreen
            if state.player.is_fullscreen {
                state.player.is_fullscreen = false;
                let mode = iced::window::Mode::Windowed;
                iced::window::get_latest().and_then(move |id| iced::window::set_mode(id, mode))
            } else {
                Task::none()
            }
        }

        _ => {
            // This should not happen as all messages should be handled above
            log::warn!("Unhandled message: {:?}", message);
            Task::none()
        }
    };

    PROFILER.end(&format!("update::{}", message_name));
    result
}
