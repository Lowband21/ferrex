package com.ferrex.android.core.image

import com.ferrex.android.core.library.ServerCacheScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.UUID

@OptIn(ExperimentalCoroutinesApi::class)
class ImageResolutionControllerTest {
    private val scope = ServerCacheScope.from("http://ferrex.local", "user-1")
    private val otherScope = ServerCacheScope.from("http://ferrex.example", "user-1")

    @Test
    fun initialResolveBatchesVisibleKeysAndPublishesRetryState() = runTest {
        val resolver = FakeImageResolver()
        val ready = key(1, BrowseImageCategory.Poster)
        val pending = key(2, BrowseImageCategory.Backdrop)
        resolver.resolveResults += mapOf(
            ready to ready(ready, "ready-token"),
            pending to pending(pending, retryAfterMillis = 1_000L),
        )
        val controller = controller(resolver)

        controller.setVisibleImages(scope, listOf(ready, pending, ready))
        runCurrent()

        assertEquals(listOf(listOf(ready, pending)), resolver.resolveRequests)
        assertEquals("ready-token", (controller.state.value.resolutions[ready] as ImageResolution.Ready).token)
        assertEquals(1_000L, (controller.state.value.resolutions[pending] as ImageResolution.Pending).retryAfterMillis)
        assertEquals(1_000L, controller.state.value.scheduledRetryAtMillis)
        controller.close()
    }

    @Test
    fun pendingArtworkRetriesAfterServerDelayAndBecomesReady() = runTest {
        val resolver = FakeImageResolver()
        val image = key(3, BrowseImageCategory.Poster)
        resolver.resolveResults += mapOf(image to pending(image, retryAfterMillis = 1_000L))
        resolver.retryResults += mapOf(image to ready(image, "resolved-token"))
        val controller = controller(resolver)

        controller.setVisibleImages(scope, listOf(image))
        runCurrent()
        advanceTimeBy(999L)
        runCurrent()

        assertTrue(resolver.retryRequests.isEmpty())

        advanceTimeBy(1L)
        runCurrent()

        assertEquals(listOf(listOf(image)), resolver.retryRequests)
        assertEquals("resolved-token", (controller.state.value.resolutions[image] as ImageResolution.Ready).token)
        assertNull(controller.state.value.scheduledRetryAtMillis)
        controller.close()
    }

    @Test
    fun networkFailureKeepsStaleReadyCacheVisibleWithoutSchedulingPendingRetry() = runTest {
        val resolver = FakeImageResolver()
        val image = key(4, BrowseImageCategory.Episode)
        resolver.resolveResults += mapOf(image to ready(image, "cached-token", stale = true, offlineMessage = "offline"))
        val controller = controller(resolver)

        controller.setVisibleImages(scope, listOf(image))
        runCurrent()

        val staleReady = controller.state.value.resolutions[image] as ImageResolution.Ready
        assertTrue(staleReady.stale)
        assertEquals("offline", staleReady.offlineMessage)
        assertEquals("cached-token", staleReady.token)
        assertNull(controller.state.value.scheduledRetryAtMillis)
        assertTrue(resolver.retryRequests.isEmpty())
        controller.close()
    }

    @Test
    fun failedMissingAndInvalidVisibleKeysRetryWithoutInvalidUuidPoisoningLoop() = runTest {
        val resolver = FakeImageResolver()
        val failed = key(5, BrowseImageCategory.Profile)
        val missing = key(6, BrowseImageCategory.Backdrop)
        val invalid = ImageRequestKey("not-a-uuid", BrowseImageCategory.Poster)
        resolver.resolveResults += mapOf(
            failed to ImageResolution.Failed(failed, reason = "profile missing", retryable = true),
            invalid to ImageResolution.Placeholder(invalid, reason = "Image iid is not a valid UUID"),
        )
        resolver.retryResults += mapOf(
            failed to ready(failed, "failed-recovered"),
            missing to ready(missing, "missing-recovered"),
        )
        val controller = controller(resolver)

        controller.setVisibleImages(scope, listOf(failed, missing, invalid))
        runCurrent()

        assertEquals(5_000L, controller.state.value.scheduledRetryAtMillis)
        assertEquals("placeholder", controller.state.value.resolutions[invalid]?.label)

        advanceTimeBy(5_000L)
        runCurrent()

        assertEquals(listOf(listOf(failed, missing)), resolver.retryRequests)
        assertEquals("failed-recovered", (controller.state.value.resolutions[failed] as ImageResolution.Ready).token)
        assertEquals("missing-recovered", (controller.state.value.resolutions[missing] as ImageResolution.Ready).token)
        assertEquals("placeholder", controller.state.value.resolutions[invalid]?.label)
        assertNull(controller.state.value.scheduledRetryAtMillis)
        controller.close()
    }

