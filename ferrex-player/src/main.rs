use iced::{
    widget::{button, column, container, image, row, scrollable, text, Space, Stack},
    Element, Font, Length, Subscription, Task,
};
use lucide_icons::{lucide_font_bytes, Icon};

mod carousel;
mod components;
mod components_enhanced;
mod config;
mod grid_view;
mod image_cache;
mod media_library;
mod message;
mod metadata_cache;
mod models;
mod player;
mod poster_cache;
mod poster_monitor;
mod profiling;
mod state;
mod theme;
mod update;
mod util;
mod views;
mod virtual_list;
mod widgets;
mod performance_config;

use gstreamer as gst;
use iced_video_player::Video;
use image_cache::ImageState;
use media_library::MediaFile;
use message::Message;
use poster_cache::PosterState;
use profiling::PROFILER;
use state::State;
use std::time::Instant;
use update::update;
use views::{view_loading_video, view_video_error};

use serde::{Deserialize, Serialize};
use serde_json;

use crate::state::{ScanProgress, ScanStatus, SortBy, SortOrder, ViewMode, ViewState};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MediaEvent {
    MediaAdded { media: MediaFile },
    MediaUpdated { media: MediaFile },
    MediaDeleted { id: String },
    MetadataUpdated { id: String },
    ScanStarted { scan_id: String },
    ScanCompleted { scan_id: String },
}

/// Get icon character string
fn icon_char(icon: lucide_icons::Icon) -> String {
    icon.unicode().to_string()
}

/// Helper function to create icon text
fn icon_text(icon: lucide_icons::Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

fn main() -> iced::Result {
    env_logger::init();

    let server_url =
        std::env::var("MEDIA_SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

    let init = move || {
        let state = State {
            server_url: server_url.clone(),
            loading: true,
            ..Default::default()
        };

        // Load library on startup
        // Load libraries on startup instead of media files
        let libraries_task =
            Task::perform(media_library::fetch_libraries(server_url.clone()), |result| {
                match result {
                    Ok(libraries) => Message::LibrariesLoaded(Ok(libraries)),
                    Err(e) => Message::LibrariesLoaded(Err(e.to_string())),
                }
            });
        
        // Legacy library loading for backward compatibility
        let library_task =
            Task::perform(media_library::fetch_library(server_url.clone()), |result| {
                match result {
                    Ok(files) => Message::LibraryLoaded(Ok(files)),
                    Err(e) => Message::LibraryLoaded(Err(e.to_string())),
                }
            });

        // Check for active scans
        let scans_task =
            Task::perform(check_active_scans(server_url.clone()), Message::ActiveScansChecked);

        (state, Task::batch([libraries_task, library_task, scans_task]))
    };
    
    iced::application(init, update, view)
        .subscription(subscription)
        .font(lucide_font_bytes())
        .theme(|_| theme::MediaServerTheme::theme())
        .run()
}

async fn check_active_scans(server_url: String) -> Vec<ScanProgress> {
    match reqwest::get(format!("{}/scan/active", server_url)).await {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(json) => {
                if let Some(scans) = json.get("scans").and_then(|s| s.as_array()) {
                    scans
                        .iter()
                        .filter_map(|scan| {
                            serde_json::from_value::<ScanProgress>(scan.clone()).ok()
                        })
                        .collect()
                } else {
                    vec![]
                }
            }
            Err(e) => {
                log::error!("Failed to parse active scans response: {}", e);
                vec![]
            }
        },
        Err(e) => {
            log::error!("Failed to check active scans: {}", e);
            vec![]
        }
    }
}

// Helper functions
fn close_video(state: &mut State) {
    if let Some(mut video) = state.player.video_opt.take() {
        log::info!("Closing video");
        video.set_paused(true);
        drop(video);
    }
    state.player.position = 0.0;
    state.player.duration = 0.0;
    state.player.dragging = false;
    state.player.last_seek_position = None;
    state.player.seeking = false;
}

fn load_video(state: &mut State) -> Task<Message> {
    // Close existing video if any
    close_video(state);

    let url = match &state.player.current_url {
        Some(url) => url.clone(),
        None => {
            state.view = ViewState::VideoError {
                message: "No URL provided".to_string(),
            };
            return Task::none();
        }
    };

    log::info!("=== VIDEO LOADING DEBUG ===");
    log::info!("Loading video URL: {}", url);
    log::info!("URL scheme: {}", url.scheme());
    log::info!("URL host: {:?}", url.host());
    log::info!("URL path: {}", url.path());

    // Initialize GStreamer if needed
    if let Err(e) = gst::init() {
        log::warn!("GStreamer init returned: {:?}", e);
    } else {
        log::info!("GStreamer initialized successfully");
    }

    // Check GStreamer version
    log::info!(
        "GStreamer version: {}.{}.{}",
        gst::version().0,
        gst::version().1,
        gst::version().2
    );

    // Create video synchronously
    log::info!("Creating Video object with URL: {}", url);
    match Video::new(&url) {
        Ok(mut video) => {
            log::info!("Video object created successfully");
            // Get duration
            let duration = video.duration().as_secs_f64();

            if duration <= 0.0 {
                log::warn!("Initial video duration is 0, video might be loading...");
            }

            state.player.duration = duration;

            log::info!("Playback duration: {}", duration);

            // Reset seeking state
            state.player.position = 0.0;
            state.player.dragging = false;

            // Start playing immediately
            video.set_paused(false);

            // Initialize volume and mute state
            video.set_volume(state.player.volume);
            video.set_muted(state.player.is_muted);

            state.player.video_opt = Some(video);
            state.error_message = None;
            state.view = ViewState::Player;

            // Query available tracks after loading
            state.player.update_available_tracks();

            update_controls(state, true);
        }
        Err(e) => {
            log::error!("=== VIDEO LOADING FAILED ===");
            log::error!("Error type: {:?}", e);

            // Provide more helpful error message
            let error_detail = format!("{:?}", e);
            let error_msg = if error_detail.contains("StateChange") {
                "Failed to start video pipeline. This usually means:\n\n• The media format is not supported\n• Required GStreamer plugins are missing\n• The server is not responding correctly\n\nTry checking the server logs for more details.".to_string()
            } else {
                format!("Video loading error: {}", error_detail)
            };

            state.error_message = Some(error_msg.clone());
            state.view = ViewState::VideoError { message: error_msg };
        }
    }

    Task::none()
}

fn update_controls(state: &mut State, show: bool) {
    state.player.controls = show;
    if show {
        state.player.controls_time = Instant::now();
    }
}

fn view(state: &State) -> Element<Message> {
    PROFILER.start("view");

    let content = match &state.view {
        ViewState::Library => view_library_old(state),
        ViewState::LibraryManagement => view_library_management(state),
        ViewState::AdminDashboard => view_admin_dashboard(state),
        ViewState::Player => view_player(state),
        ViewState::LoadingVideo { url } => view_loading_video(url),
        ViewState::VideoError { message } => view_video_error(message),
        ViewState::MovieDetail { media } => view_movie_detail(state, media),
        ViewState::TvShowDetail { show_name } => view_tv_show_detail(state, show_name),
        ViewState::SeasonDetail {
            show_name,
            season_num,
        } => view_season_detail(state, show_name, *season_num),
        ViewState::EpisodeDetail { media } => view_episode_detail(state, media),
    };

    let result = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::Container::Default.style())
        .into();

    PROFILER.end("view");
    result
}

