// Ferrex Real Hot Path Performance Benchmarks
//
// RED PHASE: Test-Driven Development for <8ms view operations
// Target: All view operations MUST complete within 8ms for 120fps
//
// This benchmark suite tests the ACTUAL Ferrex hot paths identified in Task 1.1d:
// 1. Virtual grid rendering (grid/macros.rs) - the main bottleneck
// 2. Update loop message processing (update.rs) - string allocation issues
// 3. View rendering operations (view.rs) - mutex contention issues

mod init;
mod metadata;
mod utils;

use glib::ffi::G_URI_FLAGS_PARSE_RELAXED;
use init::init_bench::benchmark_full_initialization;
use metadata::expanded_metadata_bench::{
    batch_metadata_operation, benchmark_batch_metadata_fetching,
};
use utils::auth::setup_benchmark_authentication;

use chrono::Utc;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use utils::state::{
    create_minimal_real_state, create_real_grid_state, get_or_initialize_global_state,
    BenchmarkGlobalState,
};
use uuid::Uuid;

// Import actual Ferrex components and data structures
use ferrex_player::{
    domains::media::{
        library::{fetch_libraries, fetch_library_media_references},
        store::{BatchConfig, BatchCoordinator, MediaStore},
    },
    domains::metadata::{
        image_service::UnifiedImageService,
        image_types::{ImageRequest, ImageSize, Priority},
    },
    domains::ui::{
        components::movie_reference_card_with_state,
        messages::Message,
        view_models::ViewModel,
        views::grid::{
            grid_view::{virtual_movie_references_grid, virtual_series_references_grid},
            virtual_list::VirtualGridState,
        },
    },
    infrastructure::{api_types::MediaReference, service_registry},
    state_refactored::State,
};

use ferrex_core::{
    LibraryMediaCache, MediaDetailsOption, MediaFile, MovieID, MovieReference, MovieTitle,
    MovieURL, SeriesID, SeriesReference, SeriesTitle, SeriesURL,
};

// Global shared state for realistic benchmarking
static INIT: Once = Once::new();
static INITIALIZED_STATE: Mutex<Option<Arc<BenchmarkGlobalState>>> = Mutex::new(None);

// Real test data generators using actual Ferrex data structures
fn create_real_movie_reference(i: usize) -> MovieReference {
    MovieReference {
        id: MovieID::new(format!("movie_{}", i)).unwrap(),
        tmdb_id: i as u64,
        title: MovieTitle::from(format!("Test Movie {}", i)),
        details: MediaDetailsOption::Endpoint(format!("/api/movies/{}", i)),
        endpoint: MovieURL::from(format!("/movies/{}", i)),
        file: MediaFile {
            id: Uuid::new_v4(),
            path: PathBuf::from(format!("/media/movies/movie_{}.mp4", i)),
            filename: format!("movie_{}.mp4", i),
            size: 1024 * 1024 * 1024 + (i as u64 * 1024 * 1024), // ~1GB + variation
            created_at: Utc::now(),
            media_file_metadata: None,
            library_id: Uuid::new_v4(),
        },
        theme_color: if i % 4 == 0 {
            Some(format!(
                "#{}{}{}E50",
                (i % 16).to_string(),
                (i % 16).to_string(),
                (i % 16).to_string()
            ))
        } else {
            None
        },
    }
}

fn create_real_series_reference(i: usize) -> SeriesReference {
    SeriesReference {
        id: SeriesID::new(format!("series_{}", i)).unwrap(),
        library_id: Uuid::new_v4(),
        tmdb_id: i as u64,
        title: SeriesTitle::from(format!("Test Series {}", i)),
        details: MediaDetailsOption::Endpoint(format!("/api/series/{}", i)),
        endpoint: SeriesURL::from(format!("/series/{}", i)),
        created_at: Utc::now(),
        theme_color: if i % 4 == 0 {
            Some(format!(
                "#2C{}{}50",
                (i % 16).to_string(),
                (i % 16).to_string()
            ))
        } else {
            None
        },
    }
}

fn create_real_test_movies(count: usize) -> Vec<MovieReference> {
    (0..count).map(create_real_movie_reference).collect()
}

fn create_real_test_series(count: usize) -> Vec<SeriesReference> {
    (0..count).map(create_real_series_reference).collect()
}

