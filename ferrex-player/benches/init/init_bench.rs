use std::time::{Duration, Instant};

use criterion::Criterion;
use ferrex_player::{
    domains::{
        library::update_handlers::fetch_libraries, metadata::image_types::{ImageRequest, Priority}
    },
    infrastructure::{api_types::Library, service_registry},
    state_refactored::State,
};
use tokio::runtime::Runtime;

use crate::utils::{auth::setup_benchmark_authentication, state::InitializationStats};

// Benchmark 11: Complete Application Initialization (CRITICAL STARTUP PERFORMANCE)
pub fn benchmark_full_initialization(c: &mut Criterion) {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Off)
        .is_test(true)
        .try_init();

    //println!("🚀 Starting application initialization benchmark with full logging...");
    log::info!("🚀 Application initialization benchmark starting");
    let mut group = c.benchmark_group("application_initialization");
    group.measurement_time(Duration::from_secs(90)); // Longer measurement for network operations
    group.sample_size(10); // Fewer samples since network calls can be variable

    // Test different initialization scenarios
    let scenarios = [
        (
            "cold_start_authenticated",
            "Complete app startup with authentication",
        ),
        ("warm_restart", "App restart with existing cache"),
    ];

    for (scenario_name, scenario_description) in scenarios {
        group.bench_function(scenario_name, |b| {
            b.iter_custom(|iters| {
                let start = Instant::now();
                let mut total_initialization_time = Duration::from_secs(0);

                for i in 0..iters {
                    let iteration_start = Instant::now();
                    //println!("🚀 Starting application initialization benchmark iteration {}/{}", i + 1, iters);
                    log::info!("🚀 Starting application initialization benchmark iteration {}/{}", i + 1, iters);

                    // This simulates the EXACT same sequence as real app startup
                    let rt = Runtime::new().unwrap();
                    let initialization_result = rt.block_on(full_initialization_operation(scenario_name));

                    let iteration_time = iteration_start.elapsed();
                    total_initialization_time += iteration_time;

                    // Performance validation: startup should be reasonable
                    if iteration_time > Duration::from_secs(30) {
                        log::error!(
                            "🔴 SLOW STARTUP: {} initialization took {:?} (target: <30s)",
                            scenario_description,
                            iteration_time
                        );
                    } else if iteration_time < Duration::from_secs(5) {
                        log::info!(
                            "⚡ FAST STARTUP: {} initialization completed in {:?}",
                            scenario_description,
                            iteration_time
                        );
                    } else {
                        log::info!(
                            "✅ NORMAL STARTUP: {} initialization completed in {:?}",
                            scenario_description,
                            iteration_time
                        );
                    }

                    // Log the result for debugging
                    match initialization_result {
                        Ok((_, stats)) => {
                            log::info!(
                                "   📊 Loaded {} libraries, {} total media items, {} metadata fetched",
                                stats.libraries_loaded,
                                stats.media_items_loaded,
                                stats.metadata_fetched
                            );
                        }
                        Err(e) => {
                            log::error!("   ❌ Initialization failed: {}", e);
                        }
                    }
                }

                total_initialization_time
            });
        });
    }

    group.finish();
}

