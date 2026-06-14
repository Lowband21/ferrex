use super::access_index::KeyDigest;
use ferrex_core::infra::cache::ImageCacheKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionReason {
    TtlExpired,
    OverSizeCap,
}

#[derive(Debug, Clone)]
pub struct CacheEntryInfo {
    pub key: ImageCacheKey,
    pub digest: KeyDigest,
    pub size_bytes: u64,
    pub last_access_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PlannedEviction {
    pub key: ImageCacheKey,
    pub digest: KeyDigest,
    pub size_bytes: u64,
    pub reason: EvictionReason,
}

#[derive(Debug, Default)]
pub struct EvictionPlan {
    pub planned: Vec<PlannedEviction>,
    pub total_bytes_before: u64,
    pub total_bytes_after: u64,
    pub removed_ttl: usize,
    pub removed_size: usize,
}

pub fn plan_evictions(
    mut entries: Vec<CacheEntryInfo>,
    now_ms: u64,
    ttl_ms: u64,
    max_bytes: u64,
) -> EvictionPlan {
    let mut plan = EvictionPlan::default();

    let mut total_bytes: u64 = entries.iter().map(|e| e.size_bytes).sum();
    plan.total_bytes_before = total_bytes;

    // TTL eviction first (idle timeout).
    let mut kept: Vec<CacheEntryInfo> = Vec::with_capacity(entries.len());
    for e in entries.drain(..) {
        let age_ms = now_ms.saturating_sub(e.last_access_ms);
        if ttl_ms > 0 && age_ms > ttl_ms {
            total_bytes = total_bytes.saturating_sub(e.size_bytes);
            plan.planned.push(PlannedEviction {
                key: e.key,
                digest: e.digest,
                size_bytes: e.size_bytes,
                reason: EvictionReason::TtlExpired,
            });
            plan.removed_ttl += 1;
        } else {
            kept.push(e);
        }
    }

    // Size cap eviction next (LRU-ish by last_access_ms).
    if max_bytes > 0 && total_bytes > max_bytes {
        kept.sort_by_key(|e| e.last_access_ms);
        for e in kept {
            if total_bytes <= max_bytes {
                break;
            }
            total_bytes = total_bytes.saturating_sub(e.size_bytes);
            plan.planned.push(PlannedEviction {
                key: e.key,
                digest: e.digest,
                size_bytes: e.size_bytes,
                reason: EvictionReason::OverSizeCap,
            });
            plan.removed_size += 1;
        }
    }

    plan.total_bytes_after = total_bytes;
    plan
}