fn view_library_old(state: &State) -> Element<Message> {
    if state.loading {
        // Loading state
        container(
            column![
                text("Media Library")
                    .size(28)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                Space::with_height(Length::Fixed(100.0)),
                text("Loading library...")
                    .size(20)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .padding(20)
        .style(theme::Container::Default.style())
        .into()
    } else {
        // Create header with admin button
        let header: iced::widget::Row<Message> = row![
            text("Media Library")
                .size(28)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_width(Length::Fill),
            button(
                text(icon_char(Icon::Settings))
                    .font(lucide_font())
                    .size(20)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY)
            )
            .on_press(Message::ShowAdminDashboard)
            .style(theme::Button::Secondary.style())
            .padding(10),
        ]
        .align_y(iced::Alignment::Center)
        .padding([10, 20]);
        
        // Create tabs
        let tabs = row![
            button(text("All").size(16))
                .on_press(Message::SetViewMode(ViewMode::All))
                .style(if state.view_mode == ViewMode::All {
                    theme::Button::Primary.style()
                } else {
                    theme::Button::Secondary.style()
                })
                .padding([8, 16]),
            Space::with_width(10),
            button(text("Movies").size(16))
                .on_press(Message::SetViewMode(ViewMode::Movies))
                .style(if state.view_mode == ViewMode::Movies {
                    theme::Button::Primary.style()
                } else {
                    theme::Button::Secondary.style()
                })
                .padding([8, 16]),
            Space::with_width(10),
            button(text("TV Shows").size(16))
                .on_press(Message::SetViewMode(ViewMode::TvShows))
                .style(if state.view_mode == ViewMode::TvShows {
                    theme::Button::Primary.style()
                } else {
                    theme::Button::Secondary.style()
                })
                .padding([8, 16]),
        ]
        .align_y(iced::Alignment::Center);

        // Create header
        let header = row![
            text("Media Library")
                .size(32)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_width(30),
            tabs,
            Space::with_width(Length::Fill),
            button("Refresh")
                .on_press(Message::RefreshLibrary)
                .style(theme::Button::Secondary.style()),
            Space::with_width(10),
            button(if state.scanning {
                "Scanning..."
            } else {
                "Scan Library"
            })
            .on_press_maybe(if state.scanning {
                None
            } else {
                Some(Message::ScanLibrary)
            })
            .style(theme::Button::Primary.style()),
            Space::with_width(10),
            button("Force Rescan")
                .on_press_maybe(if state.scanning {
                    None
                } else {
                    Some(Message::ForceRescan)
                })
                .style(theme::Button::Destructive.style()),
            Space::with_width(10),
            button("Manage Libraries")
                .on_press(Message::ShowLibraryManagement)
                .style(theme::Button::Secondary.style()),
            // Show scan progress button if there's an active scan or progress
            //if state.active_scan_id.is_some() || state.scan_progress.is_some() {
            Element::from(row![
                Space::with_width(10),
                button(
                    row![
                        icon_text(Icon::Activity),
                        Space::with_width(5),
                        text("Progress")
                    ]
                    .align_y(iced::Alignment::Center)
                )
                .on_press(Message::ToggleScanProgress)
                .style(if state.show_scan_progress {
                    theme::Button::Primary.style()
                } else {
                    theme::Button::Secondary.style()
                })
            ]) //} else {
               //    Element::from(Space::with_width(0))
               //},
        ]
        .align_y(iced::Alignment::Center)
        .padding(20);
        
        // Wrap header in a container with opaque background to cover any overflow
        let header = container(header)
            .width(Length::Fill)
            .style(theme::Container::Header.style());

        // Error message if any
        let error_section: Element<Message> = if let Some(error) = &state.error_message {
            container(
                row![
                    text(error).color(theme::MediaServerTheme::ERROR),
                    Space::with_width(Length::Fill),
                    button("×")
                        .on_press(Message::ClearError)
                        .style(theme::Button::Text.style()),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(10)
            .style(theme::Container::Card.style())
            .into()
        } else {
            container(Space::with_height(0)).into()
        };

        // Scan progress section - SAVED FOR FUTURE ADMIN PAGE
        /* Inline scan progress implementation - commented out in favor of overlay
        let scan_progress_section: Element<Message> = if let Some(progress) = &state.scan_progress {
            let progress_percentage = if progress.total_files > 0 {
                (progress.scanned_files as f32 / progress.total_files as f32) * 100.0
            } else {
                0.0
            };

            let eta_text = if let Some(eta) = progress.estimated_time_remaining {
                let seconds = eta.as_secs();
                if seconds < 60 {
                    format!("ETA: {} seconds", seconds)
                } else {
                    let minutes = seconds / 60;
                    let remaining_seconds = seconds % 60;
                    format!("ETA: {}:{:02}", minutes, remaining_seconds)
                }
            } else {
                "Calculating ETA...".to_string()
            };

            let current_file_text = if let Some(file) = &progress.current_file {
                // Extract just the filename from the path for cleaner display
                let filename = std::path::Path::new(file)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(file);
                format!("Processing: {}", filename)
            } else {
                "Scanning directories...".to_string()
            };

            let status_text = match progress.status {
                ScanStatus::Pending => "Preparing scan...",
                ScanStatus::Scanning => "Scanning files...",
                ScanStatus::Processing => "Processing metadata...",
                ScanStatus::Completed => "Scan completed!",
                ScanStatus::Failed => "Scan failed",
                ScanStatus::Cancelled => "Scan cancelled",
            };

            container(
                column![
                    row![
                        text(status_text)
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        Space::with_width(Length::Fill),
                        container(
                            container(Space::with_width(Length::Fixed(
                                progress_percentage * 0.01 * 200.0
                            )))
                            .height(3)
                            .style(theme::Container::ProgressBar.style())
                        )
                        .width(200)
                        .height(3)
                        .style(theme::Container::ProgressBarBackground.style()),
                        Space::with_width(10),
                        text(format!("{:.0}%", progress_percentage))
                            .size(12)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        Space::with_width(15),
                        text(eta_text)
                            .size(12)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .align_y(iced::Alignment::Center),
                    row![
                        text(format!(
                            "{}/{} files • {} stored • {} metadata",
                            progress.scanned_files, progress.total_files,
                            progress.stored_files, progress.metadata_fetched
                        ))
                        .size(11)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        Space::with_width(Length::Fill),
                        if !progress.errors.is_empty() {
                            Element::from(
                                text(format!("{} errors", progress.errors.len()))
                                    .size(11)
                                    .color(theme::MediaServerTheme::ERROR),
                            )
                        } else {
                            Element::from(Space::with_width(0))
                        },
                    ],
                    text(current_file_text)
                        .size(10)
                        .color(theme::MediaServerTheme::TEXT_DIMMED),
                ]
                .spacing(3),
            )
            .padding(10)
            .style(theme::Container::Card.style())
            .into()
        } else {
            container(Space::with_height(0)).into()
        };
        */
        let scan_progress_section: Element<Message> = container(Space::with_height(0)).into();

        if state.movies.is_empty() && state.tv_shows.is_empty() {
            // Empty state
            container(
                column![
                    header,
                    error_section,
                    Space::with_height(Length::Fill),
                    container(
                        column![
                            text("No media files found")
                                .size(18)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            Space::with_height(20),
                            text("Click 'Scan Library' to find media files")
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        ]
                        .align_x(iced::Alignment::Center)
                        .spacing(10)
                    )
                    .align_x(iced::alignment::Horizontal::Center),
                    Space::with_height(Length::Fill)
                ]
                .spacing(20)
                .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .style(theme::Container::Default.style())
            .into()
        } else {
            // Choose view based on mode
            let library_content = match state.view_mode {
                ViewMode::All => {
                    // Use carousel view for main page
                    let mut content = column![].spacing(30).padding(20);

                    // TV Shows carousel - show all shows
                    if !state.tv_shows.is_empty() {
                        let tv_show_cards: Vec<_> = state
                            .tv_shows
                            .values()
                            .map(|show| {
                                crate::components::tv_show_card(show, &state.poster_cache, false)
                            })
                            .collect();

                        let tv_carousel = carousel::media_carousel(
                            "tv_shows".to_string(),
                            "TV Shows",
                            tv_show_cards,
                            &state.tv_shows_carousel,
                        );

                        content = content.push(tv_carousel);
                    }

                    // Movies carousel - show all movies
                    if !state.movies.is_empty() {
                        log::debug!("Creating movie carousel with {} movies", state.movies.len());
                        
                        // Log info about last few movies (likely the newest)
                        for (i, movie) in state.movies.iter().rev().take(3).enumerate() {
                            let poster_state = state.poster_cache.get(&movie.id);
                            log::debug!(
                                "Recent movie[{}]: {}, has_poster: {}, poster_cache: {:?}", 
                                i, movie.filename, movie.has_poster(), 
                                poster_state.as_ref().map(|s| format!("{:?}", std::mem::discriminant(s)))
                            );
                        }
                        
                        let movie_cards: Vec<_> = state
                            .movies
                            .iter()
                            .map(|movie| {
                                crate::components::movie_card_lazy(
                                    movie,
                                    &state.poster_cache,
                                    false, // is_hovered
                                    false, // is_loading - not used, poster cache handles loading state
                                    &state.poster_animation_types,
                                )
                            })
                            .collect();

                        let movies_carousel = carousel::media_carousel(
                            "movies".to_string(),
                            "Movies",
                            movie_cards,
                            &state.movies_carousel,
                        );

                        content = content.push(movies_carousel);
                    }

                    // If no content
                    if state.movies.is_empty() && state.tv_shows.is_empty() && !state.loading {
                        content = content.push(
                            container(
                                column![
                                    text("📁").size(64),
                                    text("No media files found")
                                        .size(24)
                                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                                    text("Click 'Scan Library' to search for media files")
                                        .size(16)
                                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                                ]
                                .spacing(10)
                                .align_x(iced::Alignment::Center),
                            )
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .center_x(Length::Fill)
                            .center_y(Length::Fill),
                        );
                    }

                    scrollable(content)
                        .direction(scrollable::Direction::Vertical(
                            scrollable::Scrollbar::default(),
                        ))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                }
                ViewMode::Movies => {
                    // Use virtual grid view for movies page with lazy loading
                    grid_view::virtual_media_grid(
                        &state.movies,
                        &state.movies_grid_state,
                        &state.poster_cache,
                        &state.loading_posters,
                        &state.hovered_media_id,
                        &state.poster_animation_types,
                        Message::MoviesGridScrolled,
                        state.fast_scrolling,
                    )
                }
                ViewMode::TvShows => {
                    // Use virtual grid view for TV shows with lazy loading
                    grid_view::virtual_tv_grid(
                        &state.tv_shows_sorted,
                        &state.tv_shows_grid_state,
                        &state.poster_cache,
                        &state.loading_posters,
                        &state.hovered_media_id,
                        &state.poster_animation_types,
                        Message::TvShowsGridScrolled,
                        state.fast_scrolling,
                    )
                }
            };

            // Calculate header height including error section if present
            let header_height = if state.error_message.is_some() {
                140.0 // Header + error message
            } else {
                80.0 // Just header
            };
            
            // Create main content with proper layering
            let main_content = {
                // Background layer: scrollable content with top padding
                let content_layer = column![
                    Space::with_height(Length::Fixed(header_height)), // Space for header
                    scan_progress_section,
                    container(library_content)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .clip(true)
                ];
                
                // Foreground layer: fixed header at top
                let header_with_error = column![
                    header,
                    error_section,
                ];
                
                let header_layer = container(header_with_error)
                    .width(Length::Fill)
                    .height(Length::Fixed(header_height));
                
                // Stack them with header on top
                Stack::new()
                    .push(content_layer)
                    .push(header_layer)
            };

            // Add scan progress overlay if visible
            if state.show_scan_progress {
                //&& state.scan_progress.is_some() {
                log::info!("Showing scan overlay - show_scan_progress: true, scan_progress: Some");
                create_scan_progress_overlay(main_content, &state.scan_progress)
            } else {
                log::debug!(
                    "Not showing scan overlay - show_scan_progress: {}, scan_progress: {}",
                    state.show_scan_progress,
                    state.scan_progress.is_some()
                );
                main_content.into()
            }
        }
    }
}

// Create scan progress overlay
fn create_scan_progress_overlay<'a>(
    content: impl Into<Element<'a, Message>>,
    scan_progress: &Option<ScanProgress>,
) -> Element<'a, Message> {
    log::info!("Creating scan overlay function called");
    use iced::widget::{mouse_area, stack};

    let base_content = content.into();

    if let Some(progress) = scan_progress {
        log::info!(
            "Scan progress data available: status={:?}, files={}/{}",
            progress.status,
            progress.scanned_files,
            progress.total_files
        );
        let progress_percentage = if progress.total_files > 0 {
            (progress.scanned_files as f32 / progress.total_files as f32) * 100.0
        } else {
            0.0
        };

        let eta_text = if let Some(eta) = progress.estimated_time_remaining {
            let seconds = eta.as_secs();
            if seconds < 60 {
                format!("{} sec", seconds)
            } else {
                let minutes = seconds / 60;
                let remaining_seconds = seconds % 60;
                format!("{}:{:02}", minutes, remaining_seconds)
            }
        } else {
            "Calculating...".to_string()
        };

        let current_file_text = if let Some(file) = &progress.current_file {
            let filename = std::path::Path::new(file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file);
            filename.to_string()
        } else {
            "Scanning...".to_string()
        };

        let status_text = match progress.status {
            ScanStatus::Pending => "Preparing",
            ScanStatus::Scanning => "Scanning",
            ScanStatus::Processing => "Processing",
            ScanStatus::Completed => "Completed",
            ScanStatus::Failed => "Failed",
            ScanStatus::Cancelled => "Cancelled",
        };

        // Create overlay content
        // Add a semi-transparent background
        let background = container(Space::new(Length::Fill, Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill);

        let overlay_content = container(
            container(
                column![
                    // Header
                    row![
                        text("Scan Progress")
                            .size(16)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        Space::with_width(Length::Fill),
                        button(icon_text(Icon::X))
                            .on_press(Message::ToggleScanProgress)
                            .style(theme::Button::Text.style())
                            .padding(5)
                    ]
                    .align_y(iced::Alignment::Center),
                    Space::with_height(15),
                    // Progress bar
                    row![
                        text(status_text)
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        Space::with_width(Length::Fill),
                        container(
                            container(Space::with_width(Length::Fixed(
                                progress_percentage * 0.01 * 250.0
                            )))
                            .height(4)
                            .style(theme::Container::ProgressBar.style())
                        )
                        .width(250)
                        .height(4)
                        .style(theme::Container::ProgressBarBackground.style()),
                        Space::with_width(10),
                        text(format!("{:.0}%", progress_percentage))
                            .size(13)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .align_y(iced::Alignment::Center),
                    Space::with_height(10),
                    // Stats
                    row![
                        column![
                            text("Files")
                                .size(11)
                                .color(theme::MediaServerTheme::TEXT_DIMMED),
                            text(format!(
                                "{}/{}",
                                progress.scanned_files, progress.total_files
                            ))
                            .size(13)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        ],
                        Space::with_width(30),
                        column![
                            text("Stored")
                                .size(11)
                                .color(theme::MediaServerTheme::TEXT_DIMMED),
                            text(format!("{}", progress.stored_files))
                                .size(13)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        ],
                        Space::with_width(30),
                        column![
                            text("Metadata")
                                .size(11)
                                .color(theme::MediaServerTheme::TEXT_DIMMED),
                            text(format!("{}", progress.metadata_fetched))
                                .size(13)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        ],
                        Space::with_width(30),
                        column![
                            text("ETA")
                                .size(11)
                                .color(theme::MediaServerTheme::TEXT_DIMMED),
                            text(eta_text)
                                .size(13)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        ],
                    ],
                    Space::with_height(10),
                    // Current file
                    column![
                        text("Current File")
                            .size(11)
                            .color(theme::MediaServerTheme::TEXT_DIMMED),
                        text(current_file_text)
                            .size(12)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ],
                    // Errors if any
                    if !progress.errors.is_empty() {
                        Element::from(column![
                            Space::with_height(10),
                            text(format!("{} errors occurred", progress.errors.len()))
                                .size(12)
                                .color(theme::MediaServerTheme::ERROR),
                        ])
                    } else {
                        Element::from(Space::with_height(0))
                    },
                ]
                .spacing(5)
                .width(400),
            )
            .padding(20)
            .style(theme::Container::Card.style())
            .width(Length::Shrink)
            .height(Length::Shrink),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Top)
        .padding(40);

        // Stack the overlay on top of the main content
        log::info!("Rendering scan overlay stack");
        stack![
            base_content,
            // Semi-transparent background
            mouse_area(background).on_press(Message::ToggleScanProgress),
            // Overlay content
            overlay_content
        ]
        .into()
    } else {
        base_content
    }
}

// Safe scrollable wrapper that prevents debug assertion failures
// NOTE: This is a workaround for iced 0.13.x scrollable widget debug assertions.
// In debug mode, scrolling is disabled but content is clipped.
// In release mode, proper scrolling functionality is available.
// See: https://github.com/iced-rs/iced/issues related to scrollable validation
fn safe_scrollable<'a>(
    content: impl Into<Element<'a, Message>>,
    direction: scrollable::Direction,
) -> Element<'a, Message> {
    match direction {
        scrollable::Direction::Vertical(_) => {
            // For vertical scrolling: content should have shrink height
            let inner = container(content)
                .width(Length::Fill)
                .height(Length::Shrink)
                .padding(0);

            scrollable(inner)
                .width(Length::Fill)
                .height(Length::Fill)
                .direction(direction)
                .into()
        }
        scrollable::Direction::Horizontal(_) => {
            // For horizontal scrolling: content should have shrink width
            let inner = container(content)
                .width(Length::Shrink)
                .height(Length::Fill)
                .padding(0);

            scrollable(inner)
                .width(Length::Fill)
                .height(Length::Fill)
                .direction(direction)
                .into()
        }
        scrollable::Direction::Both { .. } => {
            // For both directions: content should shrink in both dimensions
            let inner = container(content)
                .width(Length::Shrink)
                .height(Length::Shrink)
                .padding(0);

            scrollable(inner)
                .width(Length::Fill)
                .height(Length::Fill)
                .direction(direction)
                .into()
        }
    }
}

// Get the lucide font
fn lucide_font() -> Font {
    Font::with_name("lucide")
}

// Glassy button style closure (keeping for settings panel)
fn glassy_button_style(_theme: &iced::Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(
            0.0, 0.0, 0.0, 0.3,
        ))),
        border: iced::Border {
            color: iced::Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: iced::Color::WHITE,
        ..button::Style::default()
    }
}

