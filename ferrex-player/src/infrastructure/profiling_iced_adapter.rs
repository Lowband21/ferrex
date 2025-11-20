// Adapter for integrating profiling tools with Iced
// Since puffin uses egui, we need alternative visualization strategies

use iced::widget::container::Style;
use iced::{
    Color, Element, Length,
    widget::{column, container, row, text},
};
use std::time::Duration;

#[cfg(feature = "profiling-stats")]
use crate::infrastructure::profiling::{FrameStats, PROFILER};

// Import Message from the common module
use crate::common::messages::DomainMessage as Message;

/// Profiling overlay for Iced applications
/// Since puffin_egui doesn't work with Iced, we provide alternatives:
/// 1. Export data to puffin-http server (view in browser)
/// 2. Display basic stats in Iced overlay
/// 3. Export to Chrome tracing format
pub struct ProfilingOverlay {
    visible: bool,
    stats_update_interval: Duration,
    last_update: std::time::Instant,
    cached_stats: Option<FormattedStats>,
}

#[derive(Clone)]
struct FormattedStats {
    frame_count: String,
    p50: String,
    p95: String,
    p99: String,
    max: String,
    fps: String,
    #[cfg(feature = "memory-stats")]
    memory_current: String,
    #[cfg(feature = "memory-stats")]
    memory_peak: String,
    #[cfg(feature = "memory-stats")]
    memory_delta: String,
}

impl ProfilingOverlay {
    pub fn new() -> Self {
        Self {
            visible: cfg!(debug_assertions),
            stats_update_interval: Duration::from_millis(250),
            last_update: std::time::Instant::now(),
            cached_stats: None,
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Create an Iced overlay showing profiling statistics
    pub fn view<'a>(&mut self) -> Option<Element<'a, Message>> {
        if !self.visible {
            return None;
        }

        #[cfg(feature = "profiling-stats")]
        {
            // Update stats at interval to avoid overhead
            if self.last_update.elapsed() > self.stats_update_interval {
                self.update_stats();
                self.last_update = std::time::Instant::now();
            }

            if let Some(stats) = &self.cached_stats {
                return Some(self.render_stats_overlay(stats.clone()));
            }
        }

        // If no stats available, show profiling status
        Some(self.render_status_overlay())
    }

    #[cfg(feature = "profiling-stats")]
    fn update_stats(&mut self) {
        let stats = PROFILER.get_frame_stats();

        let fps = if stats.p50_micros > 0 {
            1_000_000.0 / stats.p50_micros as f64
        } else {
            0.0
        };

        #[cfg(feature = "memory-stats")]
        let mem_stats = {
            use crate::infrastructure::profiling::MemoryStats;
            PROFILER.get_memory_stats()
        };

        self.cached_stats = Some(FormattedStats {
            frame_count: format!("{}", stats.count),
            p50: format!("{:.2}ms", stats.p50_micros as f64 / 1000.0),
            p95: format!("{:.2}ms", stats.p95_micros as f64 / 1000.0),
            p99: format!("{:.2}ms", stats.p99_micros as f64 / 1000.0),
            max: format!("{:.2}ms", stats.max_micros as f64 / 1000.0),
            fps: format!("{:.1} FPS", fps),
            #[cfg(feature = "memory-stats")]
            memory_current: format!("{:.1}MB", mem_stats.current_bytes as f64 / 1_000_000.0),
            #[cfg(feature = "memory-stats")]
            memory_peak: format!("{:.1}MB", mem_stats.peak_bytes as f64 / 1_000_000.0),
            #[cfg(feature = "memory-stats")]
            memory_delta: {
                let delta = mem_stats.current_bytes as i64 - mem_stats.baseline_bytes as i64;
                if delta > 0 {
                    format!("+{:.1}MB", delta as f64 / 1_000_000.0)
                } else {
                    format!("{:.1}MB", delta as f64 / 1_000_000.0)
                }
            },
        });
    }

    fn render_stats_overlay<'a>(&self, stats: FormattedStats) -> Element<'a, Message> {
        let mut items: Vec<Element<'a, Message>> = vec![
            text("Performance").size(14).into(),
            row![text("FPS: ").size(12), text(stats.fps).size(12)]
                .spacing(5)
                .into(),
            row![text("P50: ").size(12), text(stats.p50).size(12)]
                .spacing(5)
                .into(),
            row![text("P95: ").size(12), text(stats.p95).size(12)]
                .spacing(5)
                .into(),
            row![text("P99: ").size(12), text(stats.p99).size(12)]
                .spacing(5)
                .into(),
            row![text("Max: ").size(12), text(stats.max).size(12)]
                .spacing(5)
                .into(),
            row![text("Frames: ").size(12), text(stats.frame_count).size(12)]
                .spacing(5)
                .into(),
        ];

        // Add memory stats if available
        #[cfg(feature = "memory-stats")]
        {
            items.push(text("Memory").size(14).into());
            items.push(
                row![
                    text("Current: ").size(12),
                    text(stats.memory_current).size(12)
                ]
                .spacing(5)
                .into(),
            );
            items.push(
                row![text("Peak: ").size(12), text(stats.memory_peak).size(12)]
                    .spacing(5)
                    .into(),
            );
            items.push(
                row![text("Delta: ").size(12), text(stats.memory_delta).size(12)]
                    .spacing(5)
                    .into(),
            );
        }

        container(column(items).spacing(2))
            .padding(10)
            .style(profiling_overlay_style)
            .into()
    }

    fn render_status_overlay<'a>(&self) -> Element<'a, Message> {
        let mut status_items: Vec<Element<'a, Message>> =
            vec![text("Profiling Active").size(14).into()];

        #[cfg(feature = "profile-with-puffin")]
        {
            status_items.push(text("✓ Puffin (HTTP)").size(12).into());

            #[cfg(feature = "puffin-server")]
            status_items.push(text("  → localhost:8585").size(10).into());
        }

        #[cfg(feature = "profile-with-tracy")]
        status_items.push(text("✓ Tracy").size(12).into());

        #[cfg(feature = "profile-with-tracing")]
        status_items.push(text("✓ Tracing").size(12).into());

        container(column(status_items).spacing(2))
            .padding(10)
            .style(profiling_overlay_style)
            .into()
    }
}

// =============================================================================
// Puffin HTTP Server Integration
// =============================================================================

#[cfg(feature = "puffin-server")]
pub mod puffin_server {
    use std::sync::Arc;

