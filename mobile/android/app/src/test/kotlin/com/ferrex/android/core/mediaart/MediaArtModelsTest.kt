package com.ferrex.android.core.mediaart

import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.UUID

class MediaArtModelsTest {
    @Test
    fun posterAndProfileTreatmentsContainWithoutCrop() {
        listOf(BrowseImageCategory.Poster, BrowseImageCategory.Profile).forEach { category ->
            val treatment = MediaArtTreatment.forCategory(category)

            assertEquals(MediaArtFitPolicy.Contain, treatment.fitPolicy)
            assertNull(treatment.cropPolicy)
            assertEquals(category.placeholderAspectRatio, MediaArtDisplaySize.forCategory(category).aspectRatio)
        }
    }

    @Test
    fun backdropAndStillTreatmentsDeclareCropAndFocalPolicy() {
        listOf(BrowseImageCategory.Backdrop, BrowseImageCategory.Episode).forEach { category ->
            val treatment = MediaArtTreatment.forCategory(category)

            assertEquals(MediaArtFitPolicy.ArtDirectedCrop, treatment.fitPolicy)
            assertEquals(MediaArtCropPolicy.CenterCrop, treatment.cropPolicy)
            assertEquals(MediaArtFocalPoint.Center, treatment.cropPolicy?.focalPoint)
        }
    }

    @Test
    fun runtimeFallbackKeepsPosterIidGuardAndDisablesPublicTmdbByDefault() {
        val serverUrl = "http://ferrex.local"
        val poster = art(BrowseImageCategory.Poster, publicFallbackPath = "/poster.jpg")
        val backdrop = art(BrowseImageCategory.Backdrop, publicFallbackPath = "/backdrop.jpg")

        assertEquals(
            "http://ferrex.local/api/v1/images/iid/${poster.requestKey!!.iid}",
            poster.runtimeFallback(serverUrl)?.url,
        )
        assertNull(backdrop.runtimeFallback(serverUrl))

        val allowed = backdrop.runtimeFallback(
            serverUrl,
            MediaArtFallbackPolicy(allowPublicTmdbCdn = true),
        )
        assertNotNull(allowed)
        assertTrue(allowed!!.url.startsWith("https://image.tmdb.org/t/p/"))
    }

    @Test
    fun visualStatesExposeScreenshotableMissingPendingFailedStaleAndLowQualityLabels() {
        val poster = art(BrowseImageCategory.Poster)
        val fallback = MediaArtFallback("http://ferrex.local/api/v1/images/iid/${poster.requestKey!!.iid}", "Poster IID fallback")

        val missing = MediaArtVisualState.from(poster, null) as MediaArtVisualState.Placeholder
        val pending = MediaArtVisualState.from(
            poster,
            ImageResolution.Pending(poster.requestKey!!, retryAfterMillis = 2500, retryAtMillis = 3000),
            fallback,
        ) as MediaArtVisualState.Loaded
        val failed = MediaArtVisualState.from(
            poster,
            ImageResolution.Failed(poster.requestKey!!, reason = "manifest failed", retryable = true),
            fallback,
        ) as MediaArtVisualState.Loaded
        val stale = MediaArtVisualState.from(
            poster,
            ImageResolution.Ready(poster.requestKey!!, url = "http://blob", token = "token", stale = true, offlineMessage = "offline"),
        ) as MediaArtVisualState.Loaded
        val noFallbackFailed = MediaArtVisualState.from(
            poster,
            ImageResolution.Failed(poster.requestKey!!, reason = "manifest failed", retryable = true),
        ) as MediaArtVisualState.Placeholder

        assertTrue(missing.screenshotLabels.contains("Missing artwork"))
        assertTrue(pending.screenshotLabels.contains("Pending"))
        assertTrue(pending.screenshotLabels.contains("Low-quality fallback"))
        assertTrue(failed.screenshotLabels.contains("Failed"))
        assertTrue(stale.screenshotLabels.any { it.contains("Stale/offline") })
        assertTrue(noFallbackFailed.label.contains("manifest failed"))
        listOf(missing, pending, failed, stale, noFallbackFailed).forEach { state ->
            assertFalse("${state.stateLabel} should be screenshotable", state.screenshotLabels.isEmpty())
        }
    }

    @Test
    fun railIdentitiesPreserveStableKeysAndDisambiguateDuplicates() {
        val identities = MediaRailIdentityResolver.assign(
            railKey = "continue-watching",
            stableIds = listOf("movie:42", "movie:42", "episode:7", "movie:42"),
        )

        assertEquals(listOf("movie:42", "movie:42#2", "episode:7", "movie:42#3"), identities.map { it.renderKey })
        assertEquals("Signal", identities.first().semanticLabel("Signal"))
        assertEquals("Signal, duplicate 2", identities[1].semanticLabel("Signal"))
        assertTrue(identities[1].focusKey.contains("movie:42#2"))
    }

    private fun art(
        category: BrowseImageCategory,
        publicFallbackPath: String? = null,
    ): MediaArtObject {
        val key = ImageRequestKey(UUID(0L, category.ordinal.toLong() + 1L).toString(), category)
        return MediaArtObject.forCategory(
            category = category,
            request = MediaArtRequest(key, publicFallbackPath),
            fallbackLabel = "Missing ${category.wireName}",
        )
    }
}
