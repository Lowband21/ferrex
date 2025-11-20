use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use iced::{
    widget::scrollable,
    Task,
};

use crate::{
    carousel::{CarouselMessage, CarouselState},
    check_media_availability, close_video, fetch_metadata_for_media,
    image_cache::{self, ImageSource},
    load_video, media_library,
    message::Message,
    models::{MediaOrganizer, TvShow},
    player::PlayerMessage,
    poster_cache::{self, PosterState},
    poster_monitor::PosterMonitor,
    profiling::PROFILER,
    start_media_scan,
    state::{ScanStatus, SortBy, SortOrder, State, ViewMode, ViewState},
    util::{sort_media, trigger_metadata_fetch},
    virtual_list::VirtualGridState,
    MediaEvent,
};

// Scrolling performance constants - tune these based on profiling
// Lower FAST_SCROLL_THRESHOLD for more aggressive fast mode activation
// Higher values keep normal rendering longer but may cause stuttering
const FAST_SCROLL_THRESHOLD: f32 = 10000.0; // pixels per second - when to switch to fast mode

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
                            "{} items missing posters! Triggering metadata refresh.",
                            missing_posters
                        );
                        
                        // Trigger metadata refresh for all items missing posters
                        let server_url_clone = state.server_url.clone();
                        let missing_ids_clone = missing_poster_ids.clone();
                        tasks.push(Task::perform(
                            async move {
                                log::info!("Starting metadata refresh for {} items", missing_ids_clone.len());
                                if let Err(e) = trigger_metadata_fetch(server_url_clone, missing_ids_clone).await {
                                    log::error!("Failed to trigger metadata refresh: {}", e);
                                } else {
                                    log::info!("Successfully triggered metadata refresh for missing posters");
                                }
                            },
                            |_| Message::NoOp,
                        ));
                    }

                    // Load only a few posters initially to keep UI responsive
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

        Message::ScanLibrary => {
            state.scanning = true;
            state.error_message = None;
            state.scan_progress = None;
            let server_url = state.server_url.clone();

            Task::perform(start_media_scan(server_url, false), |result| match result {
                Ok(scan_id) => Message::ScanStarted(Ok(scan_id)),
                Err(e) => Message::ScanStarted(Err(e.to_string())),
            })
        }

        Message::ForceRescan => {
            state.scanning = true;
            state.error_message = None;
            state.scan_progress = None;
            let server_url = state.server_url.clone();

            Task::perform(start_media_scan(server_url, true), |result| match result {
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

            // Update state with organized media (already sorted in background)
            state.movies = movies;
            state.tv_shows = tv_shows;
            
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
            let max_concurrent = 3usize; // Max concurrent poster loads
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
                                log::debug!("Poster {} is visible, starting fade-in animation", media_id);
                                // Start at 0 opacity for fade-in animation
                                state.poster_cache.set_loaded(media_id.clone(), thumbnail_handle, full_size_handle);
                                state.poster_animation_states.insert(media_id.clone(), 0.0);

                                // Start fade-in animation
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

        Message::AnimatePoster(media_id) => {
            // Animate poster fade-in only if still visible
            if !state.is_media_visible(&media_id) {
                // Item scrolled out of view, complete animation immediately
                state.poster_cache.update_opacity(&media_id, 1.0);
                state.poster_animation_states.remove(&media_id);
                return Task::none();
            }

            if let Some(opacity) = state.poster_animation_states.get_mut(&media_id) {
                if *opacity < 1.0 {
                    // Much slower fade-in for visibility
                    let increment = 0.02; // 50 frames total at 16ms = 800ms fade-in
                    *opacity = (*opacity + increment).min(1.0);

                    // Simple linear fade for now to make it more visible
                    state.poster_cache.update_opacity(&media_id, *opacity);

                    // Continue animation if not complete
                    if *opacity < 1.0 {
                        Task::perform(
                            async move {
                                tokio::time::sleep(std::time::Duration::from_millis(16)).await; // ~60fps
                                media_id
                            },
                            Message::AnimatePoster,
                        )
                    } else {
                        // Animation complete, remove from tracking
                        state.poster_animation_states.remove(&media_id);
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            } else {
                Task::none()
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
            
            // Skip poster loading entirely during fast scrolling
            if state.fast_scrolling {
                log::debug!("Skipping poster loading during fast scrolling");
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

            log::debug!(
                "PosterMonitorTick: {} posters currently loading",
                current_loading
            );

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

            // First check media availability
            let server_url = state.server_url.clone();
            let media_id = media.id.clone();
            let media_clone = media.clone();

            Task::perform(
                async move {
                    // Check if media is available
                    match check_media_availability(&server_url, &media_id).await {
                        Ok(availability) => {
                            if availability.available {
                                Ok(media_clone)
                            } else {
                                Err((availability.reason, availability.message))
                            }
                        }
                        Err(e) => {
                            // If we can't check availability, try to play anyway
                            log::warn!("Failed to check media availability: {}", e);
                            Ok(media_clone)
                        }
                    }
                },
                |result| match result {
                    Ok(media) => Message::MediaAvailabilityChecked(media),
                    Err((reason, message)) => Message::MediaUnavailable(reason, message),
                },
            )
        }

        Message::MediaAvailabilityChecked(media) => {
            // Media is available, proceed with playing
            log::info!("Media is available, proceeding to play");

            // Create video URL - always use streaming endpoint for better compatibility
            let video_url = if media.path.starts_with("http") {
                media.path.clone()
            } else {
                // Always use streaming endpoint for local files
                // This ensures proper handling of complex codecs (HDR, x265, etc)
                let stream_url = format!("{}/stream/{}", state.server_url, media.id);
                log::info!("Constructed stream URL: {}", stream_url);
                stream_url
            };

            log::info!("Final video URL: {}", video_url);

            // Parse URL
            match url::Url::parse(&video_url) {
                Ok(url) => {
                    state.player.current_url = Some(url.clone());
                    // Set loading state immediately
                    state.view = ViewState::LoadingVideo {
                        url: video_url.clone(),
                    };
                    state.error_message = None;
                    // Load the video
                    load_video(state)
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
                // Update duration if it wasn't available during load
                if state.player.duration <= 0.0 {
                    let new_duration = video.duration().as_secs_f64();
                    if new_duration > 0.0 {
                        log::info!("Duration now available in tick: {} seconds", new_duration);
                        state.player.duration = new_duration;
                    }
                }

                if !state.player.dragging && !state.player.seeking {
                    let new_position = video.position().as_secs_f64();
                    // Only update if position changed significantly
                    if (new_position - state.player.position).abs() >= 0.5 {
                        state.player.position = new_position;
                    }
                }
            }

            // Update track notification timeout
            state.player.update_track_notification();

            Task::none()
        }

        Message::VideoLoaded(_success) => {
            // This message is not used in current implementation
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
            let seek_position = percentage * state.player.duration;

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

        _ => {
            // This should not happen as all messages should be handled above
            log::warn!("Unhandled message: {:?}", message);
            Task::none()
        }
    };

    PROFILER.end(&format!("update::{}", message_name));
    result
}