// Transparent icon style - no background, just floating icons
fn transparent_icon_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    button::Style {
        background: None,
        border: iced::Border::default(),
        text_color: match status {
            button::Status::Hovered => iced::Color::from_rgba(1.0, 1.0, 1.0, 0.8),
            _ => iced::Color::WHITE,
        },
        ..button::Style::default()
    }
}

// Active setting button style
fn active_setting_style(_theme: &iced::Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(
            1.0, 1.0, 1.0, 0.2,
        ))),
        border: iced::Border {
            color: iced::Color::from_rgba(1.0, 1.0, 1.0, 0.3),
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: iced::Color::WHITE,
        ..button::Style::default()
    }
}

// Helper function to create a control button with icon
fn icon_button(icon: Icon, message: Option<Message>) -> Element<'static, Message> {
    let btn = button(icon_text(icon))
        .style(transparent_icon_style as fn(&iced::Theme, button::Status) -> button::Style);

    if let Some(msg) = message {
        btn.on_press(msg)
    } else {
        btn
    }
    .padding(8)
    .into()
}

fn view_player(state: &State) -> Element<Message> {
    state.player.view()
}

fn view_movie_detail<'a>(state: &'a State, media: &'a MediaFile) -> Element<'a, Message> {
    // Create main layout with poster on left, details on right
    let mut main_content = column![].spacing(20).height(Length::Shrink).padding(20);

    // Header with back button
    main_content = main_content.push(
        row![
            button(
                row![icon_text(Icon::ArrowLeft), text(" Back to Library")]
                    .spacing(5)
                    .align_y(iced::Alignment::Center)
            )
            .on_press(Message::BackToLibrary)
            .style(theme::Button::Secondary.style()),
            Space::with_width(Length::Fill),
        ]
        .align_y(iced::Alignment::Center),
    );

    // Content row with poster and details - always check cache
    let poster_element = match state.poster_cache.get(&media.id) {
        Some(PosterState::Loaded { full_size, .. }) => container(
            image(full_size)
                .content_fit(iced::ContentFit::Cover)
                .width(Length::Fill),
        )
        .width(Length::Fixed(300.0))
        .height(Length::Fixed(450.0))
        .style(theme::Container::Card.style()),
        _ => {
            // No poster loaded - show default movie icon
            container(text("🎬").size(64))
                .width(Length::Fixed(300.0))
                .height(Length::Fixed(450.0))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .style(theme::Container::Card.style())
        }
    };

    // Details column
    let mut details = column![]
        .spacing(15)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Shrink);

    // Title
    details = details.push(
        text(media.display_title())
            .size(32)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
    );

    // Basic info
    details = details.push(
        text(media.display_info())
            .size(16)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    );

    // Play button
    details = details.push(
        button(
            row![icon_text(Icon::Play), text(" Play").size(18)]
                .spacing(8)
                .align_y(iced::Alignment::Center),
        )
        .on_press(Message::PlayMedia(media.clone()))
        .padding([12, 24])
        .style(theme::Button::Primary.style()),
    );

    // Refresh metadata button
    details = details.push(
        button(
            row![
                icon_text(Icon::RefreshCw),
                text(" Refresh Metadata").size(16)
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::FetchMetadata(media.id.clone()))
        .padding([10, 20])
        .style(theme::Button::Secondary.style()),
    );

    // Metadata sections
    if let Some(metadata) = &media.metadata {
        // Synopsis
        if let Some(external) = &metadata.external_info {
            if let Some(desc) = &external.description {
                details = details.push(Space::with_height(20));
                details = details.push(text("Synopsis").size(20));
                details = details.push(
                    container(text(desc).size(14))
                        .padding(10)
                        .width(Length::Fill),
                );
            }

            // Additional metadata
            if !external.genres.is_empty() {
                details = details.push(row![
                    text("Genres: ").size(14),
                    text(external.genres.join(", "))
                        .size(14)
                        .color(iced::Color::from_rgb(0.7, 0.7, 0.7))
                ]);
            }

            if let Some(rating) = external.rating {
                details = details.push(row![
                    text("Rating: ").size(14),
                    text(format!("{:.1}/10", rating))
                        .size(14)
                        .color(iced::Color::from_rgb(0.7, 0.7, 0.7))
                ]);
            }
        }

        // Technical details
        details = details.push(Space::with_height(20));
        details = details.push(text("Technical Details").size(20));

        if let Some(width) = metadata.width {
            if let Some(height) = metadata.height {
                details = details.push(
                    text(format!("Resolution: {}x{}", width, height))
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                );
            }
        }

        if let Some(codec) = &metadata.video_codec {
            details = details.push(
                text(format!("Video Codec: {}", codec))
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            );
        }

        details = details.push(
            text(format!("Duration: {}", format_duration(metadata.duration)))
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    // Add the poster and details to main content
    main_content = main_content.push(
        row![poster_element, Space::with_width(30), details,]
            .height(Length::Shrink)
            .align_y(iced::alignment::Vertical::Top),
    );

    // Wrap in scrollable
    scrollable(
        container(main_content)
            .width(Length::Fill)
            .height(Length::Shrink),
    )
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::default(),
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::Scrollable::style())
    .into()
}

// Helper function to format duration
fn format_duration(seconds: f64) -> String {
    let hours = (seconds / 3600.0) as u32;
    let minutes = ((seconds % 3600.0) / 60.0) as u32;
    let secs = (seconds % 60.0) as u32;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

fn subscription(state: &State) -> Subscription<Message> {
    let mut subscriptions = vec![];

    match &state.view {
        ViewState::Player if state.player.video_opt.is_some() => {
            // Timer for controls hide/show
            subscriptions.push(
                iced::time::every(std::time::Duration::from_millis(500)).map(|_| Message::Tick),
            );

            // Subscribe to keyboard shortcuts
            subscriptions.push(iced::keyboard::on_key_press(|key, modifiers| {
                use iced::keyboard::{key::Named, Key};
                match key {
                    Key::Named(Named::Space) => Some(Message::PlayPause),
                    Key::Named(Named::ArrowLeft) => {
                        if modifiers.shift() {
                            Some(Message::SeekRelative(-30.0))
                        } else {
                            Some(Message::SeekBackward)
                        }
                    }
                    Key::Named(Named::ArrowRight) => {
                        if modifiers.shift() {
                            Some(Message::SeekRelative(30.0))
                        } else {
                            Some(Message::SeekForward)
                        }
                    }
                    Key::Named(Named::ArrowUp) => Some(Message::SetVolume(1.1)),
                    Key::Named(Named::ArrowDown) => Some(Message::SetVolume(0.9)),
                    Key::Named(Named::Escape) => Some(Message::ToggleFullscreen), // Simplify - escape always toggles fullscreen
                    Key::Character(c) if c.as_str() == "f" || c.as_str() == "F" => {
                        Some(Message::ToggleFullscreen)
                    }
                    Key::Character(c) if c.as_str() == "m" || c.as_str() == "M" => {
                        Some(Message::ToggleMute)
                    }
                    Key::Character(c) if c.as_str() == "s" || c.as_str() == "S" => {
                        if modifiers.shift() {
                            Some(Message::ToggleSubtitleMenu)
                        } else {
                            Some(Message::CycleSubtitleSimple)
                        }
                    }
                    Key::Character(c) if c.as_str() == "a" || c.as_str() == "A" => {
                        Some(Message::CycleAudioTrack)
                    }
                    _ => None,
                }
            }));
        }
        _ => {}
    }

    // Always subscribe to window resize events
    subscriptions
        .push(iced::window::resize_events().map(|(_id, size)| Message::WindowResized(size)));

    // Subscribe to scan progress if we have an active scan
    if let Some(scan_id) = &state.active_scan_id {
        //log::info!("Creating scan progress subscription for scan ID: {}", scan_id);
        subscriptions.push(scan_progress_subscription(
            state.server_url.clone(),
            scan_id.clone(),
        ));
    } else {
        log::debug!("No active scan ID, not creating scan progress subscription");
    }

    // Removed redundant poster update timer - PosterMonitorTick handles this

    // Subscribe to media events SSE stream
    if !state.server_url.is_empty() {
        subscriptions.push(media_events_subscription(state.server_url.clone()));
    }

    // Poster monitor background task - moderate frequency to avoid channel overflow
    if state.poster_monitor.is_some() {
        subscriptions.push(
            iced::time::every(performance_config::posters::MONITOR_TICK_INTERVAL)
                .map(|_| Message::PosterMonitorTick),
        );
    }

    Subscription::batch(subscriptions)
}

fn media_events_subscription(server_url: String) -> Subscription<Message> {
    #[derive(Debug, Clone, Hash)]
    struct MediaEventsId(String);
    
    Subscription::run_with(
        MediaEventsId(server_url.clone()),
        |MediaEventsId(server_url)| futures::stream::unfold(
            (server_url.clone(), None::<reqwest_eventsource::EventSource>, 0u32),
            |(server_url, mut event_source, retry_count)| async move {
                use futures::StreamExt;

                // Create event source if we don't have one
                if event_source.is_none() {
                    // Add delay for retries only, not initial connection
                    if retry_count > 0 {
                        let delay = std::time::Duration::from_secs(retry_count.min(30) as u64);
                        log::info!(
                            "Retrying SSE connection after {} seconds (attempt #{})",
                            delay.as_secs(),
                            retry_count
                        );
                        tokio::time::sleep(delay).await;
                    }

                    let url = format!("{}/library/events/sse", server_url);
                    log::info!(
                        "Creating media events SSE connection to: {} (attempt #{})",
                        url,
                        retry_count + 1
                    );

                    let es = reqwest_eventsource::EventSource::get(&url);
                    event_source = Some(es);
                }

                // Read from event source
                if let Some(es) = &mut event_source {
                    match es.next().await {
                        Some(Ok(reqwest_eventsource::Event::Message(msg))) => {
                            // Check if this is a keepalive message
                            if msg.data == "keepalive" || msg.data.is_empty() {
                                log::debug!("Received media event SSE keepalive");
                                // Continue listening
                                Some((Message::NoOp, (server_url, event_source, retry_count)))
                            } else if msg.event == "media_event" {
                                log::debug!("Received media event SSE message: {}", msg.data);
                                // Parse the media event data
                                match serde_json::from_str::<MediaEvent>(&msg.data) {
                                    Ok(event) => {
                                        log::debug!("Successfully parsed media event");
                                        Some((
                                            Message::MediaEventReceived(event),
                                            (server_url, event_source, 0), // Reset retry count on success
                                        ))
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Failed to parse media event: {} - Data: {}",
                                            e,
                                            msg.data
                                        );
                                        // Don't close the connection on parse error
                                        Some((
                                            Message::NoOp,
                                            (server_url, event_source, retry_count),
                                        ))
                                    }
                                }
                            } else {
                                log::debug!(
                                    "Received unknown media event type: {:?}, data: {}",
                                    msg.event,
                                    msg.data
                                );
                                Some((Message::NoOp, (server_url, event_source, retry_count)))
                            }
                        }
                        Some(Ok(reqwest_eventsource::Event::Open)) => {
                            log::info!("Media events SSE connection opened");
                            // Continue reading - not an error
                            Some((
                                Message::NoOp,                 // Use NoOp instead of error for connection status
                                (server_url, event_source, 0), // Reset retry count
                            ))
                        }
                        Some(Err(e)) => {
                            log::error!("Media events SSE error: {}", e);
                            Some((
                                Message::MediaEventsError(format!("SSE error: {}", e)),
                                (server_url, None, retry_count + 1),
                            ))
                        }
                        None => {
                            log::info!("Media events SSE stream ended");
                            None
                        }
                    }
                } else {
                    None
                }
            },
        ),
    )
}

fn scan_progress_subscription(server_url: String, scan_id: String) -> Subscription<Message> {
    #[derive(Debug, Clone, Hash)]
    struct ScanProgressId(String, String);
    
    Subscription::run_with(
        ScanProgressId(server_url.clone(), scan_id.clone()),
        |ScanProgressId(server_url, scan_id)| futures::stream::unfold(
            (
                server_url.clone(),
                scan_id.clone(),
                None::<reqwest_eventsource::EventSource>,
            ),
            |(server_url, scan_id, mut event_source)| async move {
                use futures::StreamExt;

                // Create event source if we don't have one
                if event_source.is_none() {
                    let url = format!("{}/scan/progress/{}/sse", server_url, scan_id);
                    log::info!("Creating SSE connection to: {}", url);
                    let es = reqwest_eventsource::EventSource::get(&url);
                    event_source = Some(es);
                }
                
                // Read from event source
                if let Some(es) = &mut event_source {
                    log::debug!("Polling SSE event source for scan {}", scan_id);
                    match es.next().await {
                        Some(Ok(reqwest_eventsource::Event::Message(msg))) => {
                            // Check if this is a keepalive message
                            if msg.data == "keepalive" || msg.data.is_empty() {
                                log::debug!("Received SSE keepalive");
                                // Continue listening
                                Some((
                                    Message::NoOp,
                                    (server_url, scan_id, event_source),
                                ))
                            } else if msg.event == "progress" {
                                log::info!("Received scan progress SSE event, data: {}", msg.data);
                                
                                // Parse scan progress event
                                match serde_json::from_str::<ScanProgress>(&msg.data) {
                                    Ok(progress) => {
                                        log::debug!("Successfully parsed scan progress");
                                        Some((
                                            Message::ScanProgressUpdate(progress),
                                            (server_url, scan_id, event_source),
                                        ))
                                    }
                                    Err(e) => {
                                        log::error!("Failed to parse scan progress: {} - Data: {}", e, msg.data);
                                        // Don't close the connection on parse error
                                        Some((
                                            Message::NoOp,
                                            (server_url, scan_id, event_source),
                                        ))
                                    }
                                }
                            } else {
                                // Unknown event type, just continue listening
                                Some((
                                    Message::NoOp,
                                    (server_url, scan_id, event_source),
                                ))
                            }
                        }
                        Some(Ok(reqwest_eventsource::Event::Open)) => {
                            log::info!("SSE connection opened for scan {}", scan_id);
                            
                            // Try to fetch initial progress via HTTP and send it
                            let initial_message = match reqwest::get(format!("{}/scan/progress/{}", server_url, scan_id)).await {
                                Ok(response) => {
                                    match response.json::<serde_json::Value>().await {
                                        Ok(json) => {
                                            log::info!("Initial progress check: {}", json);
                                            if let Some(progress_data) = json.get("progress") {
                                                match serde_json::from_value::<ScanProgress>(progress_data.clone()) {
                                                    Ok(progress) => {
                                                        log::info!("Initial scan status: {:?}, files: {}/{}", 
                                                            progress.status, progress.scanned_files, progress.total_files);
                                                        // Send initial progress update
                                                        Message::ScanProgressUpdate(progress)
                                                    }
                                                    Err(e) => {
                                                        log::error!("Failed to parse initial progress: {}", e);
                                                        Message::Tick
                                                    }
                                                }
                                            } else {
                                                log::warn!("No progress field in initial response");
                                                Message::Tick
                                            }
                                        }
                                        Err(e) => {
                                            log::error!("Failed to parse initial response: {}", e);
                                            Message::Tick
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Failed to fetch initial progress: {}", e);
                                    Message::Tick
                                }
                            };
                            
                            Some((
                                initial_message,
                                (server_url, scan_id, event_source),
                            ))
                        }
                        Some(Err(e)) => {
                            log::error!("SSE error: {}", e);
                            // Wait a bit before retrying
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            // Continue with HTTP fallback
                            Some((
                                Message::NoOp,
                                (server_url, scan_id, None), // Reset event source to retry
                            ))
                        }
                        None => {
                            log::warn!("SSE stream ended");
                            // Wait a bit before checking scan status
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            
                            // Try to fetch progress via regular HTTP
                            match reqwest::get(format!("{}/scan/progress/{}", server_url, scan_id)).await {
                                Ok(response) => {
                                    match response.json::<serde_json::Value>().await {
                                        Ok(json) => {
                                            log::info!("HTTP fallback response: {}", json);
                                            if let Some(progress_data) = json.get("progress") {
                                                match serde_json::from_value::<ScanProgress>(progress_data.clone()) {
                                                    Ok(progress) => {
                                                        log::info!("Fallback to HTTP polling for scan progress");
                                                        Some((
                                                            Message::ScanProgressUpdate(progress),
                                                            (server_url, scan_id, None), // Reset event source
                                                        ))
                                                    }
                                                    Err(e) => {
                                                        log::error!("Failed to parse progress from HTTP: {}", e);
                                                        Some((
                                                            Message::ScanCompleted(Err(format!("Failed to get progress: {}", e))),
                                                            (server_url, scan_id, None),
                                                        ))
                                                    }
                                                }
                                            } else {
                                                // Scan might be done
                                                Some((
                                                    Message::ScanCompleted(Ok("Scan completed".to_string())),
                                                    (server_url, scan_id, None),
                                                ))
                                            }
                                        }
                                        Err(e) => {
                                            log::error!("Failed to parse HTTP response: {}", e);
                                            Some((
                                                Message::ScanCompleted(Err(format!("Connection error: {}", e))),
                                                (server_url, scan_id, None),
                                            ))
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::error!("HTTP fallback failed: {}", e);
                                    Some((
                                        Message::ScanCompleted(Err(format!("Connection lost: {}", e))),
                                        (server_url, scan_id, None),
                                    ))
                                }
                            }
                        }
                    }
                } else {
                    // Event source was reset, continue to next iteration to recreate it
                    log::debug!("Event source is None, will recreate on next iteration");
                    Some((
                        Message::Tick,
                        (server_url, scan_id, event_source),
                    ))
                }
            },
        ),
    )
}

async fn start_media_scan(server_url: String, force_rescan: bool) -> Result<String, anyhow::Error> {
    log::info!(
        "Starting media library scan (force_rescan: {})",
        force_rescan
    );

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/scan/start", server_url))
        .json(&serde_json::json!({
            "paths": null,  // Will use configured MEDIA_ROOT
            "max_depth": null,  // No depth limit
            "follow_links": true,  // DO follow symlinks by default
            "extract_metadata": true,  // Always extract metadata
            "force_rescan": force_rescan
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow::anyhow!("Server returned error: {}", error_text));
    }

    let json: serde_json::Value = response.json().await?;

    if let Some(scan_id) = json.get("scan_id").and_then(|id| id.as_str()) {
        Ok(scan_id.to_string())
    } else if let Some(error) = json.get("error").and_then(|e| e.as_str()) {
        Err(anyhow::anyhow!("Scan error: {}", error))
    } else {
        Err(anyhow::anyhow!("Invalid response from server"))
    }
}

// Library-specific scan function
async fn start_library_scan(
    server_url: String,
    library_id: String,
    streaming: bool,
) -> Result<String, anyhow::Error> {
    log::info!(
        "Starting library scan (library_id: {}, streaming: {})",
        library_id,
        streaming
    );

    media_library::scan_library(server_url, library_id, streaming).await
}

async fn fetch_scan_progress(
    server_url: String,
    scan_id: String,
) -> Result<ScanProgress, anyhow::Error> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/scan/progress/{}", server_url, scan_id))
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow::anyhow!("Failed to fetch progress: {}", error_text));
    }

    let progress: ScanProgress = response.json().await?;
    Ok(progress)
}

async fn fetch_metadata_for_media(
    server_url: String,
    media_id: String,
) -> Result<(), anyhow::Error> {
    log::info!("Fetching metadata for media: {}", media_id);

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/metadata/fetch/{}", server_url, media_id))
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow::anyhow!("Metadata fetch failed: {}", error_text));
    }

    Ok(())
}

// TV Show detail views
fn view_tv_show_detail<'a>(state: &'a State, _show_name: &'a str) -> Element<'a, Message> {
    let mut content = column![].spacing(20).padding(20);

    // Header with back button
    content = content.push(
        row![
            button(
                row![icon_text(Icon::ArrowLeft), text(" Back to Library")]
                    .spacing(5)
                    .align_y(iced::Alignment::Center)
            )
            .on_press(Message::BackToLibrary)
            .style(theme::Button::Secondary.style()),
            Space::with_width(Length::Fill),
        ]
        .align_y(iced::Alignment::Center),
    );

    // Check if we have show details loaded
    if let Some(show_details) = &state.current_show_details {
        // Show poster and details side by side
        let poster_element: Element<Message> = if let Some(poster_url) = &show_details.poster_url {
            // Use the image cache to load from URL
            match state.image_cache.get(poster_url) {
                Some(ImageState::Loaded(handle)) => container(
                    image(handle)
                        .content_fit(iced::ContentFit::Cover)
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .width(Length::Fixed(300.0))
                .height(Length::Fixed(450.0))
                .style(theme::Container::Card.style())
                .into(),
                Some(ImageState::Loading) => container(
                    column![text("⏳").size(32), text("Loading...").size(14)]
                        .align_x(iced::Alignment::Center)
                        .spacing(5),
                )
                .width(Length::Fixed(300.0))
                .height(Length::Fixed(450.0))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .style(theme::Container::Card.style())
                .into(),
                _ => container(container(text("📺").size(64)).padding(50))
                    .width(Length::Fixed(300.0))
                    .height(Length::Fixed(450.0))
                    .style(theme::Container::Card.style())
                    .into(),
            }
        } else {
            container(container(text("📺").size(64)).padding(50))
                .width(Length::Fixed(300.0))
                .height(Length::Fixed(450.0))
                .style(theme::Container::Card.style())
                .into()
        };

        // Details column
        let mut details = column![].spacing(15).padding(20).width(Length::Fill);

        // Title
        details = details.push(
            text(&show_details.name)
                .size(32)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        );

        // Stats
        let stats = format!(
            "{} Seasons • {} Episodes",
            show_details.seasons.len(),
            show_details.total_episodes
        );
        details = details.push(
            text(stats)
                .size(16)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );

        // Rating
        if let Some(rating) = show_details.rating {
            details = details.push(
                text(format!("★ {:.1}", rating))
                    .size(16)
                    .color(theme::MediaServerTheme::ACCENT_BLUE),
            );
        }

        // Refresh metadata button for the show
        details = details.push(
            button(
                row![
                    icon_text(Icon::RefreshCw),
                    text(" Refresh Show Metadata").size(16)
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::RefreshShowMetadata(show_details.name.clone()))
            .padding([10, 20])
            .style(theme::Button::Secondary.style()),
        );

        // Description
        if let Some(desc) = &show_details.description {
            details = details.push(Space::with_height(10));
            details = details.push(
                container(
                    text(desc)
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                )
                .width(Length::Fill)
                .padding(10),
            );
        }

        // Genres
        if !show_details.genres.is_empty() {
            details = details.push(
                text(format!("Genres: {}", show_details.genres.join(", ")))
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            );
        }

        // Content row with poster and details
        let info_row =
            row![poster_element, Space::with_width(20), details].align_y(iced::Alignment::Start);

        content = content.push(info_row);

        // Seasons carousel
        if !show_details.seasons.is_empty() {
            content = content.push(Space::with_height(20));

            if let Some(carousel_state) = &state.show_seasons_carousel {
                let season_cards: Vec<_> = show_details
                    .seasons
                    .iter()
                    .map(|season| {
                        components_enhanced::season_card_with_cache(
                            season,
                            &show_details.name,
                            &state.image_cache,
                            &state.server_url,
                        )
                    })
                    .collect();

                let seasons_carousel = carousel::media_carousel(
                    "show_seasons".to_string(),
                    "Seasons",
                    season_cards,
                    carousel_state,
                );

                content = content.push(seasons_carousel);
            }
        }
    } else {
        // Loading state
        content = content.push(
            container(
                column![
                    text("⏳").size(48),
                    text("Loading show details...")
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                ]
                .spacing(10),
            )
            .padding(100),
        );
    }

    safe_scrollable(
        content,
        scrollable::Direction::Vertical(scrollable::Scrollbar::default()),
    )
}

fn view_season_detail<'a>(
    state: &'a State,
    show_name: &'a str,
    season_num: u32,
) -> Element<'a, Message> {
    let mut content = column![].spacing(20).padding(20);

    // Header with back button
    content = content.push(
        row![
            button(
                row![icon_text(Icon::ArrowLeft), text(" Back to Show")]
                    .spacing(5)
                    .align_y(iced::Alignment::Center)
            )
            .on_press(Message::ViewTvShow(show_name.to_string()))
            .style(theme::Button::Secondary.style()),
            Space::with_width(Length::Fill),
        ]
        .align_y(iced::Alignment::Center),
    );

    // Check if we have season details loaded
    if let Some(season_details) = &state.current_season_details {
        // Season poster and details side by side
        let poster_element: Element<Message> = if let Some(poster_url) = &season_details.poster_url
        {
            // Convert relative paths to full URLs
            let full_url = if poster_url.starts_with("/") {
                format!("{}{}", state.server_url, poster_url)
            } else {
                poster_url.clone()
            };
            // Use the image cache to load from URL
            match state.image_cache.get(&full_url) {
                Some(ImageState::Loaded(handle)) => container(
                    image(handle)
                        .content_fit(iced::ContentFit::Cover)
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .width(Length::Fixed(300.0))
                .height(Length::Fixed(450.0))
                .style(theme::Container::Card.style())
                .into(),
                Some(ImageState::Loading) => container(
                    column![text("⏳").size(32), text("Loading...").size(14)]
                        .align_x(iced::Alignment::Center)
                        .spacing(5),
                )
                .width(Length::Fixed(300.0))
                .height(Length::Fixed(450.0))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .style(theme::Container::Card.style())
                .into(),
                _ => container(container(text("📺").size(64)).padding(50))
                    .width(Length::Fixed(300.0))
                    .height(Length::Fixed(450.0))
                    .style(theme::Container::Card.style())
                    .into(),
            }
        } else {
            container(container(text("📺").size(64)).padding(50))
                .width(Length::Fixed(300.0))
                .height(Length::Fixed(450.0))
                .style(theme::Container::Card.style())
                .into()
        };

        // Details column
        let mut details = column![].spacing(15).padding(20).width(Length::Fill);

        // Title
        let season_title = season_details.name.clone().unwrap_or_else(|| {
            if season_num == 0 {
                "Specials".to_string()
            } else {
                format!("Season {}", season_num)
            }
        });
        details = details.push(
            text(format!("{} - {}", show_name, season_title))
                .size(32)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        );

        // Episode count
        details = details.push(
            text(format!("{} Episodes", season_details.episodes.len()))
                .size(16)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );

        // Air date range if available
        let air_dates: Vec<&str> = season_details
            .episodes
            .iter()
            .filter_map(|e| e.air_date.as_deref())
            .collect();

        if !air_dates.is_empty() {
            let first_date = air_dates.iter().min().unwrap_or(&"");
            let last_date = air_dates.iter().max().unwrap_or(&"");

            let date_range = if first_date == last_date {
                first_date.to_string()
            } else {
                format!("{} - {}", first_date, last_date)
            };

            details = details.push(
                text(format!("Aired: {}", date_range))
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            );
        }

        // Total duration
        let total_duration: f64 = season_details
            .episodes
            .iter()
            .filter_map(|e| e.duration)
            .sum();

        if total_duration > 0.0 {
            let hours = (total_duration / 3600.0) as u32;
            let minutes = ((total_duration % 3600.0) / 60.0) as u32;
            details = details.push(
                text(format!("Total Runtime: {}h {}m", hours, minutes))
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            );
        }

        // Content row with poster and details
        let info_row =
            row![poster_element, Space::with_width(20), details].align_y(iced::Alignment::Start);

        content = content.push(info_row);

        // Episodes carousel
        if !season_details.episodes.is_empty() {
            content = content.push(Space::with_height(20));

            if let Some(carousel_state) = &state.season_episodes_carousel {
                let episode_cards: Vec<_> = season_details
                    .episodes
                    .iter()
                    .map(|episode| {
                        components_enhanced::episode_card_with_cache(
                            episode,
                            show_name,
                            season_num,
                            &state.image_cache,
                        )
                    })
                    .collect();

                let episodes_carousel = carousel::media_carousel(
                    "season_episodes".to_string(),
                    "Episodes",
                    episode_cards,
                    carousel_state,
                );

                content = content.push(episodes_carousel);
            }
        }
    } else {
        // Loading state
        content = content.push(
            container(
                column![
                    text("⏳").size(48),
                    text("Loading season details...")
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                ]
                .spacing(10),
            )
            .padding(100),
        );
    }

    safe_scrollable(
        content,
        scrollable::Direction::Vertical(scrollable::Scrollbar::default()),
    )
}

fn view_episode_detail<'a>(state: &'a State, media: &'a MediaFile) -> Element<'a, Message> {
    // TODO: Implement episode detail view
    // For now, reuse movie detail view
    view_movie_detail(state, media)
}

// Media availability structures and functions
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MediaAvailability {
    available: bool,
    reason: String,
    message: String,
}

async fn check_media_availability(
    server_url: &str,
    media_id: &str,
) -> Result<MediaAvailability, String> {
    let url = format!("{}/media/{}/availability", server_url, media_id);

    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Server error: {}", response.status()));
    }

    response
        .json::<MediaAvailability>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

// Admin Dashboard View
fn view_admin_dashboard(state: &State) -> Element<Message> {
    let mut content = column![].spacing(30).padding(20);

    // Header with back button
    content = content.push(
        row![
            button(
                row![icon_text(Icon::ArrowLeft), text(" Back to Library")]
                    .spacing(5)
                    .align_y(iced::Alignment::Center)
            )
            .on_press(Message::HideAdminDashboard)
            .style(theme::Button::Secondary.style()),
            Space::with_width(Length::Fill),
            text("Admin Dashboard")
                .size(32)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_width(Length::Fill),
            Space::with_width(Length::Fixed(100.0)), // Balance the back button
        ]
        .align_y(iced::Alignment::Center),
    );

    // Admin sections grid
    let admin_sections = row![
        // Library Management section
        container(
            column![
                row![
                    text("📚").size(32),
                    Space::with_width(15),
                    column![
                        text("Library Management")
                            .size(20)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        text("Manage media libraries, scanning, and organization")
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(5),
                ]
                .align_y(iced::Alignment::Center),
                Space::with_height(20),
                button("Manage Libraries")
                    .on_press(Message::ShowLibraryManagement)
                    .style(theme::Button::Primary.style())
                    .padding([12, 20])
                    .width(Length::Fill),
            ]
            .spacing(15)
            .padding(20)
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill),
        
        Space::with_width(20),
        
        // Server Settings section
        container(
            column![
                row![
                    text("⚙️").size(32),
                    Space::with_width(15),
                    column![
                        text("Server Settings")
                            .size(20)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        text("Configure server settings, API, and performance")
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(5),
                ]
                .align_y(iced::Alignment::Center),
                Space::with_height(20),
                button("Server Settings")
                    .on_press(Message::NoOp) // TODO: Implement server settings
                    .style(theme::Button::Secondary.style())
                    .padding([12, 20])
                    .width(Length::Fill),
            ]
            .spacing(15)
            .padding(20)
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill),
    ]
    .align_y(iced::Alignment::Start);

    content = content.push(admin_sections);

    // Second row of admin sections
    let admin_sections_2 = row![
        // Player Settings section
        container(
            column![
                row![
                    text("🎬").size(32),
                    Space::with_width(15),
                    column![
                        text("Player Settings")
                            .size(20)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        text("Configure video player, codecs, and playback")
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(5),
                ]
                .align_y(iced::Alignment::Center),
                Space::with_height(20),
                button("Player Settings")
                    .on_press(Message::NoOp) // TODO: Implement player settings
                    .style(theme::Button::Secondary.style())
                    .padding([12, 20])
                    .width(Length::Fill),
            ]
            .spacing(15)
            .padding(20)
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill),
        
        Space::with_width(20),
        
        // System Info section
        container(
            column![
                row![
                    text("📊").size(32),
                    Space::with_width(15),
                    column![
                        text("System Information")
                            .size(20)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        text("View system stats, logs, and health monitoring")
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(5),
                ]
                .align_y(iced::Alignment::Center),
                Space::with_height(20),
                button("System Info")
                    .on_press(Message::NoOp) // TODO: Implement system info
                    .style(theme::Button::Secondary.style())
                    .padding([12, 20])
                    .width(Length::Fill),
            ]
            .spacing(15)
            .padding(20)
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill),
    ]
    .align_y(iced::Alignment::Start);

    content = content.push(admin_sections_2);

    // System Status section (full width)
    let system_status = container(
        column![
            text("System Status")
                .size(20)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_height(15),
            row![
                column![
                    text("Server Status")
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    text("🟢 Online")
                        .size(16)
                        .color(iced::Color::from_rgb(0.0, 0.8, 0.0)),
                ]
                .spacing(5),
                Space::with_width(50),
                column![
                    text("Libraries")
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    text(format!("{} configured", state.libraries.len()))
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                ]
                .spacing(5),
                Space::with_width(50),
                column![
                    text("Total Media")
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    text(format!("{} movies, {} shows", state.movies.len(), state.tv_shows.len()))
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                ]
                .spacing(5),
                Space::with_width(Length::Fill),
                // Scan status
                if state.scanning || state.scan_progress.is_some() {
                    Element::from(
                        column![
                            text("Scan Status")
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                            text("🔄 Scanning...")
                                .size(16)
                                .color(iced::Color::from_rgb(0.0, 0.6, 1.0)),
                        ]
                        .spacing(5)
                    )
                } else {
                    Element::from(
                        column![
                            text("Scan Status")
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                            text("✅ Idle")
                                .size(16)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        ]
                        .spacing(5)
                    )
                },
            ]
            .align_y(iced::Alignment::Start),
        ]
        .spacing(10)
        .padding(20)
    )
    .style(theme::Container::Card.style())
    .width(Length::Fill);

    content = content.push(system_status);

    scrollable(
        container(content)
            .width(Length::Fill)
            .height(Length::Shrink),
    )
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::default(),
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::Scrollable::style())
    .into()
}

// Library Form View
fn view_library_form<'a>(state: &'a State, form_data: &'a crate::state::LibraryFormData) -> Element<'a, Message> {
    use iced::widget::{checkbox, radio, text_input};
    
    let mut content = column![].spacing(20).padding(20);

    // Header with back button
    content = content.push(
        row![
            button(
                row![icon_text(Icon::ArrowLeft), text(" Back to Library Management")]
                    .spacing(5)
                    .align_y(iced::Alignment::Center)
            )
            .on_press(Message::HideLibraryForm)
            .style(theme::Button::Secondary.style()),
            Space::with_width(Length::Fill),
            text(if form_data.editing { "Edit Library" } else { "Create Library" })
                .size(28)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_width(Length::Fill),
            Space::with_width(Length::Fixed(100.0)), // Balance the back button
        ]
        .align_y(iced::Alignment::Center),
    );

    // Simple form for now
    content = content.push(
        container(
            column![
                text("Library Form - Coming Soon")
                    .size(20)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text("This will contain the library creation/edit form")
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .spacing(10)
            .padding(20)
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill)
    );

    scrollable(
        container(content)
            .width(Length::Fill)
            .height(Length::Shrink),
    )
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::default(),
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::Scrollable::style())
    .into()
}

// Library Management View
fn view_library_management(state: &State) -> Element<Message> {
    // If form is open, show the form instead
    if let Some(form_data) = &state.library_form_data {
        return view_library_form(state, form_data);
    }
    
    let mut content = column![].spacing(20).padding(20);

    // Header with back button
    content = content.push(
        row![
            button(
                row![icon_text(Icon::ArrowLeft), text(" Back to Library")]
                    .spacing(5)
                    .align_y(iced::Alignment::Center)
            )
            .on_press(Message::HideLibraryManagement)
            .style(theme::Button::Secondary.style()),
            Space::with_width(Length::Fill),
            text("Library Management")
                .size(28)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_width(Length::Fill),
            button("Create Library")
                .on_press(Message::ShowLibraryForm(None))
                .style(theme::Button::Primary.style()),
        ]
        .align_y(iced::Alignment::Center),
    );

    // Libraries list
    if state.libraries.is_empty() {
        content = content.push(
            container(
                column![
                    text("📚").size(64),
                    text("No libraries configured")
                        .size(24)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    text("Create a library to start organizing your media")
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                ]
                .spacing(10)
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        );
    } else {
        content = content.push(Space::with_height(20));
        
        for library in &state.libraries {
            let library_card = container(
                column![
                    row![
                        column![
                            text(&library.name)
                                .size(20)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            text(format!(
                                "{} • {} paths • {}",
                                library.library_type,
                                library.paths.len(),
                                if library.enabled { "Enabled" } else { "Disabled" }
                            ))
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                            if !library.paths.is_empty() {
                                Element::from(
                                    text(library.paths.join(", "))
                                        .size(12)
                                        .color(theme::MediaServerTheme::TEXT_DIMMED),
                                )
                            } else {
                                Element::from(Space::with_height(0))
                            },
                        ]
                        .spacing(5)
                        .width(Length::Fill),
                        Space::with_width(20),
                        column![
                            button("Select")
                                .on_press(Message::SelectLibrary(library.id.clone()))
                                .style(if state.current_library_id.as_ref() == Some(&library.id) {
                                    theme::Button::Primary.style()
                                } else {
                                    theme::Button::Secondary.style()
                                }),
                            button("✏️ Edit")
                                .on_press(Message::ShowLibraryForm(Some(library.clone())))
                                .style(theme::Button::Secondary.style()),
                            button(
                                if library.library_type == "Movies" {
                                    "🎬 Scan Movies"
                                } else {
                                    "📺 Scan TV Shows"
                                }
                            )
                            .on_press(Message::ScanLibrary_(library.id.clone()))
                            .style(theme::Button::Secondary.style()),
                            button("🗑 Delete")
                                .on_press(Message::DeleteLibrary(library.id.clone()))
                                .style(theme::Button::Destructive.style()),
                        ]
                        .spacing(5)
                        .align_x(iced::Alignment::End),
                    ]
                    .align_y(iced::Alignment::Center)
                ]
                .spacing(10)
                .padding(15),
            )
            .width(Length::Fill)
            .style(theme::Container::Card.style());
            
            content = content.push(library_card);
            content = content.push(Space::with_height(10));
        }
    }

    scrollable(
        container(content)
            .width(Length::Fill)
            .height(Length::Shrink),
    )
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::default(),
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::Scrollable::style())
    .into()
}