    @Test
    fun scopeAndKeyChangesCancelScheduledRetries() = runTest {
        val resolver = FakeImageResolver()
        val oldImage = key(7, BrowseImageCategory.Poster)
        val newImage = key(8, BrowseImageCategory.Poster)
        resolver.resolveResults += mapOf(oldImage to pending(oldImage, retryAfterMillis = 1_000L))
        resolver.resolveResults += mapOf(newImage to ready(newImage, "new-token"))
        val controller = controller(resolver)

        controller.setVisibleImages(scope, listOf(oldImage))
        runCurrent()
        controller.setVisibleImages(otherScope, listOf(newImage))
        runCurrent()
        advanceTimeBy(1_000L)
        runCurrent()

        assertEquals(listOf(listOf(oldImage), listOf(newImage)), resolver.resolveRequests)
        assertTrue(resolver.retryRequests.isEmpty())
        assertFalse(controller.state.value.resolutions.containsKey(oldImage))
        assertEquals("new-token", (controller.state.value.resolutions[newImage] as ImageResolution.Ready).token)
        controller.close()
    }

    @Test
    fun readyResolutionsSurviveVisibleWindowChanges() = runTest {
        val resolver = FakeImageResolver()
        val first = key(9, BrowseImageCategory.Poster)
        val retained = key(10, BrowseImageCategory.Poster)
        val added = key(11, BrowseImageCategory.Poster)
        resolver.resolveResults += mapOf(
            first to ready(first, "first-token"),
            retained to ready(retained, "retained-token"),
        )
        val controller = controller(resolver)

        controller.setVisibleImages(scope, listOf(first, retained))
        runCurrent()
        controller.setVisibleImages(scope, listOf(retained, added))

        val immediate = controller.state.value
        assertFalse(immediate.resolutions.containsKey(first))
        assertEquals("retained-token", (immediate.resolutions[retained] as ImageResolution.Ready).token)
        assertNull(immediate.resolutions[added])
        controller.close()
    }

    private fun TestScope.controller(resolver: ImageResolver): ImageResolutionController = ImageResolutionController(
        resolver = resolver,
        coroutineScope = backgroundScope,
        retryPolicy = ImageResolutionRetryPolicy(
            failedOrMissingRetryDelayMillis = 5_000L,
            minimumPendingRetryDelayMillis = 0L,
        ),
        clockMillis = { currentTime },
    )

    private class FakeImageResolver : ImageResolver {
        val resolveRequests = mutableListOf<List<ImageRequestKey>>()
        val retryRequests = mutableListOf<List<ImageRequestKey>>()
        val resolveResults = mutableListOf<Map<ImageRequestKey, ImageResolution>>()
        val retryResults = mutableListOf<Map<ImageRequestKey, ImageResolution>>()

        override suspend fun resolveImages(
            scope: ServerCacheScope,
            requestedKeys: Collection<ImageRequestKey>,
        ): Map<ImageRequestKey, ImageResolution> {
            resolveRequests += requestedKeys.toList()
            return if (resolveResults.isEmpty()) emptyMap() else resolveResults.removeAt(0)
        }

        override suspend fun retryPendingOrFailed(
            scope: ServerCacheScope,
            visibleKeys: Collection<ImageRequestKey>,
        ): Map<ImageRequestKey, ImageResolution> {
            retryRequests += visibleKeys.toList()
            return if (retryResults.isEmpty()) emptyMap() else retryResults.removeAt(0)
        }
    }

    private fun key(seed: Int, category: BrowseImageCategory): ImageRequestKey =
        ImageRequestKey(UUID(0L, seed.toLong()).toString(), category)

    private fun ready(
        key: ImageRequestKey,
        token: String,
        stale: Boolean = false,
        offlineMessage: String? = null,
    ): ImageResolution.Ready = ImageResolution.Ready(
        key = key,
        url = "http://ferrex.local/api/v1/images/blob/$token",
        token = token,
        stale = stale,
        offlineMessage = offlineMessage,
    )

    private fun pending(key: ImageRequestKey, retryAfterMillis: Long): ImageResolution.Pending = ImageResolution.Pending(
        key = key,
        retryAfterMillis = retryAfterMillis,
        retryAtMillis = retryAfterMillis,
    )
}
