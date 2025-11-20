use std::time::{Duration, Instant};

use criterion::Criterion;
use ferrex_player::{
    domains::media::library::{fetch_libraries, fetch_library_media_references},
    infrastructure::{
        api_types::{Library, LibraryType},
        service_registry,
    },
    state_refactored::State,
};
use tokio::runtime::Runtime;

use crate::utils::auth::setup_benchmark_authentication;

#[derive(Debug)]
pub struct BatchMetadataStats {
    items_needing_metadata: usize,
    items_successfully_fetched: usize,
    items_failed: usize,
    total_time: Duration,
    libraries_processed: usize,
    verification_passed: bool,
    success_rate: f64,
    errors: Vec<String>,
}

impl BatchMetadataStats {
    /// Check if the benchmark result indicates healthy batch fetching
    pub fn is_healthy(&self) -> bool {
        self.verification_passed && self.success_rate > 99.0
    }
}

pub fn benchmark_batch_metadata_fetching(c: &mut Criterion) {
    // Initialize logging for benchmarks
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init();

    log::info!("🧪 Batch metadata fetching benchmark starting");

    let mut group = c.benchmark_group("batch_metadata_fetching");
    group.measurement_time(Duration::from_secs(60)); // Longer for network operations
    group.sample_size(10); // Fewer samples due to network variability

    // Test different batch scenarios using real server data
    let scenarios = [
        ("movies_capped_100", "Movies capped at 100"),
        ("tv_capped_100", "TV shows capped at 100"),
        ("both_capped_100", "Both movies and TV shows capped at 100"),
        ("movies_uncapped", "Movies uncapped"),
        ("tv_uncapped", "TV shows uncapped"),
        ("both_uncapped", "Both movies and TV shows uncapped"),
    ];

    for (scenario_name, scenario_description) in scenarios {
        group.bench_function(scenario_name, |b| {
            let mut printed_time_per_item = false;
            b.iter_custom(|iters| {
                let mut total_batch_time = Duration::from_secs(0);
                let mut time_per_item_sum = 0.0;

                for i in 0..iters {
                    let iteration_start = Instant::now();
                    log::info!("📡 Batch metadata benchmark iteration {}/{}", i + 1, iters);

                    let rt = Runtime::new().unwrap();
                    let batch_result = rt.block_on(batch_metadata_operation(scenario_name));

                    let iteration_time = iteration_start.elapsed();
                    total_batch_time += iteration_time;

                    match batch_result {
                        Ok(stats) => {
                            // Performance validation: batch processing should be efficient
                            if stats.items_needing_metadata > 0 {
                                let time_per_item = iteration_time.as_millis() as f64 / stats.items_needing_metadata as f64;

                                time_per_item_sum += time_per_item;

                                if time_per_item > 10.0 { // More than 100ms per item is slow
                                    log::warn!(
                                        "🔶 SLOW BATCH: {:.2}ms per item in {} (target: <10ms per item)",
                                        time_per_item,
                                        scenario_description
                                    );
                                } else {
                                    log::debug!(
                                        "⚡ FAST BATCH: {:.1}ms per item in {}",
                                        time_per_item,
                                        scenario_description
                                    );
                                }
                                if stats.success_rate < 1.0 || stats.items_needing_metadata < 1 || !stats.verification_passed {
                                    println!(
                                        "✅ Batch metadata iteration {} completed in {:?} - {}/{} items fetched ({}% success) with {} failures in {:.2}ms, verification: {}",
                                        i + 1,
                                        stats.total_time,
                                        stats.items_successfully_fetched,
                                        stats.items_needing_metadata,
                                        stats.success_rate,
                                        stats.items_failed,
                                        time_per_item,
                                        if stats.verification_passed { "PASSED" } else { "FAILED" }
                                    );
                                }
                            }


                            // Verification validation
                            if stats.is_healthy() {
                                log::info!("🟢 HEALTHY BATCH: Metadata fetched and verified successfully with a success rate of {:.1}%", stats.success_rate);
                            } else {
                                println!("🔴 UNHEALTHY BATCH: Verification failed or poor success rate");
                                if !stats.errors.is_empty() {
                                    println!("   Errors: {:?}", stats.errors);
                                }
                            }
                        }
                        Err(e) => {
                            println!("❌ Batch metadata iteration {} failed: {}", i + 1, e);
                        }
                    }
                }

                if (time_per_item_sum / iters as f64) > 10.0 && !printed_time_per_item {
                    println!("Average time per item: {:.1}ms", (time_per_item_sum / iters as f64));
                    printed_time_per_item = true;
                }

                total_batch_time
            });
        });
    }

    group.finish();
}

