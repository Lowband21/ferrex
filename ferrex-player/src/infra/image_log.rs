use dashmap::{DashMap, DashSet};
use ferrex_core::player_prelude::ImageSize;
use once_cell::sync::Lazy;
use uuid::Uuid;

// Temporary diagnostics: associate media_id -> human-readable name/title.
static MEDIA_TITLES: Lazy<DashMap<Uuid, String>> = Lazy::new(|| DashMap::new());

// Guard to avoid flooding logs: record a small set of keys we've already logged.
// Keys are formed as "kind:media_id:category:size" where kind is one of
//   - fetch-ok
//   - fetch-fail
//   - decode-fallback
static LOGGED_KEYS: Lazy<DashSet<String>> = Lazy::new(|| DashSet::new());

/// Register a title for a media id (no overwrite).
pub fn register_media_title(media_id: Uuid, title: &str) {
    MEDIA_TITLES
        .entry(media_id)
        .or_insert_with(|| title.to_string());
}

fn title_for(media_id: &Uuid) -> String {
    MEDIA_TITLES
        .get(media_id)
        .map(|v| v.value().clone())
        .unwrap_or_else(|| "<unknown>".to_string())
}

fn size_label(size: &ImageSize) -> &'static str {
    match size {
        ImageSize::Thumbnail => "Thumbnail",
        ImageSize::Poster => "Poster",
        ImageSize::Backdrop => "Backdrop",
        ImageSize::Full => "Full",
        ImageSize::Profile => "Profile",
    }
}

/// Log a successful image fetch exactly once per (media, category, size).
pub fn log_fetch_once(
    media_id: Uuid,
    category: &str,
    size: ImageSize,
    target_w: u32,
    url: &str,
    byte_len: usize,
) {
    let key = format!("fetch-ok:{media_id}:{category}:{:?}", size);
    if LOGGED_KEYS.insert(key) {
        let title = title_for(&media_id);
        log::info!(
            "ImageFetch OK media_id={} title=\"{}\" category={} size={} target_w={} url={} bytes={}",
            media_id,
            title,
            category,
            size_label(&size),
            target_w,
            url,
            byte_len
        );
    }
}

/// Log a failed image fetch exactly once per (media, category, size).
pub fn log_fetch_failure_once(
    media_id: Uuid,
    category: &str,
    size: ImageSize,
    target_w: u32,
    url: &str,
    error: &str,
) {
    let key = format!("fetch-fail:{media_id}:{category}:{:?}", size);
    if LOGGED_KEYS.insert(key) {
        let title = title_for(&media_id);
        log::info!(
            "ImageFetch FAIL media_id={} title=\"{}\" category={} size={} target_w={} url={} error={}",
            media_id,
            title,
            category,
            size_label(&size),
            target_w,
            url,
            error
        );
    }
}

/// Log that we had to fall back to raw bytes handle (decode/resize failed).
pub fn log_decode_fallback_once(
    media_id: Uuid,
    category: &str,
    size: ImageSize,
    target_w: u32,
    url: &str,
    error: &str,
) {
    let key = format!("decode-fallback:{media_id}:{category}:{:?}", size);
    if LOGGED_KEYS.insert(key) {
        let title = title_for(&media_id);
        log::info!(
            "ImageDecode Fallback media_id={} title=\"{}\" category={} size={} target_w={} url={} reason={}",
            media_id,
            title,
            category,
            size_label(&size),
            target_w,
            url,
            error
        );
    }
}
