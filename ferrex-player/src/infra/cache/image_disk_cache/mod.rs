mod access_index;
mod eviction;
pub(crate) mod stats;

use ferrex_core::{
    error::MediaError,
    infra::cache::{
        ImageBlobStore, ImageCacheKey, ImageCacheRoot, image_cache_key_for,
    },
    player_prelude::ImageRequest,
};

use crate::infra::{constants::memory_usage, units::ByteSize};
use access_index::{AccessIndex, KeyDigest, write_snapshot_sync};
use eviction::{EvictionReason, plan_evictions};
use stats::{PlayerDiskImageCacheStats, PlayerDiskImageCacheStatsSnapshot};

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use dashmap::{DashMap, try_result::TryResult};
use directories::ProjectDirs;
use sha2::Digest;
use tokio::sync::Mutex;

#[derive(Debug)]
struct IndexedUsageBytes(AtomicU64);

impl IndexedUsageBytes {
    fn new(initial: u64) -> Self {
        Self(AtomicU64::new(initial))
    }

    fn load(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    fn store(&self, value: u64) {
        self.0.store(value, Ordering::Relaxed);
    }

    fn add_saturating(&self, add: u64) {
        let mut current = self.load();
        loop {
            let next = current.saturating_add(add);
            match self.0.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    fn sub_saturating(&self, sub: u64) {
        let mut current = self.load();
        loop {
            let next = current.saturating_sub(sub);
            match self.0.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageCacheNamespace(String);

impl ImageCacheNamespace {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct PlayerDiskImageCacheLimits {
    pub max_bytes: ByteSize,
    pub ttl: Duration,
    pub touch_interval: Duration,
    pub access_index_flush_interval: Duration,
}

impl PlayerDiskImageCacheLimits {
    pub const fn defaults() -> Self {
        Self {
            max_bytes: ByteSize::from_bytes(
                memory_usage::MAX_IMAGE_CACHE_BYTES,
            ),
            ttl: Duration::from_secs(30 * 24 * 60 * 60),
            touch_interval: Duration::from_secs(3),
            access_index_flush_interval: Duration::from_secs(30),
        }
    }
}

#[derive(Debug)]
pub struct PlayerDiskImageCache {
    blob_store: ImageBlobStore,
    max_bytes: AtomicU64,
    ttl_ms: AtomicU64,
    touch_interval_ms: AtomicU64,
    access_index_flush_interval_ms: AtomicU64,
    last_touch: DashMap<ImageCacheKey, Instant>,
    cleanup_lock: Mutex<()>,
    current_usage_bytes: IndexedUsageBytes,
    access_index: Mutex<AccessIndex>,
    access_index_flush_lock: Mutex<()>,
    stats: PlayerDiskImageCacheStats,
}

impl PlayerDiskImageCache {
    pub fn try_new_for_server(
        server_url: &str,
        limits: PlayerDiskImageCacheLimits,
    ) -> anyhow::Result<Self> {
        let namespace = namespace_for_server_url(server_url);
        let root = image_cache_root_for_namespace(&namespace)?;
        Self::try_new_for_root(root, limits)
    }

    pub fn try_new_for_root(
        root: ImageCacheRoot,
        limits: PlayerDiskImageCacheLimits,
    ) -> anyhow::Result<Self> {
        let root_path = root.as_path();
        std::fs::create_dir_all(root_path)?;

        let now_ms = unix_ms_now();
        let access_index_path =
            root_path.join("player-image-access-index-v1.bin");
        let mut access_index =
            AccessIndex::load_or_default(access_index_path, now_ms);

        let indexed_usage_bytes = cleanup_sync(
            root_path,
            &limits,
            &mut access_index,
            now_ms,
        )
        .unwrap_or_else(|e| {
            log::warn!(
                "disk image cache init cleanup failed; root={}, err={e}",
                root_path.display()
            );
            compute_indexed_usage_bytes_sync(root_path).unwrap_or_else(|e| {
                log::warn!(
                    "disk image cache init usage probe failed; root={}, err={e}",
                    root_path.display()
                );
                0
            })
        });

        Ok(Self {
            blob_store: ImageBlobStore::new(root),
            max_bytes: AtomicU64::new(limits.max_bytes.as_bytes()),
            ttl_ms: AtomicU64::new(
                limits.ttl.as_millis().min(u128::from(u64::MAX)) as u64,
            ),
            touch_interval_ms: AtomicU64::new(
                limits.touch_interval.as_millis().min(u128::from(u64::MAX))
                    as u64,
            ),
            access_index_flush_interval_ms: AtomicU64::new(
                limits
                    .access_index_flush_interval
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
            ),
            last_touch: DashMap::new(),
            cleanup_lock: Mutex::new(()),
            current_usage_bytes: IndexedUsageBytes::new(indexed_usage_bytes),
            access_index: Mutex::new(access_index),
            access_index_flush_lock: Mutex::new(()),
            stats: PlayerDiskImageCacheStats::default(),
        })
    }

    pub fn root(&self) -> &ImageCacheRoot {
        self.blob_store.root()
    }

    pub fn set_limits(&self, limits: PlayerDiskImageCacheLimits) {
        self.max_bytes
            .store(limits.max_bytes.as_bytes(), Ordering::SeqCst);
        self.ttl_ms.store(
            limits.ttl.as_millis().min(u128::from(u64::MAX)) as u64,
            Ordering::SeqCst,
        );
        self.touch_interval_ms.store(
            limits.touch_interval.as_millis().min(u128::from(u64::MAX)) as u64,
            Ordering::SeqCst,
        );
        self.access_index_flush_interval_ms.store(
            limits
                .access_index_flush_interval
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
            Ordering::SeqCst,
        );
    }

    pub fn set_limits_and_enforce(
        self: &Arc<Self>,
        limits: PlayerDiskImageCacheLimits,
    ) {
        self.set_limits(limits);

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let cache = Arc::clone(self);
            handle.spawn(async move {
                cache.cleanup_once().await;
            });
        }
    }

    pub async fn read_bytes(&self, request: &ImageRequest) -> Option<Vec<u8>> {
        let key = image_cache_key_for(request.iid, request.size);
        let res = self.blob_store.read(&key).await;

        match res {
            Ok(bytes) => {
                self.maybe_touch_key(&key).await;
                Some(bytes)
            }
            Err(MediaError::NotFound(_)) => None,
            Err(e) => {
                log::warn!(
                    "disk image cache read failed; key={}, err={}",
                    key.as_str(),
                    e
                );
                None
            }
        }
    }

    pub async fn write_bytes(&self, request: &ImageRequest, bytes: &[u8]) {
        let key = image_cache_key_for(request.iid, request.size);
        let digest = KeyDigest::from_key_str(key.as_str());
        let now_ms = unix_ms_now();

        let old_size_bytes = match cacache::metadata(
            self.root().as_path(),
            key.as_str(),
        )
        .await
        {
            Ok(Some(m)) => Some(m.size as u64),
            Ok(None) => None,
            Err(e) => {
                log::debug!(
                    "disk image cache write preflight metadata failed; key={}, err={}",
                    key.as_str(),
                    e
                );
                None
            }
        };

        let mut removed_old = false;
        let mut needs_usage_refresh = false;
        if old_size_bytes.is_some() {
            if let Err(e) = self.blob_store.remove(&key).await {
                log::warn!(
                    "disk image cache failed to remove stale entry before overwrite; key={}, err={}",
                    key.as_str(),
                    e
                );
                needs_usage_refresh = true;
            } else {
                removed_old = true;
            }
        }

        match self.blob_store.write(&key, bytes).await {
            Ok(_blob) => {
                if removed_old && let Some(old) = old_size_bytes {
                    self.current_usage_bytes.sub_saturating(old);
                }
                self.current_usage_bytes.add_saturating(bytes.len() as u64);

                {
                    let mut guard = self.access_index.lock().await;
                    guard.insert_on_write(digest, now_ms);
                }
            }
            Err(e) => {
                log::warn!(
                    "disk image cache write failed; key={}, err={}",
                    key.as_str(),
                    e
                );
                if removed_old {
                    needs_usage_refresh = true;
                }
            }
        }

        if needs_usage_refresh {
            let _ = self.refresh_indexed_usage_bytes().await;
        }

        let max_bytes = self.max_bytes.load(Ordering::Relaxed);
        if max_bytes > 0 && self.current_usage_bytes.load() > max_bytes {
            self.cleanup_once().await;
        } else {
            self.maybe_flush_access_index(now_ms).await;
        }
    }

    pub async fn cleanup_once(&self) {
        let _guard = self.cleanup_lock.lock().await;

        let start = Instant::now();
        let ttl_ms = self.ttl_ms.load(Ordering::Relaxed);
        let max_bytes = self.max_bytes.load(Ordering::Relaxed);
        let now_ms = unix_ms_now();

        let root = self.root().as_path().to_path_buf();
        let entries: Vec<cacache::index::Metadata> =
            match tokio::task::spawn_blocking(move || {
                let mut out = Vec::new();
                for entry in cacache::index::ls(&root) {
                    match entry {
                        Ok(m) => out.push(m),
                        Err(e) => {
                            log::warn!(
                                "disk image cache index ls entry error: {e}"
                            );
                        }
                    }
                }
                out
            })
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("disk image cache index ls join error: {e}");
                    return;
                }
            };

        if entries.is_empty() {
            self.current_usage_bytes.store(0);
            return;
        }

        let mut present_digests: HashSet<KeyDigest> =
            HashSet::with_capacity(entries.len());
        let mut infos = Vec::with_capacity(entries.len());
        {
            let guard = self.access_index.lock().await;
            for e in entries.iter() {
                let digest = KeyDigest::from_key_str(&e.key);
                present_digests.insert(digest);
                let key = ImageCacheKey::new(e.key.clone());
                let last_access_ms = guard
                    .last_access_ms(&digest)
                    .unwrap_or_else(|| u128_to_u64(e.time));
                infos.push(eviction::CacheEntryInfo {
                    key,
                    digest,
                    size_bytes: e.size as u64,
                    last_access_ms,
                });
            }
        }

        let plan = plan_evictions(infos, now_ms, ttl_ms, max_bytes);
        let mut total_bytes = plan.total_bytes_before;
        let mut removed_ttl = 0u64;
        let mut removed_size = 0u64;
        let mut removed_digests: Vec<KeyDigest> =
            Vec::with_capacity(plan.planned.len());

        for eviction in plan.planned {
            if self.blob_store.remove(&eviction.key).await.is_ok() {
                total_bytes = total_bytes.saturating_sub(eviction.size_bytes);
                removed_digests.push(eviction.digest);
                match eviction.reason {
                    EvictionReason::TtlExpired => removed_ttl += 1,
                    EvictionReason::OverSizeCap => removed_size += 1,
                }
            }
        }

        let maybe_snapshot = {
            let mut guard = self.access_index.lock().await;
            for d in removed_digests.iter() {
                let _ = guard.remove(d);
            }
            let removed_pruned = guard.prune_not_in_set(&present_digests);
            if removed_pruned > 0 {
                log::debug!(
                    "disk image cache access index pruned {} stale entries",
                    removed_pruned
                );
            }
            guard.prepare_flush(now_ms)
        };
        if let Some((path, bytes)) = maybe_snapshot {
            self.flush_access_index_snapshot(path, bytes).await;
        }

        self.current_usage_bytes.store(total_bytes);
        let duration_ms =
            start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        self.stats.on_cleanup_finished(
            present_digests.len() as u64,
            removed_ttl,
            removed_size,
            duration_ms,
        );

        if removed_ttl + removed_size > 0 {
            log::info!(
                "disk image cache cleanup removed {} entries (ttl={}, size={}) in {}ms",
                removed_ttl + removed_size,
                removed_ttl,
                removed_size,
                duration_ms
            );
        }
    }

    pub async fn current_usage_bytes(&self) -> ByteSize {
        ByteSize::from_bytes(self.current_usage_bytes.load())
    }

    pub fn stats_snapshot(&self) -> PlayerDiskImageCacheStatsSnapshot {
        self.stats.snapshot()
    }

    async fn refresh_indexed_usage_bytes(&self) -> u64 {
        let root = self.root().as_path().to_path_buf();
        let root_display = root.display().to_string();
        match tokio::task::spawn_blocking(move || {
            compute_indexed_usage_bytes_sync(&root)
        })
        .await
        {
            Ok(Ok(total)) => {
                self.current_usage_bytes.store(total);
                total
            }
            Ok(Err(e)) => {
                log::warn!(
                    "disk image cache usage probe failed; root={}, err={e}",
                    root_display
                );
                self.current_usage_bytes.load()
            }
            Err(e) => {
                log::warn!("disk image cache usage probe join failed: {e}");
                self.current_usage_bytes.load()
            }
        }
    }

    async fn maybe_touch_key(&self, key: &ImageCacheKey) {
        let touch_interval = Duration::from_millis(
            self.touch_interval_ms.load(Ordering::Relaxed).max(1),
        );

        self.stats.on_touch_attempt();
        let now = std::time::Instant::now();
        if let TryResult::Present(last) = self.last_touch.try_get(key)
            && now.duration_since(*last) < touch_interval
        {
            return;
        }
        self.last_touch.insert(key.clone(), now);
        self.stats.on_touch_update();

        let now_ms = unix_ms_now();
        let digest = KeyDigest::from_key_str(key.as_str());
        let mut maybe_snapshot: Option<(PathBuf, Vec<u8>)> = None;
        {
            let mut guard = self.access_index.lock().await;
            guard.touch(digest, now_ms);

            let max_bytes = self.max_bytes.load(Ordering::Relaxed);
            let usage = self.current_usage_bytes.load();
            let under_pressure = is_under_pressure(usage, max_bytes);
            let flush_interval = Duration::from_millis(
                self.access_index_flush_interval_ms
                    .load(Ordering::Relaxed)
                    .max(1),
            );
            if guard.should_flush(now_ms, flush_interval, under_pressure) {
                maybe_snapshot = guard.prepare_flush(now_ms);
            }
        }
        if let Some((path, bytes)) = maybe_snapshot {
            self.flush_access_index_snapshot(path, bytes).await;
        }
    }

    async fn maybe_flush_access_index(&self, now_ms: u64) {
        let max_bytes = self.max_bytes.load(Ordering::Relaxed);
        let usage = self.current_usage_bytes.load();
        let under_pressure = is_under_pressure(usage, max_bytes);

        let mut maybe_snapshot: Option<(PathBuf, Vec<u8>)> = None;
        {
            let mut guard = self.access_index.lock().await;
            let flush_interval = Duration::from_millis(
                self.access_index_flush_interval_ms
                    .load(Ordering::Relaxed)
                    .max(1),
            );
            if guard.should_flush(now_ms, flush_interval, under_pressure) {
                maybe_snapshot = guard.prepare_flush(now_ms);
            }
        }

        if let Some((path, bytes)) = maybe_snapshot {
            self.flush_access_index_snapshot(path, bytes).await;
        }
    }

    async fn flush_access_index_snapshot(&self, path: PathBuf, bytes: Vec<u8>) {
        let _guard = self.access_index_flush_lock.lock().await;
        let path_for_write = path.clone();
        let path_display = path.display().to_string();
        match tokio::task::spawn_blocking(move || {
            write_snapshot_sync(&path_for_write, &bytes)
        })
        .await
        {
            Ok(Ok(())) => self.stats.on_access_index_flush_ok(),
            Ok(Err(e)) => {
                self.stats.on_access_index_flush_err();
                log::warn!(
                    "disk image cache access index flush failed; path={}, err={e}",
                    path_display
                );
                let mut guard = self.access_index.lock().await;
                guard.mark_dirty();
            }
            Err(e) => {
                self.stats.on_access_index_flush_err();
                log::warn!(
                    "disk image cache access index flush join failed: {e}"
                );
                let mut guard = self.access_index.lock().await;
                guard.mark_dirty();
            }
        }
    }

    #[cfg(test)]
    async fn flush_access_index_for_tests(&self) {
        let now_ms = unix_ms_now();
        let maybe_snapshot = {
            let mut guard = self.access_index.lock().await;
            guard.prepare_flush(now_ms)
        };
        if let Some((path, bytes)) = maybe_snapshot {
            self.flush_access_index_snapshot(path, bytes).await;
        }
    }
}

fn image_cache_root_for_namespace(
    namespace: &ImageCacheNamespace,
) -> anyhow::Result<ImageCacheRoot> {
    let proj_dirs = ProjectDirs::from("", "ferrex", "ferrex-player")
        .ok_or_else(|| anyhow::anyhow!("Failed to resolve ProjectDirs"))?;
    let root: PathBuf = proj_dirs
        .cache_dir()
        .join("images")
        .join(namespace.as_str());
    Ok(ImageCacheRoot::new(root))
}

fn namespace_for_server_url(server_url: &str) -> ImageCacheNamespace {
    let normalized = normalize_server_url(server_url);
    let digest = sha2::Sha256::digest(normalized.as_bytes());
    ImageCacheNamespace(hex_encode(&digest[..16]))
}

fn normalize_server_url(server_url: &str) -> String {
    server_url.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn u128_to_u64(v: u128) -> u64 {
    v.min(u128::from(u64::MAX)) as u64
}

fn is_under_pressure(usage_bytes: u64, max_bytes: u64) -> bool {
    max_bytes > 0
        && usage_bytes.saturating_mul(100) >= max_bytes.saturating_mul(80)
}

fn compute_indexed_usage_bytes_sync(
    root: &std::path::Path,
) -> anyhow::Result<u64> {
    let mut total: u64 = 0;
    for entry in cacache::index::ls(root) {
        match entry {
            Ok(m) => total = total.saturating_add(m.size as u64),
            Err(e) => log::warn!("disk image cache index ls entry error: {e}"),
        }
    }
    Ok(total)
}

fn cleanup_sync(
    root: &std::path::Path,
    limits: &PlayerDiskImageCacheLimits,
    access_index: &mut AccessIndex,
    now_ms: u64,
) -> anyhow::Result<u64> {
    let ttl_ms = limits.ttl.as_millis().min(u128::from(u64::MAX)) as u64;
    let max_bytes = limits.max_bytes.as_bytes();

    let mut candidates: Vec<cacache::Metadata> = Vec::new();
    for entry in cacache::index::ls(root) {
        match entry {
            Ok(m) => candidates.push(m),
            Err(e) => log::warn!("disk image cache index ls entry error: {e}"),
        }
    }

    if candidates.is_empty() {
        if let Some((path, bytes)) = access_index.prepare_flush(now_ms)
            && let Err(err) = write_snapshot_sync(&path, &bytes)
        {
            log::warn!(
                "disk image cache init access index persist failed; path={}, err={err}",
                path.display()
            );
            access_index.mark_dirty();
        }
        return Ok(0);
    }

    let mut present_digests: HashSet<KeyDigest> =
        HashSet::with_capacity(candidates.len());
    let mut infos = Vec::with_capacity(candidates.len());
    for e in candidates.iter() {
        let digest = KeyDigest::from_key_str(&e.key);
        present_digests.insert(digest);
        let last_access_ms = access_index
            .last_access_ms(&digest)
            .unwrap_or_else(|| u128_to_u64(e.time));
        infos.push(eviction::CacheEntryInfo {
            key: ImageCacheKey::new(e.key.clone()),
            digest,
            size_bytes: e.size as u64,
            last_access_ms,
        });
    }

    let plan = plan_evictions(infos, now_ms, ttl_ms, max_bytes);
    let mut total_bytes = plan.total_bytes_before;
    let remover = cacache::index::RemoveOpts::new().remove_fully(true);
    for e in plan.planned {
        if let Err(err) = remover.clone().remove_sync(root, e.key.as_str()) {
            log::warn!(
                "disk image cache init eviction failed; key={}, err={err}",
                e.key.as_str()
            );
            continue;
        }
        total_bytes = total_bytes.saturating_sub(e.size_bytes);
        let _ = access_index.remove(&e.digest);
    }

    let _ = access_index.prune_not_in_set(&present_digests);
    if let Some((path, bytes)) = access_index.prepare_flush(now_ms)
        && let Err(err) = write_snapshot_sync(&path, &bytes)
    {
        log::warn!(
            "disk image cache init access index persist failed; path={}, err={err}",
            path.display()
        );
        access_index.mark_dirty();
    }

    Ok(total_bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        PlayerDiskImageCache, PlayerDiskImageCacheLimits, normalize_server_url,
    };
    use ferrex_core::infra::cache::ImageCacheRoot;
    use ferrex_model::image::{ImageSize, PosterSize};
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn normalize_server_url_is_stable() {
        assert_eq!(
            normalize_server_url("HTTPS://localhost:3000/"),
            "https://localhost:3000"
        );
    }

    #[tokio::test]
    async fn cache_eviction_works_across_restart_and_empty_last_touch() {
        let dir = tempdir().unwrap();
        let root = ImageCacheRoot::new(dir.path().join("images"));

        let mut limits = PlayerDiskImageCacheLimits::defaults();
        limits.touch_interval = std::time::Duration::from_secs(0);
        limits.ttl = std::time::Duration::from_secs(365 * 24 * 60 * 60);
        limits.max_bytes = crate::infra::units::ByteSize::from_bytes(0);

        let cache = PlayerDiskImageCache::try_new_for_root(
            root.clone(),
            limits.clone(),
        )
        .unwrap();

        let iid = Uuid::now_v7();
        let req_a = ferrex_model::ImageRequest::new(
            iid,
            ImageSize::Poster(PosterSize::W185),
        );
        let req_b = ferrex_model::ImageRequest::new(
            Uuid::now_v7(),
            ImageSize::Poster(PosterSize::W185),
        );

        let bytes_a = vec![1u8; 500];
        let bytes_b = vec![2u8; 700];
        cache.write_bytes(&req_a, &bytes_a).await;
        cache.write_bytes(&req_b, &bytes_b).await;

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let _ = cache.read_bytes(&req_a).await;
        cache.flush_access_index_for_tests().await;

        drop(cache);

        let mut limits2 = limits.clone();
        limits2.max_bytes = crate::infra::units::ByteSize::from_bytes(600);
        let cache2 =
            PlayerDiskImageCache::try_new_for_root(root, limits2).unwrap();

        cache2.cleanup_once().await;

        assert!(cache2.current_usage_bytes().await.as_bytes() <= 600);
        assert!(cache2.read_bytes(&req_a).await.is_some());
    }

    #[tokio::test]
    async fn ttl_is_time_since_last_touch_across_restart() {
        let dir = tempdir().unwrap();
        let root = ImageCacheRoot::new(dir.path().join("images"));

        let mut limits = PlayerDiskImageCacheLimits::defaults();
        limits.touch_interval = std::time::Duration::from_secs(0);
        limits.max_bytes = crate::infra::units::ByteSize::from_bytes(0);
        limits.ttl = std::time::Duration::from_millis(2);

        let cache = PlayerDiskImageCache::try_new_for_root(
            root.clone(),
            limits.clone(),
        )
        .unwrap();

        let req_a = ferrex_model::ImageRequest::new(
            Uuid::now_v7(),
            ImageSize::Poster(PosterSize::W185),
        );
        let req_b = ferrex_model::ImageRequest::new(
            Uuid::now_v7(),
            ImageSize::Poster(PosterSize::W185),
        );

        cache.write_bytes(&req_a, &[1u8; 128]).await;
        cache.write_bytes(&req_b, &[2u8; 128]).await;

        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
        let _ = cache.read_bytes(&req_a).await;
        cache.flush_access_index_for_tests().await;

        drop(cache);

        let cache2 =
            PlayerDiskImageCache::try_new_for_root(root, limits).unwrap();

        cache2.cleanup_once().await;

        assert!(cache2.read_bytes(&req_a).await.is_some());
        assert!(cache2.read_bytes(&req_b).await.is_none());
    }
}