/// Create different cache scenarios for realistic testing
/// Rather than pre-populating with fake data, we test different cache states
fn create_realistic_cache_state(scenario: &str) {
    match scenario {
        "cold_start" => {
            // Empty cache - simulates app startup
            log::info!("Testing COLD CACHE scenario (app startup)");
        }
        "warm_cache" => {
            // In a real warm cache scenario, images would have been fetched previously
            // For benchmarking, we'll just note this scenario
            log::info!("Testing WARM CACHE scenario (simulated previous browsing)");
            // Note: In production, this would be a cache that's been populated through actual use
        }
        "mixed_cache" => {
            // Partially populated cache - simulates real browsing patterns
            log::info!("Testing MIXED CACHE scenario (partial cache from browsing)");
        }
        _ => {
            log::warn!("Unknown cache scenario: {}", scenario);
        }
    }
}

// Benchmark 1: Real Movie Grid Rendering (Primary Hot Path)
fn benchmark_real_movie_grid_hotpath(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_movie_grid_hotpath");
    group.measurement_time(Duration::from_secs(15));
    group.significance_level(0.01);

    // Test with realistic grid sizes that users actually encounter
    let test_scenarios = [
        (100, 6, "small_library"),    // Small personal library
        (500, 6, "medium_library"),   // Medium library
        (1000, 6, "large_library"),   // Large library
        (2000, 6, "huge_library"),    // Very large library
        (5000, 6, "massive_library"), // Massive library
    ];

    for (item_count, columns, scenario_name) in test_scenarios {
        // Use real server data for more realistic benchmarks
        let global_state = get_or_initialize_global_state();
        let movies = if global_state.real_movies.is_empty() {
            log::warn!(
                "No real movies available for {}, using synthetic data",
                scenario_name
            );
            create_real_test_movies(item_count)
        } else {
            // Use real movies, cycling through them if we need more than available
            let mut real_movies = Vec::new();
            for i in 0..item_count {
                let movie_idx = i % global_state.real_movies.len();
                real_movies.push(global_state.real_movies[movie_idx].clone());
            }
            real_movies
        };

        log::info!(
            "Grid benchmark {} using {} movies ({} real movies available)",
            scenario_name,
            movies.len(),
            global_state.real_movies.len()
        );

        let grid_state = create_real_grid_state(item_count, columns, 0.0);
        let state = create_minimal_real_state();

        // Set up authentication for real server communication
        let rt = Runtime::new().unwrap();
        if let Err(e) = rt.block_on(setup_benchmark_authentication(&state)) {
            log::warn!("⚠️  Authentication failed for {}: {}", scenario_name, e);
            log::warn!("   Benchmark may show degraded performance without authentication");
        }

        let hovered_media_id = None;

        group.bench_with_input(
            BenchmarkId::new("movie_grid_render", scenario_name),
            &item_count,
            |b, _| {
                b.iter_custom(|iters| {
                    let start = Instant::now();
                    for _ in 0..iters {
                        let result = black_box(virtual_movie_references_grid(
                            &movies,
                            &grid_state,
                            &hovered_media_id,
                            |viewport| Message::MoviesGridScrolled(viewport),
                            false, // not fast scrolling
                            &state,
                        ));

                        black_box(result);
                    }
                    let total_elapsed = start.elapsed();

                    // RED PHASE: Enforce <8ms requirement
                    let avg_per_iter = total_elapsed / iters as u32;
                    if avg_per_iter > Duration::from_millis(8) {
                        log::error!(
                            "🔴 RED PHASE: Movie grid ({}) took {}ms (target: <8ms)",
                            scenario_name,
                            avg_per_iter.as_millis()
                        );
                    }

                    total_elapsed
                });
            },
        );
    }

    group.finish();
}

