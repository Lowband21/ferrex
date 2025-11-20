// Profiling Scope Names and Analysis
//
// This module defines standard profiling scope names for use with puffin/tracy/fastrace
// and provides analysis functions to extract performance insights from profiling data.
//
// The actual profiling is done by profiling_v2.rs - this just defines conventions
// and analysis tools for UI-specific performance tracking.

use std::time::Duration;

/// Standard profiling scope names for UI operations
pub mod scopes {
    // Grid rendering scopes
    pub const GRID_RENDER: &str = "UI::Grid::Render";
    pub const GRID_LAYOUT: &str = "UI::Grid::Layout";
    pub const GRID_SCROLL: &str = "UI::Grid::Scroll";
    pub const GRID_ITEM_CREATE: &str = "UI::Grid::ItemCreate";

    // Poster loading scopes
    pub const POSTER_LOAD: &str = "UI::Poster::Load";
    pub const POSTER_NETWORK: &str = "UI::Poster::Network";
    pub const POSTER_DECODE: &str = "UI::Poster::Decode";
    pub const POSTER_GPU_UPLOAD: &str = "UI::Poster::GPUUpload";
    pub const POSTER_CACHE_HIT: &str = "UI::Poster::CacheHit";

    // Animation scopes
    pub const ANIM_HOVER: &str = "UI::Animation::Hover";
    pub const ANIM_LOADING: &str = "UI::Animation::Loading";
    pub const ANIM_TRANSITION: &str = "UI::Animation::Transition";
    pub const ANIM_SCROLL: &str = "UI::Animation::Scroll";

    // View operation scopes
    pub const VIEW_UPDATE: &str = "UI::View::Update";
    pub const VIEW_RENDER: &str = "UI::View::Render";
    pub const VIEW_LAYOUT: &str = "UI::View::Layout";
    pub const VIEW_DRAW: &str = "UI::View::Draw";

    // Metadata operation scopes
    pub const METADATA_FETCH: &str = "Metadata::Fetch";
    pub const METADATA_BATCH: &str = "Metadata::Batch";
    pub const METADATA_TV: &str = "Metadata::TV";
    pub const METADATA_MOVIE: &str = "Metadata::Movie";

    // Specific view function scopes
    pub const LIBRARY_VIEW: &str = "UI::View::Library";
    pub const VIRTUAL_LIST_RENDER: &str = "UI::VirtualList::Render";
    pub const VIRTUAL_LIST_CALC: &str = "UI::VirtualList::Calculate";
    pub const HEADER_VIEW: &str = "UI::View::Header";
    pub const MOVIE_DETAIL_VIEW: &str = "UI::View::MovieDetail";
    pub const TV_DETAIL_VIEW: &str = "UI::View::TVDetail";
    pub const SEASON_DETAIL_VIEW: &str = "UI::View::SeasonDetail";
    pub const EPISODE_DETAIL_VIEW: &str = "UI::View::EpisodeDetail";

    // Domain update scopes
    pub const AUTH_UPDATE: &str = "Domain::Auth::Update";
    pub const LIBRARY_UPDATE: &str = "Domain::Library::Update";
    pub const MEDIA_UPDATE: &str = "Domain::Media::Update";
    pub const METADATA_UPDATE: &str = "Domain::Metadata::Update";
    pub const PLAYER_UPDATE: &str = "Domain::Player::Update";
    pub const SEARCH_UPDATE: &str = "Domain::Search::Update";
    pub const SETTINGS_UPDATE: &str = "Domain::Settings::Update";
    pub const STREAMING_UPDATE: &str = "Domain::Streaming::Update";
    pub const UI_UPDATE: &str = "Domain::UI::Update";
    pub const USER_MGMT_UPDATE: &str = "Domain::UserManagement::Update";
}

/// Profiling macros that use the standard scope names
#[macro_export]
macro_rules! profile_ui {
    (grid_render) => {
        profiling::scope!($crate::infra::profiling_scopes::scopes::GRID_RENDER)
    };
    (grid_layout) => {
        profiling::scope!($crate::infra::profiling_scopes::scopes::GRID_LAYOUT)
    };
    (grid_scroll) => {
        profiling::scope!($crate::infra::profiling_scopes::scopes::GRID_SCROLL)
    };
    (poster_load, $media_id:expr_2021) => {
        profiling::scope!(&format!(
            "{}::{}",
            $crate::infra::profiling_scopes::scopes::POSTER_LOAD,
            $media_id
        ))
    };
    (animation, $type:expr_2021) => {
        profiling::scope!(&format!("UI::Animation::{}", $type))
    };
}

