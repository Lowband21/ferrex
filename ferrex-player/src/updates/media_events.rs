use crate::view_models::ViewModel;
use crate::{api_types::MediaId, messages::media::Message, state::State, MediaEvent};
use iced::Task;

/// Handle incoming media events from the server via SSE
pub fn handle_media_event_received(state: &mut State, event: MediaEvent) -> Task<Message> {
    match event {
        MediaEvent::MovieAdded { movie } => {
            log::info!("Movie added: {}", movie.title.as_str());

            // NEW ARCHITECTURE: Add to MediaStore
            if let Ok(mut store) = state.media_store.write() {
                store.upsert(crate::api_types::MediaReference::Movie(movie.clone()));
            }

            // Add to current library's references if it matches
            if let Some(library_id) = &state.current_library_id {
                if let Some(library) = state.libraries.iter_mut().find(|l| &l.id == library_id) {
                    library
                        .media
                        .push(crate::api_types::MediaReference::Movie(movie.clone()));
                }
            }

            // NEW ARCHITECTURE: Refresh ViewModels
            state.all_view_model.refresh_from_store();
            state.movies_view_model.refresh_from_store();

            Task::none()
        }
        MediaEvent::SeriesAdded { series } => {
            log::info!("Series added: {}", series.title.as_str());

            // NEW ARCHITECTURE: Add to MediaStore
            if let Ok(mut store) = state.media_store.write() {
                store.upsert(crate::api_types::MediaReference::Series(series.clone()));
            }

            // Add to current library's references if it matches
            if let Some(library_id) = &state.current_library_id {
                if let Some(library) = state.libraries.iter_mut().find(|l| &l.id == library_id) {
                    library
                        .media
                        .push(crate::api_types::MediaReference::Series(series.clone()));
                }
            }

            // NEW ARCHITECTURE: Refresh ViewModels
            state.all_view_model.refresh_from_store();
            state.tv_view_model.refresh_from_store();

            Task::none()
        }
        MediaEvent::SeasonAdded { season } => {
            log::info!(
                "Season added: {:?} S{}",
                season.series_id,
                season.season_number.value()
            );

            // NEW ARCHITECTURE: Add to MediaStore
            if let Ok(mut store) = state.media_store.write() {
                store.upsert(crate::api_types::MediaReference::Season(season));
            }

            // NEW ARCHITECTURE: Refresh TV ViewModel
            state.tv_view_model.refresh_from_store();

            Task::none()
        }
        MediaEvent::EpisodeAdded { episode } => {
            log::info!(
                "Episode added: {:?} S{}E{}",
                episode.series_id,
                episode.season_number.value(),
                episode.episode_number.value()
            );

            // NEW ARCHITECTURE: Add to MediaStore
            if let Ok(mut store) = state.media_store.write() {
                store.upsert(crate::api_types::MediaReference::Episode(episode));
            }

            // NEW ARCHITECTURE: Refresh TV ViewModel
            state.tv_view_model.refresh_from_store();

            Task::none()
        }
        MediaEvent::MovieUpdated { movie } => {
            log::info!("Movie updated: {}", movie.title.as_str());

            // NEW ARCHITECTURE: Update MediaStore
            if let Ok(mut store) = state.media_store.write() {
                store.upsert(crate::api_types::MediaReference::Movie(movie.clone()));
            }

            // Update library references
            for library in state.libraries.iter_mut() {
                for media_ref in library.media.iter_mut() {
                    if let crate::api_types::MediaReference::Movie(m) = media_ref {
                        if m.id == movie.id {
                            media_ref.clone_from(&crate::api_types::MediaReference::Movie(
                                movie.clone(),
                            ));
                            break;
                        }
                    }
                }
            }

            // NEW ARCHITECTURE: Refresh ViewModels
            state.all_view_model.refresh_from_store();
            state.movies_view_model.refresh_from_store();

            Task::none()
        }
        MediaEvent::SeriesUpdated { series } => {
            log::info!("Series updated: {}", series.title.as_str());

            // NEW ARCHITECTURE: Update MediaStore
            if let Ok(mut store) = state.media_store.write() {
                store.upsert(crate::api_types::MediaReference::Series(series.clone()));
            }

            // Update library references
            for library in state.libraries.iter_mut() {
                for media_ref in library.media.iter_mut() {
                    if let crate::api_types::MediaReference::Series(s) = media_ref {
                        if s.id == series.id {
                            media_ref.clone_from(&crate::api_types::MediaReference::Series(
                                series.clone(),
                            ));
                            break;
                        }
                    }
                }
            }

            // NEW ARCHITECTURE: Refresh ViewModels
            state.all_view_model.refresh_from_store();
            state.tv_view_model.refresh_from_store();

            Task::none()
        }
        MediaEvent::SeasonUpdated { season } => {
            log::info!(
                "Season updated: {:?} S{}",
                season.series_id,
                season.season_number.value()
            );

            // NEW ARCHITECTURE: Update MediaStore
            if let Ok(mut store) = state.media_store.write() {
                store.upsert(crate::api_types::MediaReference::Season(season));
            }

            // NEW ARCHITECTURE: Refresh TV ViewModel
            state.tv_view_model.refresh_from_store();

            Task::none()
        }
        MediaEvent::EpisodeUpdated { episode } => {
            log::info!(
                "Episode updated: {:?} S{}E{}",
                episode.series_id,
                episode.season_number.value(),
                episode.episode_number.value()
            );

            // NEW ARCHITECTURE: Update MediaStore
            if let Ok(mut store) = state.media_store.write() {
                store.upsert(crate::api_types::MediaReference::Episode(episode));
            }

            // NEW ARCHITECTURE: Refresh TV ViewModel
            state.tv_view_model.refresh_from_store();

            Task::none()
        }
        MediaEvent::MediaDeleted { id } => {
            log::info!("Media deleted: {}", id);

            // NEW ARCHITECTURE: Parse the id string into a MediaId
            // The id could be in format "movie:XXX", "series:XXX", "season:XXX", or "episode:XXX"
            let media_id = if id.starts_with("movie:") {
                MediaId::Movie(
                    ferrex_core::MovieID::new(id.strip_prefix("movie:").unwrap().to_string())
                        .unwrap(),
                )
            } else if id.starts_with("series:") {
                MediaId::Series(
                    ferrex_core::SeriesID::new(id.strip_prefix("series:").unwrap().to_string())
                        .unwrap(),
                )
            } else if id.starts_with("season:") {
                MediaId::Season(
                    ferrex_core::SeasonID::new(id.strip_prefix("season:").unwrap().to_string())
                        .unwrap(),
                )
            } else if id.starts_with("episode:") {
                MediaId::Episode(
                    ferrex_core::EpisodeID::new(id.strip_prefix("episode:").unwrap().to_string())
                        .unwrap(),
                )
            } else {
                // Fallback: try to determine type by querying MediaStore
                log::warn!("MediaDeleted event with unprefixed id: {}, skipping", id);
                return Task::none();
            };

            // NEW ARCHITECTURE: Remove from MediaStore
            if let Ok(mut store) = state.media_store.write() {
                store.remove(&media_id);
            }

            // Remove from library references
            for library in state.libraries.iter_mut() {
                library.media.retain(|media_ref| match media_ref {
                    crate::api_types::MediaReference::Movie(m) => {
                        MediaId::Movie(m.id.clone()) != media_id
                    }
                    crate::api_types::MediaReference::Series(s) => {
                        MediaId::Series(s.id.clone()) != media_id
                    }
                    crate::api_types::MediaReference::Season(s) => {
                        MediaId::Season(s.id.clone()) != media_id
                    }
                    crate::api_types::MediaReference::Episode(e) => {
                        MediaId::Episode(e.id.clone()) != media_id
                    }
                });
            }

            // NEW ARCHITECTURE: Refresh all ViewModels
            state.all_view_model.refresh_from_store();
            state.movies_view_model.refresh_from_store();
            state.tv_view_model.refresh_from_store();

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
        MediaEvent::ScanProgress { scan_id, progress } => {
            log::info!(
                "Scan progress: {} - {}%",
                scan_id,
                (progress.scanned_files as f32 / progress.total_files.max(1) as f32 * 100.0) as u32
            );

            // Update scan progress if this is our active scan
            if state.active_scan_id.as_ref() == Some(&scan_id) {
                // Convert ferrex_core::ScanProgress to state::ScanProgress
                state.scan_progress = Some(progress);
            }
            Task::none()
        }
        MediaEvent::ScanFailed { scan_id, error } => {
            log::error!("Scan failed: {} - {}", scan_id, error);
            if state.active_scan_id.as_ref() == Some(&scan_id) {
                state.active_scan_id = None;
                state.scanning = false;
                state.error_message = Some(format!("Scan failed: {}", error));
            }
            Task::none()
        }
    }
}

/// Handle media events connection error
pub fn handle_media_events_error(_state: &mut State, error: String) -> Task<Message> {
    log::error!("Media events SSE error: {}", error);
    // TODO: Implement retry logic
    Task::none()
}