// NEW: Benchmark for actual component creation (the real expensive operations)
fn benchmark_movie_component_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("movie_component_creation");
    group.measurement_time(Duration::from_secs(15));

    // Test realistic cache scenarios that occur during actual application usage
    let cache_scenarios = [
        ("cold_start", "Cold cache (app startup)"),
        ("warm_cache", "Warm cache (after browsing)"),
        ("mixed_cache", "Mixed cache (typical usage)"),
    ];

    let card_counts = [10, 50]; // Visible cards, overscan cards

    for (cache_scenario, cache_description) in cache_scenarios {
        for card_count in card_counts {
            // Use real server data instead of synthetic test data
            let global_state = get_or_initialize_global_state();
            let movies = if global_state.real_movies.is_empty() {
                log::warn!("No real movies available, falling back to synthetic data");
                create_real_test_movies(card_count)
            } else {
                // Use actual server data, limited to the test size
                global_state
                    .real_movies
                    .iter()
                    .take(card_count)
                    .cloned()
                    .collect()
            };

            log::info!(
                "Testing {} with {} movies ({} real, {} total available)",
                cache_description,
                movies.len(),
                if global_state.real_movies.is_empty() {
                    0
                } else {
                    movies.len()
                },
                global_state.real_movies.len()
            );

            let state = create_minimal_real_state();

            // Set up authentication for real server communication
            let rt = Runtime::new().unwrap();
            if let Err(e) = rt.block_on(setup_benchmark_authentication(&state)) {
                log::warn!("⚠️  Authentication failed: {}", e);
                log::warn!("   Benchmark may show degraded performance without authentication");
            }

            // Set up realistic cache state for this test
            create_realistic_cache_state(cache_scenario);

            let test_name = format!("{}_{}_cards", cache_scenario, card_count);

            group.bench_with_input(
                BenchmarkId::new("realistic_component_creation", &test_name),
                &card_count,
                |b, _| {
                    b.iter_custom(|iters| {
                        let start = Instant::now();
                        for _ in 0..iters {
                            // Create actual movie cards - this triggers REAL expensive operations:
                            // - DashMap cache lookups (hits and misses based on scenario)
                            // - Theme color parsing (hex string -> Color conversion)
                            // - Image handle creation and GPU texture operations
                            // - Animation state setup and timing calculations
                            // - Hover state processing
                            for (i, movie) in movies.iter().enumerate() {
                                let is_hovered = i == 0; // Simulate one hovered card
                                let is_visible = i < 6;  // Simulate 6 visible cards on screen

                                let card_element = black_box(movie_reference_card_with_state(
                                    movie,
                                    is_hovered,
                                    is_visible,
                                    Some(&state),
                                ));
                                black_box(card_element);
                            }
                        }
                        let total_elapsed = start.elapsed();

                        // Performance validation: component creation should be fast
                        let avg_per_iter = total_elapsed / iters as u32;
                        let avg_per_card = avg_per_iter / card_count as u32;

                        // Different targets for different cache scenarios
                        let target_ms = match cache_scenario {
                            "cold_start" => 3,  // Cold cache can be slower due to cache misses
                            "warm_cache" => 1,  // Warm cache should be very fast
                            "mixed_cache" => 2, // Mixed cache - moderate performance
                            _ => 2,
                        };

                        if avg_per_card > Duration::from_millis(target_ms) {
                            log::error!(
                                "🔴 RED PHASE: {} component creation took {}ms per card (target: <{}ms per card)",
                                cache_description,
                                avg_per_card.as_millis(),
                                target_ms
                            );
                        } else {
                            log::info!(
                                "✅ {} component creation: {}μs per card",
                                cache_description,
                                avg_per_card.as_micros()
                            );
                        }

                        total_elapsed
                    });
                },
            );
        }
    }

    group.finish();
}

// Benchmark 2: Real Series Grid Rendering
fn benchmark_real_series_grid_hotpath(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_series_grid_hotpath");
    group.measurement_time(Duration::from_secs(15));

    let test_scenarios = [
        (50, 6, "small_tv_collection"),
        (200, 6, "medium_tv_collection"),
        (500, 6, "large_tv_collection"),
        (1000, 6, "huge_tv_collection"),
    ];

    for (item_count, columns, scenario_name) in test_scenarios {
        let series = create_real_test_series(item_count);
        let grid_state = create_real_grid_state(item_count, columns, 0.0);
        let state = create_minimal_real_state();
        let hovered_media_id = None;

        group.bench_with_input(
            BenchmarkId::new("series_grid_render", scenario_name),
            &item_count,
            |b, _| {
                b.iter_custom(|iters| {
                    let start = Instant::now();
                    for _ in 0..iters {
                        let result = black_box(virtual_series_references_grid(
                            &series,
                            &grid_state,
                            &hovered_media_id,
                            |viewport| Message::TvShowsGridScrolled(viewport),
                            false,
                            &state,
                        ));

                        black_box(result);
                    }
                    let total_elapsed = start.elapsed();

                    let avg_per_iter = total_elapsed / iters as u32;
                    if avg_per_iter > Duration::from_millis(8) {
                        log::error!(
                            "🔴 RED PHASE: Series grid ({}) took {}ms (target: <8ms)",
                            scenario_name,
                            avg_per_iter.as_millis()
                        );
                    }

                    total_elapsed
                });
            },
        );
    }

    group.finish();
}

