use std::{sync::Arc, time::Duration};

use ferrex_player::{
    domains::ui::views::grid::VirtualGridState,
    infrastructure::{
        api_types::{LibraryMediaCache, MovieReference, SeriesReference},
        service_registry,
    },
    state_refactored::State,
};
use tokio::runtime::Runtime;

use crate::{
    init::init_bench::{self, benchmark_full_initialization, full_initialization_operation},
    INIT, INITIALIZED_STATE,
};

use super::auth::setup_benchmark_authentication;

/// Statistics for application initialization
#[derive(Debug)]
pub struct InitializationStats {
    pub libraries_loaded: usize,
    pub media_items_loaded: usize,
    pub metadata_fetched: usize,
    pub posters_loaded: usize,
    pub total_time: Duration,
}

/// Shared state populated by initialization benchmark and used by all other benchmarks
/// This ensures we're testing with real server data instead of synthetic test data
#[derive(Debug)]
pub struct BenchmarkGlobalState {
    /// Fully initialized application state with real server data
    pub state: Arc<State>,
    /// Real movie references loaded from the server
    pub real_movies: Vec<MovieReference>,
    /// Real series references loaded from the server
    pub real_series: Vec<SeriesReference>,
    /// Statistics about what was loaded during initialization
    pub initialization_stats: InitializationStats,
}

/// Get or initialize the global benchmark state
/// This ensures all benchmarks use the same real server data for consistency
pub fn get_or_initialize_global_state() -> Arc<BenchmarkGlobalState> {
    INIT.call_once(|| {
        log::info!("🚀 Initializing global benchmark state with real server data...");

        let rt = Runtime::new().unwrap();
        let global_state = rt.block_on(async {
            // Run the full initialization to populate real data
            match full_initialization_operation("global_initialization").await {
                Ok((state, stats)) => {
                    // Extract real media from the initialized state
                    let mut real_movies = Vec::new();
                    let mut real_series = Vec::new();

                    // Extract from library caches
                    for (_, cache) in &state.domains.library.state.library_media_cache {
                        match cache {
                            LibraryMediaCache::Movies { references } => {
                                real_movies.extend(references.clone());
                            }
                            LibraryMediaCache::TvShows {
                                series_references_sorted,
                                ..
                            } => {
                                real_series.extend(series_references_sorted.clone());
                            }
                        }
                    }

                    log::info!(
                        "✅ Global state initialized with {} movies, {} series",
                        real_movies.len(),
                        real_series.len()
                    );

                    BenchmarkGlobalState {
                        state: Arc::new(state),
                        real_movies,
                        real_series,
                        initialization_stats: stats,
                    }
                }
                Err(e) => {
                    log::error!("❌ Failed to initialize global state: {}", e);
                    log::warn!("🔄 Falling back to minimal state for benchmarks");

                    // Fallback to minimal state
                    let state = State::new("http://localhost:3000".to_string());
                    service_registry::init_registry(state.image_service.clone());

                    BenchmarkGlobalState {
                        state: Arc::new(state),
                        real_movies: Vec::new(),
                        real_series: Vec::new(),
                        initialization_stats: InitializationStats {
                            libraries_loaded: 0,
                            media_items_loaded: 0,
                            metadata_fetched: 0,
                            posters_loaded: 0,
                            total_time: Duration::from_secs(0),
                        },
                    }
                }
            }
        });

        *INITIALIZED_STATE.lock().unwrap() = Some(Arc::new(global_state));
    });

    INITIALIZED_STATE.lock().unwrap().as_ref().unwrap().clone()
}

pub fn create_minimal_real_state() -> Arc<State> {
    // Use the global initialized state instead of creating a new one
    let global_state = get_or_initialize_global_state();

    // Note: We can't clone State directly, so we'll create a new one
    // but with the same configuration. The real magic is that the
    // global initialization has already populated the server with data
    // and established authentication, so subsequent calls will be faster
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let state = State::new("http://localhost:3000".to_string());

        // Set up authentication using the same method as global initialization
        if let Err(e) = setup_benchmark_authentication(&state).await {
            log::warn!("Authentication setup failed: {}", e);
        }

        service_registry::init_registry(state.image_service.clone());
        state
    });
    Arc::clone(&global_state.state)
}

pub fn create_real_grid_state(
    total_items: usize,
    columns: usize,
    scroll_position: f32,
) -> VirtualGridState {
    let mut grid_state = VirtualGridState::new(total_items, columns, 300.0); // 300px row height
    grid_state.scroll_position = scroll_position;
    grid_state.viewport_height = 1080.0;
    grid_state.viewport_width = 1920.0;
    grid_state.calculate_visible_range();
    grid_state
}