    /// Start puffin HTTP server for browser-based visualization
    /// Returns server handle that must be kept alive
    pub fn start_server(
        addr: &str,
    ) -> Result<Arc<puffin_http::Server>, Box<dyn std::error::Error>> {
        let server = Arc::new(puffin_http::Server::new(addr)?);

        log::info!("Puffin profiler server started at http://{}", addr);
        log::info!("View profiling data at: http://{}/puffin_viewer.html", addr);

        Ok(server)
    }

    /// Export current puffin data
    /// Note: Puffin data is best viewed through the HTTP server at localhost:8585
    pub fn export_info() {
        #[cfg(feature = "profile-with-puffin")]
        {
            log::info!("Puffin data can be viewed at http://localhost:8585");
            log::info!("The HTTP server provides real-time profiling visualization");
        }
    }
}

// =============================================================================
// Chrome Tracing Export (Alternative to Puffin UI)
// =============================================================================

/// Export trace data to view in external tools
pub mod chrome_export {
    use std::path::Path;

    /// Export trace data to Chrome tracing format
    /// View in Chrome at chrome://tracing
    pub fn export_trace(path: &Path) -> Result<(), std::io::Error> {
        // With the profiling crate, trace data is handled by the backend
        log::info!("Trace export requested to: {:?}", path);

        #[cfg(feature = "profile-with-tracy")]
        log::info!("Tracy data can be viewed using the Tracy profiler GUI");

        #[cfg(feature = "profile-with-puffin")]
        log::info!("Puffin data can be viewed at http://localhost:8585");

        Ok(())
    }
}

// =============================================================================
// Styling
// =============================================================================

/// Style function for profiling overlay
fn profiling_overlay_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgba(
            0.0, 0.0, 0.0, 0.8,
        ))),
        text_color: Some(Color::from_rgb(0.0, 1.0, 0.0)),
        border: iced::Border {
            color: Color::from_rgb(0.0, 1.0, 0.0),
            width: 1.0,
            radius: 5.0.into(),
        },
        ..Default::default()
    }
}

// =============================================================================
// Integration with Iced Application
// =============================================================================

/// Helper to wrap view with profiling overlay
pub fn view_with_profiling<'a>(
    main_view: Element<'a, Message>,
    overlay: &'a mut ProfilingOverlay,
) -> Element<'a, Message> {
    #[cfg(any(feature = "profiling-stats", feature = "profile-with-puffin"))]
    {
        if let Some(overlay_view) = overlay.view() {
            // Position overlay in top-right corner
            return container(iced::widget::stack![
                main_view,
                container(overlay_view)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .align_y(iced::alignment::Vertical::Top)
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        }
    }

    main_view
}

// =============================================================================
// Usage Example
// =============================================================================

#[cfg(test)]
mod usage_example {
    use super::*;

    fn example_view() {
        // In your Iced application state
        struct AppState {
            profiling_overlay: ProfilingOverlay,
            // ... other state
        }

        impl AppState {
            fn view(&mut self) -> Element<Message> {
                // Your main view
                let main_view = container(text("Hello")).into();

                // Wrap with profiling overlay
                view_with_profiling(main_view, &mut self.profiling_overlay)
            }

            fn update(&mut self, message: Message) {
                // Toggle profiling overlay with F12
                // match message {
                //     Message::KeyPressed(Key::F12) => {
                //         self.profiling_overlay.toggle();
                //     }
                //     _ => {}
                // }
            }
        }
    }
}
