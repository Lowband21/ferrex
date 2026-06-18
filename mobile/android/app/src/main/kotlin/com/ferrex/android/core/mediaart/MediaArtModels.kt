package com.ferrex.android.core.mediaart

import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.ferrex.android.core.image.ImageResolution
import com.ferrex.android.core.image.PosterOnlyIidFallback
import com.ferrex.android.core.image.TmdbImageFallbackPolicy

/**
 * Display bounds for a media-art object.
 *
 * This is intentionally separate from [ImageRequestKey]: changing layout size must not change the
 * manifest/cache request category, and requesting a different image category must not silently
 * change the Compose bounds used by rails or detail stages.
 */
data class MediaArtDisplaySize(
    val aspectRatio: Float,
    val minHeightDp: Float? = null,
    val maxHeightDp: Float? = null,
) {
    init {
        require(aspectRatio > 0f) { "Media-art aspect ratio must be positive" }
        require(minHeightDp == null || minHeightDp >= 0f) { "Minimum height must be non-negative" }
        require(maxHeightDp == null || maxHeightDp >= 0f) { "Maximum height must be non-negative" }
        require(minHeightDp == null || maxHeightDp == null || minHeightDp <= maxHeightDp) {
            "Minimum height must not exceed maximum height"
        }
    }

    companion object {
        fun forCategory(category: BrowseImageCategory): MediaArtDisplaySize = MediaArtDisplaySize(
            aspectRatio = category.placeholderAspectRatio,
        )
    }
}

/** Request information for one media-art object. */
data class MediaArtRequest(
    val key: ImageRequestKey,
    val publicFallbackPath: String? = null,
)

/** Stable focus/click target identity for a media-art object. */
data class MediaArtTargetIdentity(
    val surfaceKey: String,
    val itemKey: String,
    val semanticLabel: String,
) {
    val focusKey: String = listOf(surfaceKey, itemKey)
        .joinToString(":")
        .replace(Regex("[^A-Za-z0-9:_-]+"), "-")
}

enum class MediaArtFitPolicy {
    /** Show the whole source image inside the art object. Required for posters and profiles. */
    Contain,

    /** Fill the art object with an explicit focal/crop policy. Used by backdrops and stills. */
    ArtDirectedCrop,
}

data class MediaArtFocalPoint(
    val x: Float,
    val y: Float,
) {
    init {
        require(x in 0f..1f) { "Focal x must be normalized" }
        require(y in 0f..1f) { "Focal y must be normalized" }
    }

    companion object {
        val Center = MediaArtFocalPoint(0.5f, 0.5f)
        val UpperCenter = MediaArtFocalPoint(0.5f, 0.35f)
    }
}

data class MediaArtCropPolicy(
    val focalPoint: MediaArtFocalPoint,
    val description: String,
) {
    companion object {
        val CenterCrop = MediaArtCropPolicy(
            focalPoint = MediaArtFocalPoint.Center,
            description = "center-crop",
        )
    }
}

enum class MediaArtGrounding {
    Flat,
    CardObject,
    TheaterPlateContactShadow,
}

data class MediaArtTreatment(
    val category: BrowseImageCategory,
    val fitPolicy: MediaArtFitPolicy,
    val cropPolicy: MediaArtCropPolicy?,
    val grounding: MediaArtGrounding,
) {
    init {
        if (fitPolicy == MediaArtFitPolicy.ArtDirectedCrop) {
            require(cropPolicy != null) { "Art-directed crop requires an explicit crop policy" }
        }
        if (fitPolicy == MediaArtFitPolicy.Contain) {
            require(cropPolicy == null) { "Contained media art must not carry a crop policy" }
        }
    }

    companion object {
        fun forCategory(
            category: BrowseImageCategory,
            grounding: MediaArtGrounding = MediaArtGrounding.CardObject,
        ): MediaArtTreatment = when (category) {
            BrowseImageCategory.Poster,
            BrowseImageCategory.Profile -> MediaArtTreatment(
                category = category,
                fitPolicy = MediaArtFitPolicy.Contain,
                cropPolicy = null,
                grounding = grounding,
            )
            BrowseImageCategory.Backdrop,
            BrowseImageCategory.Episode -> MediaArtTreatment(
                category = category,
                fitPolicy = MediaArtFitPolicy.ArtDirectedCrop,
                cropPolicy = MediaArtCropPolicy.CenterCrop,
                grounding = grounding,
            )
        }
    }
}

/** Normalized media-art object consumed by phone, TV, rails, and detail stages. */
data class MediaArtObject(
    val displaySize: MediaArtDisplaySize,
    val request: MediaArtRequest?,
    val fallbackLabel: String,
    val treatment: MediaArtTreatment,
    val targetIdentity: MediaArtTargetIdentity? = null,
) {
    val requestKey: ImageRequestKey? get() = request?.key

    companion object {
        fun forCategory(
            category: BrowseImageCategory,
            request: MediaArtRequest? = null,
            fallbackLabel: String = "Image unavailable",
            targetIdentity: MediaArtTargetIdentity? = null,
            grounding: MediaArtGrounding = MediaArtGrounding.CardObject,
        ): MediaArtObject = MediaArtObject(
            displaySize = MediaArtDisplaySize.forCategory(category),
            request = request,
            fallbackLabel = fallbackLabel,
            treatment = MediaArtTreatment.forCategory(category, grounding),
            targetIdentity = targetIdentity,
        )
    }
}

enum class MediaArtSourceQuality(val screenshotLabel: String) {
    ManifestReady("Manifest image"),
    LowQualityFallback("Low-quality fallback"),
}

data class MediaArtFallback(
    val url: String,
    val label: String,
    val quality: MediaArtSourceQuality = MediaArtSourceQuality.LowQualityFallback,
)