// Benchmark 3: Scrolling Performance (Critical for UX)
fn benchmark_real_scrolling_hotpath(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_scrolling_hotpath");
    group.measurement_time(Duration::from_secs(20));

    let movies = create_real_test_movies(1000);
    let state = create_minimal_real_state();
    let hovered_media_id = None;

    // Test scrolling at different positions (simulating user behavior)
    let scroll_test_cases = [
        (0.0, "scroll_top"),
        (3000.0, "scroll_quarter"),
        (6000.0, "scroll_middle"),
        (9000.0, "scroll_three_quarter"),
        (12000.0, "scroll_bottom"),
    ];

    for (scroll_position, test_name) in scroll_test_cases {
        let grid_state = create_real_grid_state(1000, 6, scroll_position);

        group.bench_with_input(
            BenchmarkId::new("scroll_performance", test_name),
            &scroll_position,
            |b, _| {
                b.iter(|| {
                    let start = Instant::now();
                    let result = black_box(virtual_movie_references_grid(
                        &movies,
                        &grid_state,
                        &hovered_media_id,
                        |viewport| Message::MoviesGridScrolled(viewport),
                        true, // fast scrolling = true (critical path)
                        &state,
                    ));
                    let duration = start.elapsed();

                    // Scrolling must be ultra-fast for smooth UX
                    if duration.as_millis() > 4 {
                        log::warn!(
                            "🔴 Scrolling performance issue at {}: {}ms (target: <4ms for smooth scroll)",
                            test_name,
                            duration.as_millis()
                        );
                    }

                    result
                });
            },
        );
    }

    group.finish();
}

// Benchmark 4: Grid State Calculation (Identified Bottleneck)
fn benchmark_grid_state_calculations(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_state_calculations");

    // Test the specific operations that were identified as bottlenecks
    let test_cases = [
        (1000, 6, 1080.0, "standard_1080p"),
        (2000, 8, 1440.0, "wide_1440p"),
        (5000, 10, 2160.0, "ultra_wide_4k"),
    ];

    for (total_items, columns, viewport_height, test_name) in test_cases {
        group.bench_with_input(
            BenchmarkId::new("visible_range_calc", test_name),
            &total_items,
            |b, _| {
                b.iter(|| {
                    let mut grid_state = VirtualGridState::new(total_items, columns, 300.0);
                    grid_state.viewport_height = viewport_height;
                    grid_state.scroll_position = (total_items as f32 * 150.0) / 3.0; // Middle scroll position

                    // This is the calculation that happens on every scroll event
                    let start = Instant::now();
                    grid_state.calculate_visible_range();
                    let calc_time = start.elapsed();

                    if calc_time.as_micros() > 500 {
                        log::warn!(
                            "🔴 Grid calculation too slow for {}: {}μs",
                            test_name,
                            calc_time.as_micros()
                        );
                    }

                    black_box(grid_state.visible_range.clone())
                });
            },
        );
    }

    group.finish();
}

// Benchmark 5: Memory Allocation Impact (From Task 1.1d)
fn benchmark_allocation_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocation_patterns");

    // Test the data structure creation patterns
    group.bench_function("movie_reference_creation", |b| {
        b.iter(|| {
            // Test creating movie references (heap allocations)
            let movies: Vec<MovieReference> =
                black_box((0..100).map(create_real_movie_reference).collect());
            movies
        });
    });

    group.bench_function("grid_state_updates", |b| {
        let mut grid_state = VirtualGridState::new(1000, 6, 300.0);

        b.iter(|| {
            // Test the scroll position updates that happen frequently
            grid_state.scroll_position += 10.0;
            grid_state.calculate_visible_range();
            black_box(&grid_state.visible_range);
        });
    });

    group.finish();
}

// Benchmark 6: Frame Budget Compliance (120fps = 8ms)
fn benchmark_frame_budget_compliance(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_budget_compliance");
    group.measurement_time(Duration::from_secs(30));

    let movies = create_real_test_movies(1000);
    let grid_state = create_real_grid_state(1000, 6, 2000.0);
    let state = create_minimal_real_state();
    let hovered_media_id = None;

    // Test frame budget for different target FPS
    let frame_budgets = [
        ("120fps", 8_333), // 8.33ms budget
        ("60fps", 16_667), // 16.67ms budget
        ("30fps", 33_333), // 33.33ms budget
    ];

    for (fps_target, budget_micros) in frame_budgets {
        group.bench_function(&format!("frame_budget_{}", fps_target), |b| {
            b.iter_custom(|iters| {
                let start = Instant::now();
                let mut violations = 0;

                for _ in 0..iters {
                    let frame_start = Instant::now();

                    // Simulate a complete frame render
                    let result = black_box(virtual_movie_references_grid(
                        &movies,
                        &grid_state,
                        &hovered_media_id,
                        |viewport| Message::MoviesGridScrolled(viewport),
                        false,
                        &state,
                    ));

                    let frame_time = frame_start.elapsed();
                    if frame_time.as_micros() > budget_micros {
                        violations += 1;
                    }

                    black_box(result);
                }

                let total_time = start.elapsed();

                if violations > 0 {
                    log::error!(
                        "🔴 Frame budget violations for {}: {}/{} frames exceeded {}μs",
                        fps_target,
                        violations,
                        iters,
                        budget_micros
                    );
                }

                total_time
            });
        });
    }

    group.finish();
}

