package com.ferrex.android.core.image

import ferrex.common.ImageCategory

/**
 * Ferrex image categories supported by the mobile manifest contract.
 *
 * The compatibility `/api/v1/images/iid/{iid}` endpoint resolves poster-sized
 * images only on the current server, so browse/home callers resolve every
 * visible category through `/api/v1/images/manifest` and load immutable
 * `/api/v1/images/blob/{token}` URLs for Ready records before considering
 * guarded poster-only fallback.
 */
enum class BrowseImageCategory(
    val flatBufferValue: Byte,
    val wireName: String,
    val placeholderAspectRatio: Float,
) {
    Poster(ImageCategory.Poster, "poster", 2f / 3f),
    Backdrop(ImageCategory.Backdrop, "backdrop", 16f / 9f),
    Profile(ImageCategory.Profile, "profile", 2f / 3f),
    Episode(ImageCategory.Episode, "episode", 16f / 9f),
    ;

    companion object {
        fun fromFlatBuffer(value: Byte): BrowseImageCategory? = entries.firstOrNull { it.flatBufferValue == value }
        fun fromWireName(value: String?): BrowseImageCategory? = entries.firstOrNull { it.wireName == value }
    }
}

data class ImageRequestKey(
    val iid: String,
    val category: BrowseImageCategory,
) {
    val cacheKey: String = "${iid.trim().lowercase()}-${category.wireName}"
}

data class ImageManifestRecord(
    val key: ImageRequestKey,
    val status: ManifestImageStatus,
)

sealed interface ManifestImageStatus {
    data class Ready(val token: String) : ManifestImageStatus
    data class Pending(val retryAfterMillis: Long) : ManifestImageStatus
    data class Failed(val reason: String) : ManifestImageStatus
}

sealed interface ImageResolution {
    val key: ImageRequestKey
    val label: String
    val stale: Boolean

    data class Ready(
        override val key: ImageRequestKey,
        val url: String,
        val token: String,
        override val stale: Boolean = false,
        val offlineMessage: String? = null,
    ) : ImageResolution {
        override val label: String = if (stale) "stale-offline-ready" else "ready"
    }

    data class Pending(
        override val key: ImageRequestKey,
        val retryAfterMillis: Long,
        val retryAtMillis: Long,
        override val stale: Boolean = false,
        val offlineMessage: String? = null,
    ) : ImageResolution {
        override val label: String = if (stale) "stale-offline-pending" else "pending"
    }

    data class Failed(
        override val key: ImageRequestKey,
        val reason: String,
        val retryable: Boolean,
        override val stale: Boolean = false,
    ) : ImageResolution {
        override val label: String = if (stale) "stale-offline-failed" else "failed"
    }

    data class Placeholder(
        override val key: ImageRequestKey,
        val reason: String,
    ) : ImageResolution {
        override val stale: Boolean = false
        override val label: String = "placeholder"
    }
}

object PosterOnlyIidFallback {
    /**
     * Poster-only compatibility URL. The current Ferrex server maps IID lookups
     * to `ImageSize::poster()`, so this intentionally returns null for backdrop,
     * profile, and episode still categories.
     */
    fun url(serverUrl: String, key: ImageRequestKey): String? {
        if (key.category != BrowseImageCategory.Poster) return null
        return "${serverUrl.trim().trimEnd('/')}/api/v1/images/iid/${key.iid}"
    }
}

object TmdbImageFallbackPolicy {
    fun publicCdnUrl(
        publicPath: String?,
        category: BrowseImageCategory,
        productCopyAllowsPublicCdn: Boolean,
    ): String? {
        val path = publicPath?.trim()?.takeIf { it.startsWith('/') } ?: return null
        if (!productCopyAllowsPublicCdn) return null
        val size = when (category) {
            BrowseImageCategory.Poster -> "w342"
            BrowseImageCategory.Backdrop -> "original"
            BrowseImageCategory.Profile -> "w185"
            BrowseImageCategory.Episode -> "w500"
        }
        return "https://image.tmdb.org/t/p/$size$path"
    }
}