data class MediaArtFallbackPolicy(
    /** Public TMDB CDN fallbacks are off by default; product copy must opt in. */
    val allowPublicTmdbCdn: Boolean = false,
)

fun MediaArtObject.runtimeFallback(
    serverUrl: String,
    policy: MediaArtFallbackPolicy = MediaArtFallbackPolicy(),
): MediaArtFallback? {
    val request = request ?: return null
    val key = request.key
    val iidUrl = PosterOnlyIidFallback.url(serverUrl, key)
    if (iidUrl != null) return MediaArtFallback(iidUrl, "Poster IID fallback")

    val tmdbUrl = TmdbImageFallbackPolicy.publicCdnUrl(
        publicPath = request.publicFallbackPath,
        category = key.category,
        productCopyAllowsPublicCdn = policy.allowPublicTmdbCdn,
    )
    return tmdbUrl?.let { MediaArtFallback(it, "TMDB fallback") }
}

sealed interface MediaArtVisualState {
    val stateLabel: String
    val screenshotLabels: List<String>

    data class Loaded(
        val url: String,
        val quality: MediaArtSourceQuality,
        override val stateLabel: String,
        override val screenshotLabels: List<String>,
    ) : MediaArtVisualState

    data class Placeholder(
        val label: String,
        override val stateLabel: String,
        override val screenshotLabels: List<String>,
    ) : MediaArtVisualState

    companion object {
        fun from(
            art: MediaArtObject,
            resolution: ImageResolution?,
            fallback: MediaArtFallback? = null,
        ): MediaArtVisualState = when (resolution) {
            is ImageResolution.Ready -> Loaded(
                url = resolution.url,
                quality = MediaArtSourceQuality.ManifestReady,
                stateLabel = resolution.label,
                screenshotLabels = buildList {
                    if (resolution.stale) add("Stale/offline")
                    resolution.offlineMessage?.let { add("Offline: $it") }
                },
            )
            is ImageResolution.Pending -> fallback?.toLoadedState(
                stateLabel = resolution.label,
                statusLabel = "Pending",
                stale = resolution.stale,
                offlineMessage = resolution.offlineMessage,
            ) ?: Placeholder(
                label = stalePrefix(resolution.stale) + "Image pending. Retry after ${resolution.retryAfterMillis} ms.",
                stateLabel = resolution.label,
                screenshotLabels = buildList {
                    add("Pending")
                    if (resolution.stale) add("Stale/offline")
                    resolution.offlineMessage?.let { add("Offline: $it") }
                },
            )
            is ImageResolution.Failed -> fallback?.toLoadedState(
                stateLabel = resolution.label,
                statusLabel = "Failed",
                stale = resolution.stale,
                offlineMessage = null,
            ) ?: Placeholder(
                label = stalePrefix(resolution.stale) + resolution.reason,
                stateLabel = resolution.label,
                screenshotLabels = buildList {
                    add("Failed")
                    if (resolution.stale) add("Stale/offline")
                },
            )
            is ImageResolution.Placeholder -> Placeholder(
                label = resolution.reason,
                stateLabel = resolution.label,
                screenshotLabels = listOf("Missing artwork"),
            )
            null -> fallback?.toLoadedState(
                stateLabel = "queued",
                statusLabel = "Manifest lookup queued",
                stale = false,
                offlineMessage = null,
            ) ?: Placeholder(
                label = art.fallbackLabel,
                stateLabel = "missing",
                screenshotLabels = listOf("Missing artwork"),
            )
        }

        private fun MediaArtFallback.toLoadedState(
            stateLabel: String,
            statusLabel: String,
            stale: Boolean,
            offlineMessage: String?,
        ): Loaded = Loaded(
            url = url,
            quality = quality,
            stateLabel = stateLabel,
            screenshotLabels = buildList {
                add(statusLabel)
                if (stale) add("Stale/offline")
                add(quality.screenshotLabel)
                add(label)
                offlineMessage?.let { add("Offline: $it") }
            },
        )
    }
}

private fun stalePrefix(stale: Boolean): String = if (stale) "Offline image. " else ""

data class MediaRailItemIdentity(
    val railKey: String,
    val itemStableId: String,
    val occurrence: Int,
) {
    init {
        require(occurrence >= 0) { "Rail item occurrence must be zero-based and non-negative" }
    }

    val renderKey: String = if (occurrence == 0) itemStableId else "$itemStableId#${occurrence + 1}"
    val focusKey: String = listOf(railKey, renderKey)
        .joinToString(":")
        .replace(Regex("[^A-Za-z0-9:_#-]+"), "-")

    fun semanticLabel(title: String): String = if (occurrence == 0) {
        title
    } else {
        "$title, duplicate ${occurrence + 1}"
    }
}

object MediaRailIdentityResolver {
    fun assign(railKey: String, stableIds: List<String>): List<MediaRailItemIdentity> {
        val seen = mutableMapOf<String, Int>()
        return stableIds.map { stableId ->
            val occurrence = seen.getOrDefault(stableId, 0)
            seen[stableId] = occurrence + 1
            MediaRailItemIdentity(
                railKey = railKey,
                itemStableId = stableId,
                occurrence = occurrence,
            )
        }
    }
}

data class MediaRailItem(
    val identity: MediaRailItemIdentity,
    val title: String,
    val subtitle: String?,
    val art: MediaArtObject,
)

data class MediaRail(
    val stableKey: String,
    val title: String,
    val items: List<MediaRailItem>,
) {
    val renderKeys: List<String> = items.map { it.identity.renderKey }
    val focusKeys: List<String> = items.map { it.identity.focusKey }
}