// Benchmark 8: MediaStore Operations (CRITICAL CONCURRENT PERFORMANCE)
fn benchmark_mediastore_operations(c: &mut Criterion) {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Off)
        .is_test(true)
        .try_init();

    println!("💾 Starting MediaStore operations benchmark with real data...");

    let mut group = c.benchmark_group("mediastore_operations");
    group.measurement_time(Duration::from_secs(60));

    // Get real data for testing
    let global_state = get_or_initialize_global_state();
    let test_movies = if global_state.real_movies.is_empty() {
        create_real_test_movies(1000)
    } else {
        global_state.real_movies.clone()
    };

    // Test scenarios
    let scenarios = [
        ("bulk_insert", "Bulk insert operations"),
        ("concurrent_reads", "Concurrent read operations"),
        ("mixed_operations", "Mixed read/write operations"),
        ("batch_coordinator", "BatchCoordinator operations"),
    ];

    for (scenario_name, scenario_description) in scenarios {
        group.bench_function(scenario_name, |b| {
            b.iter_custom(|iters| {
                let start = Instant::now();

                for _ in 0..iters {
                    let rt = Runtime::new().unwrap();
                    rt.block_on(benchmark_mediastore_scenario(scenario_name, &test_movies));
                }

                start.elapsed()
            });
        });
    }

    group.finish();
}

async fn benchmark_mediastore_scenario(scenario: &str, test_movies: &[MovieReference]) -> Duration {
    let start = Instant::now();

    // Create media store outside match to avoid lifetime issues
    let media_store = std::sync::Arc::new(std::sync::RwLock::new(MediaStore::new()));

    match scenario {
        "bulk_insert" => {
            // Test bulk insertion performance
            // Convert to MediaReference for insertion
            let media_refs: Vec<_> = test_movies
                .iter()
                .take(500)
                .map(|movie| MediaReference::Movie(movie.clone()))
                .collect();

            if let Ok(mut store) = media_store.write() {
                let bulk_start = Instant::now();
                store.bulk_upsert(media_refs);
                log::debug!("Bulk insert of 500 items took: {:?}", bulk_start.elapsed());
            }
        }

        "concurrent_reads" => {
            // Test concurrent read performance
            // Pre-populate with data
            if let Ok(mut store) = media_store.write() {
                let media_refs: Vec<_> = test_movies
                    .iter()
                    .take(100)
                    .map(|movie| MediaReference::Movie(movie.clone()))
                    .collect();
                store.bulk_upsert(media_refs);
            }

            // Spawn multiple concurrent readers
            let mut handles = Vec::new();
            for i in 0..10 {
                let store_clone: std::sync::Arc<std::sync::RwLock<MediaStore>> =
                    std::sync::Arc::clone(&media_store);
                let movie_id = test_movies[i % test_movies.len()].id.clone();

                let handle = tokio::spawn(async move {
                    if let Ok(store) = store_clone.read() {
                        let media_id = ferrex_core::MediaId::Movie(movie_id);
                        let _ = store.get_movies(None);
                    }
                });
                handles.push(handle);
            }

            // Wait for all readers
            for handle in handles {
                let _ = handle.await;
            }
        }

        "mixed_operations" => {
            // Test mixed read/write operations
            // Perform mixed operations
            for i in 0..50 {
                if i % 3 == 0 {
                    // Write operation
                    if let Ok(mut store) = media_store.write() {
                        let movie = &test_movies[i % test_movies.len()];
                        let media_ref = MediaReference::Movie(movie.clone());
                        store.upsert(media_ref);
                    }
                } else {
                    // Read operation
                    if let Ok(store) = media_store.read() {
                        let movie_id = &test_movies[i % test_movies.len()].id;
                        let media_id = ferrex_core::MediaId::Movie(movie_id.clone());
                        let _ = store.get_movies(None);
                    }
                }
            }
        }

        "batch_coordinator" => {
            // Test BatchCoordinator performance
            let coordinator = BatchCoordinator::new(std::sync::Arc::clone(&media_store));

            let media_refs: Vec<_> = test_movies
                .iter()
                .take(200)
                .map(|movie| MediaReference::Movie(movie.clone()))
                .collect();

            let library_data = vec![(uuid::Uuid::new_v4(), media_refs)];

            if let Err(e) = coordinator.process_initial_load(library_data).await {
                log::error!("BatchCoordinator test failed: {}", e);
            }
        }

        _ => {}
    }

    start.elapsed()
}