/// Performance target definitions
pub struct PerformanceTargets {
    pub view_operation_ms: f32,    // Target: 8ms
    pub frame_time_ms: f32,        // Target: 8.33ms (120fps)
    pub scroll_frame_ms: f32,      // Target: 4ms during scroll
    pub metadata_per_item_ms: f32, // Target: 10ms
    pub poster_load_ms: f32,       // Target: 50ms
    pub cache_hit_rate: f32,       // Target: 80%
}

impl Default for PerformanceTargets {
    fn default() -> Self {
        Self {
            view_operation_ms: 8.0,
            frame_time_ms: 8.33, // 120fps
            scroll_frame_ms: 4.0,
            metadata_per_item_ms: 10.0,
            poster_load_ms: 50.0,
            cache_hit_rate: 0.8,
        }
    }
}

/// Performance analysis result
#[derive(Debug, Clone)]
pub struct PerformanceAnalysis {
    pub view_operations_over_target: Vec<String>,
    pub frame_drops: usize,
    pub cache_hit_rate: f32,
    pub slowest_operation: Option<(String, Duration)>,
    pub metadata_bottleneck: bool,
    pub recommendations: Vec<String>,
}

/// Analyze profiling data to identify performance issues
///
/// This would integrate with puffin's ProfilerScope data to extract metrics
/// In production, this would read from puffin's GlobalProfiler
pub fn analyze_performance() -> PerformanceAnalysis {
    let targets = PerformanceTargets::default();
    let mut analysis = PerformanceAnalysis {
        view_operations_over_target: Vec::new(),
        frame_drops: 0,
        cache_hit_rate: 0.0,
        slowest_operation: None,
        metadata_bottleneck: false,
        recommendations: Vec::new(),
    };

    // In a real implementation, we would:
    // 1. Read puffin::GlobalProfiler data
    // 2. Filter for our specific scopes
    // 3. Calculate metrics
    // 4. Generate recommendations

    #[cfg(feature = "profile-with-puffin")]
    {
        // Example of how to read puffin data (simplified)
        // let recent_frame = puffin::GlobalProfiler::lock().recent_frame();
        // for scope in recent_frame.scopes() {
        //     if scope.name.starts_with("UI::") {
        //         if scope.duration_ms() > targets.view_operation_ms {
        //             analysis.view_operations_over_target.push(scope.name.to_string());
        //         }
        //     }
        // }
    }

    // Generate recommendations based on findings
    if analysis.metadata_bottleneck {
        analysis.recommendations.push(
            "TV metadata fetching is the primary bottleneck (55ms per item). \
             Consider caching, batching, or async prefetching."
                .to_string(),
        );
    }

    if analysis.cache_hit_rate < targets.cache_hit_rate {
        analysis.recommendations.push(format!(
            "Cache hit rate ({:.1}%) below target ({:.1}%). \
                    Consider increasing cache size or improving prefetch logic.",
            analysis.cache_hit_rate * 100.0,
            targets.cache_hit_rate * 100.0
        ));
    }

    analysis
}

/// Helper to check if an operation meets performance targets
pub fn check_performance_target(operation: &str, duration: Duration) -> bool {
    let targets = PerformanceTargets::default();
    let ms = duration.as_secs_f32() * 1000.0;

    match operation {
        s if s.starts_with(scopes::GRID_RENDER) => {
            ms <= targets.view_operation_ms
        }
        s if s.starts_with(scopes::GRID_SCROLL) => {
            ms <= targets.scroll_frame_ms
        }
        s if s.starts_with(scopes::POSTER_LOAD) => ms <= targets.poster_load_ms,
        s if s.starts_with(scopes::METADATA_TV) => {
            ms <= targets.metadata_per_item_ms * 10.0
        } // TV is slower
        s if s.starts_with(scopes::METADATA_MOVIE) => {
            ms <= targets.metadata_per_item_ms
        }
        _ => ms <= targets.frame_time_ms,
    }
}

/// Log a performance warning if an operation exceeds its target
pub fn log_if_slow(operation: &str, duration: Duration) {
    if !check_performance_target(operation, duration) {
        let ms = duration.as_secs_f32() * 1000.0;
        log::warn!(
            "🔴 Performance: {} took {:.2}ms (over target)",
            operation,
            ms
        );
    }
}

// Integration with puffin's web UI
//
// When running with puffin enabled, you can:
// 1. Open http://127.0.0.1:8585 in a browser
// 2. Use the filter to show only "UI::" scopes
// 3. Look for operations taking >8ms
// 4. Drill down into hierarchical scopes to find bottlenecks
//
// The scope naming convention makes it easy to filter and analyze:
// - "UI::Grid::" - all grid operations
// - "UI::Poster::" - all poster loading
// - "UI::Animation::" - all animations
// - "Metadata::" - all metadata operations