/// Test batch metadata operations with real server data
pub async fn batch_metadata_operation(
    scenario: &str,
) -> Result<BatchMetadataStats, Box<dyn std::error::Error>> {
    log::info!(
        "🔬 Testing batch metadata operation for scenario: {}",
        scenario
    );

    // Create state and setup authentication
    let state = State::new("http://localhost:3000".to_string());
    service_registry::init_registry(state.image_service.clone());

    if let Err(e) = setup_benchmark_authentication(&state).await {
        println!("❌ Benchmark authentication failed: {}", e);
        println!("💡 To fix this:");
        println!("   1. Open the Ferrex application");
        println!("   2. Sign in with your credentials");
        println!("   3. Enable 'Auto-login' in Settings → Security");
        println!("   4. Close the app and re-run the benchmark");
        return Err(format!("Authentication required for benchmarks: {}", e).into());
    }

    // Load real libraries from server
    let libraries = fetch_libraries(state.server_url.clone()).await?;
    let enabled_libraries: Vec<_> = libraries.iter().filter(|lib| lib.enabled).collect();

    if enabled_libraries.is_empty() {
        return Err("No enabled libraries found for benchmarking".into());
    }

    let mut total_items_needing = 0;
    let mut total_items_fetched = 0;
    let mut total_items_failed = 0;
    let mut all_verified = true;
    let mut all_errors = Vec::new();
    let batch_start = Instant::now();

    // Select libraries based on scenario
    let libraries_to_test: Vec<&Library> = match scenario {
        "movies_capped_100" => enabled_libraries
            .into_iter()
            .filter(|lib| lib.library_type == LibraryType::Movies)
            .collect::<Vec<&Library>>(),
        "movies_uncapped" => enabled_libraries
            .into_iter()
            .filter(|lib| lib.library_type == LibraryType::Movies)
            .collect::<Vec<&Library>>(),
        "tv_capped_100" => enabled_libraries
            .into_iter()
            .filter(|lib| lib.library_type == LibraryType::TvShows)
            .collect::<Vec<&Library>>(),
        "tv_uncapped" => enabled_libraries
            .into_iter()
            .filter(|lib| lib.library_type == LibraryType::TvShows)
            .collect::<Vec<&Library>>(),
        "both_capped_100" => enabled_libraries.into(),
        "both_uncapped" => enabled_libraries.into(),
        scenario => return Err(format!("Invalid scenario: {}", scenario).into()),
    };

    // Process each library for batch metadata fetching
    let libraries_count = libraries_to_test.len();
    for library in libraries_to_test {
        log::info!("📚 Processing library: {} for batch metadata", library.name);

        // Load media references
        let media_response =
            fetch_library_media_references(state.server_url.clone(), library.id).await?;
        log::info!(
            "📋 Loaded {} media references from {}",
            media_response.media.len(),
            library.name
        );

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
            .collect();

        if !items_needing_metadata.is_empty() {
            log::info!(
                "🔄 {} items need metadata fetching",
                items_needing_metadata.len()
            );

            // Use actual BatchMetadataFetcher with verification
            if let Some(fetcher) = &state.batch_metadata_fetcher {
                let library_data = match scenario {
                    "movies_capped_100" => vec![(
                        library.id,
                        media_response.media.into_iter().take(100).collect(),
                    )],
                    "movies_uncapped" => vec![(library.id, media_response.media)],
                    "tv_capped_100" => vec![(
                        library.id,
                        media_response.media.into_iter().take(100).collect(),
                    )],
                    "tv_uncapped" => vec![(library.id, media_response.media)],
                    "both_capped_100" => vec![(
                        library.id,
                        media_response.media.into_iter().take(100).collect(),
                    )],
                    "both_uncapped" => vec![(library.id, media_response.media)],
                    _ => vec![],
                };

                // Measure the actual batch metadata fetching with verification
                let metadata_start = Instant::now();
                let verification_results = fetcher
                    .clone()
                    .process_libraries_with_verification(library_data)
                    .await;
                let metadata_time = metadata_start.elapsed();

                // Process verification results
                for (_lib_id, result) in verification_results {
                    // Accumulate stats across all libraries
                    total_items_needing += result.items_needing_metadata;
                    total_items_fetched += result.items_successfully_fetched;
                    total_items_failed += result.items_failed;

                    if !result.verification_passed {
                        all_verified = false;
                    }

                    all_errors.extend(result.errors.clone());

                    log::info!(
                        "⚡ Batch metadata for {} completed in {:?} - {}/{} items fetched ({}% success), verification: {}",
                        library.name,
                        metadata_time,
                        result.items_successfully_fetched,
                        result.items_needing_metadata,
                        result.success_rate(),
                        if result.verification_passed { "PASSED" } else { "FAILED" }
                    );

                    // Log any errors
                    if !result.errors.is_empty() {
                        log::warn!(
                            "❌ Batch fetch errors for {}: {:?}",
                            library.name,
                            result.errors
                        );
                    }

                    // Performance validation
                    if !result.is_fully_successful() {
                        log::error!(
                            "🔴 BATCH FETCH ISSUE: {} had failures or verification issues",
                            library.name
                        );
                    }
                }
            }
        } else {
            log::info!("ℹ️  No items need metadata fetching in {}", library.name);
        }
    }

    let total_time = batch_start.elapsed();
    let success_rate = if total_items_needing > 0 {
        (total_items_fetched as f64 / total_items_needing as f64) * 100.0
    } else {
        100.0
    };

    log::info!(
        "🎯 Batch metadata operation completed: {}/{} items fetched ({}% success), verified: {}, in {:?}",
        total_items_fetched,
        total_items_needing,
        success_rate,
        all_verified,
        total_time
    );

    Ok(BatchMetadataStats {
        items_needing_metadata: total_items_needing,
        items_successfully_fetched: total_items_fetched,
        items_failed: total_items_failed,
        total_time,
        libraries_processed: libraries_count,
        verification_passed: all_verified,
        success_rate,
        errors: all_errors,
    })
}