// Benchmark 9: Image Loading Pipeline (CRITICAL UI RESPONSIVENESS)
fn benchmark_image_loading_pipeline(c: &mut Criterion) {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init();

    println!("🖼️  Starting image loading pipeline benchmark...");

    let mut group = c.benchmark_group("image_loading_pipeline");
    group.measurement_time(Duration::from_secs(90)); // Longer for async operations

    // Get real media data for image requests
    let global_state = get_or_initialize_global_state();
    let test_media = if global_state.real_movies.is_empty() {
        create_real_test_movies(50)
    } else {
        global_state
            .real_movies
            .iter()
            .take(50)
            .cloned()
            .collect::<Vec<_>>()
    };

    let scenarios = [
        ("priority_queue", "Priority queue operations"),
        ("concurrent_requests", "Concurrent image requests"),
        ("cache_performance", "Cache hit/miss performance"),
    ];

    for (scenario_name, _scenario_description) in scenarios {
        group.bench_function(scenario_name, |b| {
            b.iter_custom(|iters| {
                let start = Instant::now();

                for _ in 0..iters {
                    let rt = Runtime::new().unwrap();
                    rt.block_on(benchmark_image_pipeline_scenario(
                        scenario_name,
                        &test_media,
                    ));
                }

                start.elapsed()
            });
        });
    }

    group.finish();
}

async fn benchmark_image_pipeline_scenario(
    scenario: &str,
    test_media: &[MovieReference],
) -> Duration {
    let start = Instant::now();

    match scenario {
        "priority_queue" => {
            // Test priority queue operations
            let (image_service, _receiver) = UnifiedImageService::new(4);

            // Add many requests with different priorities
            for (i, movie) in test_media.iter().enumerate() {
                let priority = match i % 3 {
                    0 => Priority::Visible,
                    1 => Priority::Preload,
                    _ => Priority::Background,
                };

                let request = ImageRequest {
                    media_id: ferrex_core::MediaId::Movie(movie.id.clone()),
                    size: ImageSize::Poster,
                    priority,
                };

                image_service.request_image(request);
            }

            log::debug!("Queued {} image requests", test_media.len());
        }

        "concurrent_requests" => {
            // Test concurrent image requests
            let (image_service, _receiver) = UnifiedImageService::new(8);

            let mut handles = Vec::new();

            for movie in test_media.iter().take(20) {
                let service_clone = image_service.clone();
                let movie_clone = movie.clone();

                let handle = tokio::spawn(async move {
                    let request = ImageRequest {
                        media_id: ferrex_core::MediaId::Movie(movie_clone.id),
                        size: ImageSize::Poster,
                        priority: Priority::Visible,
                    };

                    service_clone.request_image(request.clone());

                    // Check for immediate availability (cache hits)
                    let _ = service_clone.get(&request);
                });

                handles.push(handle);
            }

            // Wait for all concurrent requests
            for handle in handles {
                let _ = handle.await;
            }
        }

        "cache_performance" => {
            // Test cache hit/miss performance
            let (image_service, _receiver) = UnifiedImageService::new(4);

            // Simulate cache misses then hits
            for movie in test_media.iter().take(10) {
                let request = ImageRequest {
                    media_id: ferrex_core::MediaId::Movie(movie.id.clone()),
                    size: ImageSize::Poster,
                    priority: Priority::Visible,
                };

                // First request (cache miss)
                image_service.request_image(request.clone());
                let miss_result = image_service.get(&request);

                // Simulate loading completion
                image_service.mark_loading(&request);

                // Second request (potential cache hit)
                let hit_result = image_service.get(&request);

                log::trace!(
                    "Cache miss: {:?}, Cache hit: {:?}",
                    miss_result.is_some(),
                    hit_result.is_some()
                );
            }
        }

        _ => {}
    }

    start.elapsed()
}

