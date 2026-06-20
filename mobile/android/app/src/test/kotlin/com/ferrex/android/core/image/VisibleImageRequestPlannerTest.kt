package com.ferrex.android.core.image

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.UUID

class VisibleImageRequestPlannerTest {
    @Test
    fun visibleWindowSelectsScrolledKeysPastLegacyGridCap() {
        val keys = keys(120)

        val selected = VisibleImageRequestPlanner.visibleWindowKeys(
            items = keys,
            visibleIndices = listOf(86, 87, 88, 89),
            overscanBefore = 2,
            overscanAfter = 3,
        ) { it }

        assertEquals(keys.subList(84, 93), selected)
        assertTrue(selected.contains(keys[88]))
        assertFalse(selected.contains(keys[80]))
    }

    @Test
    fun visibleWindowOverscanIsClampedToListBounds() {
        val keys = keys(5)

        val selected = VisibleImageRequestPlanner.visibleWindowKeys(
            items = keys,
            visibleIndices = listOf(0, 1, 99, -1),
            overscanBefore = 10,
            overscanAfter = 10,
        ) { it }

        assertEquals(keys, selected)
    }

    @Test
    fun duplicateStableKeysCollapseByCacheKeyInWindowOrder() {
        val duplicateA = key(7)
        val duplicateB = ImageRequestKey(duplicateA.iid.uppercase(), duplicateA.category)
        val keys = listOf(key(1), duplicateA, key(2), duplicateB, key(3))

        val selected = VisibleImageRequestPlanner.visibleWindowKeys(
            items = keys,
            visibleIndices = keys.indices.toList(),
            overscanBefore = 0,
        ) { it }

        assertEquals(listOf(key(1), duplicateA, key(2), key(3)), selected)
    }

    @Test
    fun visibleKeysWinOverPrefetchBudget() {
        val visiblePastBudget = listOf(key(100), key(101), key(102))
        val prefetch = keys(10)

        val merged = VisibleImageRequestPlanner.mergeVisibleWithCappedPrefetch(
            visibleKeys = visiblePastBudget,
            prefetchKeys = prefetch,
            prefetchLimit = 2,
        )

        assertEquals(visiblePastBudget + prefetch.take(2), merged)
        assertTrue(merged.containsAll(visiblePastBudget))
        assertEquals(5, merged.size)
    }

    private fun keys(count: Int): List<ImageRequestKey> = List(count) { key(it) }

    private fun key(seed: Int): ImageRequestKey = ImageRequestKey(
        iid = UUID(0L, seed.toLong()).toString(),
        category = BrowseImageCategory.Poster,
    )
}
