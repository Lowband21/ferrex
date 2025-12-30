use crate::{
    domains::{
        metadata::image_service::UnifiedImageService, ui::messages::UiMessage,
    },
    infra::{
        cache::{
            PlayerDiskImageCache, stats::PlayerDiskImageCacheStatsSnapshot,
        },
        units::ByteSize,
    },
    state::State,
};

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use iced::{
    Element, Length, Theme,
    widget::{Space, column, container, text},
};
use sysinfo::{
    Process, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System,
};

#[derive(Debug, Clone)]
pub struct CacheOverlaySample {
    pub captured_at: Instant,
    pub process_used_bytes: ByteSize,
    pub image_cache_used_bytes: ByteSize,
    pub ram_max_bytes: ByteSize,
    pub disk_usage_bytes: Option<ByteSize>,
    pub disk_stats: Option<PlayerDiskImageCacheStatsSnapshot>,
    pub disk_max_bytes: ByteSize,
    pub disk_ttl_days: u64,
    pub string_buf: String,
}

pub async fn sample_cache_overlay(
    image_service: Arc<UnifiedImageService>,
    disk_cache: Option<Arc<PlayerDiskImageCache>>,
    ram_max_bytes: ByteSize,
    disk_max_bytes: ByteSize,
    disk_ttl_days: u64,
) -> CacheOverlaySample {
    let process_used_bytes = probe_process_used_bytes().await;
    let (disk_usage_bytes, disk_stats) = if let Some(cache) = disk_cache {
        (
            Some(cache.current_usage_bytes().await),
            Some(cache.stats_snapshot()),
        )
    } else {
        (None, None)
    };

    CacheOverlaySample {
        captured_at: Instant::now(),
        process_used_bytes,
        image_cache_used_bytes: image_service.resident_bytes(),
        ram_max_bytes,
        disk_usage_bytes,
        disk_stats,
        disk_max_bytes,
        disk_ttl_days,
        string_buf: String::with_capacity(256),
    }
}

async fn probe_process_used_bytes() -> ByteSize {
    let start = Instant::now();
    let join = tokio::task::spawn_blocking(|| {
        let pid = sysinfo::get_current_pid().ok()?;

        let mut system = System::new_with_specifics(RefreshKind::nothing());
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing().with_memory(),
        );

        let bytes = system.process(pid).map(Process::memory).unwrap_or(0);
        Some(ByteSize::from_bytes(bytes))
    });

    let bytes = match join.await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => ByteSize::from_bytes(0),
        Err(e) => {
            log::debug!("cache overlay process memory probe join failed: {e}");
            ByteSize::from_bytes(0)
        }
    };

    let elapsed = start.elapsed();
    if elapsed > Duration::from_millis(25) {
        let ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        log::debug!("cache overlay process memory probe took {ms}ms");
    }

    bytes
}

pub fn view_cache_debug_overlay(state: &State) -> Element<'_, UiMessage> {
    let Some(sample) = state.domains.ui.state.cache_overlay_sample.as_ref()
    else {
        return container(text("")).into();
    };

    let now = Instant::now();
    let age = now
        .checked_duration_since(sample.captured_at)
        .unwrap_or(Duration::from_secs(0));

    let process_ram = format!(
        "Process RAM: {}",
        format_bytes(sample.process_used_bytes.as_bytes()),
    );

    let image_ram = format!(
        "Image cache (est): {} / {:.2}GiB",
        format_bytes(sample.image_cache_used_bytes.as_bytes()),
        sample.ram_max_bytes.as_gib(),
    );

    let disk = match sample.disk_usage_bytes {
        Some(usage) => {
            format!(
                "Disk: {:.2}GiB / {:.2}GiB",
                usage.as_gib(),
                sample.disk_max_bytes.as_gib()
            )
        }
        None => "Disk: disabled".to_string(),
    };

    let ttl = format!("Disk TTL: {} days", sample.disk_ttl_days);
    let updated = format!("Updated: {}s ago", age.as_secs());

    let disk_stats = sample.disk_stats.map(|stats| {
        let touches = format!(
            "Disk touches: {} / {}",
            stats.touch_updates, stats.touch_attempts
        );
        let flushes = format!(
            "Disk index flushes: ok={} err={}",
            stats.access_index_flushes, stats.access_index_flush_errors
        );
        let cleanup = format!(
            "Disk cleanup: runs={} scanned={} ttl_rm={} size_rm={} last={}ms",
            stats.cleanup_runs,
            stats.cleanup_scanned_entries,
            stats.cleanup_removed_ttl,
            stats.cleanup_removed_size,
            stats.last_cleanup_duration_ms
        );
        (touches, flushes, cleanup)
    });

    let touches_str = disk_stats
        .as_ref()
        .map(|(touches, _, _)| touches.to_string())
        .unwrap_or("Disk touches: n/a".to_string());
    let flushes_str = disk_stats
        .as_ref()
        .map(|(_, flushes, _)| flushes.to_string())
        .unwrap_or("Disk index flushes: n/a".to_string());
    let cleanup_str = disk_stats
        .as_ref()
        .map(|(_, _, cleanup)| cleanup.to_string())
        .unwrap_or("Disk cleanup: n/a".to_string());

    let content = column![
        text("Image Cache").size(14),
        text(process_ram).size(12),
        text(image_ram).size(12),
        text(disk).size(12),
        text(ttl).size(12),
        text(touches_str).size(11),
        text(flushes_str).size(11),
        text(cleanup_str).size(11),
        text(updated).size(11),
    ]
    .spacing(2)
    .padding(8);

    column![
        Space::new().height(Length::Fill),
        container(content)
            .width(Length::Shrink)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                iced::widget::container::Style {
                    background: Some(
                        palette.background.weak.color.scale_alpha(0.6).into(),
                    ),
                    border: iced::Border {
                        color: palette.background.strong.color,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    text_color: Some(palette.background.strong.text),
                    ..Default::default()
                }
            })
            .padding(6)
    ]
    .into()
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GiB", b / GB)
    } else if b >= MB {
        format!("{:.1} MiB", b / MB)
    } else if b >= KB {
        format!("{:.1} KiB", b / KB)
    } else {
        format!("{bytes} B")
    }
}