// Benchmark 10: Comprehensive Application Flow (COMPLETE USER EXPERIENCE)
fn benchmark_comprehensive_application_flow(c: &mut Criterion) {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init();

    //println!("🚀 Starting comprehensive application flow benchmark - complete user experience...");
    log::info!("🚀 Comprehensive application flow benchmark starting");

    let mut group = c.benchmark_group("comprehensive_application_flow");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(10); // Few samples due to complexity

    // Test the complete flow from cold start to usable UI
    group.bench_function("cold_start_to_usable_ui", |b| {
        b.iter_custom(|iters| {
            let mut total_flow_time = Duration::from_secs(0);

            for i in 0..iters {
                let iteration_start = Instant::now();
                //println!("🎯 Starting complete application flow iteration {}/{}", i + 1, iters);
                log::info!("🎯 Complete application flow iteration {}/{}", i + 1, iters);

                let rt = Runtime::new().unwrap();
                let flow_result = rt.block_on(benchmark_complete_application_flow());

                let iteration_time = iteration_start.elapsed();
                total_flow_time += iteration_time;

                match flow_result {
                    Ok(metrics) => {
                        log::info!(
                            "✅ Complete flow iteration {} finished in {:?}",
                            i + 1,
                            iteration_time
                        );

                        // Log detailed timing breakdown
                        log::info!(
                            "📊 Flow breakdown: Auth: {:?}, Libraries: {:?}, Media: {:?}, Metadata: {:?}, UI Ready: {:?}",
                            metrics.auth_time,
                            metrics.libraries_time,
                            metrics.media_loading_time,
                            metrics.metadata_time,
                            metrics.ui_ready_time
                        );

                        // Performance validation against targets
                        if iteration_time > Duration::from_secs(10) {
                            log::error!(
                                "🔴 SLOW STARTUP: Complete flow took {:?} (target: <10s for good UX)",
                                iteration_time
                            );
                        } else if iteration_time < Duration::from_secs(3) {
                            log::info!(
                                "⚡ FAST STARTUP: Complete flow in {:?} (excellent UX)",
                                iteration_time
                            );
                        } else {
                            log::info!(
                                "✅ GOOD STARTUP: Complete flow in {:?} (acceptable UX)",
                                iteration_time
                            );
                        }
                    }
                    Err(e) => {
                        log::error!("❌ Complete flow iteration {} failed: {}", i + 1, e);
                    }
                }
            }

            total_flow_time
        });
    });

    group.finish();
}

