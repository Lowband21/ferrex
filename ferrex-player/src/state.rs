use crate::carousel::CarouselState;
use crate::image_cache::ImageCache;
use crate::media_library::{MediaFile, MediaLibrary};
use crate::metadata_cache::MetadataCache;
use crate::models::{SeasonDetails, TvShow, TvShowDetails};
use crate::player::PlayerState;
use crate::poster_cache::PosterCache;
use crate::poster_monitor::PosterMonitor;
use crate::profiling::PROFILER;
use crate::virtual_list::VirtualGridState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanStatus {
    Pending,
    Scanning,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scan_id: String,
    pub status: ScanStatus,
    pub path: String,
    pub total_files: usize,
    pub scanned_files: usize,
    pub stored_files: usize,
    pub metadata_fetched: usize,
    pub errors: Vec<String>,
    pub current_file: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub estimated_time_remaining: Option<Duration>,
}

#[derive(Debug, Clone, Default)]
pub enum ViewState {
    #[default]
    Library,
    Player,
    LoadingVideo {
        url: String,
    },
    VideoError {
        message: String,
    },
    MovieDetail {
        media: MediaFile,
    },
    TvShowDetail {
        show_name: String,
    },
    SeasonDetail {
        show_name: String,
        season_num: u32,
    },
    EpisodeDetail {
        media: MediaFile,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ViewMode {
    #[default]
    All,
    Movies,
    TvShows,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortBy {
    #[default]
    DateAdded,
    Title,
    Year,
    Rating,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortOrder {
    #[default]
    Descending,
    Ascending,
}

// Application state
#[derive(Debug)]
pub struct State {
    // Current view state
    pub view: ViewState,
    pub view_mode: ViewMode, // All, Movies, or TV Shows
    pub sort_by: SortBy,
    pub sort_order: SortOrder,

    // Media library
    pub library: MediaLibrary,
    pub server_url: String,

    // Player module state
    pub player: PlayerState,

    // UI state
    pub loading: bool,
    pub error_message: Option<String>,
    pub scanning: bool,

    // Scan progress tracking
    pub active_scan_id: Option<String>,
    pub scan_progress: Option<ScanProgress>,
    pub show_scan_progress: bool, // Show scan progress overlay

    // Image caches
    pub poster_cache: PosterCache,
    pub image_cache: ImageCache,

    // Metadata cache
    pub metadata_cache: MetadataCache,

    // Media organization
    pub movies: Vec<MediaFile>,
    pub tv_shows: HashMap<String, TvShow>,
    pub tv_shows_sorted: Vec<TvShow>,  // Pre-sorted TV shows for grid view
    pub expanded_shows: HashSet<String>,

    // Carousel states
    pub movies_carousel: CarouselState,
    pub tv_shows_carousel: CarouselState,

    // Window state
    pub window_size: iced::Size,

    // TV show detail data
    pub current_show_details: Option<TvShowDetails>,
    pub current_season_details: Option<SeasonDetails>,

    // Carousel states for detail views
    pub show_seasons_carousel: Option<CarouselState>,
    pub season_episodes_carousel: Option<CarouselState>,

    // Virtual grid states for lazy loading
    pub movies_grid_state: VirtualGridState,
    pub tv_shows_grid_state: VirtualGridState,

    // Track which posters are being loaded
    pub posters_to_load: VecDeque<String>,
    pub loading_posters: HashSet<String>,

    // Track how far we've marked posters for loading (continues beyond visible range)
    pub poster_mark_progress: usize,

    // Scroll throttling
    pub last_scroll_time: Option<Instant>,

    // Track animation states for fade-in effect
    pub poster_animation_states: HashMap<String, f32>, // media_id -> opacity
    pub poster_animation_types: HashMap<String, (crate::widgets::AnimationType, Instant)>, // media_id -> (animation type, start time)

    // Performance tracking
    pub poster_load_semaphore: Arc<tokio::sync::Semaphore>,

    // Poster monitoring service
    pub poster_monitor: Option<PosterMonitor>,
    
    // Scroll velocity tracking
    pub scroll_velocity: f32,
    pub last_scroll_position: f32,
    pub scroll_samples: VecDeque<(Instant, f32)>, // (timestamp, position) for velocity calculation
    pub fast_scrolling: bool,
    pub scroll_stopped_time: Option<Instant>, // When scrolling stopped for debouncing
    
    // Scroll position persistence
    pub movies_scroll_position: Option<f32>,    // Saved scroll position for movies view
    pub tv_shows_scroll_position: Option<f32>,  // Saved scroll position for TV shows view
    
    // Hover state
    pub hovered_media_id: Option<String>,
}

impl State {
    /// Save the current scroll position based on the view mode
    pub fn save_scroll_position(&mut self) {
        match self.view_mode {
            ViewMode::Movies => {
                self.movies_scroll_position = Some(self.last_scroll_position);
                log::debug!("Saved movies scroll position: {}", self.last_scroll_position);
            }
            ViewMode::TvShows => {
                self.tv_shows_scroll_position = Some(self.last_scroll_position);
                log::debug!("Saved TV shows scroll position: {}", self.last_scroll_position);
            }
            ViewMode::All => {
                // In All mode, save positions for both grids based on what's visible
                if !self.movies_grid_state.visible_range.is_empty() {
                    self.movies_scroll_position = Some(self.last_scroll_position);
                }
                if !self.tv_shows_grid_state.visible_range.is_empty() {
                    self.tv_shows_scroll_position = Some(self.last_scroll_position);
                }
            }
        }
    }
    
    /// Check if a media item is currently visible in either movies or TV shows grid
    pub fn is_media_visible(&self, media_id: &str) -> bool {
        // Check if it's a visible movie
        if let Some(index) = self.movies.iter().position(|m| m.id == media_id) {
            return self.movies_grid_state.visible_range.contains(&index);
        }

        // Check if it's a visible TV show
        for (idx, show) in self.tv_shows_sorted.iter().enumerate() {
            if let Some(poster_id) = show.get_poster_id() {
                if poster_id == media_id {
                    return self.tv_shows_grid_state.visible_range.contains(&idx);
                }
            }
        }

        // Check carousel visibility (first few items)
        match self.view_mode {
            ViewMode::Movies => {
                if let Some(index) = self.movies.iter().position(|m| m.id == media_id) {
                    return index < self.movies_carousel.items_per_page;
                }
            }
            ViewMode::TvShows => {
                for (idx, show) in self.tv_shows_sorted.iter().enumerate() {
                    if let Some(poster_id) = show.get_poster_id() {
                        if poster_id == media_id && idx < self.tv_shows_carousel.items_per_page {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }

        false
    }

    /// Mark visible posters for loading, respecting the concurrent loading limit
    /// Only marks as many posters as there are available loading slots
    pub fn mark_visible_posters_for_loading(&mut self) -> Vec<String> {
        PROFILER.start("mark_visible_posters");
        const MAX_CONCURRENT_LOADS: usize = 3;
        const PRELOAD_AHEAD_ROWS: usize = 2; // Number of rows to preload ahead
        const PRELOAD_BELOW_ROWS: usize = 5; // Number of rows to preload below visible area

        // Calculate available slots
        let current_loading = self.loading_posters.len();
        let available_slots = MAX_CONCURRENT_LOADS.saturating_sub(current_loading);

        if available_slots == 0 {
            PROFILER.end("mark_visible_posters");
            return Vec::new();
        }

        let mut posters_to_mark = Vec::new();

        // Check movies in visible range first
        match self.view_mode {
            ViewMode::All | ViewMode::Movies => {
                for index in self.movies_grid_state.visible_range.clone() {
                    if posters_to_mark.len() >= available_slots {
                        break;
                    }

                    if let Some(movie) = self.movies.get(index) {
                        // Only mark if not already loading or loaded
                        if !self.loading_posters.contains(&movie.id)
                            && self.poster_cache.get(&movie.id).is_none()
                        {
                            posters_to_mark.push(movie.id.clone());
                        }
                    }
                }

                // If we still have slots and no visible items need loading, preload ahead
                if posters_to_mark.len() < available_slots {
                    let preload_range =
                        self.movies_grid_state.get_preload_range(PRELOAD_AHEAD_ROWS);
                    for index in preload_range {
                        if posters_to_mark.len() >= available_slots {
                            break;
                        }

                        if let Some(movie) = self.movies.get(index) {
                            if !self.loading_posters.contains(&movie.id)
                                && self.poster_cache.get(&movie.id).is_none()
                            {
                                posters_to_mark.push(movie.id.clone());
                            }
                        }
                    }
                }

                // If we still have slots after visible and preload ahead, load items below current position
                if posters_to_mark.len() < available_slots
                    && !self.movies_grid_state.visible_range.is_empty()
                {
                    let visible_end = self.movies_grid_state.visible_range.end;
                    let items_per_row = self.movies_grid_state.columns;
                    let below_end =
                        (visible_end + (PRELOAD_BELOW_ROWS * items_per_row)).min(self.movies.len());

                    for index in visible_end..below_end {
                        if posters_to_mark.len() >= available_slots {
                            break;
                        }

                        if let Some(movie) = self.movies.get(index) {
                            if !self.loading_posters.contains(&movie.id)
                                && self.poster_cache.get(&movie.id).is_none()
                            {
                                posters_to_mark.push(movie.id.clone());
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // Check TV shows in visible range if we still have slots
        if posters_to_mark.len() < available_slots {
            match self.view_mode {
                ViewMode::All | ViewMode::TvShows => {
                    for index in self.tv_shows_grid_state.visible_range.clone() {
                        if posters_to_mark.len() >= available_slots {
                            break;
                        }

                        if let Some(show) = self.tv_shows_sorted.get(index) {
                            if let Some(poster_id) = show.get_poster_id() {
                                // Only mark if not already loading or loaded
                                if !self.loading_posters.contains(&poster_id)
                                    && self.poster_cache.get(&poster_id).is_none()
                                {
                                    posters_to_mark.push(poster_id);
                                }
                            }
                        }
                    }

                    // If we still have slots and no visible shows need loading, preload ahead
                    if posters_to_mark.len() < available_slots {
                        let preload_range = self
                            .tv_shows_grid_state
                            .get_preload_range(PRELOAD_AHEAD_ROWS);
                        for index in preload_range {
                            if posters_to_mark.len() >= available_slots {
                                break;
                            }

                            if let Some(show) = self.tv_shows_sorted.get(index) {
                                if let Some(poster_id) = show.get_poster_id() {
                                    if !self.loading_posters.contains(&poster_id)
                                        && self.poster_cache.get(&poster_id).is_none()
                                    {
                                        posters_to_mark.push(poster_id);
                                    }
                                }
                            }
                        }
                    }

                    // If we still have slots after visible and preload ahead, load shows below current position
                    if posters_to_mark.len() < available_slots
                        && !self.tv_shows_grid_state.visible_range.is_empty()
                    {
                        let visible_end = self.tv_shows_grid_state.visible_range.end;
                        let items_per_row = self.tv_shows_grid_state.columns;
                        let below_end = (visible_end + (PRELOAD_BELOW_ROWS * items_per_row))
                            .min(self.tv_shows_sorted.len());

                        for index in visible_end..below_end {
                            if posters_to_mark.len() >= available_slots {
                                break;
                            }

                            if let Some(show) = self.tv_shows_sorted.get(index) {
                                if let Some(poster_id) = show.get_poster_id() {
                                    if !self.loading_posters.contains(&poster_id)
                                        && self.poster_cache.get(&poster_id).is_none()
                                    {
                                        posters_to_mark.push(poster_id);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Mark the selected posters as loading in the cache
        for poster_id in &posters_to_mark {
            self.poster_cache.set_loading(poster_id.clone());
        }

        PROFILER.end("mark_visible_posters");
        posters_to_mark
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            view: ViewState::default(),
            view_mode: ViewMode::default(),
            sort_by: SortBy::default(),
            sort_order: SortOrder::default(),
            library: MediaLibrary::new(),
            server_url: String::new(),
            player: PlayerState::default(),
            loading: false,
            error_message: None,
            scanning: false,
            active_scan_id: None,
            scan_progress: None,
            show_scan_progress: false,
            poster_cache: PosterCache::new(),
            image_cache: ImageCache::new(),
            metadata_cache: MetadataCache::new(60), // 60 minute TTL
            movies: Vec::new(),
            tv_shows: HashMap::new(),
            tv_shows_sorted: Vec::new(),
            expanded_shows: HashSet::new(),
            movies_carousel: CarouselState::new(0),
            tv_shows_carousel: CarouselState::new(0),
            window_size: iced::Size::new(1280.0, 720.0), // Default window size
            current_show_details: None,
            current_season_details: None,
            show_seasons_carousel: None,
            season_episodes_carousel: None,
            movies_grid_state: VirtualGridState::new(0, 5, 350.0),
            tv_shows_grid_state: VirtualGridState::new(0, 5, 350.0),
            posters_to_load: VecDeque::new(),
            loading_posters: HashSet::new(),
            poster_animation_states: HashMap::new(),
            poster_animation_types: HashMap::new(),
            poster_load_semaphore: Arc::new(tokio::sync::Semaphore::new(8)), // Max 8 concurrent poster loads
            poster_monitor: None,
            poster_mark_progress: 0,
            last_scroll_time: None,
            scroll_velocity: 0.0,
            last_scroll_position: 0.0,
            scroll_samples: VecDeque::with_capacity(5), // Keep last 5 samples for smoothing
            fast_scrolling: false,
            scroll_stopped_time: None,
            movies_scroll_position: None,
            tv_shows_scroll_position: None,
            hovered_media_id: None,
        }
    }
}
