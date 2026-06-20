package com.ferrex.android.core.image

/**
 * Selects manifest image keys from a LazyGrid/LazyList visible window while keeping
 * non-visible prefetch bounded. Visible keys are intentionally not capped by the
 * prefetch budget so posters that are actually on screen are never stranded behind
 * an old first-N lookup limit.
 */
object VisibleImageRequestPlanner {
    fun <T> visibleWindowKeys(
        items: List<T>,
        visibleIndices: Collection<Int>,
        overscanBefore: Int,
        overscanAfter: Int = overscanBefore,
        keyOf: (T) -> ImageRequestKey?,
    ): List<ImageRequestKey> {
        if (items.isEmpty() || visibleIndices.isEmpty()) return emptyList()
        val validIndices = visibleIndices.filter { it in items.indices }
        if (validIndices.isEmpty()) return emptyList()

        val firstIndex = (validIndices.minOrNull()!! - overscanBefore.coerceAtLeast(0)).coerceAtLeast(0)
        val lastIndex = (validIndices.maxOrNull()!! + overscanAfter.coerceAtLeast(0)).coerceAtMost(items.lastIndex)
        return (firstIndex..lastIndex)
            .mapNotNull { keyOf(items[it]) }
            .distinctByCacheKey()
    }

    fun cappedPrefetchKeys(
        keys: Iterable<ImageRequestKey>,
        limit: Int,
    ): List<ImageRequestKey> = if (limit <= 0) {
        emptyList()
    } else {
        keys.distinctByCacheKey().take(limit)
    }

    fun mergeVisibleWithCappedPrefetch(
        visibleKeys: Iterable<ImageRequestKey>,
        prefetchKeys: Iterable<ImageRequestKey>,
        prefetchLimit: Int,
    ): List<ImageRequestKey> {
        val merged = LinkedHashMap<String, ImageRequestKey>()
        visibleKeys.distinctByCacheKey().forEach { key ->
            merged.putIfAbsent(key.cacheKey, key)
        }
        cappedPrefetchKeys(prefetchKeys, prefetchLimit).forEach { key ->
            merged.putIfAbsent(key.cacheKey, key)
        }
        return merged.values.toList()
    }
}

private fun Iterable<ImageRequestKey>.distinctByCacheKey(): List<ImageRequestKey> {
    val seen = LinkedHashSet<String>()
    return filter { seen.add(it.cacheKey) }
}