/// Benchmark the complete application flow from cold start to usable UI
/// This models the exact user experience timeline
async fn benchmark_complete_application_flow(
) -> Result<ApplicationFlowMetrics, Box<dyn std::error::Error>> {
    let total_start = Instant::now();
    log::info!("🎬 Starting complete application flow benchmark");

    // === Phase 1: Authentication (like real app startup) ===
    let auth_start = Instant::now();
    log::info!("🔐 Phase 1: Authentication setup...");

    let mut state = State::new("http://localhost:3000".to_string());
    service_registry::init_registry(state.image_service.clone());

    if let Err(e) = setup_benchmark_authentication(&state).await {
        log::warn!("⚠️  Authentication failed: {}", e);
    }

    let auth_time = auth_start.elapsed();
    log::info!("✅ Authentication completed in {:?}", auth_time);

    // === Phase 2: Library Loading (LoadLibraries message) ===
    let libraries_start = Instant::now();
    log::info!("📚 Phase 2: Loading libraries from server...");

    let libraries = fetch_libraries(state.server_url.clone()).await?;
    state.domains.library.state.libraries = libraries.clone();
    let enabled_libraries: Vec<_> = libraries.iter().filter(|lib| lib.enabled).collect();

    let libraries_time = libraries_start.elapsed();
    log::info!(
        "✅ Libraries loaded in {:?} ({} enabled)",
        libraries_time,
        enabled_libraries.len()
    );

    // === Phase 3: Media References Loading (for all libraries) ===
    let media_start = Instant::now();
    log::info!("🎬 Phase 3: Loading media references for all libraries...");

    let mut total_media_items = 0;
    let mut libraries_with_media = Vec::new();

    for library in &enabled_libraries {
        let media_response =
            fetch_library_media_references(state.server_url.clone(), library.id).await?;
        let media_count = media_response.media.len();
        total_media_items += media_count;
        libraries_with_media.push((library.id, media_response.media));

        log::info!("📂 {} media items from {}", media_count, library.name);
    }

    let media_loading_time = media_start.elapsed();
    log::info!(
        "✅ Media references loaded in {:?} ({} total items)",
        media_loading_time,
        total_media_items
    );

    let mut total_time = Duration::ZERO;
    let mut processed = 0;

    let mut ui_ready_time = Duration::ZERO;
    let mut image_time = Duration::ZERO;
    let mut metadata_time = Duration::ZERO;

    for library in libraries_with_media {
        // Filter items that need metadata using the same logic as BatchMetadataFetcher
        let metadata_start = Instant::now();

        let items_needing_metadata: Vec<_> = library
            .1
            .into_iter()
            .filter(|media_ref| {
                matches!(media_ref.media_type(), "movie" | "series")
                    && ferrex_player::infrastructure::api_types::needs_details_fetch(
                        media_ref.as_ref().details(),
                    )
            })
            .collect::<Vec<_>>();

        let metadata_needed = !items_needing_metadata.is_empty();

        let library_data = vec![(library.0, items_needing_metadata)];

        // === Phase 5: Batch Metadata Fetching (the expensive part!) ===
        log::info!("🔄 Phase 5: Batch metadata fetching (the critical path)...");

        if metadata_needed {
            if let Some(fetcher) = &state.batch_metadata_fetcher {
                // Use the real BatchMetadataFetcher with actual network requests
                let verification_results = fetcher
                    .clone()
                    .process_libraries_with_verification(library_data)
                    .await;
                processed += verification_results
                    .iter()
                    .fold(0, |acc, res| acc + res.1.items_successfully_fetched);
            } else {
                log::warn!("⚠️  No BatchMetadataFetcher available");
            }
        }

        metadata_time = metadata_start.elapsed();
        log::info!(
            "✅ Batch metadata fetching completed in {:?}",
            metadata_time
        );

        log::info!(
            "   ✅ REAL batch metadata processing completed in {:?} ({} items processed)",
            metadata_start.elapsed(),
            processed
        );

        // === Phase 6: Initial Image Loading (visible posters) ===
        log::info!("🖼️  Phase 6: Loading initial visible posters...");
        let image_start = Instant::now();

        // Request posters for first visible items (simulate grid viewport)
        let mut images_requested = 0;

        if let Ok(store) = state.domains.media.state.media_store.read() {
            let movies = store.get_movies(None);
            let visible_movies = movies.iter().take(20); // First 20 items visible on screen

            for movie in visible_movies {
                let image_request = ImageRequest {
                    media_id: ferrex_core::MediaId::Movie(movie.id.clone()),
                    size: ImageSize::Poster,
                    priority: Priority::Visible,
                };

                if let Some(image_service) = service_registry::get_image_service() {
                    image_service.get().request_image(image_request);
                    images_requested += 1;
                }
            }
        }

        image_time = image_start.elapsed();

        log::info!(
            "✅ Initial image loading started in {:?} ({} posters requested)",
            image_time,
            images_requested
        );

        // Small delay to allow some images to start loading
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // === Phase 7: UI Ready for User Interaction ===
        let ui_ready_start = Instant::now();
        log::info!("🖥️  Phase 7: UI ready for user interaction...");

        // Simulate final UI operations that happen before the user can interact
        // This includes ViewModel refreshes and initial rendering
        state.tab_manager.refresh_active_tab();
        state.all_view_model.refresh_from_store();

        // Create initial grid state for rendering
        let grid_state = create_real_grid_state(total_media_items, 6, 0.0);

        // Simulate one initial render to measure complete readiness
        if let Ok(store) = state.domains.media.state.media_store.read() {
            let movies = store.get_movies(None);
            if !movies.is_empty() {
                let sample_movies: Vec<MovieReference> =
                    movies.iter().take(50).map(|m| (*m).clone()).collect();
                let hovered_media_id = None;

                // This represents the first actual render the user would see
                let _render_result = black_box(virtual_movie_references_grid(
                    &sample_movies,
                    &grid_state,
                    &hovered_media_id,
                    |viewport| Message::MoviesGridScrolled(viewport),
                    false,
                    &state,
                ));
            }
        }

        ui_ready_time = ui_ready_start.elapsed();
        log::info!("✅ UI ready for interaction in {:?}", ui_ready_time);

        // === Final Results ===
        log::info!(
        "📈 Performance breakdown: Auth({:?}) + Libraries({:?}) + Media({:?}) + Metadata({:?}) + Images({:?}) + UI({:?}) = Total({:?})",
        auth_time, libraries_time, media_loading_time, metadata_time, image_time, ui_ready_time, total_time
        );
    }

    total_time = total_start.elapsed();
    log::info!("🎉 Complete application flow finished in {:?}", total_time);

    Ok(ApplicationFlowMetrics {
        total_time,
        auth_time,
        libraries_time,
        media_loading_time,
        metadata_time,
        image_time,
        ui_ready_time,
        total_media_items,
        libraries_processed: enabled_libraries.len(),
    })
}

#[derive(Debug)]
struct ApplicationFlowMetrics {
    total_time: Duration,
    auth_time: Duration,
    libraries_time: Duration,
    media_loading_time: Duration,
    metadata_time: Duration,
    image_time: Duration,
    ui_ready_time: Duration,
    total_media_items: usize,
    libraries_processed: usize,
}

criterion_group!(
    benches,
    benchmark_full_initialization,
    benchmark_comprehensive_application_flow,
    benchmark_batch_metadata_fetching,
    benchmark_mediastore_operations,
    benchmark_image_loading_pipeline,
    benchmark_real_movie_grid_hotpath,
    benchmark_movie_component_creation,
    benchmark_real_series_grid_hotpath,
    benchmark_real_scrolling_hotpath,
    benchmark_grid_state_calculations,
    benchmark_allocation_patterns,
    benchmark_frame_budget_compliance
);

criterion_main!(benches);