/// Benchmark the complete application initialization sequence
/// This replicates the exact flow that happens during real app startup
pub async fn full_initialization_operation(
    scenario: &str,
) -> Result<(State, InitializationStats), Box<dyn std::error::Error>> {
    let total_start = Instant::now();
    //println!("📱 Initializing application for scenario: {}", scenario);
    log::info!("📱 Initializing application for scenario: {}", scenario);

    // Step 1: Create application state (identical to real app)
    log::info!("⚙️  Step 1: Creating application state...");
    let step1_start = Instant::now();
    let mut state = State::new("https://localhost:3000".to_string());

    // Set up authentication for server communication
    if let Err(e) = setup_benchmark_authentication(&state).await {
        log::warn!("⚠️  Authentication setup failed: {}", e);
    }

    // Initialize the global service registry with the real image service
    service_registry::init_registry(state.image_service.clone());
    log::info!("✅ State created in {:?}", step1_start.elapsed());

    // Step 2: Load libraries from server (exactly like LoadLibraries message)
    log::info!("📚 Step 2: Loading libraries from server...");
    let step2_start = Instant::now();
    let libraries = fetch_libraries(state.server_url.clone()).await?;
    log::info!(
        "✅ Loaded {} libraries in {:?}",
        libraries.len(),
        step2_start.elapsed()
    );

    // Process libraries (exactly like LibrariesLoaded handler)
    state.domains.library.state.libraries = libraries.clone();
    let enabled_libraries: Vec<_> = libraries.iter().filter(|lib| lib.enabled).collect();
    log::info!("🔧 {} libraries are enabled", enabled_libraries.len());

    let mut total_media_items = 0;
    let mut total_metadata_fetched = 0;

    // Step 3: Load media references for each enabled library
    log::info!("🎬 Step 3: Loading media references for all libraries...");
    let step3_start = Instant::now();

    let mut poster_loaded_count = 0;

    for library in &enabled_libraries {
        let library_start = Instant::now();
        log::info!(
            "   📂 Loading media for library: {} ({})",
            library.name,
            library.id
        );

        // Load media references
        let media_response =
            fetch_library_media_references(state.server_url.clone(), library.id).await?;
        let media_count = media_response.media.len();
        total_media_items += media_count;

        log::info!(
            "   ✅ Loaded {} media references for {} in {:?}",
            media_count,
            library.name,
            library_start.elapsed()
        );

        // Process media references into MediaStore using ACTUAL BatchCoordinator
        log::info!(
            "   💾 Populating MediaStore with {} items using BatchCoordinator...",
            media_count
        );
        let mediastore_start = Instant::now();

        // Filter items that need metadata using the same logic as BatchMetadataFetcher
        let items_needing_metadata: Vec<_> = media_response
            .media
            .iter()
            .filter(|media_ref| {
                matches!(media_ref.media_type(), "movie" | "series")
                    && ferrex_player::infrastructure::api_types::needs_details_fetch(
                        media_ref.as_ref().details(),
                    )
            })
            .collect::<Vec<_>>();

        if !items_needing_metadata.is_empty() {
            // Step 4: Trigger ACTUAL batch metadata fetching (using real BatchMetadataFetcher)
            if let Some(fetcher) = &state.batch_metadata_fetcher {
                log::info!(
                    "   🔄 Starting REAL batch metadata fetch for library {}...",
                    library.name
                );
                let metadata_start = Instant::now();

                // Use the actual BatchMetadataFetcher logic instead of simulation
                let library_data = vec![(library.id, media_response.media.clone())];

                // Call the actual process_libraries_direct method which performs real network requests
                log::info!("   📡 Executing real batch metadata fetching...");
                let _verification_results = fetcher
                    .clone()
                    .process_libraries_with_verification(library_data.clone())
                    .await;

                let processed = items_needing_metadata.len();
                total_metadata_fetched += processed;

                log::info!(
                    "   ✅ REAL batch metadata processing completed in {:?} ({} items processed)",
                    metadata_start.elapsed(),
                    processed
                );

                // Step 5: Trigger actual image loading to verify poster fetching
                log::info!("   🖼️  Step 5: Testing poster/image loading...");
                log::info!(
                    "   📡 Expected image URLs: {}/images/{{media_id}}/poster",
                    state.server_url
                );
                let image_start = Instant::now();
                let sample_media: Vec<_> = library_data
                    .into_iter()
                    .flat_map(|library| library.1.clone().into_iter())
                    .collect();

                for media_ref in sample_media.iter().by_ref() {
                    let media_id = media_ref.as_ref().media_id();
                    log::info!(
                        "     🎨 Requesting poster for {} ({})",
                        media_ref.media_type(),
                        media_id
                    );

                    // Create image request (this will trigger actual HTTP requests to server)
                    let image_request = ImageRequest {
                        media_id: media_id.clone(),
                        size: ImageSize::Poster,
                        priority: Priority::Visible,
                    };

                    // Request the image - this should show up in server logs and network activity
                    if let Some(image_service) = service_registry::get_image_service() {
                        image_service.get().request_image(image_request.clone());
                        log::info!("     📡 Image request queued for {}", media_id);

                        // Check if it's immediately available (cache hit)
                        if let Some((handle, load_time)) =
                            image_service.get().get_with_load_time(&image_request)
                        {
                            log::info!(
                                "     ✅ Cache HIT for {} (loaded at: {:?})",
                                media_id,
                                load_time
                            );
                        } else {
                            log::info!("     ⏳ Cache MISS for {} - queued for loading", media_id);
                        }
                    } else {
                        log::warn!("     ⚠️  No image service available for poster loading");
                    }
                }

                // Small delay to allow image loading to start (since it's async)
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

                // Check again for any that might have loaded quickly
                for media_ref in sample_media.iter() {
                    let media_id = media_ref.as_ref().media_id();
                    let image_request = ImageRequest {
                        media_id: media_id.clone(),
                        size: ImageSize::Poster,
                        priority: Priority::Visible,
                    };

                    if let Some(image_service) = service_registry::get_image_service() {
                        if let Some((_, load_time)) =
                            image_service.get().get_with_load_time(&image_request)
                        {
                            poster_loaded_count += 1;
                            log::info!(
                                "     🎉 Poster loaded for {} (loaded at: {:?})",
                                media_id,
                                load_time
                            );
                        }
                    }
                }

                log::info!(
                    "   ✅ Image loading verification completed in {:?} ({}/{} posters loaded)",
                    image_start.elapsed(),
                    poster_loaded_count,
                    sample_media.len()
                );
            }
        }
    }

    log::info!(
        "✅ All media loading completed in {:?}",
        step3_start.elapsed()
    );

    let total_time = total_start.elapsed();
    log::info!(
        "🎉 Application initialization completed in {:?}",
        total_time
    );
    log::info!(
        "📊 Final stats: {} libraries, {} media items, {} metadata requests, {} posters loaded",
        enabled_libraries.len(),
        total_media_items,
        total_metadata_fetched,
        poster_loaded_count,
    );

    let stats = InitializationStats {
        libraries_loaded: enabled_libraries.len(),
        media_items_loaded: total_media_items,
        metadata_fetched: total_metadata_fetched,
        posters_loaded: poster_loaded_count,
        total_time,
    };

    Ok((state, stats))
}
