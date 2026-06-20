package com.ferrex.android.core.image

import com.ferrex.android.core.library.ServerCacheScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

interface ScopedImageLoaderClearer {
    fun clearImageLoaderState(scope: ServerCacheScope)
}

/**
 * Observable scoped ImageLoader generations that active image surfaces use to reacquire loaders
 * after recovery actions shut down the previous loader for a cache scope.
 */
class ScopedImageLoaderInvalidations {
    private val _generations = MutableStateFlow<Map<String, Long>>(emptyMap())
    val generations: StateFlow<Map<String, Long>> = _generations.asStateFlow()

    fun generationFor(scope: ServerCacheScope): Long = generations.value[scope.directoryName] ?: 0L

    fun invalidate(scope: ServerCacheScope) {
        _generations.update { generations ->
            generations + (scope.directoryName to ((generations[scope.directoryName] ?: 0L) + 1L))
        }
    }
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
