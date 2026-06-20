package com.ferrex.android.core.image

import com.ferrex.android.core.library.ServerCacheScope

interface ScopedImageLoaderClearer {
    fun clearImageLoaderState(scope: ServerCacheScope)
}

/**
 * Clears both manifest metadata and scoped image-loader state for user-visible recovery actions.
 */
class ScopedImageCacheClearer(
    private val manifestCacheClearer: ImageCacheClearer,
    private val imageLoaderClearer: ScopedImageLoaderClearer,
) : ImageCacheClearer {
    override fun clearSelectedImages(scope: ServerCacheScope, keys: Collection<ImageRequestKey>) {
        imageLoaderClearer.clearImageLoaderState(scope)
        manifestCacheClearer.clearSelectedImages(scope, keys)
    }

    override fun clearAllImages(scope: ServerCacheScope) {
        imageLoaderClearer.clearImageLoaderState(scope)
        manifestCacheClearer.clearAllImages(scope)
    }
}
